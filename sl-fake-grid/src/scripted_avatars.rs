//! Scripted avatars: a second real client a scenario brings along, so a script
//! that asks an avatar something has somebody to answer it.
//!
//! Most of what a script does to an avatar needs the avatar to answer back.
//! `llRequestPermissions` waits for a `ScriptAnswerYes`, `llDialog` for a
//! button on its hidden channel, `money` for somebody to pay, a pose ball's
//! `llAvatarOnSitTarget` for somebody to sit. The grid's NPC fixtures cannot:
//! they are avatar-shaped objects with no circuit behind them. A
//! [`ScriptedAvatar`] is a headless `sl-client-tokio` session the grid logs in
//! itself, as one of its own accounts, and drives from a short script.
//!
//! # The script
//!
//! Its steps are a [`Timeline`](crate::Timeline)'s seen from the other side: an
//! [`AvatarStep`] is the same [`At`] — waited on against the avatar's **own**
//! session on the grid, by the code a timeline waits with — and an
//! [`AvatarAction`] the avatar takes as a client: walk here, touch that, sit,
//! stand, say, pay, answer the next dialog with this button, grant the next
//! question these permissions, wait until somebody says this.
//!
//! What it is asked without a step to answer it, an [`AnswerPolicy`] answers:
//! grant every permission, take every dialog's first button, pay a vendor its
//! default price when touching it. That is the cheap default — a scenario that
//! only needs the grant to exist need not spell out a conversation — and the
//! explicit steps are for a case that asserts *which* answer was given. A
//! prompt goes to the step being run when that step is the one that answers
//! it, and to the policy otherwise.
//!
//! # Determinism
//!
//! Everything it does is tied to its session's events and its region's ticks:
//! [`At::AfterTicks`] is the wait to write, a walk is checked once a tick, and
//! no step sleeps unless a scenario asks it to with a duration. What it is asked
//! and what it answers goes into its [`Transcript`], which states names, labels
//! and amounts and never a time — so two runs of one seed write the same one.
//!
//! # What it is not
//!
//! A load generator, or anything that decides for itself. It is the second
//! half of a two-party protocol test, and it stays in the region it logged into
//! unless a script moves it: it follows its agent's root session there, but
//! its targets are resolved in whichever region that is.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use sl_client_tokio::{
    CircuitId, Client, Command, Event, LoginParams, LoginRequest, ScopedObjectId, StartLocation,
};
use sl_proto::{
    ChatType, ClickAction, LindenAmount, MoneyTransactionType, RegionLocalObjectId, ScriptDialog,
    ScriptPermissionRequest,
};
use sl_types::chat::ChatChannel;
use sl_types::key::{AgentKey, ObjectKey};
use sl_types::lsl::{Rotation, ScriptPermissions, Vector};
use sl_types::map::RegionCoordinates;
use tokio::sync::{mpsc, watch};

use crate::accounts::AccountConfig;
use crate::driver::SharedSim;
use crate::runtime::GridCore;
use crate::timeline::{At, Wait, WaitChannels};

/// How close a walk has to come to where it was going to have got there, in
/// metres: about a stride, which one tick of walking can overshoot by.
pub const WALK_ARRIVAL_M: f32 = 0.5;

/// How many ticks a walk may go without getting any closer before it gives
/// up: an avatar walking into a wall stops, and so does its script.
const WALK_STALL_TICKS: u32 = 10;

/// The channel depth the client's events are forwarded on. Unbounded on the
/// avatar's side, because a client whose events are not read stops.
const CLIENT_EVENTS: usize = 256;

/// One avatar the grid logs in and drives: who it is, where it starts, how it
/// answers what it is not scripted to answer, and its script.
///
/// Stated on the [`RegionConfig`](crate::RegionConfig) it logs into; the grid
/// registers its account (with that region as its start) when it starts and
/// logs it in straight away.
#[derive(Clone, Debug)]
pub struct ScriptedAvatar {
    /// Its account. Its start region is the region it is stated on unless the
    /// account names one.
    pub account: AccountConfig,
    /// Where in that region it logs in, or `None` for where the account would
    /// land.
    pub start: Option<Vector>,
    /// How it answers what no step answers.
    pub policy: AnswerPolicy,
    /// Its script, run once it has arrived.
    pub steps: Vec<AvatarStep>,
}

impl ScriptedAvatar {
    /// An avatar called `first last`, wearing nothing more than its body
    /// parts, answering nothing, with no script.
    #[must_use]
    pub fn new(first: impl Into<String>, last: impl Into<String>) -> Self {
        let (first, last) = (first.into(), last.into());
        let password = format!("{first}.{last}.scripted");
        Self {
            account: AccountConfig::new(first, last, password),
            start: None,
            policy: AnswerPolicy::default(),
            steps: Vec::new(),
        }
    }

    /// The same avatar, wearing clothes over its body parts
    /// ([`AccountConfig::dressed`]), so a frame of it is worth looking at.
    #[must_use]
    pub const fn dressed(mut self) -> Self {
        self.account.dressed = true;
        self
    }

    /// The same avatar, logging in at `position` in its region.
    #[must_use]
    pub const fn starting_at(mut self, position: Vector) -> Self {
        self.start = Some(position);
        self
    }

    /// The same avatar, answering what no step answers by `policy`.
    #[must_use]
    pub const fn answering(mut self, policy: AnswerPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Appends a step.
    #[must_use]
    pub fn then(mut self, at: At, action: AvatarAction) -> Self {
        self.steps.push(AvatarStep { at, action });
        self
    }

    /// Appends a step `ticks` ticks of its region after the previous one —
    /// the shape a script against scripts is written in.
    #[must_use]
    pub fn after_ticks(self, ticks: u32, action: AvatarAction) -> Self {
        self.then(At::AfterTicks(ticks), action)
    }

    /// Its full name, as the grid's login notices and its transcript's
    /// readers know it.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{} {}", self.account.first_name, self.account.last_name)
    }
}

