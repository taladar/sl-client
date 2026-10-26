//! Run the LSL probe corpus on a live grid and keep what each probe printed.
//!
//! The corpus is `sl-lsl-runtime/probes/` ([`sl_lsl_runtime::probe`] is its
//! format, the book's `tools/lsl-probes.md` its conventions). For each probe,
//! in id order, the case:
//!
//! 1. rezzes a cube per prim the probe names — prim A against the reference
//!    primitive, every other prim the `distance` its scripts' header gives
//!    east of A;
//! 2. makes each script in its prim with [`Command::RezScript`] under the
//!    name its header gives (a single-script probe's is `Probe`), in the
//!    header's save order, and uploads the source running — on OpenSim with
//!    the first line that picks the engine (`//YEngine:` or `//XEngine:`).
//!    The upload names the compile target the header gives (`target`, Mono
//!    unless it says `lsl2`), which OpenSim ignores.
//!    A script whose `skip-on` names this grid is left out, with a note;
//! 3. answers each `waiting` record the probe prints with the stimulus it
//!    names ([`Stimulus`]): a touch of prim A, a line of chat on a channel,
//!    an answer to the permission question or the dialog the probe's script
//!    puts, a sit on prim A, a stand, or movement controls held down for a
//!    moment, from the owner or from a second avatar, or the owner leaving
//!    the region for a while, attaching prim A from the ground, dropping it,
//!    or taking it and rezzing it back (when it takes a new key), or the
//!    region restarting — asked for by the estate owner the runner keeps on
//!    call, after which both avatars log in again — one the probe's header
//!    declares among its `drive`s;
//! 4. keeps every owner-say line of the probe's (`PROBE <id> …`) from its
//!    prims, and every `DEBUG_CHANNEL` message they shout as a `#` note,
//!    until the probe prints `end` — and a little longer, for a second
//!    prim's late lines — or [`PROBE_DEADLINE`] passes. What the grid sends
//!    the two viewers about the avatars meanwhile is noted too: permission
//!    questions, dialogs, instant messages and chat from the probe's prims,
//!    sit responses, control changes, follow cameras, animation lists and
//!    alerts, with both avatars' names put as placeholders;
//! 5. derezzes its prims and writes the transcript as a
//!    [`ResultFile`] to
//!    `sl-lsl-runtime/probes/results/<grid>/<id>.txt`.
//!
//! The case collects; it never judges what a probe printed. It fails only
//! when it could not run the corpus (a refused rez, a script the grid did
//! not name as asked); a probe that never printed `end` is written with
//! `complete: no`, and a script that does not compile is noted in its file.
//!
//! Two cases, one body: `lsl-probes` (Second Life's Mono, OpenSim's YEngine)
//! and `lsl-probes-xengine` (OpenSim's XEngine). `2av`: the secondary is
//! the other avatar a probe can ask to touch or speak, and is only that.
//! `--probe <id>` runs only the probes it names. On Second Life the case
//! needs the fixtures' build location, as every case that rezzes does.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AssetType, AttachmentMode, AttachmentPoint, ChatAudible, ChatChannel, ChatMessage,
    ChatSource, ChatType, Command, ControlFlags, DeRezDestination, DisconnectReason, Event,
    FolderType, InstantMessage, InventoryFolder, InventoryFolderKey, InventoryKey, InventoryType,
    LindenAmount, MoneyBalance, MoneyTransactionType, Object, ObjectKey, OwnerKey, PrimShape,
    RegionHandle, RegionLocalObjectId, RestoreItem, RezScriptParams, Rotation, ScopedObjectId,
    ScriptDialog, ScriptPermissionRequest, ScriptPermissions, ScriptTarget, ScriptUploadLocation,
    SimStatId, SurfaceInfo, TaskInventoryItem, TransactionId, Uuid, Vector, XferListing,
};
use sl_lsl_runtime::probe::{
    Action, By, COMPILE_ERROR_NOTE, Drive, Fetch, Line, PREFIX, ResultFile, Stimulus, Target,
    TouchSurface,
};

use super::object_rez_derez::rez_params;
use crate::context::{Session, StandbyLogin, TestContext, TestFailure, now_rfc3339};
use crate::grid::Grid;
use crate::probe_corpus::{Probe, corpus_root, load_corpus, result_path, select};
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, created_item_announcement_observing, is_opensim, is_own,
    settle_scene, settle_scene_observing, settle_scene_with_avatar, wait_for_own_new_object,
    walk_within_region,
};
use crate::teleport_trace::{other_region, request_teleport};

/// The OpenSim start location: the "Default Region" (1000,1000), centred,
/// whose test prim is the rez placement reference. On Second Life the avatar
/// keeps `"last"`, unless the fixtures name a build location.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long each step's confirming event may take: a rez, a listing, a
/// compile, a derez.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a probe may run before its transcript is written as
/// incomplete. The longest probe in the corpus, `http/outbound`, runs about
/// a minute and a half.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(180);

/// How long the case keeps listening after a probe's `end`, for a second
/// prim's lines that are still on their way.
const LINGER: Duration = Duration::from_secs(3);

/// How far above the reference primitive prim A is rezzed, in metres.
const LIFT_M: f32 = 1.0;

/// The script engine (virtual machine) a run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vm {
    /// Second Life's Mono.
    Mono,
    /// OpenSim's YEngine.
    YEngine,
    /// OpenSim's XEngine.
    XEngine,
}

impl Vm {
    /// The engine a case runs on `grid`.
    const fn on(grid: Grid, xengine: bool) -> Self {
        match (is_opensim(grid), xengine) {
            (false, _) => Self::Mono,
            (true, false) => Self::YEngine,
            (true, true) => Self::XEngine,
        }
    }

    /// The first line that picks this engine for a script on OpenSim.
    const fn source_prefix(self) -> &'static str {
        match self {
            Self::Mono => "",
            Self::YEngine => "//YEngine:\n",
            Self::XEngine => "//XEngine:\n",
        }
    }

    /// Its name in a result header.
    const fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::YEngine => "YEngine",
            Self::XEngine => "XEngine",
        }
    }

    /// The results directory for this engine.
    const fn results_grid(self) -> &'static str {
        match self {
            Self::Mono => "aditi",
            Self::YEngine => "opensim-yengine",
            Self::XEngine => "opensim-xengine",
        }
    }
}

/// Runs the corpus under Second Life's Mono or OpenSim's YEngine.
#[derive(Debug)]
pub struct LslProbes;

impl GridTest for LslProbes {
    fn name(&self) -> &'static str {
        "lsl-probes"
    }

    fn description(&self) -> &'static str {
        "Run the LSL probe corpus and write each probe's transcript (Mono / YEngine)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn selects_probes(&self) -> bool {
        true
    }

    fn estate_on_call(&self) -> bool {
        true
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        start_location(grid)
    }

    fn timeout(&self) -> Duration {
        CORPUS_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(run_corpus(ctx, false))
    }
}

/// Runs the corpus under OpenSim's XEngine.
#[derive(Debug)]
pub struct XEngineProbes;

impl GridTest for XEngineProbes {
    fn name(&self) -> &'static str {
        "lsl-probes-xengine"
    }

    fn description(&self) -> &'static str {
        "Run the LSL probe corpus and write each probe's transcript (XEngine)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn selects_probes(&self) -> bool {
        true
    }

    fn estate_on_call(&self) -> bool {
        true
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        start_location(grid)
    }

    fn timeout(&self) -> Duration {
        CORPUS_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(run_corpus(ctx, true))
    }
}

/// The whole corpus's budget: every probe's deadline, with room for the
/// rezzes and uploads.
const CORPUS_TIMEOUT: Duration = Duration::from_secs(60 * 30);

/// The instant `wait` from now (now itself, were that past the clock's
/// range).
fn after(wait: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(wait).unwrap_or(now)
}

/// Where both cases log in.
const fn start_location(grid: Grid) -> &'static str {
    if is_opensim(grid) {
        OPENSIM_START
    } else {
        "last"
    }
}

/// Who the two avatars are, so that a note can say which one a thing is
/// about, and never name either: results are committed.
#[derive(Debug, Clone, Default)]
struct Cast {
    /// The owner's agent id.
    owner: Option<AgentKey>,
    /// The other avatar's agent id.
    other: Option<AgentKey>,
    /// The owner's first and last name.
    owner_name: (String, String),
    /// The other avatar's first and last name.
    other_name: (String, String),
    /// The on-call estate owner's first and last name, which the notices of
    /// a restart it asks for may carry.
    estate_name: (String, String),
}

impl Cast {
    /// Which avatar `agent` is, as a note says it.
    fn role(&self, agent: AgentKey) -> &'static str {
        if Some(agent) == self.owner {
            "owner"
        } else if Some(agent) == self.other {
            "other"
        } else {
            "someone"
        }
    }

    /// `text` with both avatars' names put as placeholders (`<first>`,
    /// `<last>` for the owner's, `<other-first>`, `<other-last>` for the
    /// other's), matched without regard to case: what a note may hold of
    /// something a grid wrote.
    fn scrub(&self, text: &str) -> String {
        let mut scrubbed = text.to_owned();
        for (name, placeholder) in [
            (&self.owner_name.0, "<first>"),
            (&self.owner_name.1, "<last>"),
            (&self.other_name.0, "<other-first>"),
            (&self.other_name.1, "<other-last>"),
            (&self.estate_name.0, "<estate-first>"),
            (&self.estate_name.1, "<estate-last>"),
        ] {
            scrubbed = replace_ignoring_case(&scrubbed, name, placeholder);
        }
        scrubbed
    }
}