/// How a scripted avatar answers what no step of its script answers.
///
/// The default answers nothing: a question stays unanswered, a dialog open,
/// a vendor touched rather than paid — and each is still written in the
/// transcript, so a case can see it was asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnswerPolicy {
    /// What it does with a permission question.
    pub permissions: PermissionAnswer,
    /// What it does with a dialog.
    pub dialogs: DialogAnswer,
    /// What a touch on an object that is clicked to pay does.
    pub payments: PaymentAnswer,
}

impl AnswerPolicy {
    /// Grants every permission, takes every dialog's first button and pays
    /// every vendor its default price: the policy of an avatar that is only
    /// there for the grant to exist.
    #[must_use]
    pub const fn agreeable() -> Self {
        Self {
            permissions: PermissionAnswer::GrantAll,
            dialogs: DialogAnswer::FirstButton,
            payments: PaymentAnswer::DefaultPrice,
        }
    }
}

/// What a scripted avatar does with a permission question no step answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionAnswer {
    /// Leaves it unanswered, as a resident who closes the window does.
    #[default]
    Ignore,
    /// Grants everything it asks for.
    GrantAll,
    /// Answers it, granting nothing.
    Refuse,
}

/// What a scripted avatar does with a dialog no step answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogAnswer {
    /// Leaves it open. A text box is always left open by a policy: what to
    /// type is a step's to say.
    #[default]
    Ignore,
    /// Takes its first button.
    FirstButton,
}

/// What a scripted avatar's touch does on an object whose click action is
/// pay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaymentAnswer {
    /// Touches it, as on any other object.
    #[default]
    Never,
    /// Pays it what its pay window offers first, as a resident who clicks it
    /// and takes the default does: its default price, else its first button.
    DefaultPrice,
}

/// One step of a scripted avatar's script: what is waited for, and what the
/// avatar then does.
#[derive(Clone, Debug)]
pub struct AvatarStep {
    /// What is waited for before [`action`](Self::action) runs, against the
    /// avatar's own session on the grid: [`At::OnEvent`] sees what its client
    /// sent, [`At::AfterTicks`] counts its region's ticks, and
    /// [`At::OnMarkerAck`] has no marker to wait for and runs at once.
    pub at: At,
    /// What the avatar does.
    pub action: AvatarAction,
}

/// An object a step names: by its key, or by its name in the region the
/// avatar is in (the first of that name, in the region's own order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The object with this key.
    Key(ObjectKey),
    /// The first object called this.
    Named(String),
}

/// Which button a step answers a dialog with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogChoice {
    /// The button at this position, the first being `0`.
    Index(usize),
    /// The button with this label.
    Label(String),
    /// For a text box: this text.
    Text(String),
}

/// What a scripted avatar does, as a client.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AvatarAction {
    /// Walks to a point of its region: turns to face it and holds the forward
    /// key until it is within [`WALK_ARRIVAL_M`], checked each tick, or gives
    /// up once it stops getting closer.
    Walk {
        /// Where to, in region metres; only the horizontal part is aimed at.
        to: Vector,
    },
    /// Touches an object — or pays it, when it is clicked to pay and the
    /// policy pays vendors.
    Touch(Target),
    /// Sits on an object, and waits for the grid's answer.
    Sit(Target),
    /// Stands up.
    Stand,
    /// Says something.
    Say {
        /// The channel, `0` for local chat.
        channel: i32,
        /// What it says.
        message: String,
    },
    /// Pays an object, and waits for the grid's answer.
    Pay {
        /// The object.
        target: Target,
        /// How much, or `None` for what its pay window offers first.
        amount: Option<i32>,
    },
    /// Answers the next dialog — one already shown, or the next to come.
    AnswerDialog(DialogChoice),
    /// Answers the next permission question with these permissions, of the
    /// ones it asked for; none refuses it.
    AnswerPermissions(ScriptPermissions),
    /// Waits until it hears a line of chat containing this, said while this
    /// step is the next to run.
    AwaitChat(String),
}

/// One line of what a scripted avatar was asked and did.
///
/// Names, labels and amounts, never a time or an id the grid mints per run
/// unless the scenario fixed it — so a transcript is comparable across runs.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Entry {
    /// It arrived in its region.
    Arrived,
    /// It walked to `to`; `reached` says whether it got there.
    Walked {
        /// Where it was going.
        to: Vector,
        /// Whether it got within [`WALK_ARRIVAL_M`].
        reached: bool,
    },
    /// It touched an object.
    Touched(String),
    /// The grid sat it on an object.
    Sat(String),
    /// The grid refused to sit it, saying this.
    SitRefused(String),
    /// It stood up.
    Stood,
    /// It said something.
    Said {
        /// The channel.
        channel: i32,
        /// What it said.
        message: String,
    },
    /// It paid an object, and the grid took it.
    Paid {
        /// The object.
        object: String,
        /// How much.
        amount: i32,
    },
    /// It paid an object, and the grid refused it.
    PaymentRefused {
        /// The object.
        object: String,
        /// How much it tried.
        amount: i32,
        /// What the grid said.
        reason: String,
    },
    /// It meant to pay an object that offers no amount to pay.
    NothingToPay(String),
    /// A step named an object its region does not have.
    NoSuchObject(String),
    /// An object asked it for permissions.
    Asked {
        /// The object.
        object: String,
        /// What it asked for.
        permissions: ScriptPermissions,
    },
    /// It answered a permission question; nothing granted is a refusal.
    Granted {
        /// The object.
        object: String,
        /// What it granted.
        permissions: ScriptPermissions,
    },
    /// An object showed it a dialog.
    Dialog {
        /// The object.
        object: String,
        /// The dialog's text.
        message: String,
        /// Its buttons.
        buttons: Vec<String>,
    },
    /// It answered a dialog with this button (or text).
    Answered {
        /// The object.
        object: String,
        /// The button's label, or the text typed.
        button: String,
    },
    /// A step's choice named no button the dialog has.
    NoSuchButton {
        /// The object.
        object: String,
        /// What the step asked for.
        choice: String,
    },
    /// It heard the line a step waited for.
    Heard {
        /// Who said it.
        from: String,
        /// The line.
        message: String,
    },
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Arrived => f.write_str("arrived"),
            Self::Walked { to, reached } => {
                let how = if *reached {
                    "walked to"
                } else {
                    "stopped short of"
                };
                write!(f, "{how} <{}, {}, {}>", to.x, to.y, to.z)
            }
            Self::Touched(object) => write!(f, "touched {object}"),
            Self::Sat(object) => write!(f, "sat on {object}"),
            Self::SitRefused(reason) => write!(f, "was refused a seat: {reason}"),
            Self::Stood => f.write_str("stood up"),
            Self::Said { channel, message } => write!(f, "said on {channel}: {message}"),
            Self::Paid { object, amount } => write!(f, "paid {object} L${amount}"),
            Self::PaymentRefused {
                object,
                amount,
                reason,
            } => write!(f, "was refused paying {object} L${amount}: {reason}"),
            Self::NothingToPay(object) => write!(f, "found nothing to pay {object}"),
            Self::NoSuchObject(object) => write!(f, "found no {object}"),
            Self::Asked {
                object,
                permissions,
            } => write!(f, "{object} asked for permissions {:#x}", permissions.0),
            Self::Granted {
                object,
                permissions,
            } => {
                if permissions.0 == 0 {
                    write!(f, "refused {object} its permissions")
                } else {
                    write!(f, "granted {object} permissions {:#x}", permissions.0)
                }
            }
            Self::Dialog {
                object,
                message,
                buttons,
            } => write!(f, "{object} showed a dialog {message:?} {buttons:?}"),
            Self::Answered { object, button } => write!(f, "answered {object} with {button:?}"),
            Self::NoSuchButton { object, choice } => {
                write!(f, "found no button {choice:?} on {object}'s dialog")
            }
            Self::Heard { from, message } => write!(f, "heard {from}: {message}"),
        }
    }
}

/// What a scripted avatar was asked and did, in order.
pub type Transcript = Vec<Entry>;

/// How far a scripted avatar has got.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Progress {
    /// It is logging in.
    LoggingIn,
    /// It has arrived and is running its script.
    Running,
    /// It has run its whole script, and goes on answering by its policy.
    Finished,
    /// It stopped: the reason. A grid shutting down stops it too.
    Stopped(String),
}

/// A running scripted avatar, as a test or a tool reads it.
#[derive(Clone)]
pub struct ScriptedAvatarHandle {
    /// Its full name.
    name: String,
    /// What it was asked and did so far.
    transcript: Arc<parking_lot::Mutex<Transcript>>,
    /// How far it has got.
    progress: watch::Receiver<Progress>,
    /// Its agent id, once it has logged in.
    agent: watch::Receiver<Option<AgentKey>>,
    /// Asks it to log out.
    log_out: mpsc::Sender<()>,
}

impl std::fmt::Debug for ScriptedAvatarHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedAvatarHandle")
            .field("name", &self.name)
            .field("progress", &*self.progress.borrow())
            .finish_non_exhaustive()
    }
}

impl ScriptedAvatarHandle {
    /// Its full name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its agent id, once it has logged in.
    #[must_use]
    pub fn agent_id(&self) -> Option<AgentKey> {
        *self.agent.borrow()
    }

    /// What it was asked and did so far.
    #[must_use]
    pub fn transcript(&self) -> Transcript {
        self.transcript.lock().clone()
    }

    /// How far it has got.
    #[must_use]
    pub fn progress(&self) -> Progress {
        self.progress.borrow().clone()
    }

    /// Waits until it is past `wanted` — running, finished, or stopped — and
    /// says where it got to.
    async fn until(&self, wanted: impl Fn(&Progress) -> bool + Send + Sync) -> Progress {
        let mut progress = self.progress.clone();
        let reached = progress.wait_for(|now| wanted(now)).await;
        reached.map_or_else(
            |_closed| Progress::Stopped("the avatar's task is gone".to_owned()),
            |now| now.clone(),
        )
    }

    /// Waits until it has arrived (or stopped first) and says where it got.
    pub async fn arrived(&self) -> Progress {
        self.until(|now| !matches!(now, Progress::LoggingIn)).await
    }

    /// Waits until it has run its whole script (or stopped first) and says
    /// where it got.
    pub async fn finished(&self) -> Progress {
        self.until(|now| matches!(now, Progress::Finished | Progress::Stopped(_)))
            .await
    }

    /// Logs it out, and waits until its client has gone.
    pub async fn log_out(&self) -> Progress {
        // Gone already is logged out already.
        let _sent = self.log_out.send(()).await;
        self.until(|now| matches!(now, Progress::Stopped(_))).await
    }
}