/// `text` with every occurrence of `name` as a whole word, in any ASCII
/// case, replaced by `placeholder`; `text` itself for an empty `name`.
fn replace_ignoring_case(text: &str, name: &str, placeholder: &str) -> String {
    if name.is_empty() {
        return text.to_owned();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_name = name.to_ascii_lowercase();
    let is_word = |c: char| c.is_alphanumeric();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0_usize;
    let mut from = 0_usize;
    while let Some(found) = lower_text
        .get(from..)
        .and_then(|rest| rest.find(&lower_name))
    {
        let at = from.saturating_add(found);
        let end = at.saturating_add(lower_name.len());
        let before = lower_text
            .get(..at)
            .and_then(|head| head.chars().next_back());
        let after = lower_text.get(end..).and_then(|tail| tail.chars().next());
        if before.is_some_and(is_word) || after.is_some_and(is_word) {
            from = at.saturating_add(lower_name.chars().next().map_or(1, char::len_utf8));
            continue;
        }
        out.push_str(text.get(copied..at).unwrap_or_default());
        out.push_str(placeholder);
        copied = end;
        from = end;
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// What one probe's run produced.
#[derive(Debug, Default)]
struct Transcript {
    /// The `PROBE` lines and `#` notes, in arrival order.
    body: Vec<String>,
    /// Whether the probe printed its `end`.
    ended: bool,
    /// The region its `start` record named.
    region: Option<String>,
    /// Its `waiting` records, in order.
    waits: Vec<Line>,
    /// Who the avatars are.
    cast: Cast,
    /// The permission questions the probe's scripts put to the owner, not
    /// yet answered, oldest first.
    questions: VecDeque<ScriptPermissionRequest>,
    /// The permission questions they put to the other avatar, likewise.
    other_questions: VecDeque<ScriptPermissionRequest>,
    /// The dialogs the probe's scripts put to the owner, not yet replied
    /// to, oldest first.
    dialogs: VecDeque<ScriptDialog>,
    /// The dialogs they put to the other avatar, likewise.
    other_dialogs: VecDeque<ScriptDialog>,
    /// The animation list last noted for each avatar.
    animations: HashMap<AgentKey, String>,
    /// The keys prim A took on beside the probe's own when a stimulus gave it
    /// a new one (taken and rezzed again, or a grid that re-keys an
    /// attachment): its lines are the probe's too.
    extra: HashSet<ObjectKey>,
    /// The newest object update of each of the probe's prims, by key: where
    /// it is now, and whether it is worn.
    latest: HashMap<ObjectKey, Object>,
    /// Where the grid last put the owner by a teleport inside the region (a
    /// script's `llTeleportAgent`), which may be far from the build location.
    owner_moved: Option<Vector>,
    /// Where it last put the other avatar, likewise.
    other_moved: Option<Vector>,
    /// Whether the probe asked for the region's active-script count
    /// (`stats: active-scripts`).
    notes_active_scripts: bool,
    /// The active-script count last noted.
    active_scripts: Option<f32>,
    /// Each avatar's L$ balance as the grid last told it (by role), so that
    /// a note can say how a balance changed and never what it is.
    balances: HashMap<&'static str, u64>,
}

impl Transcript {
    /// An empty transcript for the avatars `cast`.
    fn new(cast: Cast) -> Self {
        Self {
            cast,
            ..Self::default()
        }
    }

    /// Whether `object` is one of the probe's prims: one of `prims`, or a key
    /// prim A took on since.
    fn is_probe(&self, prims: &HashSet<ObjectKey>, object: ObjectKey) -> bool {
        prims.contains(&object) || self.extra.contains(&object)
    }

    /// What `object` is to a note: one of the probe's prims (`probe`), or
    /// another object.
    fn object_role(&self, prims: &HashSet<ObjectKey>, object: ObjectKey) -> &'static str {
        if self.is_probe(prims, object) {
            "probe"
        } else {
            "other-object"
        }
    }

    /// Notes a permission question from one of the probe's prims, put to
    /// the avatar `by` names (`""` for the owner, `" by=other"`), and keeps
    /// it for the answer.
    fn question(&mut self, by: &str, request: &ScriptPermissionRequest) {
        let experience = if request.experience_id.is_some() {
            "set"
        } else {
            "none"
        };
        self.body.push(format!(
            "# question{by} permissions={} object={} owner={} experience={experience}",
            request.permissions.0,
            quote(&self.cast.scrub(&request.object_name)),
            quote(&self.cast.scrub(&request.object_owner)),
        ));
        if by.is_empty() {
            self.questions.push_back(request.clone());
        } else {
            self.other_questions.push_back(request.clone());
        }
    }

    /// Notes an alert the grid sent the avatar `by` names.
    fn alert(&mut self, by: &str, event: &Event) {
        match event {
            Event::AlertMessage { message, .. } => self.body.push(format!(
                "# alert{by} text={}",
                quote(&self.cast.scrub(message))
            )),
            Event::AgentAlertMessage { modal, message, .. } => self.body.push(format!(
                "# agent-alert{by} modal={} text={}",
                u8::from(*modal),
                quote(&self.cast.scrub(message))
            )),
            _ => {}
        }
    }

    /// Notes a teleport message the grid sent the avatar `by` names — a
    /// script's `llTeleportAgent` moves an avatar without its asking: the
    /// start, a teleport inside the region and a failure, each with its flags
    /// (a landing position would differ between build locations).
    fn teleport(&mut self, by: &str, event: &Event) {
        match event {
            Event::TeleportStarted { flags } => self
                .body
                .push(format!("# teleport-started{by} flags={:#x}", flags.0)),
            Event::TeleportLocal {
                flags, position, ..
            } => {
                let landed = Some(Vector {
                    x: position.x(),
                    y: position.y(),
                    z: position.z(),
                });
                if by.is_empty() {
                    self.owner_moved = landed;
                } else {
                    self.other_moved = landed;
                }
                self.body
                    .push(format!("# teleport-local{by} flags={:#x}", flags.0));
            }
            Event::TeleportFailed { reason, .. } => self.body.push(format!(
                "# teleport-failed{by} reason={}",
                quote(&self.cast.scrub(reason))
            )),
            _ => {}
        }
    }

    /// Notes a dialog from one of the probe's prims, put to the avatar `by`
    /// names, and keeps it for the reply.
    fn dialog(&mut self, by: &str, dialog: &ScriptDialog) {
        let owner = format!("{} {}", dialog.owner_first_name, dialog.owner_last_name);
        self.body.push(format!(
            "# dialog{by} object={} owner={} channel={} message_length={} message={} \
             buttons={} count={}",
            quote(&self.cast.scrub(&dialog.object_name)),
            quote(&self.cast.scrub(owner.trim())),
            dialog.chat_channel.0,
            dialog.message.len(),
            quote(&clip(&self.cast.scrub(&dialog.message))),
            quote(&dialog.buttons.join("|")),
            dialog.buttons.len()
        ));
        if by.is_empty() {
            self.dialogs.push_back(dialog.clone());
        } else {
            self.other_dialogs.push_back(dialog.clone());
        }
    }

    /// Notes an instant message an object sent the avatar `by` names
    /// (`llInstantMessage`'s dialog 19), with who the grid says sent it and
    /// what its id is, as roles.
    fn instant_message(&mut self, by: &str, prims: &HashSet<ObjectKey>, im: &InstantMessage) {
        let from_prim = self.is_probe(prims, ObjectKey::from(im.from_agent_id.uuid()));
        let id_prim = self.is_probe(prims, ObjectKey::from(im.id));
        let dialog = im.dialog.to_u8();
        if dialog != MESSAGE_FROM_OBJECT && !from_prim && !id_prim {
            return;
        }
        let role = |is_prim: bool, id: Uuid| {
            if is_prim {
                "probe"
            } else if id.is_nil() {
                "nil"
            } else {
                self.cast.role(AgentKey::from(id))
            }
        };
        let note = format!(
            "# im{by} dialog={dialog} from={} id={} name={} offline={} length={} message={} \
             bucket={}",
            role(from_prim, im.from_agent_id.uuid()),
            role(id_prim, im.id),
            quote(&self.cast.scrub(&im.from_agent_name)),
            u8::from(im.offline),
            im.message.len(),
            quote(&clip(&self.cast.scrub(&im.message))),
            im.binary_bucket.len()
        );
        self.body.push(note);
    }

    /// Notes a line of chat from one of the probe's prims that is no probe
    /// line, heard by the avatar `by` names: what an avatar is sent of a
    /// script's speech.
    fn heard(&mut self, by: &str, chat: &ChatMessage) {
        let note = if chat.chat_type == ChatType::Owner && by.is_empty() {
            format!(
                "# owner-say: from={} text={}",
                quote(&chat.from_name),
                quote(&self.cast.scrub(&chat.message))
            )
        } else {
            format!(
                "# chat{by} type={} from={} length={} text={}",
                chat_type_name(chat.chat_type),
                quote(&chat.from_name),
                chat.message.len(),
                quote(&clip(&self.cast.scrub(&chat.message)))
            )
        };
        self.body.push(note);
    }

    /// Notes a `DEBUG_CHANNEL` message from one of the probe's prims, as the
    /// avatar `by` names was sent it: a script error, a warning, or what a
    /// script said on that channel.
    fn debug_channel(&mut self, by: &str, chat: &ChatMessage) {
        // A line the listener could not hear comes with its text left out,
        // which a note must not read as an empty message.
        let audible = match chat.audible {
            ChatAudible::Fully => String::new(),
            ChatAudible::Barely => " audible=barely".to_owned(),
            ChatAudible::Not => " audible=not".to_owned(),
            ChatAudible::Unknown(byte) => format!(" audible={byte}"),
            _ => " audible=other".to_owned(),
        };
        self.body.push(format!(
            "# debug-channel{by}: from={} text={}{audible}",
            quote(&chat.from_name),
            quote(&self.cast.scrub(&chat.message))
        ));
    }

    /// What `id` — a transaction's source or destination — is to a note:
    /// one of the avatars, one of the probe's prims, `nil`, or `someone`.
    fn party(&self, prims: &HashSet<ObjectKey>, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nil"
        } else if self.is_probe(prims, ObjectKey::from(id)) {
            "probe"
        } else {
            self.cast.role(AgentKey::from(id))
        }
    }

    /// Notes a balance the grid sent the avatar `by` names, when it changed
    /// or names a transaction: how much it changed by (`unknown` before a
    /// first balance is known), never what it is, and the transaction's
    /// type, parties, amount and description. A first balance that names no
    /// transaction is kept quietly, as what later changes are measured from.
    fn money(&mut self, by: &str, prims: &HashSet<ObjectKey>, balance: &MoneyBalance) {
        let role = if by.is_empty() { "owner" } else { "other" };
        let LindenAmount(now) = balance.balance;
        let before = self.balances.insert(role, now);
        let change = before.map(|before| i128::from(now).saturating_sub(i128::from(before)));
        if balance.transaction.is_none() && change.is_none_or(|change| change == 0) {
            return;
        }
        let change = change.map_or_else(|| "unknown".to_owned(), |change| change.to_string());
        let transaction = balance
            .transaction
            .as_ref()
            .map_or_else(String::new, |transaction| {
                let uuid = |owner: &OwnerKey| match owner {
                    OwnerKey::Agent(agent) => agent.uuid(),
                    OwnerKey::Group(group) => group.uuid(),
                };
                let LindenAmount(amount) = transaction.amount;
                format!(
                    " type={} source={} dest={} amount={amount} item={}",
                    transaction.transaction_type,
                    self.party(prims, uuid(&transaction.source)),
                    self.party(prims, uuid(&transaction.dest)),
                    quote(&self.cast.scrub(&transaction.item_description))
                )
            });
        self.body.push(format!(
            "# money-balance{by} success={} change={change}{transaction} description={}",
            u8::from(balance.success),
            quote(&self.cast.scrub(&balance.description))
        ));
    }

    /// Takes one event of the other avatar's session: the permission
    /// questions and dialogs from the probe's prims (kept for an answer by
    /// that avatar), what it hears of their chat (`DEBUG_CHANNEL` too) and
    /// instant messages, and alerts. Its other events are dropped.
    fn take_other(&mut self, prims: &HashSet<ObjectKey>, event: &Event) {
        const BY: &str = " by=other";
        match event {
            Event::ScriptPermissionRequest(request) if self.is_probe(prims, request.task_id) => {
                self.question(BY, request);
            }
            Event::ScriptDialog(dialog) if self.is_probe(prims, dialog.object_id) => {
                self.dialog(BY, dialog);
            }
            Event::InstantMessageReceived(im) => self.instant_message(BY, prims, im),
            Event::MoneyBalance(balance) => self.money(BY, prims, balance),
            Event::ChatReceived(chat) => match chat.source {
                ChatSource::Object(source) if self.is_probe(prims, source) => {
                    if chat.chat_type == ChatType::DebugChannel {
                        self.debug_channel(BY, chat);
                    } else {
                        self.heard(BY, chat);
                    }
                }
                _ => {}
            },
            Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                self.alert(BY, event);
            }
            Event::TeleportStarted { .. }
            | Event::TeleportLocal { .. }
            | Event::TeleportFailed { .. } => self.teleport(BY, event),
            _ => {}
        }
    }

    /// Takes one event of the owner's session: a probe line, a
    /// `DEBUG_CHANNEL` message or another owner-say line from one of the
    /// probe's prims, or what the grid sent the owner's viewer about the
    /// avatars meanwhile — a permission question or a dialog from the prims,
    /// an object's instant message, a control change, a follow camera, an
    /// animation list that changed, an alert — which it notes; and an update
    /// of one of the prims, which it keeps as that prim's newest. Returns the
    /// record name of a probe line it kept.
    fn take(&mut self, id: &str, prims: &HashSet<ObjectKey>, event: &Event) -> Option<String> {
        match event {
            Event::SimStats(stats) if self.notes_active_scripts => {
                let count = stats
                    .stats
                    .iter()
                    .find(|(stat, _)| *stat == SimStatId::ActiveScripts)
                    .map(|(_, value)| *value);
                if count.is_some() && count != self.active_scripts {
                    self.active_scripts = count;
                    if let Some(count) = count {
                        self.body
                            .push(format!("# sim-stats active_scripts={count}"));
                    }
                }
                return None;
            }
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if self.is_probe(prims, object.full_id) =>
            {
                let _previous = self.latest.insert(object.full_id, (**object).clone());
                return None;
            }
            Event::ScriptPermissionRequest(request) if self.is_probe(prims, request.task_id) => {
                self.question("", request);
                return None;
            }
            Event::ScriptDialog(dialog) if self.is_probe(prims, dialog.object_id) => {
                self.dialog("", dialog);
                return None;
            }
            Event::InstantMessageReceived(im) => {
                self.instant_message("", prims, im);
                return None;
            }
            Event::MoneyBalance(balance) => {
                self.money("", prims, balance);
                return None;
            }
            Event::ScriptControlChange(changes) => {
                for change in changes {
                    self.body.push(format!(
                        "# script-control-change action={} controls={} pass_to_agent={}",
                        if change.action.takes_controls() {
                            "take"
                        } else {
                            "release"
                        },
                        change.controls.bits(),
                        u8::from(change.pass_to_agent)
                    ));
                }
                return None;
            }
            Event::SetFollowCamProperties {
                object_id,
                properties,
            } => {
                let values: Vec<String> = properties
                    .iter()
                    .map(|value| format!("{}={}", value.property.to_i32(), value.value))
                    .collect();
                self.body.push(format!(
                    "# follow-cam object={} {}",
                    self.object_role(prims, *object_id),
                    values.join(" ")
                ));
                return None;
            }
            Event::ClearFollowCamProperties { object_id } => {
                self.body.push(format!(
                    "# follow-cam-clear object={}",
                    self.object_role(prims, *object_id)
                ));
                return None;
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } => {
                let role = self.cast.role(*avatar_id);
                if role == "someone" {
                    return None;
                }
                // Only the built-in animations: an uploaded one is what the
                // account wears (an animation overrider), which changes by
                // itself and is no grid's behaviour. A source that is the
                // avatar itself is no object's.
                let mut playing: Vec<String> = animations
                    .iter()
                    .filter_map(|animation| {
                        let name = sl_anim::builtin_animation(animation.anim_id)?.name;
                        Some(match animation.source_id {
                            Some(source) if source.0 != avatar_id.0 => {
                                format!("{name}@{}", self.object_role(prims, source))
                            }
                            Some(_) | None => name.to_owned(),
                        })
                    })
                    .collect();
                playing.sort();
                let playing = playing.join(",");
                if self.animations.get(avatar_id) != Some(&playing) {
                    self.body.push(format!(
                        "# animations avatar={role} playing={}",
                        quote(&playing)
                    ));
                    let _previous = self.animations.insert(*avatar_id, playing);
                }
                return None;
            }
            Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                self.alert("", event);
                return None;
            }
            Event::TeleportStarted { .. }
            | Event::TeleportLocal { .. }
            | Event::TeleportFailed { .. } => {
                self.teleport("", event);
                return None;
            }
            _ => {}
        }
        let Event::ChatReceived(chat) = event else {
            return None;
        };
        let ChatSource::Object(source) = chat.source else {
            return None;
        };
        if !self.is_probe(prims, source) {
            return None;
        }
        if chat.chat_type == ChatType::DebugChannel {
            self.debug_channel("", chat);
            return None;
        }
        let line = chat.message.trim_end();
        if chat.chat_type != ChatType::Owner || !line.starts_with(&format!("{PREFIX}{id} ")) {
            self.heard("", chat);
            return None;
        }
        let Ok(parsed) = Line::parse(line) else {
            self.body.push(line.to_owned());
            return None;
        };
        self.body.push(shown_line(line, &parsed));
        if parsed.rec == "start" && self.region.is_none() {
            self.region = parsed.field("region").map(str::to_owned);
        }
        if parsed.rec == "end" {
            self.ended = true;
        }
        let rec = parsed.rec.clone();
        if rec == "waiting" {
            self.waits.push(parsed);
        }
        Some(rec)
    }
}