/// Starts `avatar` against the grid `core` runs.
pub(crate) fn spawn(core: Arc<GridCore>, avatar: ScriptedAvatar) -> ScriptedAvatarHandle {
    let transcript = Arc::new(parking_lot::Mutex::new(Transcript::new()));
    let (progress_tx, progress) = watch::channel(Progress::LoggingIn);
    let (agent_tx, agent) = watch::channel(None);
    let (log_out, log_out_rx) = mpsc::channel(1);
    let handle = ScriptedAvatarHandle {
        name: avatar.name(),
        transcript: Arc::clone(&transcript),
        progress,
        agent,
        log_out,
    };
    let name = avatar.name();
    drop(tokio::spawn(async move {
        let reason = run(
            core,
            avatar,
            &transcript,
            &progress_tx,
            &agent_tx,
            log_out_rx,
        )
        .await;
        tracing::debug!("scripted avatar {name} stopped: {reason}");
        progress_tx.send_replace(Progress::Stopped(reason));
    }));
    handle
}

/// What the avatar's client told it, of what a script can ask or answer.
#[derive(Debug)]
enum Told {
    /// A dialog.
    Dialog(Box<ScriptDialog>),
    /// A permission question.
    Question(Box<ScriptPermissionRequest>),
    /// A line of chat: who said it, and what.
    Chat(String, String),
    /// The grid sat the avatar on this object.
    Sat(ObjectKey),
    /// An alert's text: how a refused sit is told.
    Alert(String),
    /// An object's pay window: its default price and its buttons.
    PayPrice(ObjectKey, i32, Vec<i32>),
    /// A money transfer's outcome: whether it went through, and what the grid
    /// said.
    Money(bool, String),
    /// The client's session ended.
    Ended,
}

/// The avatar's run: logs in, runs the script, then answers by policy until
/// it is asked to log out or the grid goes. Returns why it stopped.
async fn run(
    core: Arc<GridCore>,
    avatar: ScriptedAvatar,
    transcript: &Arc<parking_lot::Mutex<Transcript>>,
    progress: &watch::Sender<Progress>,
    agent_tx: &watch::Sender<Option<AgentKey>>,
    mut log_out: mpsc::Receiver<()>,
) -> String {
    let mut shutdown = core.shutdown_tx.subscribe();
    // Subscribed before the login, so its own notice cannot be missed.
    let mut logins = core.logins_tx.subscribe();
    let start = match (&avatar.account.start_region, &avatar.start) {
        (Some(region), Some(at)) => {
            StartLocation::region(region.clone(), RegionCoordinates::new(at.x, at.y, at.z))
        }
        _ => StartLocation::Last,
    };
    let client = match Client::connect(LoginParams {
        login_uri: core.login_uri.clone(),
        request: LoginRequest::new(
            &avatar.account.first_name,
            &avatar.account.last_name,
            &avatar.account.password,
            start,
            "sl-fake-grid-scripted-avatar",
            env!("CARGO_PKG_VERSION"),
        ),
    })
    .await
    {
        Ok(client) => client,
        Err(error) => return format!("the login failed: {error}"),
    };
    let Some(circuit) = client.root_circuit_id() else {
        return "the login opened no circuit".to_owned();
    };
    let Some(agent) = client.agent_id() else {
        return "the login named no agent".to_owned();
    };
    agent_tx.send_replace(Some(agent));
    // The login answered after the grid made the session, and said so first.
    let session = loop {
        match logins.recv().await {
            Ok(notice) if notice.agent_id == agent => break notice.session_seq,
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return "the grid stopped before its login notice".to_owned();
            }
        }
    };
    drop(logins);
    let (event_tx, event_rx) = mpsc::channel::<Event>(CLIENT_EVENTS);
    let (commands, command_rx) = mpsc::channel::<Command>(CLIENT_EVENTS);
    let (diag_tx, diag_rx) = mpsc::channel(CLIENT_EVENTS);
    let client_run = tokio::spawn(async move {
        let _ended = client.run(event_tx, diag_tx, command_rx).await;
    });
    let (told_tx, told) = mpsc::unbounded_channel();
    let forward = tokio::spawn(forward(event_rx, diag_rx, told_tx));
    let mut runner = Runner {
        core,
        agent,
        circuit,
        commands,
        told,
        policy: avatar.policy,
        pending: VecDeque::new(),
        transcript: Arc::clone(transcript),
    };
    let reason = tokio::select! {
        reason = runner.play(session, &avatar.steps, progress) => reason,
        _ = log_out.recv() => runner.leave().await,
        () = shut_down(&mut shutdown) => "the grid shut down".to_owned(),
    };
    client_run.abort();
    forward.abort();
    reason
}

/// Resolves once the grid is shutting down.
async fn shut_down(shutdown: &mut watch::Receiver<bool>) {
    // A dropped sender is a grid that is gone.
    drop(shutdown.wait_for(|down| *down).await);
}

/// Passes on, from everything the client says, what a scripted avatar reads.
///
/// Reads every event, whatever it is: a client whose events go unread stops.
async fn forward(
    mut events: mpsc::Receiver<Event>,
    mut diagnostics: mpsc::Receiver<sl_client_tokio::Diagnostic>,
    told: mpsc::UnboundedSender<Told>,
) {
    loop {
        let event = tokio::select! {
            event = events.recv() => event,
            // Drained so they never fill; a scripted avatar has no use for them.
            Some(_diagnostic) = diagnostics.recv() => continue,
        };
        let Some(event) = event else {
            drop(told.send(Told::Ended));
            return;
        };
        let passed = match event {
            Event::ScriptDialog(dialog) => Told::Dialog(dialog),
            Event::ScriptPermissionRequest(question) => Told::Question(question),
            Event::ChatReceived(chat) => Told::Chat(chat.from_name, chat.message),
            Event::SitResult { sit_object, .. } => Told::Sat(sit_object),
            Event::AlertMessage { message, .. } => Told::Alert(message),
            Event::PayPriceReply {
                object_id,
                default_pay_price,
                pay_buttons,
            } => Told::PayPrice(object_id, default_pay_price, pay_buttons),
            Event::MoneyBalance(balance) => Told::Money(balance.success, balance.description),
            Event::Disconnected(_) | Event::LoggedOut => Told::Ended,
            _other => continue,
        };
        if told.send(passed).is_err() {
            return;
        }
    }
}

/// An object a step resolved: what a command and a transcript line need of
/// it.
#[derive(Clone, Debug)]
struct Resolved {
    /// Its key.
    key: ObjectKey,
    /// Its region-local id in the avatar's region.
    local_id: RegionLocalObjectId,
    /// Its name.
    name: String,
    /// What clicking it does.
    click: ClickAction,
}

/// The state of one scripted avatar's run.
struct Runner {
    /// The grid.
    core: Arc<GridCore>,
    /// The avatar's agent id.
    agent: AgentKey,
    /// Its client's root circuit, which its object ids are scoped to.
    circuit: CircuitId,
    /// Its client's commands.
    commands: mpsc::Sender<Command>,
    /// What its client told it.
    told: mpsc::UnboundedReceiver<Told>,
    /// How it answers what no step answers.
    policy: AnswerPolicy,
    /// What it was told that the step being run, or about to run, answers.
    pending: VecDeque<Told>,
    /// Its transcript.
    transcript: Arc<parking_lot::Mutex<Transcript>>,
}

impl Runner {
    /// Writes a line of the transcript.
    fn note(&self, entry: Entry) {
        tracing::debug!(agent = %self.agent, "scripted avatar: {entry}");
        self.transcript.lock().push(entry);
    }

    /// Sends a command to the client; `false` when the client has gone.
    async fn send(&self, command: Command) -> bool {
        self.commands.send(command).await.is_ok()
    }

    /// The avatar's root session on the grid: where it is now.
    async fn session(&self) -> Result<SharedSim, String> {
        self.core
            .root_session_of(self.agent)
            .await
            .ok_or_else(|| "the avatar has no session on the grid".to_owned())
    }

    /// Logs in through arrival, runs the steps, then answers by policy until
    /// the session ends. Returns why it ended.
    async fn play(
        &mut self,
        session: u64,
        steps: &[AvatarStep],
        progress: &watch::Sender<Progress>,
    ) -> String {
        let (arrival, shared) = match self.arrive(session).await {
            Ok(arrived) => arrived,
            Err(reason) => return reason,
        };
        self.note(Entry::Arrived);
        progress.send_replace(Progress::Running);
        let mut previous = arrival;
        let mut session = shared;
        for (index, step) in steps.iter().enumerate() {
            let upcoming = steps.get(index..).unwrap_or_default();
            if let Err(reason) = self.wait(&session, step, arrival, previous, upcoming).await {
                return reason;
            }
            previous = session.now();
            if let Err(reason) = self.act(&step.action, upcoming).await {
                return reason;
            }
            // A step may have carried it to another region.
            session = match self.session().await {
                Ok(session) => session,
                Err(reason) => return reason,
            };
        }
        progress.send_replace(Progress::Finished);
        // Policy prompts still arrive; a dialog after the script ended is
        // answered as one before it would have been.
        for told in std::mem::take(&mut self.pending) {
            self.by_policy(told).await;
        }
        loop {
            match self.told.recv().await {
                None | Some(Told::Ended) => return "its session ended".to_owned(),
                Some(told) => self.by_policy(told).await,
            }
        }
    }

    /// Asks the client to log out and waits for its session to end.
    async fn leave(&mut self) -> String {
        if !self.send(Command::Logout).await {
            return "logged out".to_owned();
        }
        loop {
            match self.told.recv().await {
                None | Some(Told::Ended) => return "logged out".to_owned(),
                Some(_told) => {}
            }
        }
    }

    /// Waits for the avatar's login session `session` on the grid to see it
    /// arrive; the grid's stamp of the arrival and the session.
    async fn arrive(&self, session: u64) -> Result<(Instant, SharedSim), String> {
        let shared = self
            .core
            .session(session)
            .await
            .ok_or_else(|| "its login session is gone".to_owned())?;
        let mut events = shared.subscribe_events();
        let mut closed_rx = shared.closed_tx.subscribe();
        let mut shutdown_rx = shared.shutdown_rx.clone();
        let arrived = shared.state.lock().await.arrived_at;
        if arrived.is_none()
            && !crate::timeline::wait_for_arrival(&mut events, &mut closed_rx, &mut shutdown_rx)
                .await
        {
            return Err("it never arrived".to_owned());
        }
        let arrival = shared
            .state
            .lock()
            .await
            .arrived_at
            .unwrap_or_else(|| shared.now());
        Ok((arrival, shared))
    }

    /// Waits for a step's [`At`] against the avatar's session, answering by
    /// policy what arrives meanwhile unless `upcoming`'s first step answers it.
    async fn wait(
        &mut self,
        shared: &SharedSim,
        step: &AvatarStep,
        arrival: Instant,
        previous: Instant,
        upcoming: &[AvatarStep],
    ) -> Result<(), String> {
        let mut events = shared.subscribe_events();
        let mut closed_rx = shared.closed_tx.subscribe();
        let mut shutdown_rx = shared.shutdown_rx.clone();
        let core = Arc::clone(&self.core);
        let waiting = crate::timeline::wait_until(
            &core,
            shared,
            &step.at,
            arrival,
            previous,
            None,
            WaitChannels {
                events: &mut events,
                closed_rx: &mut closed_rx,
                shutdown_rx: &mut shutdown_rx,
            },
        );
        tokio::pin!(waiting);
        loop {
            tokio::select! {
                waited = &mut waiting => {
                    return match waited {
                        Wait::Ready => Ok(()),
                        Wait::Stop => Err("its session ended while it waited".to_owned()),
                    };
                }
                told = self.told.recv() => match told {
                    None | Some(Told::Ended) => return Err("its session ended".to_owned()),
                    Some(told) => self.route(told, upcoming).await,
                },
            }
        }
    }