/// `line` as a transcript keeps it: a `fetch` record's URL, which names the
/// simulator's host and changes with every run, is put as `<url>`.
fn shown_line(line: &str, parsed: &Line) -> String {
    match parsed.field("url") {
        Some(url) if parsed.rec == "waiting" && parsed.field("drive") == Some("fetch") => {
            line.replace(url, "<url>")
        }
        Some(_) | None => line.to_owned(),
    }
}

/// `ImprovedInstantMessage`'s dialog for a message from an object
/// (`IM_FROM_TASK`), which `llInstantMessage` sends.
const MESSAGE_FROM_OBJECT: u8 = 19;

/// A chat type as a note names it.
fn chat_type_name(chat_type: ChatType) -> String {
    match chat_type {
        ChatType::Whisper => "whisper".to_owned(),
        ChatType::Normal => "say".to_owned(),
        ChatType::Shout => "shout".to_owned(),
        ChatType::StartTyping => "start-typing".to_owned(),
        ChatType::StopTyping => "stop-typing".to_owned(),
        ChatType::DebugChannel => "debug".to_owned(),
        ChatType::Region => "region".to_owned(),
        ChatType::Owner => "owner".to_owned(),
        ChatType::Direct => "direct".to_owned(),
        other => format!("type-{}", other.to_u8()),
    }
}

/// A vector as a note puts it.
fn vector_note(vector: &Vector) -> String {
    format!("<{:.3}, {:.3}, {:.3}>", vector.x, vector.y, vector.z)
}

/// The most characters of a long text a note quotes; its length is noted
/// beside it.
const CLIP_CHARS: usize = 60;

/// `text`, or its first [`CLIP_CHARS`] characters and `…` when it is
/// longer: what a note quotes of a long message.
fn clip(text: &str) -> String {
    match text.char_indices().nth(CLIP_CHARS) {
        Some((at, _)) => format!("{}…", text.get(..at).unwrap_or_default()),
        None => text.to_owned(),
    }
}

/// A note value in the probe lines' quoting.
fn quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// The case body both cases share.
async fn run_corpus(ctx: &mut TestContext, xengine: bool) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let engine = Vm::on(grid, xengine);
    let build_position = ctx.build_position();
    let settings = if is_opensim(grid) {
        ctx.lsl_settings().map_or_else(
            || "not given to the runner (no `lsl_settings` fixture)".to_owned(),
            str::to_owned,
        )
    } else {
        "Linden Lab's; a resident cannot set them".to_owned()
    };
    let root = corpus_root();
    let probes = load_corpus(&root)
        .and_then(|probes| select(probes, ctx.probe_selection()))
        .map_err(|error| TestFailure::State(error.to_string()))?;

    let trash = {
        let session = ctx.primary();
        let folders = session
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::InventorySkeleton(folders) => Some(folders.clone()),
                _ => None,
            })
            .await?;
        trash_folder(&folders).ok_or_else(|| {
            TestFailure::Assertion("inventory skeleton had neither a Trash nor a root".to_owned())
        })?
    };
    let elsewhere = ctx.ban_line_regions().to_vec();
    let estate = ctx.estate().cloned();
    let travels = probes
        .iter()
        .any(|probe| probe.drives().contains(&Drive::Away));
    let (mut seen, reference, trip) = {
        let session = ctx.primary();
        session.wait_for_region(REGION_TIMEOUT).await?;
        let settled = settle_scene_with_avatar(
            session,
            grid,
            build_position.clone(),
            SETTLE_WINDOW,
            SETTLE_IDLE,
        )
        .await?;
        let trip = if travels {
            let origin = session
                .region_handle()
                .ok_or_else(|| TestFailure::State("the owner is in no region".to_owned()))?;
            Some(Trip {
                origin,
                home: settled.avatar.clone().unwrap_or(Vector {
                    x: 128.0,
                    y: 128.0,
                    z: 30.0,
                }),
                destination: other_region(session, origin, &elsewhere).await?,
                grid,
                build_position: build_position.clone(),
            })
        } else {
            None
        };
        (settled.seen, settled.anchor, trip)
    };
    let other_active_group = {
        let other = ctx.secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        other.wait_for_region(REGION_TIMEOUT).await?;
        // The other avatar stands where the owner does, within chat range of
        // the prims: a login that a telehub redirected (Mauve's) leaves it at
        // the hub.
        if build_position.is_some() {
            let _settled = settle_scene(
                other,
                grid,
                build_position.clone(),
                SETTLE_WINDOW,
                SETTLE_IDLE,
            )
            .await?;
        }
        active_group(other).await?
    };
    let Some(reference) = reference else {
        if is_opensim(grid) {
            return Err(TestFailure::Assertion(
                "no primitive appeared in the region's object stream".to_owned(),
            ));
        }
        ctx.mark_partial("landing region streamed no primitive to place against");
        return Ok(());
    };
    let base = Vector {
        x: reference.x,
        y: reference.y,
        z: reference.z + LIFT_M,
    };
    let date = now_rfc3339()?.chars().take(10).collect::<String>();
    let cast = {
        let (session, other) = ctx.primary_and_secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        Cast {
            owner: session.agent_id(),
            other: other.agent_id(),
            owner_name: split_name(&session.avatar_legacy_name()),
            other_name: split_name(&other.avatar_legacy_name()),
            estate_name: estate.as_ref().map(StandbyLogin::name).unwrap_or_default(),
        }
    };

    let mut incomplete = Vec::new();
    for probe in &probes {
        let (session, other) = ctx.primary_and_secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        let started = Instant::now();
        let transcript = run_probe(
            &mut Avatars {
                session,
                other,
                other_active_group,
                cast: cast.clone(),
                trip: trip.clone(),
                estate: estate.as_ref(),
                comeback: Comeback {
                    grid,
                    build_position: build_position.clone(),
                },
            },
            probe,
            engine,
            &base,
            &mut seen,
            trash,
        )
        .await?;
        // A probe's script may have teleported an avatar inside the region,
        // and a grid that routes such a teleport (Mauve's telehub) leaves it
        // far from the build location, out of the next probe's chat range.
        if let Some(build) = &build_position {
            let (session, other) = ctx.primary_and_secondary().ok_or_else(|| {
                TestFailure::State("the case runs with a secondary avatar".to_owned())
            })?;
            for (moved, avatar) in [
                (transcript.owner_moved.clone(), session),
                (transcript.other_moved.clone(), other),
            ] {
                if let Some(from) = moved {
                    let _stopped = walk_within_region(avatar, from, build, |_event| {}).await?;
                }
            }
        }
        let ended = transcript.ended;
        let file = ResultFile {
            header: result_header(&ResultHeader {
                grid: engine.results_grid(),
                date: &date,
                region: transcript.region.as_deref(),
                engine,
                settings: &settings,
                ended,
            }),
            body: transcript.body,
        };
        let path = result_path(&root, engine.results_grid(), &probe.id);
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)
                .map_err(|error| TestFailure::State(error.to_string()))?;
        }
        fs_err::write(&path, file.render())
            .map_err(|error| TestFailure::State(error.to_string()))?;
        tracing::info!(
            probe = probe.id,
            ended,
            lines = file.body.len(),
            seconds = started.elapsed().as_secs_f64(),
            "probe transcript written"
        );
        let metrics = ctx.metrics();
        let key = probe.id.replace(['/', '-'], "_");
        metrics.set(&format!("{key}_ended"), ended);
        metrics.set(&format!("{key}_lines"), file.body.len().to_string());
        if !ended {
            incomplete.push(probe.id.as_str());
        }
    }
    ctx.metrics().set("probes", probes.len().to_string());
    if !incomplete.is_empty() {
        ctx.mark_partial(&format!("probes that never printed `end`: {incomplete:?}"));
    }
    Ok(())
}