    /// The next thing the client tells it that `wanted` takes, everything else
    /// routed as it comes; pending things first.
    async fn next<T>(
        &mut self,
        upcoming: &[AvatarStep],
        wanted: impl Fn(&Told) -> Option<T>,
    ) -> Result<T, String> {
        if let Some(position) = self.pending.iter().position(|told| wanted(told).is_some())
            && let Some(told) = self.pending.remove(position)
            && let Some(found) = wanted(&told)
        {
            return Ok(found);
        }
        loop {
            match self.told.recv().await {
                None | Some(Told::Ended) => return Err("its session ended".to_owned()),
                Some(told) => {
                    if let Some(found) = wanted(&told) {
                        return Ok(found);
                    }
                    self.route(told, upcoming).await;
                }
            }
        }
    }

    /// Keeps what the step about to run answers; answers the rest by policy.
    async fn route(&mut self, told: Told, upcoming: &[AvatarStep]) {
        let kept = upcoming.first().is_some_and(|step| {
            matches!(
                (&step.action, &told),
                (AvatarAction::AnswerDialog(_), Told::Dialog(_))
                    | (AvatarAction::AnswerPermissions(_), Told::Question(_))
                    | (AvatarAction::AwaitChat(_), Told::Chat(..))
            )
        });
        if kept {
            self.pending.push_back(told);
        } else {
            self.by_policy(told).await;
        }
    }

    /// Answers a prompt by the policy, writing it in the transcript; drops
    /// anything that is not a prompt.
    async fn by_policy(&self, told: Told) {
        match told {
            Told::Question(question) => {
                self.note(Entry::Asked {
                    object: question.object_name.clone(),
                    permissions: question.permissions,
                });
                let granted = match self.policy.permissions {
                    PermissionAnswer::Ignore => return,
                    PermissionAnswer::GrantAll => question.permissions,
                    PermissionAnswer::Refuse => ScriptPermissions::default(),
                };
                self.grant(&question, granted).await;
            }
            Told::Dialog(dialog) => {
                self.note(Entry::Dialog {
                    object: dialog.object_name.clone(),
                    message: dialog.message.clone(),
                    buttons: dialog.buttons.clone(),
                });
                if self.policy.dialogs == DialogAnswer::FirstButton && !is_text_box(&dialog) {
                    self.reply(&dialog, 0).await;
                }
            }
            Told::Chat(..)
            | Told::Sat(_)
            | Told::Alert(_)
            | Told::PayPrice(..)
            | Told::Money(..)
            | Told::Ended => {}
        }
    }

    /// Answers `question` granting `permissions` (only what it asked for).
    async fn grant(&self, question: &ScriptPermissionRequest, permissions: ScriptPermissions) {
        let permissions = ScriptPermissions(permissions.0 & question.permissions.0);
        if self
            .send(Command::AnswerScriptPermissions {
                task_id: question.task_id,
                item_id: question.item_id,
                permissions,
                experience_id: question.experience_id,
            })
            .await
        {
            self.note(Entry::Granted {
                object: question.object_name.clone(),
                permissions,
            });
        }
    }

    /// Answers `dialog` with the button at `index`.
    async fn reply(&self, dialog: &ScriptDialog, index: usize) {
        let Some(label) = dialog.buttons.get(index) else {
            return;
        };
        let Ok(button_index) = i32::try_from(index) else {
            return;
        };
        if self
            .send(Command::ReplyScriptDialog {
                object_id: dialog.object_id,
                chat_channel: dialog.chat_channel,
                button_index,
                button_label: label.clone(),
            })
            .await
        {
            self.note(Entry::Answered {
                object: dialog.object_name.clone(),
                button: label.clone(),
            });
        }
    }

    /// Finds `target` in the avatar's region.
    async fn resolve(&self, target: &Target) -> Result<Option<Resolved>, String> {
        let shared = self.session().await?;
        let world = Arc::clone(&shared.state.lock().await.world);
        let world = world.lock();
        let found = world
            .objects
            .iter()
            .find(|object| match target {
                Target::Key(key) => object.full_id == *key,
                Target::Named(name) => world
                    .properties_of(object.local_id)
                    .is_some_and(|properties| properties.name == *name),
            })
            .map(|object| Resolved {
                key: object.full_id,
                local_id: object.local_id,
                name: world
                    .properties_of(object.local_id)
                    .map(|properties| properties.name.clone())
                    .unwrap_or_default(),
                click: ClickAction::from_code(object.click_action),
            });
        drop(world);
        Ok(found)
    }

    /// Finds `target`, writing it in the transcript when the region has no
    /// such object.
    async fn find(&self, target: &Target) -> Result<Option<Resolved>, String> {
        let found = self.resolve(target).await?;
        if found.is_none() {
            self.note(Entry::NoSuchObject(match target {
                Target::Key(key) => key.uuid().to_string(),
                Target::Named(name) => name.clone(),
            }));
        }
        Ok(found)
    }

    /// Takes one action. An error is a session that ended under it.
    async fn act(&mut self, action: &AvatarAction, upcoming: &[AvatarStep]) -> Result<(), String> {
        let gone = || "its client has gone".to_owned();
        match action {
            AvatarAction::Walk { to } => self.walk(to, upcoming).await?,
            AvatarAction::Touch(target) => {
                let Some(object) = self.find(target).await? else {
                    return Ok(());
                };
                if object.click == ClickAction::Pay
                    && self.policy.payments == PaymentAnswer::DefaultPrice
                {
                    return self.pay(&object, None, upcoming).await;
                }
                if !self
                    .send(Command::TouchObject {
                        local_id: ScopedObjectId::new(self.circuit, object.local_id),
                        surface: None,
                    })
                    .await
                {
                    return Err(gone());
                }
                self.note(Entry::Touched(object.name));
            }
            AvatarAction::Sit(target) => {
                let Some(object) = self.find(target).await? else {
                    return Ok(());
                };
                if !self
                    .send(Command::Sit {
                        target: object.key,
                        offset: Vector {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    })
                    .await
                {
                    return Err(gone());
                }
                let key = object.key;
                let answer = self
                    .next(upcoming, |told| match told {
                        Told::Sat(on) if *on == key => Some(None),
                        Told::Alert(message) => Some(Some(message.clone())),
                        _other => None,
                    })
                    .await?;
                self.note(answer.map_or(Entry::Sat(object.name), Entry::SitRefused));
            }
            AvatarAction::Stand => {
                if !self.send(Command::Stand).await {
                    return Err(gone());
                }
                self.note(Entry::Stood);
            }
            AvatarAction::Say { channel, message } => {
                if !self
                    .send(Command::Chat {
                        message: message.clone(),
                        chat_type: ChatType::Normal,
                        channel: ChatChannel(*channel),
                    })
                    .await
                {
                    return Err(gone());
                }
                self.note(Entry::Said {
                    channel: *channel,
                    message: message.clone(),
                });
            }
            AvatarAction::Pay { target, amount } => {
                let Some(object) = self.find(target).await? else {
                    return Ok(());
                };
                self.pay(&object, *amount, upcoming).await?;
            }
            AvatarAction::AnswerDialog(choice) => {
                let dialog = self
                    .next(upcoming, |told| match told {
                        Told::Dialog(dialog) => Some(dialog.clone()),
                        _other => None,
                    })
                    .await?;
                self.note(Entry::Dialog {
                    object: dialog.object_name.clone(),
                    message: dialog.message.clone(),
                    buttons: dialog.buttons.clone(),
                });
                self.answer(&dialog, choice).await?;
            }
            AvatarAction::AnswerPermissions(permissions) => {
                let question = self
                    .next(upcoming, |told| match told {
                        Told::Question(question) => Some(question.clone()),
                        _other => None,
                    })
                    .await?;
                self.note(Entry::Asked {
                    object: question.object_name.clone(),
                    permissions: question.permissions,
                });
                self.grant(&question, *permissions).await;
            }
            AvatarAction::AwaitChat(wanted) => {
                let (from, message) = self
                    .next(upcoming, |told| match told {
                        Told::Chat(from, message) if message.contains(wanted.as_str()) => {
                            Some((from.clone(), message.clone()))
                        }
                        _other => None,
                    })
                    .await?;
                self.note(Entry::Heard { from, message });
                // Lines kept for this step that it did not want are nobody's.
                self.pending.retain(|told| !matches!(told, Told::Chat(..)));
            }
        }
        Ok(())
    }

    /// Answers `dialog` as `choice` says.
    async fn answer(&self, dialog: &ScriptDialog, choice: &DialogChoice) -> Result<(), String> {
        let index = match choice {
            DialogChoice::Index(index) => Some(*index),
            DialogChoice::Label(label) => dialog.buttons.iter().position(|button| button == label),
            DialogChoice::Text(text) => {
                if !self
                    .send(Command::ReplyScriptDialog {
                        object_id: dialog.object_id,
                        chat_channel: dialog.chat_channel,
                        button_index: 0,
                        button_label: text.clone(),
                    })
                    .await
                {
                    return Err("its client has gone".to_owned());
                }
                self.note(Entry::Answered {
                    object: dialog.object_name.clone(),
                    button: text.clone(),
                });
                return Ok(());
            }
        };
        match index.filter(|index| *index < dialog.buttons.len()) {
            Some(index) => self.reply(dialog, index).await,
            None => self.note(Entry::NoSuchButton {
                object: dialog.object_name.clone(),
                choice: match choice {
                    DialogChoice::Index(index) => index.to_string(),
                    DialogChoice::Label(label) | DialogChoice::Text(label) => label.clone(),
                },
            }),
        }
        Ok(())
    }

    /// Pays `object` `amount`, or what its pay window offers first, and waits
    /// for the grid's answer.
    async fn pay(
        &mut self,
        object: &Resolved,
        amount: Option<i32>,
        upcoming: &[AvatarStep],
    ) -> Result<(), String> {
        let gone = || "its client has gone".to_owned();
        let amount = match amount {
            Some(amount) => amount,
            None => {
                if !self
                    .send(Command::RequestPayPrice {
                        object_id: object.key,
                    })
                    .await
                {
                    return Err(gone());
                }
                let key = object.key;
                let (default, buttons) = self
                    .next(upcoming, |told| match told {
                        Told::PayPrice(paid, default, buttons) if *paid == key => {
                            Some((*default, buttons.clone()))
                        }
                        _other => None,
                    })
                    .await?;
                let offered = std::iter::once(default)
                    .chain(buttons)
                    .find(|amount| *amount > 0);
                let Some(offered) = offered else {
                    self.note(Entry::NothingToPay(object.name.clone()));
                    return Ok(());
                };
                offered
            }
        };
        let Ok(linden) = u64::try_from(amount) else {
            self.note(Entry::NothingToPay(object.name.clone()));
            return Ok(());
        };
        if !self
            .send(Command::SendMoneyTransfer {
                dest: object.key.uuid(),
                amount: LindenAmount(linden),
                kind: MoneyTransactionType::PayObject,
                description: object.name.clone(),
            })
            .await
        {
            return Err(gone());
        }
        let (success, reason) = self
            .next(upcoming, |told| match told {
                Told::Money(success, reason) => Some((*success, reason.clone())),
                _other => None,
            })
            .await?;
        self.note(if success {
            Entry::Paid {
                object: object.name.clone(),
                amount,
            }
        } else {
            Entry::PaymentRefused {
                object: object.name.clone(),
                amount,
                reason,
            }
        });
        Ok(())
    }

    /// Walks to `to`: faces it, holds the forward key, and checks each tick
    /// whether it is there or has stopped getting closer.
    async fn walk(&mut self, to: &Vector, upcoming: &[AvatarStep]) -> Result<(), String> {
        let gone = || "its client has gone".to_owned();
        let from = self.position().await?;
        let facing = facing(&from, to);
        if !self
            .send(Command::SetRotation {
                body: facing.clone(),
                head: facing,
            })
            .await
            || !self
                .send(Command::SetControls(sl_proto::ControlFlags::AT_POS))
                .await
        {
            return Err(gone());
        }
        let mut best = horizontal_distance(&from, to);
        let mut stalled = 0_u32;
        let reached = loop {
            let shared = self.session().await?;
            let arrival = shared.now();
            let step = AvatarStep {
                at: At::AfterTicks(1),
                action: AvatarAction::Stand,
            };
            self.wait(&shared, &step, arrival, arrival, upcoming)
                .await?;
            let now = horizontal_distance(&self.position().await?, to);
            if now <= WALK_ARRIVAL_M {
                break true;
            }
            if now < best {
                best = now;
                stalled = 0;
            } else {
                stalled = stalled.saturating_add(1);
                if stalled >= WALK_STALL_TICKS {
                    break false;
                }
            }
        };
        if !self
            .send(Command::SetControls(sl_proto::ControlFlags::empty()))
            .await
        {
            return Err(gone());
        }
        self.note(Entry::Walked {
            to: to.clone(),
            reached,
        });
        Ok(())
    }

    /// Where the grid has the avatar now.
    async fn position(&self) -> Result<Vector, String> {
        let shared = self.session().await?;
        let at = shared.state.lock().await.sim.arrival_position().position;
        Ok(Vector {
            x: at.x(),
            y: at.y(),
            z: at.z(),
        })
    }
}

/// Whether `dialog` is a text box rather than a choice of buttons.
fn is_text_box(dialog: &ScriptDialog) -> bool {
    dialog.buttons.len() == 1
        && dialog
            .buttons
            .first()
            .is_some_and(|button| button == ScriptDialog::TEXT_BOX_BUTTON)
}

/// The turn about the vertical that faces `to` from `from`.
fn facing(from: &Vector, to: &Vector) -> Rotation {
    let yaw = (to.y - from.y).atan2(to.x - from.x);
    let half = yaw / 2.0;
    Rotation {
        x: 0.0,
        y: 0.0,
        z: half.sin(),
        s: half.cos(),
    }
}

/// How far apart `a` and `b` are, ignoring height.
fn horizontal_distance(a: &Vector, b: &Vector) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A turn facing along +Y is a quarter turn about Z.
    #[test]
    fn facing_north_is_a_quarter_turn() {
        let turn = facing(
            &Vector {
                x: 10.0,
                y: 10.0,
                z: 0.0,
            },
            &Vector {
                x: 10.0,
                y: 20.0,
                z: 5.0,
            },
        );
        let half = std::f32::consts::FRAC_PI_4;
        assert!((turn.z - half.sin()).abs() < 1e-6, "{turn:?}");
        assert!((turn.s - half.cos()).abs() < 1e-6, "{turn:?}");
    }