/// What a result header states.
struct ResultHeader<'a> {
    /// The results directory's grid.
    grid: &'a str,
    /// The run's date.
    date: &'a str,
    /// The region the probe named, if it started.
    region: Option<&'a str>,
    /// The engine.
    engine: Vm,
    /// The engine settings.
    settings: &'a str,
    /// Whether the probe printed its `end`.
    ended: bool,
}

/// A result file's header lines.
fn result_header(header: &ResultHeader<'_>) -> Vec<(String, String)> {
    let mut lines = vec![
        ("grid".to_owned(), header.grid.to_owned()),
        ("date".to_owned(), header.date.to_owned()),
        (
            "runner".to_owned(),
            "sl-conformance lsl-probes (rezzed, uploaded and read by our client)".to_owned(),
        ),
        (
            "region".to_owned(),
            header
                .region
                .unwrap_or("unknown (no start record)")
                .to_owned(),
        ),
        ("engine".to_owned(), header.engine.label().to_owned()),
        ("settings".to_owned(), header.settings.to_owned()),
    ];
    if !header.ended {
        lines.push(("complete".to_owned(), "no".to_owned()));
    }
    lines
}

/// The two avatars a probe's run has: the owner, which rezzes, plants and
/// listens, and another, which only touches or speaks when asked to.
struct Avatars<'a> {
    /// The owner's session.
    session: &'a mut Session,
    /// The other avatar's session.
    other: &'a mut Session,
    /// Whether the other avatar has an active group, which decides what a
    /// script's `llDetectedGroup` says of its touch; `None` when the grid
    /// did not say.
    other_active_group: Option<bool>,
    /// Who the two avatars are.
    cast: Cast,
    /// Where the owner goes when a probe sends it `away`, and comes back
    /// to; `None` when no probe of the run asks.
    trip: Option<Trip>,
    /// The estate owner the runner keeps on call, who restarts the region
    /// when a probe asks; `None` when the credentials name none.
    estate: Option<&'a StandbyLogin>,
    /// How both avatars come back to the probe's prims after a restart.
    comeback: Comeback,
}

/// How both avatars come back to the probe's prims after a restart: logged
/// in again where they first logged in, then walked to the build location.
#[derive(Debug, Clone)]
struct Comeback {
    /// The grid, for the settle on coming back.
    grid: Grid,
    /// The fixtures' build location, which an avatar walks to on a grid
    /// whose arrival point is elsewhere.
    build_position: Option<Vector>,
}

/// The owner's way out of the probe's region and back.
#[derive(Debug, Clone)]
struct Trip {
    /// The probe's region.
    origin: RegionHandle,
    /// Where the owner stood in it, which it comes back to.
    home: Vector,
    /// The region it goes to.
    destination: RegionHandle,
    /// The grid, for the settle on coming back.
    grid: Grid,
    /// The fixtures' build location, which the owner walks back to on a
    /// grid whose arrival point is elsewhere.
    build_position: Option<Vector>,
}

/// A legacy name's first and last name (the last empty for a name of one
/// word).
fn split_name(legacy: &str) -> (String, String) {
    let (first, last) = legacy.split_once(' ').unwrap_or((legacy, ""));
    (first.to_owned(), last.to_owned())
}

/// Whether `session`'s avatar has an active group, as the grid answers an
/// `AgentDataUpdateRequest`; `None`, with a warning, when it does not answer.
async fn active_group(session: &mut Session) -> Result<Option<bool>, TestFailure> {
    session.send(Command::RequestAgentDataUpdate).await?;
    let answer = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ActiveGroupChanged(active) => Some(active.active_group_id.is_some()),
            _ => None,
        })
        .await;
    match answer {
        Ok(set) => Ok(Some(set)),
        Err(TestFailure::Timeout(_)) => {
            tracing::warn!("the grid did not say whether the other avatar has an active group");
            Ok(None)
        }
        Err(other) => Err(other),
    }
}

/// Runs one probe: rezzes its prims, plants its scripts, drives it, keeps
/// what it printed and clears its prims away.
async fn run_probe(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    engine: Vm,
    base: &Vector,
    seen: &mut HashSet<ScopedObjectId>,
    trash: InventoryFolderKey,
) -> Result<Transcript, TestFailure> {
    let session = &mut *avatars.session;
    let mut prims: BTreeMap<String, Object> = BTreeMap::new();
    for (label, distance, height) in probe.prims() {
        let position = Vector {
            x: base.x + distance,
            y: base.y,
            z: base.z + height,
        };
        session
            .send(Command::RezObject {
                shape: PrimShape::cube(position.clone()),
                group_id: None,
            })
            .await?;
        let object = match wait_for_own_new_object(session, seen, STEP_TIMEOUT).await? {
            Ok(object) => object,
            Err(reason) => {
                // The prims already rezzed are not left behind.
                clear_away(session, &prims, trash).await?;
                return Err(TestFailure::Assertion(format!(
                    "{}: prim {label} was not rezzed at {:.0}/{:.0}/{:.0}: {reason}",
                    probe.id, position.x, position.y, position.z
                )));
            }
        };
        seen.insert(object.scoped_id());
        prims.insert(label, object);
    }
    let keys: HashSet<ObjectKey> = prims.values().map(|object| object.full_id).collect();
    let mut transcript = Transcript::new(avatars.cast.clone());
    transcript.notes_active_scripts = probe.notes_active_scripts();
    if probe.drives().contains(&Drive::Pay) {
        // What each avatar holds before anything is paid, so that the notes
        // can say how a payment changed it. The answers come while the
        // scripts are planted.
        session.send(Command::RequestMoneyBalance).await?;
        avatars.other.send(Command::RequestMoneyBalance).await?;
    }

    let mut planted = true;
    for script in &probe.scripts {
        if script.header.skipped_on(engine.results_grid()) {
            transcript.body.push(format!(
                "# skipped: script {} (skip-on: {})",
                quote(&script.name),
                engine.results_grid()
            ));
            continue;
        }
        let prim = prims.get(&script.header.prim).ok_or_else(|| {
            TestFailure::State(format!("{}: no prim {}", probe.id, script.header.prim))
        })?;
        let item = new_script(
            session,
            prim,
            &script.name,
            &mut Watch {
                id: &probe.id,
                prims: &keys,
                transcript: &mut transcript,
            },
        )
        .await?;
        if script.header.target == Target::Lsl2 {
            // The result header names the run's engine; a script uploaded
            // for another target says so where it was saved.
            transcript
                .body
                .push(format!("# target: script {} lsl2", quote(&script.name)));
        }
        let source = format!("{}{}", engine.source_prefix(), script.source);
        session
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: prim.full_id,
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: match script.header.target {
                    Target::Mono => ScriptTarget::Mono,
                    Target::Lsl2 => ScriptTarget::Lsl2,
                },
                source: source.into_bytes(),
            })
            .await?;
        let id = &probe.id;
        let compiled = session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ScriptUploaded {
                    compiled, errors, ..
                } => Some(Ok((*compiled, errors.clone()))),
                Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                _ => {
                    let _kept = transcript.take(id, &keys, event);
                    None
                }
            })
            .await?;
        match compiled {
            Ok((true, _)) => {}
            Ok((false, errors)) => {
                for error in errors {
                    transcript.body.push(format!(
                        "{COMPILE_ERROR_NOTE}{}: {}",
                        quote(&script.name),
                        quote(&error.raw)
                    ));
                }
                planted = false;
                break;
            }
            Err(reason) => {
                return Err(TestFailure::Assertion(format!(
                    "{}: uploading {} failed: {reason}",
                    probe.id, script.name
                )));
            }
        }
    }

    if planted {
        let mut deadline = after(PROBE_DEADLINE);
        let mut answered = 0_usize;
        while collect(
            avatars.session,
            probe,
            &keys,
            &mut transcript,
            deadline,
            Until::Stimulus(answered),
        )
        .await?
        {
            let Some(wait) = transcript.waits.get(answered).cloned() else {
                break;
            };
            answered = answered.saturating_add(1);
            let stimulus = Stimulus::from_waiting(&wait)
                .map_err(|reason| TestFailure::Assertion(format!("{}: {reason}", probe.id)))?;
            drain_other(avatars.other, &keys, &mut transcript).await?;
            let stimulated = Instant::now();
            stimulate(
                avatars,
                probe,
                &mut Scene {
                    prims: &mut prims,
                    seen: &mut *seen,
                    trash,
                },
                &keys,
                &mut transcript,
                &stimulus,
            )
            .await?;
            // A restart's time is the region's, not the probe's: the probe
            // gets it back.
            if matches!(stimulus.action, Action::Restart { .. }) {
                deadline = deadline
                    .checked_add(stimulated.elapsed())
                    .unwrap_or(deadline);
            }
        }
        if transcript.ended {
            let linger = after(LINGER);
            collect(
                avatars.session,
                probe,
                &keys,
                &mut transcript,
                linger,
                Until::Deadline,
            )
            .await?;
        }
        drain_other(avatars.other, &keys, &mut transcript).await?;
    }

    // Where the prims are now, and whether one is worn: a stimulus, or the
    // probe's own script, may have moved, attached or re-keyed one.
    for prim in prims.values_mut() {
        if let Some(latest) = transcript.latest.get(&prim.full_id) {
            prim.clone_from(latest);
        }
    }
    clear_away(avatars.session, &prims, trash).await?;
    Ok(transcript)
}