    /// A transcript line names the object and the answer, and a refusal reads
    /// as one.
    #[test]
    fn transcript_lines_read_as_what_happened() {
        assert_eq!(
            Entry::Granted {
                object: "Pose ball".to_owned(),
                permissions: ScriptPermissions(0x10),
            }
            .to_string(),
            "granted Pose ball permissions 0x10"
        );
        assert_eq!(
            Entry::Granted {
                object: "Pose ball".to_owned(),
                permissions: ScriptPermissions(0),
            }
            .to_string(),
            "refused Pose ball its permissions"
        );
        assert_eq!(
            Entry::Answered {
                object: "Menu".to_owned(),
                button: "Blue".to_owned(),
            }
            .to_string(),
            "answered Menu with \"Blue\""
        );
    }

    /// The builder keeps its steps in order, `after_ticks` being the tick
    /// spelling of a step.
    #[test]
    fn a_script_keeps_the_order_it_was_written_in() {
        let avatar = ScriptedAvatar::new("Scripted", "Sitter")
            .after_ticks(1, AvatarAction::Sit(Target::Named("Pose ball".to_owned())))
            .after_ticks(5, AvatarAction::Stand);
        assert_eq!(avatar.name(), "Scripted Sitter");
        assert_eq!(avatar.steps.len(), 2);
        assert!(matches!(
            avatar.steps.first().map(|step| &step.at),
            Some(At::AfterTicks(1))
        ));
        assert_eq!(
            avatar.steps.get(1).map(|step| &step.action),
            Some(&AvatarAction::Stand)
        );
    }

    /// Only a lone text-box button makes a dialog a text box.
    #[test]
    fn a_text_box_is_told_from_a_dialog() {
        let mut dialog = ScriptDialog {
            object_id: ObjectKey::from(uuid::Uuid::nil()),
            object_name: String::new(),
            owner_first_name: String::new(),
            owner_last_name: String::new(),
            owner_id: None,
            message: String::new(),
            chat_channel: ChatChannel(-1),
            image_id: sl_types::key::TextureKey::from(uuid::Uuid::nil()),
            buttons: vec![ScriptDialog::TEXT_BOX_BUTTON.to_owned()],
        };
        assert!(is_text_box(&dialog));
        dialog.buttons.push("OK".to_owned());
        assert!(!is_text_box(&dialog));
    }
}