/// Derezzes `prims` to the Trash — a prim still worn (a probe that ended
/// before it was dropped) is detached into inventory instead — and waits
/// until the region has removed every one.
async fn clear_away(
    session: &mut Session,
    prims: &BTreeMap<String, Object>,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if prims.is_empty() {
        return Ok(());
    }
    let local_ids: Vec<ScopedObjectId> = prims.values().map(Object::scoped_id).collect();
    let (worn, grounded): (Vec<&Object>, Vec<&Object>) = prims
        .values()
        .partition(|prim| prim.attachment_point_id().is_some());
    if !worn.is_empty() {
        session
            .send(Command::DetachObjects {
                local_ids: worn.iter().map(|prim| prim.scoped_id()).collect(),
            })
            .await?;
    }
    if !grounded.is_empty() {
        session
            .send(Command::DerezObjects {
                local_ids: grounded.iter().map(|prim| prim.scoped_id()).collect(),
                destination: DeRezDestination::Trash(trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
    }
    let mut gone: HashSet<ScopedObjectId> = HashSet::new();
    while gone.len() < local_ids.len() {
        let removed = session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ObjectRemoved { local_id, .. } if local_ids.contains(local_id) => {
                    Some(*local_id)
                }
                _ => None,
            })
            .await?;
        gone.insert(removed);
    }
    Ok(())
}

/// What ends a [`collect`].
#[derive(Debug, Clone, Copy)]
enum Until {
    /// The probe printing its `end`, or a `waiting` record beyond the first
    /// this many, which have been answered.
    Stimulus(usize),
    /// Only the deadline.
    Deadline,
}

impl Until {
    /// Whether `transcript` has come this far.
    const fn reached(self, transcript: &Transcript) -> bool {
        match self {
            Self::Stimulus(answered) => transcript.ended || transcript.waits.len() > answered,
            Self::Deadline => false,
        }
    }
}

/// Keeps the probe's lines until `until` or `deadline`, whichever is first;
/// whether `until` came is the answer — at once, for a record the probe
/// printed already (while the case waited for something else). The
/// deadline passing is not a failure.
async fn collect(
    session: &mut Session,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    deadline: Instant,
    until: Until,
) -> Result<bool, TestFailure> {
    if until.reached(transcript) {
        return Ok(true);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let outcome = session
        .wait_for(remaining, |event| {
            let _kept = transcript.take(&probe.id, prims, event);
            until.reached(transcript).then_some(())
        })
        .await;
    match outcome {
        Ok(()) => Ok(true),
        Err(TestFailure::Timeout(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

/// Gives the stimulus a probe asked for, [`STIMULUS_SETTLE`] after it asked,
/// and notes it in the transcript: a touch of prim A or a line of chat, from
/// the owner or the other avatar.
async fn stimulate(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    stimulus: &Stimulus,
) -> Result<(), TestFailure> {
    let drive = stimulus.action.drive();
    if !probe.drives().contains(&drive) {
        return Err(TestFailure::Assertion(format!(
            "{}: asked for {drive:?}, which its header's `drive` does not declare",
            probe.id
        )));
    }
    let settle = after(STIMULUS_SETTLE);
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        settle,
        Until::Deadline,
    )
    .await?;
    let by = match stimulus.by {
        By::Owner => "",
        By::Other => " by=other",
    };
    let prims = &*scene.prims;
    match &stimulus.action {
        Action::Answer { grant } => {
            return answer(avatars, stimulus.by, probe, keys, transcript, *grant).await;
        }
        Action::Dialog { reply } => {
            return reply_dialog(avatars, stimulus.by, probe, keys, transcript, reply).await;
        }
        Action::Away { seconds } => {
            return away(avatars, probe, scene, keys, transcript, *seconds).await;
        }
        Action::Sit => return sit(avatars, stimulus.by, probe, prims, keys, transcript).await,
        Action::Attach { point } => {
            return attach(avatars.session, probe, scene, keys, transcript, *point).await;
        }
        Action::Drop => return drop_prim(avatars.session, probe, scene, keys, transcript).await,
        Action::Rerez => return rerez(avatars.session, probe, scene, keys, transcript).await,
        Action::Restart { seconds } => {
            return restart(avatars, probe, scene, keys, transcript, *seconds).await;
        }
        Action::Fetch(fetch) => {
            return fetch_url(avatars.session, probe, keys, transcript, fetch).await;
        }
        Action::Pay { amount, prim } => {
            return pay(
                avatars,
                stimulus.by,
                probe,
                prims,
                keys,
                transcript,
                (*amount, prim),
            )
            .await;
        }
        Action::PayPrice { prim } => {
            return pay_price(avatars, stimulus.by, probe, prims, keys, transcript, prim).await;
        }
        Action::Revoke {
            permissions,
            whole_region,
        } => {
            let actor = match stimulus.by {
                By::Owner => &mut *avatars.session,
                By::Other => &mut *avatars.other,
            };
            let (object_id, target) = if *whole_region {
                let region = actor
                    .region_identity()
                    .map(|identity| identity.region_id)
                    .ok_or_else(|| {
                        TestFailure::State(format!("{}: the region's id is unknown", probe.id))
                    })?;
                (ObjectKey::from(region), "region")
            } else {
                let prim = prims
                    .get("A")
                    .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?;
                (prim.full_id, "object")
            };
            transcript
                .body
                .push(format!("# revoke{by} perms={permissions} target={target}"));
            return actor
                .send(Command::RevokeScriptPermissions {
                    object_id,
                    permissions: ScriptPermissions(i32::from_ne_bytes(permissions.to_ne_bytes())),
                })
                .await;
        }
        Action::Control { keys: held } => {
            transcript.body.push(format!(
                "# control{by} keys={held} held={}s",
                CONTROL_HOLD.as_secs()
            ));
            let controls = ControlFlags::from_bits(*held);
            match stimulus.by {
                By::Owner => avatars.session.send(Command::SetControls(controls)).await?,
                By::Other => avatars.other.send(Command::SetControls(controls)).await?,
            }
            collect(
                avatars.session,
                probe,
                keys,
                transcript,
                after(CONTROL_HOLD),
                Until::Deadline,
            )
            .await?;
            let released = Command::SetControls(ControlFlags::empty());
            return match stimulus.by {
                By::Owner => avatars.session.send(released).await,
                By::Other => avatars.other.send(released).await,
            };
        }
        Action::Touch(_) | Action::Chat { .. } | Action::Stand => {}
    }
    let actor = match stimulus.by {
        By::Owner => &mut *avatars.session,
        By::Other => &mut *avatars.other,
    };
    match &stimulus.action {
        Action::Stand => {
            transcript.body.push(format!("# stand{by}"));
            actor.send(Command::Stand).await
        }
        Action::Answer { .. }
        | Action::Dialog { .. }
        | Action::Away { .. }
        | Action::Sit
        | Action::Control { .. }
        | Action::Attach { .. }
        | Action::Drop
        | Action::Rerez
        | Action::Restart { .. }
        | Action::Fetch(_)
        | Action::Pay { .. }
        | Action::PayPrice { .. }
        | Action::Revoke { .. } => Ok(()),
        Action::Touch(surface) => {
            let prim = prims
                .get("A")
                .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?;
            let group = match (stimulus.by, avatars.other_active_group) {
                (By::Owner, _) => "",
                (By::Other, Some(true)) => " active_group=set",
                (By::Other, Some(false)) => " active_group=none",
                (By::Other, None) => " active_group=unknown",
            };
            let surface = surface
                .as_ref()
                .map(|surface| surface_info(surface, &prim.motion.position));
            let sent = surface.as_ref().map_or_else(
                || " surface=none".to_owned(),
                |info| {
                    format!(
                        " face={} uv=<{:.3}, {:.3}> st=<{:.3}, {:.3}> pos={} normal={} \
                         binormal={}",
                        info.face_index,
                        info.uv[0],
                        info.uv[1],
                        info.st[0],
                        info.st[1],
                        vector_note(&info.position),
                        vector_note(&info.normal),
                        vector_note(&info.binormal)
                    )
                },
            );
            transcript.body.push(format!("# touch{by}{group}{sent}"));
            let local_id = scoped_for(actor, prim);
            actor
                .send(Command::GrabObject {
                    local_id,
                    grab_offset: Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    surface: surface.clone(),
                })
                .await?;
            actor
                .send(Command::DegrabObject { local_id, surface })
                .await
        }
        Action::Chat { channel, text } => {
            transcript
                .body
                .push(format!("# chat{by} channel={channel} text={}", quote(text)));
            actor
                .send(Command::Chat {
                    message: text.clone(),
                    chat_type: ChatType::Normal,
                    channel: ChatChannel(*channel),
                })
                .await
        }
    }
}

/// How long a `pay-price` waits for the answer: a grid answers at once or
/// not at all (OpenSim sends nothing for a layout of zeros), and a probe
/// that asks after each change must not find the runner still waiting for
/// the last.
const PAY_PRICE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a `pay` keeps the probe's lines after it pays, for the
/// `money` event and the balances the grid sends both avatars.
const PAY_SETTLE: Duration = Duration::from_secs(5);

/// Pays the prim `paid` labels `amount` L$ from the avatar `by` names, as a
/// viewer's pay dialog does — a `MoneyTransferRequest` of the object-payment
/// type, to the object — and notes it, then keeps the probe's lines and both
/// avatars' balances for [`PAY_SETTLE`].
async fn pay(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &BTreeMap<String, Object>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    (amount, paid): (u64, &str),
) -> Result<(), TestFailure> {
    let prim = prims
        .get(paid)
        .ok_or_else(|| TestFailure::State(format!("{}: no prim {paid}", probe.id)))?;
    let label = match by {
        By::Owner => "",
        By::Other => " by=other",
    };
    transcript
        .body
        .push(format!("# pay{label} amount={amount} prim={paid}"));
    let command = Command::SendMoneyTransfer {
        dest: prim.full_id.uuid(),
        amount: LindenAmount(amount),
        kind: MoneyTransactionType::PayObject,
        description: String::new(),
    };
    match by {
        By::Owner => avatars.session.send(command).await?,
        By::Other => avatars.other.send(command).await?,
    }
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        after(PAY_SETTLE),
        Until::Deadline,
    )
    .await?;
    drain_other(avatars.other, keys, transcript).await
}

/// Asks the prim `asked` labels for its pay buttons from the avatar `by`
/// names (`RequestPayPrice`) and notes the answer — the default amount and
/// the buttons, as sent — or that none came within [`PAY_PRICE_TIMEOUT`].
async fn pay_price(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &BTreeMap<String, Object>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    asked: &str,
) -> Result<(), TestFailure> {
    let object_id = prims
        .get(asked)
        .ok_or_else(|| TestFailure::State(format!("{}: no prim {asked}", probe.id)))?
        .full_id;
    let (actor, label) = match by {
        By::Owner => (&mut *avatars.session, ""),
        By::Other => (&mut *avatars.other, " by=other"),
    };
    actor.send(Command::RequestPayPrice { object_id }).await?;
    let outcome = actor
        .wait_for(PAY_PRICE_TIMEOUT, |event| match event {
            Event::PayPriceReply {
                object_id: replied,
                default_pay_price,
                pay_buttons,
            } if *replied == object_id => Some(format!(
                "# pay-price{label} prim={asked} default={default_pay_price} buttons={}",
                quote(
                    &pay_buttons
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            )),
            _ => {
                match by {
                    By::Owner => {
                        let _kept = transcript.take(&probe.id, keys, event);
                    }
                    By::Other => transcript.take_other(keys, event),
                }
                None
            }
        })
        .await;
    match outcome {
        Ok(note) => transcript.body.push(note),
        Err(TestFailure::Timeout(_)) => transcript
            .body
            .push(format!("# pay-price{label} prim={asked} response=none")),
        Err(other) => return Err(other),
    }
    Ok(())
}

/// The surface block a touch sends for `surface`, its point put in the
/// region at prim A's `centre`.
fn surface_info(surface: &TouchSurface, centre: &Vector) -> SurfaceInfo {
    let vector = |[x, y, z]: [f32; 3]| Vector { x, y, z };
    SurfaceInfo {
        uv: surface.uv,
        st: surface.st,
        face_index: surface.face,
        position: Vector {
            x: centre.x + surface.at[0],
            y: centre.y + surface.at[1],
            z: centre.z + surface.at[2],
        },
        normal: vector(surface.normal),
        binormal: vector(surface.binormal),
    }
}

/// `prim`'s id as `actor`'s session scopes it: its region-local id on that
/// session's root circuit. The prim was seen by the owner's session, whose
/// circuit ids are its own — the other avatar's session numbers its circuits
/// apart, and the owner's change with every trip `away`.
fn scoped_for(actor: &Session, prim: &Object) -> ScopedObjectId {
    actor.circuit_id().map_or_else(
        || prim.scoped_id(),
        |circuit| ScopedObjectId::new(circuit, prim.local_id),
    )
}

/// How long a `fetch` waits for the grid's answer: longer than either
/// grid keeps a request a script does not answer.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// How often a `fetch` in flight looks whether it is answered, keeping the
/// probe's lines meanwhile.
const FETCH_POLL: Duration = Duration::from_millis(200);

/// The `User-Agent` a `fetch` sends, so that a probe can tell it apart.
const FETCH_USER_AGENT: &str = "sl-conformance-probe";

/// What the grid answered a `fetch`.
#[derive(Debug)]
struct Fetched {
    /// The status.
    status: u16,
    /// The `Content-Type`, if any.
    content_type: Option<String>,
    /// The body.
    body: String,
}

/// Sends the HTTP request a `fetch` stimulus names from outside the grid —
/// from the runner, which is no avatar — and notes it and the answer: the
/// status, the `Content-Type`, the body's length and its start, and how long
/// the answer took. The probe's lines that arrive meanwhile are kept, so the
/// script's view of the request comes before the answer's note.
async fn fetch_url(
    session: &mut Session,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    fetch: &Fetch,
) -> Result<(), TestFailure> {
    let header = fetch
        .header
        .as_ref()
        .map_or_else(String::new, |(name, value)| {
            format!(" header={}", quote(&format!("{name}: {value}")))
        });
    let content_type = fetch
        .content_type
        .as_ref()
        .map_or_else(String::new, |content_type| {
            format!(" type={}", quote(content_type))
        });
    transcript.body.push(format!(
        "# fetch method={} path={} body_length={}{content_type}{header}",
        fetch.method,
        quote(&fetch.path),
        fetch.body.len()
    ));
    let client = reqwest::Client::builder()
        .user_agent(FETCH_USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|error| TestFailure::State(format!("{}: no HTTP client: {error}", probe.id)))?;
    let method = reqwest::Method::from_bytes(fetch.method.as_bytes())
        .map_err(|error| TestFailure::State(format!("{}: {error}", probe.id)))?;
    let mut request = client.request(method, format!("{}{}", fetch.url, fetch.path));
    if let Some(content_type) = &fetch.content_type {
        request = request.header(reqwest::header::CONTENT_TYPE, content_type);
    }
    if let Some((name, value)) = &fetch.header {
        request = request.header(name, value);
    }
    if !fetch.body.is_empty() {
        request = request.body(fetch.body.clone());
    }
    let started = Instant::now();
    let flight = tokio::spawn(async move {
        let response = request.send().await?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        let body = response.text().await?;
        Ok::<_, reqwest::Error>(Fetched {
            status,
            content_type,
            body,
        })
    });
    while !flight.is_finished() {
        collect(
            session,
            probe,
            prims,
            transcript,
            after(FETCH_POLL),
            Until::Deadline,
        )
        .await?;
    }
    let seconds = started.elapsed().as_secs();
    let outcome = flight
        .await
        .map_err(|error| TestFailure::State(format!("{}: the fetch failed: {error}", probe.id)))?;
    let note = match outcome {
        Ok(fetched) => {
            let body = transcript.cast.scrub(&fetched.body);
            format!(
                "# fetched status={} type={} length={} body={} seconds={seconds}",
                fetched.status,
                quote(fetched.content_type.as_deref().unwrap_or("")),
                fetched.body.len(),
                quote(&clip(&body))
            )
        }
        Err(error) => {
            let kind = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect"
            } else {
                "other"
            };
            format!("# fetched status=none error={kind} seconds={seconds}")
        }
    };
    transcript.body.push(note);
    Ok(())
}

/// How long the case waits after a probe asks for a stimulus before it
/// gives it.
const STIMULUS_SETTLE: Duration = Duration::from_secs(1);

/// How long a `control` stimulus holds its keys down.
const CONTROL_HOLD: Duration = Duration::from_secs(1);

/// How long an `answer` stimulus waits for the question it answers, and a
/// `sit` for the grid's sit response.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a look at the other avatar's events waits for more.
const DRAIN_WINDOW: Duration = Duration::from_millis(200);

/// Notes what the grid sent the other avatar's viewer since the last look
/// ([`Transcript::take_other`]).
async fn drain_other(
    other: &mut Session,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let outcome = other
        .wait_for(DRAIN_WINDOW, |event| {
            transcript.take_other(prims, event);
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Answers the permission question the probe's script put to the avatar
/// `by` names with `grant`, as it is, and notes it — or notes that no
/// question came within [`ANSWER_TIMEOUT`].
async fn answer(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    grant: i32,
) -> Result<(), TestFailure> {
    let deadline = after(ANSWER_TIMEOUT);
    let request = match by {
        By::Owner => {
            if transcript.questions.is_empty() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let outcome = avatars
                    .session
                    .wait_for(remaining, |event| {
                        let _kept = transcript.take(&probe.id, prims, event);
                        (!transcript.questions.is_empty()).then_some(())
                    })
                    .await;
                match outcome {
                    Ok(()) | Err(TestFailure::Timeout(_)) => {}
                    Err(other) => return Err(other),
                }
            }
            transcript.questions.pop_front()
        }
        By::Other => {
            while transcript.other_questions.is_empty() && Instant::now() < deadline {
                drain_other(avatars.other, prims, transcript).await?;
            }
            transcript.other_questions.pop_front()
        }
    };
    let label = match by {
        By::Owner => "",
        By::Other => " by=other",
    };
    let Some(request) = request else {
        transcript
            .body
            .push(format!("# answer{label} grant={grant} question=none"));
        return Ok(());
    };
    transcript
        .body
        .push(format!("# answer{label} grant={grant}"));
    let command = Command::AnswerScriptPermissions {
        task_id: request.task_id,
        item_id: request.item_id,
        permissions: ScriptPermissions(grant),
        experience_id: request.experience_id,
    };
    match by {
        By::Owner => avatars.session.send(command).await,
        By::Other => avatars.other.send(command).await,
    }
}

/// The probe's prims, by label, and every object the run has sighted: what
/// a stimulus that takes the owner out of the region renews on its return,
/// or that moves, attaches or re-keys prim A.
struct Scene<'a> {
    /// The probe's prims.
    prims: &'a mut BTreeMap<String, Object>,
    /// Every object the run has sighted.
    seen: &'a mut HashSet<ScopedObjectId>,
    /// The owner's Trash, which a re-rez takes prim A into.
    trash: InventoryFolderKey,
}

impl Scene<'_> {
    /// Prim A as the region last sent it.
    fn prim_a(&self, probe: &Probe, transcript: &Transcript) -> Result<Object, TestFailure> {
        let prim = self
            .prims
            .get("A")
            .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?;
        Ok(transcript.latest.get(&prim.full_id).unwrap_or(prim).clone())
    }

    /// Prim A is `object` from now on: sighted, and its newest update.
    fn renew_a(&mut self, transcript: &mut Transcript, object: &Object) {
        self.seen.insert(object.scoped_id());
        let _previous = transcript.latest.insert(object.full_id, object.clone());
        let _old = self.prims.insert("A".to_owned(), object.clone());
    }
}

/// How long a stimulus that moves prim A waits for the region to show it
/// moved: attached, dropped, or rezzed again.
const MOVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Waits until an object update `matches` (an update of a prim already
/// known, or a new object — `true` beside an [`Event::ObjectAdded`]),
/// holding every other event meanwhile and handing them to the transcript
/// once it is known — with the object's key among the probe's, since a
/// line it printed may come ahead of the update that names it. `None` when
/// no update matched within `timeout`.
async fn await_prim(
    session: &mut Session,
    probe: &Probe,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    timeout: Duration,
    mut matches: impl FnMut(&Object, bool) -> bool,
) -> Result<Option<Object>, TestFailure> {
    let mut held: Vec<Event> = Vec::new();
    let outcome = session
        .wait_for(timeout, |event| match event {
            Event::ObjectAdded(object) if matches(object, true) => Some((**object).clone()),
            Event::ObjectUpdated(object) if matches(object, false) => Some((**object).clone()),
            other => {
                held.push(other.clone());
                None
            }
        })
        .await;
    let found = match outcome {
        Ok(object) => Some(object),
        Err(TestFailure::Timeout(_)) => None,
        Err(other) => return Err(other),
    };
    if let Some(object) = &found
        && !keys.contains(&object.full_id)
    {
        transcript.extra.insert(object.full_id);
    }
    for event in &held {
        let _kept = transcript.take(&probe.id, keys, event);
    }
    Ok(found)
}

/// The owner's agent id, as an object's owner names it.
fn owner_uuid(session: &Session) -> Result<Uuid, TestFailure> {
    session
        .agent_id()
        .map(|agent| agent.uuid())
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
}

/// Whether `object` is `prim`'s key (`same`) or another (`new`), as a note
/// says it.
fn key_note(object: &Object, prim: &Object) -> &'static str {
    if object.full_id == prim.full_id {
        "same"
    } else {
        "new"
    }
}

/// Attaches prim A from the ground to the owner at `point`, alongside what
/// is worn there, and notes the attachment the region shows — its point,
/// and whether it kept its key — or that none came within
/// [`MOVE_TIMEOUT`].
async fn attach(
    session: &mut Session,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    point: u8,
) -> Result<(), TestFailure> {
    let prim = scene.prim_a(probe, transcript)?;
    let owner = owner_uuid(session)?;
    transcript.body.push(format!("# attach point={point}"));
    session
        .send(Command::AttachObject {
            local_id: prim.scoped_id(),
            attachment_point: AttachmentPoint::from_code(point),
            mode: AttachmentMode::Add,
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
        })
        .await?;
    let seen = &*scene.seen;
    let worn = await_prim(
        session,
        probe,
        keys,
        transcript,
        MOVE_TIMEOUT,
        |object, added| {
            object.attachment_point_id().is_some()
                && (object.full_id == prim.full_id
                    || (added && is_own(object, owner) && !seen.contains(&object.scoped_id())))
        },
    )
    .await?;
    match worn {
        Some(object) => {
            transcript.body.push(format!(
                "# attached point={} key={}",
                object.attachment_point_id().unwrap_or_default(),
                key_note(&object, &prim)
            ));
            scene.renew_a(transcript, &object);
        }
        None => transcript.body.push("# attached response=none".to_owned()),
    }
    Ok(())
}

/// Drops prim A, worn by the owner, to the ground, and notes it on the
/// ground — whether it kept its key — or that it did not come down within
/// [`MOVE_TIMEOUT`].
async fn drop_prim(
    session: &mut Session,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let prim = scene.prim_a(probe, transcript)?;
    let owner = owner_uuid(session)?;
    let worn = prim.attachment_point_id().is_some();
    transcript
        .body
        .push(format!("# drop worn={}", u8::from(worn)));
    session
        .send(Command::DropAttachments {
            local_ids: vec![prim.scoped_id()],
        })
        .await?;
    let seen = &*scene.seen;
    let grounded = await_prim(
        session,
        probe,
        keys,
        transcript,
        MOVE_TIMEOUT,
        |object, added| {
            object.attachment_point_id().is_none()
                && object.parent_id == RegionLocalObjectId(0)
                && (object.full_id == prim.full_id
                    || (added && is_own(object, owner) && !seen.contains(&object.scoped_id())))
        },
    )
    .await?;
    match grounded {
        Some(object) => {
            transcript
                .body
                .push(format!("# dropped key={}", key_note(&object, &prim)));
            scene.renew_a(transcript, &object);
        }
        None => transcript.body.push("# dropped response=none".to_owned()),
    }
    Ok(())
}

/// Takes prim A into the owner's Trash and rezzes the item back where the
/// prim stood, and notes how each went: the item the take made, and the
/// prim rezzed from it — whose key is new.
async fn rerez(
    session: &mut Session,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let prim = scene.prim_a(probe, transcript)?;
    let owner = owner_uuid(session)?;
    transcript.body.push("# take".to_owned());
    session
        .send(Command::DerezObjects {
            local_ids: vec![prim.scoped_id()],
            destination: DeRezDestination::TakeIntoAgentInventory(scene.trash),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let id = probe.id.clone();
    let announced = created_item_announcement_observing(
        session,
        MOVE_TIMEOUT,
        AssetType::Object.to_code(),
        |event| {
            let _kept = transcript.take(&id, keys, event);
        },
    )
    .await;
    let item = match announced {
        Ok((how, item)) => {
            transcript.body.push(format!("# taken announced={how}"));
            item
        }
        Err(TestFailure::Timeout(_)) => {
            transcript.body.push("# taken response=none".to_owned());
            return Ok(());
        }
        Err(other) => return Err(other),
    };
    transcript.body.push("# rez".to_owned());
    session
        .send(Command::RezObjectFromInventory {
            params: Box::new(rez_params(&item, &prim.motion.position)),
        })
        .await?;
    let seen = &*scene.seen;
    let rezzed = await_prim(
        session,
        probe,
        keys,
        transcript,
        MOVE_TIMEOUT,
        |object, added| {
            added
                && is_own(object, owner)
                && object.parent_id == RegionLocalObjectId(0)
                && !seen.contains(&object.scoped_id())
        },
    )
    .await?;
    match rezzed {
        Some(object) => {
            transcript
                .body
                .push(format!("# rezzed key={}", key_note(&object, &prim)));
            scene.renew_a(transcript, &object);
        }
        None => transcript.body.push("# rezzed response=none".to_owned()),
    }
    Ok(())
}

/// How long a restart may take to bring the region down after the time it
/// was scheduled for.
const DOWN_GRACE: Duration = Duration::from_secs(60);

/// How long the region may take to come back up after it went down.
const UP_TIMEOUT: Duration = Duration::from_secs(180);

/// How long between two logins that find the region still down.
const LOGIN_RETRY: Duration = Duration::from_secs(5);

/// How long the other avatar is given to hear that the region went down
/// once the owner has.
const OTHER_DOWN_GRACE: Duration = Duration::from_secs(10);

/// Restarts the probe's region in `seconds`, as an estate manager does, and
/// brings both avatars back to the probe's prims once it is up again: the
/// on-call estate owner logs in, asks for the restart and is dropped once the
/// region goes down; the owner and the other avatar hear it out, are logged
/// in again as soon as the region takes a login, and walk back; the prims
/// are renewed as the region streams them (a region keeps its objects' keys
/// across a restart). Notes the restart, the region going down — what the
/// owner was told — and coming back, or that no restart was sent, since the
/// credentials name no estate owner.
async fn restart(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    seconds: u32,
) -> Result<(), TestFailure> {
    let Some(estate) = avatars.estate else {
        transcript
            .body
            .push("# restart sent=no reason=\"the credentials name no estate owner\"".to_owned());
        return Ok(());
    };
    let scheduled = i32::try_from(seconds).map_err(|_overflow| {
        TestFailure::Assertion(format!("{}: a restart in {seconds} s", probe.id))
    })?;
    let region = avatars
        .session
        .region_handle()
        .ok_or_else(|| TestFailure::State("the owner is in no region".to_owned()))?;
    let mut manager = estate.login().await?;
    manager.wait_for_region(REGION_TIMEOUT).await?;
    transcript.body.push(format!("# restart seconds={seconds}"));
    manager
        .send(Command::RestartRegion { seconds: scheduled })
        .await?;
    // The owner hears the notices, then the region going down.
    let down_within = Duration::from_secs(u64::from(seconds)).saturating_add(DOWN_GRACE);
    let id = probe.id.clone();
    let down = avatars
        .session
        .wait_for(down_within, |event| match event {
            Event::Disconnected(reason) => Some(reason.clone()),
            _ => {
                let _kept = transcript.take(&id, keys, event);
                None
            }
        })
        .await;
    let reason = match down {
        Ok(reason) => reason,
        Err(TestFailure::Timeout(_)) => {
            transcript
                .body
                .push("# region-down response=none".to_owned());
            return Ok(());
        }
        Err(other) => return Err(other),
    };
    let told = match &reason {
        DisconnectReason::Kicked { message } => quote(&transcript.cast.scrub(message)),
        other => quote(&format!("{other:?}")),
    };
    transcript.body.push(format!("# region-down told={told}"));
    let gone = avatars
        .other
        .wait_for(OTHER_DOWN_GRACE, |event| {
            transcript.take_other(keys, event);
            matches!(event, Event::Disconnected(_)).then_some(())
        })
        .await;
    match gone {
        Ok(()) | Err(TestFailure::Timeout(_) | TestFailure::Disconnected(_)) => {}
        Err(other) => return Err(other),
    }
    drop(manager);

    // Back in as soon as the region takes a login.
    let up_by = after(UP_TIMEOUT);
    relogin_into(avatars.session, region, up_by).await?;
    transcript.body.push("# region-up".to_owned());
    let mut renewed: HashMap<ObjectKey, Object> = HashMap::new();
    let settled = settle_scene_observing(
        avatars.session,
        avatars.comeback.grid,
        avatars.comeback.build_position.clone(),
        SETTLE_WINDOW,
        SETTLE_IDLE,
        |event| {
            if let Event::ObjectAdded(object) = event
                && keys.contains(&object.full_id)
            {
                let _previous = renewed.insert(object.full_id, (**object).clone());
            }
            let _kept = transcript.take(&probe.id, keys, event);
        },
    )
    .await?;
    scene.seen.extend(settled.seen);
    let mut missing = Vec::new();
    for (label, prim) in &mut *scene.prims {
        match renewed.remove(&prim.full_id) {
            Some(object) => *prim = object,
            None => missing.push(label.clone()),
        }
    }
    for label in &missing {
        // A prim the region did not bring back is not there to clear away.
        let _gone = scene.prims.remove(label);
    }
    transcript.body.push(if missing.is_empty() {
        "# back prims=all".to_owned()
    } else {
        format!("# back missing={}", missing.join(","))
    });
    relogin_into(avatars.other, region, after(UP_TIMEOUT)).await?;
    if avatars.comeback.build_position.is_some() {
        let _settled = settle_scene(
            avatars.other,
            avatars.comeback.grid,
            avatars.comeback.build_position.clone(),
            SETTLE_WINDOW,
            SETTLE_IDLE,
        )
        .await?;
    }
    Ok(())
}

/// Logs `session`'s avatar in again until the login lands it in `region`,
/// and waits for the region: a login while the region is still down fails,
/// or lands the avatar in another region (OpenSim's default), which is
/// logged out again. Fails once `up_by` has passed.
async fn relogin_into(
    session: &mut Session,
    region: RegionHandle,
    up_by: Instant,
) -> Result<(), TestFailure> {
    loop {
        let outcome = match session.relogin().await {
            Ok(()) if session.region_handle() == Some(region) => {
                return session.wait_for_region(REGION_TIMEOUT).await;
            }
            Ok(()) => {
                session.disconnect().await?;
                TestFailure::State("the login landed in another region".to_owned())
            }
            Err(error) => error,
        };
        if Instant::now() >= up_by {
            return Err(outcome);
        }
        tracing::info!("the region is not back yet: {outcome}");
        tokio::time::sleep(LOGIN_RETRY).await;
    }
}

/// The newest of `dialogs`, with the older ones dropped: a dialog a probe
/// put without asking for a reply is left unanswered, as a resident would
/// close it.
fn newest(dialogs: &mut VecDeque<ScriptDialog>) -> Option<ScriptDialog> {
    let last = dialogs.pop_back();
    dialogs.clear();
    last
}

/// Replies `reply` to the newest dialog the probe's script put to the
/// avatar `by` names — the index of the button with that label, or 0 when the dialog
/// has none (a text box, or a label it does not offer) — and notes it, or
/// notes that no dialog came within [`ANSWER_TIMEOUT`].
async fn reply_dialog(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    reply: &str,
) -> Result<(), TestFailure> {
    let deadline = after(ANSWER_TIMEOUT);
    let dialog = match by {
        By::Owner => {
            if transcript.dialogs.is_empty() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let outcome = avatars
                    .session
                    .wait_for(remaining, |event| {
                        let _kept = transcript.take(&probe.id, prims, event);
                        (!transcript.dialogs.is_empty()).then_some(())
                    })
                    .await;
                match outcome {
                    Ok(()) | Err(TestFailure::Timeout(_)) => {}
                    Err(other) => return Err(other),
                }
            }
            newest(&mut transcript.dialogs)
        }
        By::Other => {
            while transcript.other_dialogs.is_empty() && Instant::now() < deadline {
                drain_other(avatars.other, prims, transcript).await?;
            }
            newest(&mut transcript.other_dialogs)
        }
    };
    let label = match by {
        By::Owner => "",
        By::Other => " by=other",
    };
    let Some(dialog) = dialog else {
        transcript.body.push(format!(
            "# dialog-reply{label} reply={} dialog=none",
            quote(reply)
        ));
        return Ok(());
    };
    let index = dialog
        .buttons
        .iter()
        .position(|button| button == reply)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0);
    transcript.body.push(format!(
        "# dialog-reply{label} reply={} index={index} sent_length={}",
        quote(&clip(reply)),
        ScriptDialog::fit_reply(reply).len()
    ));
    let command = Command::ReplyScriptDialog {
        object_id: dialog.object_id,
        chat_channel: dialog.chat_channel,
        button_index: index,
        button_label: reply.to_owned(),
    };
    match by {
        By::Owner => avatars.session.send(command).await,
        By::Other => avatars.other.send(command).await,
    }
}

/// How long a teleport of the owner's may take to arrive or fail.
const TRIP_TIMEOUT: Duration = Duration::from_secs(60);

/// Teleports the owner to `destination` at `position`, keeping the probe's
/// lines meanwhile, and says how it went: `arrived`, or why not.
async fn teleport_owner(
    session: &mut Session,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    destination: RegionHandle,
    position: &Vector,
) -> Result<String, TestFailure> {
    request_teleport(
        session,
        destination,
        (position.x, position.y, position.z),
        (position.x + 1.0, position.y, position.z),
    )
    .await?;
    let outcome = session
        .wait_for(TRIP_TIMEOUT, |event| match event {
            Event::RegionChanged { .. } => Some("arrived".to_owned()),
            Event::TeleportLocal { .. } => Some("local".to_owned()),
            Event::TeleportFailed { reason, .. } => Some(format!("failed {}", quote(reason))),
            _ => {
                let _kept = transcript.take(&probe.id, prims, event);
                None
            }
        })
        .await;
    match outcome {
        Ok(how) => Ok(how),
        Err(TestFailure::Timeout(_)) => Ok("timeout".to_owned()),
        Err(other) => Err(other),
    }
}

/// Sends the owner out of the probe's region, to the trip's destination,
/// for `seconds`, keeping the probe's lines meanwhile; then brings it back
/// where it stood, walks it to the build location where the arrival put it
/// elsewhere, and renews the probe's prims as the region now streams them.
/// Notes how each teleport went.
async fn away(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    scene: &mut Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    seconds: u32,
) -> Result<(), TestFailure> {
    let trip = avatars.trip.clone().ok_or_else(|| {
        TestFailure::State(format!(
            "{}: asked for `away` with no trip planned",
            probe.id
        ))
    })?;
    let session = &mut *avatars.session;
    let how = teleport_owner(
        session,
        probe,
        keys,
        transcript,
        trip.destination,
        &Vector {
            x: 128.0,
            y: 128.0,
            z: 30.0,
        },
    )
    .await?;
    transcript
        .body
        .push(format!("# away {how} seconds={seconds}"));
    collect(
        session,
        probe,
        keys,
        transcript,
        after(Duration::from_secs(u64::from(seconds))),
        Until::Deadline,
    )
    .await?;
    let how = teleport_owner(session, probe, keys, transcript, trip.origin, &trip.home).await?;
    transcript.body.push(format!("# back {how}"));
    let mut renewed: HashMap<ObjectKey, Object> = HashMap::new();
    let settled = settle_scene_observing(
        session,
        trip.grid,
        trip.build_position.clone(),
        SETTLE_WINDOW,
        SETTLE_IDLE,
        |event| {
            if let Event::ObjectAdded(object) = event
                && keys.contains(&object.full_id)
            {
                let _previous = renewed.insert(object.full_id, (**object).clone());
            }
            let _kept = transcript.take(&probe.id, keys, event);
        },
    )
    .await?;
    scene.seen.extend(settled.seen);
    for prim in scene.prims.values_mut() {
        if let Some(object) = renewed.remove(&prim.full_id) {
            *prim = object;
        }
    }
    Ok(())
}

/// Sits the avatar `by` names on prim A and notes the grid's sit response,
/// or that none came within [`ANSWER_TIMEOUT`].
async fn sit(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &BTreeMap<String, Object>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let target = prims
        .get("A")
        .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?
        .full_id;
    let (actor, label) = match by {
        By::Owner => (&mut *avatars.session, ""),
        By::Other => (&mut *avatars.other, " by=other"),
    };
    actor
        .send(Command::Sit {
            target,
            offset: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let outcome = actor
        .wait_for(ANSWER_TIMEOUT, |event| match event {
            Event::SitResult {
                sit_object,
                autopilot,
                sit_position,
                sit_rotation,
                camera_eye_offset,
                camera_at_offset,
                force_mouselook,
            } if *sit_object == target => Some(format!(
                "# sit{label} sit_position={} sit_rotation=<{:.3}, {:.3}, {:.3}, {:.3}> \
                 camera_eye={} camera_at={} force_mouselook={} autopilot={}",
                vector_note(sit_position),
                sit_rotation.x,
                sit_rotation.y,
                sit_rotation.z,
                sit_rotation.s,
                vector_note(camera_eye_offset),
                vector_note(camera_at_offset),
                u8::from(*force_mouselook),
                u8::from(*autopilot)
            )),
            _ => {
                match by {
                    By::Owner => {
                        let _kept = transcript.take(&probe.id, keys, event);
                    }
                    By::Other => transcript.take_other(keys, event),
                }
                None
            }
        })
        .await;
    match outcome {
        Ok(note) => transcript.body.push(note),
        Err(TestFailure::Timeout(_)) => {
            transcript.body.push(format!("# sit{label} response=none"));
        }
        Err(other) => return Err(other),
    }
    Ok(())
}

/// Waits for `prim`'s task inventory listing, keeping the probe's lines
/// that arrive meanwhile.
async fn listing(
    session: &mut Session,
    prim: &Object,
    watch: &mut Watch<'_>,
) -> Result<Vec<TaskInventoryItem>, TestFailure> {
    let task = prim.full_id;
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::TaskInventoryContents {
                task: got, items, ..
            } if *got == task => Some(Ok(items.clone())),
            Event::XferDecodeFailed {
                file: XferListing::TaskInventory { task: got, .. },
                error,
                ..
            } if *got == task => Some(Err(error.clone())),
            other => {
                let _kept = watch.transcript.take(watch.id, watch.prims, other);
                None
            }
        })
        .await?
        .map_err(|error| {
            TestFailure::Assertion(format!(
                "the prim's task inventory listing failed to parse: {error}"
            ))
        })
}

/// What a wait inside a probe's run keeps: the probe's lines from its
/// prims.
struct Watch<'a> {
    /// The probe's id.
    id: &'a str,
    /// Its prims.
    prims: &'a HashSet<ObjectKey>,
    /// Where its lines go.
    transcript: &'a mut Transcript,
}

/// Makes a script named `name` in `prim` and returns its task item id, once
/// the prim's listing shows it under that name.
async fn new_script(
    session: &mut Session,
    prim: &Object,
    name: &str,
    watch: &mut Watch<'_>,
) -> Result<InventoryKey, TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    session
        .send(Command::RezScript {
            target: prim.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: false,
                item: RestoreItem::new_script(agent, prim.full_id, name, Uuid::new_v4()),
            }),
        })
        .await?;
    let started = Instant::now();
    loop {
        session
            .send(Command::FetchTaskInventory {
                target: prim.scoped_id(),
                held_serial: None,
            })
            .await?;
        let listing = listing(session, prim, watch).await?;
        if let Some(item) = listing
            .iter()
            .find(|item| item.inv_type == InventoryType::Script && item.name == name)
        {
            return Ok(item.item_id);
        }
        if started.elapsed() >= STEP_TIMEOUT {
            let names: Vec<&str> = listing.iter().map(|item| item.name.as_str()).collect();
            return Err(TestFailure::Assertion(format!(
                "no script named {name:?} appeared in the prim; it holds {names:?}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The Trash folder of a login skeleton, or its root when it has none.
fn trash_folder(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::Trash.to_code())
        .or_else(|| folders.iter().find(|folder| folder.parent_id.is_none()))
        .map(|folder| folder.folder_id)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use sl_lsl_runtime::probe::Line;

    use std::collections::HashSet;

    use sl_client_tokio::{
        AgentKey, ChatAudible, ChatMessage, ChatSource, ChatType, Event, LandArea, LindenAmount,
        MoneyBalance, MoneyTransaction, ObjectKey, OwnerKey, RegionCoordinates, Uuid,
    };

    use super::{Cast, Transcript, clip, replace_ignoring_case, shown_line, split_name};

    #[test]
    fn a_fetch_record_keeps_its_url_out_of_the_transcript() -> Result<(), String> {
        let shown = |line: &str| {
            Line::parse(line)
                .map(|parsed| shown_line(line, &parsed))
                .map_err(|error| error.to_string())
        };
        assert_eq!(
            shown(
                "PROBE http/url rec=waiting drive=fetch url=http://h.example:12046/cap/k path=\"/a\""
            )?,
            "PROBE http/url rec=waiting drive=fetch url=<url> path=\"/a\""
        );
        assert_eq!(
            shown("PROBE http/url rec=granted url=http://h.example:12046/cap/k")?,
            "PROBE http/url rec=granted url=http://h.example:12046/cap/k"
        );
        Ok(())
    }

    #[test]
    fn a_name_is_replaced_as_a_whole_word_in_any_case() {
        assert_eq!(
            replace_ignoring_case("Knob Label, knob label: KNOB", "Knob", "<first>"),
            "<first> Label, <first> label: <first>"
        );
        assert_eq!(
            replace_ignoring_case("Knobs knobby aknob knob", "knob", "<first>"),
            "Knobs knobby aknob <first>"
        );
        assert_eq!(replace_ignoring_case("anything", "", "<first>"), "anything");
    }

    #[test]
    fn every_avatars_name_becomes_a_placeholder() {
        let cast = Cast {
            owner_name: split_name("Knob Label"),
            other_name: split_name("Lever Dial"),
            estate_name: split_name("Switch Gauge"),
            ..Cast::default()
        };
        assert_eq!(
            cast.scrub("Knob Label offered Lever Dial a seat"),
            "<first> <last> offered <other-first> <other-last> a seat"
        );
        assert_eq!(
            cast.scrub("Switch Gauge restarted the region"),
            "<estate-first> <estate-last> restarted the region"
        );
        assert_eq!(split_name("Single"), ("Single".to_owned(), String::new()));
    }

    #[test]
    fn a_balance_is_noted_as_its_change_with_the_parties_as_roles() {
        let owner = AgentKey::from(Uuid::from_u128(1));
        let other = AgentKey::from(Uuid::from_u128(2));
        let prim = Uuid::from_u128(3);
        let mut transcript = Transcript::new(Cast {
            owner: Some(owner),
            other: Some(other),
            owner_name: split_name("Knob Label"),
            other_name: split_name("Lever Dial"),
            estate_name: (String::new(), String::new()),
        });
        let prims: HashSet<ObjectKey> = HashSet::from([ObjectKey::from(prim)]);
        let balance = |amount: u64, transaction: Option<MoneyTransaction>| MoneyBalance {
            agent_id: owner,
            transaction_id: Uuid::nil(),
            success: true,
            balance: LindenAmount(amount),
            square_meters_credit: LandArea(0),
            square_meters_committed: LandArea(0),
            description: transaction
                .as_ref()
                .map_or_else(String::new, |_| "Lever Dial paid you L$2.".to_owned()),
            transaction,
        };
        // The first balance, a poll, is only what later changes are measured
        // from; an unchanged poll says nothing either.
        transcript.money("", &prims, &balance(100, None));
        transcript.money("", &prims, &balance(100, None));
        assert!(transcript.body.is_empty());
        transcript.money(
            "",
            &prims,
            &balance(
                102,
                Some(MoneyTransaction {
                    transaction_type: 5008,
                    source: OwnerKey::Agent(other),
                    dest: OwnerKey::Agent(owner),
                    amount: LindenAmount(2),
                    item_description: "Object".to_owned(),
                }),
            ),
        );
        // A change without a transaction is noted too, and the other
        // avatar's first balance has no change to tell.
        transcript.money("", &prims, &balance(101, None));
        transcript.money(" by=other", &prims, &balance(7, None));
        assert_eq!(
            transcript.body,
            vec![
                "# money-balance success=1 change=2 type=5008 source=other dest=owner amount=2 \
                 item=\"Object\" description=\"<other-first> <other-last> paid you L$2.\""
                    .to_owned(),
                "# money-balance success=1 change=-1 description=\"\"".to_owned(),
            ]
        );
        assert_eq!(transcript.party(&prims, prim), "probe");
        assert_eq!(transcript.party(&prims, Uuid::nil()), "nil");
        assert_eq!(transcript.party(&prims, Uuid::from_u128(9)), "someone");
    }

    #[test]
    fn the_other_avatars_debug_channel_chat_is_noted_as_its_own() {
        let prim = ObjectKey::from(Uuid::from_u128(3));
        let mut transcript = Transcript::new(Cast {
            owner_name: split_name("Knob Label"),
            other_name: split_name("Lever Dial"),
            ..Cast::default()
        });
        let prims: HashSet<ObjectKey> = HashSet::from([prim]);
        let chat = |source: ObjectKey, chat_type: ChatType, message: &str| {
            Event::ChatReceived(Box::new(ChatMessage {
                from_name: "Object".to_owned(),
                source: ChatSource::Object(source),
                owner_id: None,
                chat_type,
                audible: ChatAudible::Fully,
                position: RegionCoordinates::new(120.0, 128.0, 27.0),
                message: message.to_owned(),
            }))
        };
        transcript.take_other(
            &prims,
            &chat(
                prim,
                ChatType::DebugChannel,
                "OSSL Permission Error: Lever Dial",
            ),
        );
        transcript.take_other(&prims, &chat(prim, ChatType::Shout, "hi"));
        // Another object's debug chat is not the probe's.
        transcript.take_other(
            &prims,
            &chat(
                ObjectKey::from(Uuid::from_u128(9)),
                ChatType::DebugChannel,
                "elsewhere",
            ),
        );
        assert_eq!(
            transcript.body,
            vec![
                "# debug-channel by=other: from=\"Object\" \
                 text=\"OSSL Permission Error: <other-first> <other-last>\""
                    .to_owned(),
                "# chat by=other type=shout from=\"Object\" length=2 text=\"hi\"".to_owned(),
            ]
        );
    }

    #[test]
    fn a_long_text_is_clipped_at_a_character() {
        assert_eq!(clip("short"), "short");
        let long = "ä".repeat(70);
        assert_eq!(clip(&long), format!("{}…", "ä".repeat(60)));
    }
}
