//! Instant messages between the grid's avatars: what is passed on and as
//! what, what the sender is told, what is kept for an avatar that is away;
//! and the teleport offers and requests that ride the same road.
//!
//! A viewer's `ImprovedInstantMessage` reaches its session as a
//! [`ServerEvent::InstantMessage`], a `StartLure` as a
//! [`ServerEvent::LureOffered`]; each session runs a responder
//! (`run_im_responder`) that passes them on to the root session of the avatar
//! they are for. What each live grid passes on, says and keeps was measured by
//! the `instant-messages` conformance case and the `teleport-offer-*`,
//! `teleport-request` and `teleport-lure-*` cases on aditi and the local
//! OpenSim (2026-10-07 and 2026-10-13, `book/src/gridspec/chat.md` § Instant
//! messages, `book/src/gridspec/teleport.md` § Offers and requests):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | a message's name, id, place and bucket | the sender's name, the session of the two, where the sender stands, one zero byte | as the viewer wrote them; an id it left out the session of the two |
//! | its region, estate and time | the sender's region, estate 1, the time the viewer wrote | the sender's region, estate 1, the time now |
//! | an automatic reply (dialog 20) | from no region, place zero, estate 0 | as any message |
//! | a script's dialog (19) from a viewer | dropped | passed on |
//! | a burst | fourteen messages a second, then a notice from no id signed "Second Life" | every message |
//! | a message to an avatar that is away, or to nobody's id | kept; the sender told, from the recipient's id, signed "Second Life" | the same, signed "System" |
//! | an automatic reply to an avatar that is away | kept, the sender told nothing | refused, from the sender's own id, signed "System" |
//! | a teleport offer | under a lure id the grid holds, its bucket naming the place and its rating | under the place itself packed into the id, no bucket |
//! | a declined or used lure | spent | a place, for ever |
//! | a teleport request (dialog 26) | its id as sent, from no region, estate 0, one zero byte | stated like a message |
//!
//! Both pass a message without text and one of a line break as they are, cut
//! a longer text at 1023 bytes, pass typing on and keep none, drop a message
//! box (dialog 1) and a dialog that is none, tell the sender nothing of a
//! message passed on, enforce no mute, and keep nothing of a teleport offer
//! or request for an avatar that is away. What is kept is read back through
//! the friendship store's [`keep`](crate::social) road: moved into the
//! avatar's session when it comes back, and handed over by
//! `RetrieveInstantMessages` and Second Life's `ReadOfflineMsgs`.
//!
//! An inventory offer and its answers ride the same road and are handed to
//! [`crate::gifts`].
//!
//! Not modelled: a script's message (the fake grid runs no scripts), how long
//! Second Life's suspension after a burst lasts (it is not measured; the fake
//! grid's lasts [`SECOND_LIFE_SUSPENSION`]), messages forwarded as mail, and
//! conferences, which start over `ChatSessionRequest` and wait for the group
//! sessions they share a road with (`server-fake-grid-group-chat`).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_proto::{DirectoryVisibility, ImDialog, InstantMessage, KeptMessage, ServerEvent, UserInfo};
use sl_types::key::AgentKey;
use sl_types::map::RegionCoordinates;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::driver::SharedSim;
use crate::runtime::GridCore;
use crate::social::{Whereabouts, now_epoch_seconds, session_of, tell, warn_on};

/// The estate every region of the fake grid belongs to.
const ESTATE: u32 = 1;

/// How many bytes of a message's text either grid passes on.
pub const LONGEST_TEXT_BYTES: usize = 1023;

/// The name Second Life signs what it says itself with.
pub const SECOND_LIFE_SIGNATURE: &str = "Second Life";

/// The name OpenSim signs what it says itself with.
pub const OPENSIM_SIGNATURE: &str = "System";

/// What Second Life tells the sender of a message it kept.
pub const SECOND_LIFE_KEPT_TEXT: &str =
    "User not online - message will be stored and delivered later.";

/// What OpenSim tells the sender of a message it kept.
pub const OPENSIM_KEPT_TEXT: &str = "User is not logged in. Message saved.";

/// What OpenSim tells the sender of an automatic reply to an avatar that is
/// away, which it refuses.
pub const OPENSIM_AUTO_RESPONSE_REFUSED: &str =
    "Unable to send instant message. User is not logged in.";

/// What Second Life tells a sender it suspends after a burst.
pub const SECOND_LIFE_SUSPENDED_TEXT: &str = "You have sent out excessive instant messages, and your ability to send IMs has been temporarily suspended. Try again later.";

/// How many messages a second Second Life passes on of one sender.
pub const SECOND_LIFE_BURST: usize = 14;

/// How long the fake grid's Second Life flavour suspends a sender after a
/// burst. Not measured: a message most of a minute later was passed on, and
/// the fake grid takes the shortest suspension that still drops the rest of
/// a burst.
pub const SECOND_LIFE_SUSPENSION: Duration = Duration::from_secs(1);

/// The window [`SECOND_LIFE_BURST`] counts messages in.
const BURST_WINDOW: Duration = Duration::from_secs(1);

/// How a grid passes instant messages on
/// ([`ImitatedGrid::im_policy`](crate::ImitatedGrid::im_policy)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImPolicy {
    /// How a message is stated when it is passed on.
    pub statement: Statement,
    /// Whether a viewer's message of a script's dialog (19) is passed on.
    pub viewer_from_task_passed_on: bool,
    /// How many messages a second one sender may send before it is
    /// suspended, and for how long; [`None`] for no limit.
    pub burst: Option<(usize, Duration)>,
    /// The name the grid signs what it says itself with.
    pub signature: &'static str,
    /// What the sender of a message the grid kept is told.
    pub kept_text: &'static str,
    /// What becomes of an automatic reply to an avatar that is away.
    pub away_auto_response: AwayAutoResponse,
    /// What a teleport offer's lure id is.
    pub lures: Lures,
}

impl Default for ImPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.im_policy()
    }
}

/// How a message is stated when it is passed on ([`ImPolicy::statement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statement {
    /// Rewritten: the sender's name, the session of the two as the id, where
    /// the sender stands, a bucket of one zero byte; the viewer's time kept;
    /// what the grid says itself states no time and one zero byte: Second
    /// Life.
    Rewritten,
    /// Stamped: the viewer's name, id, place and bucket, an id it left out the
    /// session of the two, and the time now; what the grid says itself states
    /// the time and no bucket: OpenSim.
    Stamped,
}

/// What becomes of an automatic reply to an avatar that is away
/// ([`ImPolicy::away_auto_response`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayAutoResponse {
    /// Kept, the sender told nothing: Second Life.
    Kept,
    /// Refused with [`OPENSIM_AUTO_RESPONSE_REFUSED`]: OpenSim.
    Refused,
}

/// What a teleport offer's lure id is ([`ImPolicy::lures`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lures {
    /// An id the grid mints and holds until it is spent — by its first use or
    /// a decline — naming where the offerer stood; the offer's bucket names
    /// the place and its rating: Second Life.
    Held,
    /// The offerer's place packed into the id (a fake parcel id, the height
    /// raised by two), good for ever; no bucket: OpenSim.
    Places,
}

/// Where a lure leads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Place {
    /// The region.
    pub(crate) region_handle: sl_wire::RegionHandle,
    /// Where in it.
    pub(crate) position: RegionCoordinates,
}

/// A lure the grid holds, not yet spent.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HeldLure {
    /// The avatar it was offered to.
    to: AgentKey,
    /// Where it leads.
    place: Place,
}

/// What an account says of its messages while it is away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Preferences {
    /// Whether they go out as mail (stored; the fake grid sends none).
    im_via_email: bool,
    /// Whether the account is shown in search.
    directory_visibility: DirectoryVisibility,
}

impl Default for Preferences {
    /// What both test accounts answered: no mail, shown in search.
    fn default() -> Self {
        Self {
            im_via_email: false,
            directory_visibility: DirectoryVisibility::Default,
        }
    }
}

/// Whether a sender's message is passed on ([`Messages::admit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// Passed on.
    Passed,
    /// Dropped: the sender is suspended, and has been told.
    Dropped,
    /// Dropped, and the sender is suspended from now on: tell it.
    Suspended,
}

/// The grid's lures and what it counts of each sender.
#[derive(Debug, Default)]
pub(crate) struct Messages {
    /// The lures held, by their id.
    lures: HashMap<Uuid, HeldLure>,
    /// When each sender's messages of the last [`BURST_WINDOW`] were sent.
    sent: HashMap<AgentKey, VecDeque<Instant>>,
    /// Until when each suspended sender is.
    suspended_until: HashMap<AgentKey, Instant>,
    /// What each account said of its messages.
    preferences: HashMap<AgentKey, Preferences>,
}

impl Messages {
    /// Spends the lure `id` offered to `by`, answering where it leads; or
    /// [`None`] where the grid holds no such lure for it.
    pub(crate) fn spend_lure(&mut self, id: Uuid, by: AgentKey) -> Option<Place> {
        let lure = self.lures.get(&id).copied().filter(|lure| lure.to == by)?;
        self.lures.remove(&id);
        Some(lure.place)
    }

    /// Counts a message of `from`'s sent at `now` against `burst`.
    fn admit(&mut self, from: AgentKey, now: Instant, burst: (usize, Duration)) -> Admission {
        let (most, suspension) = burst;
        if self
            .suspended_until
            .get(&from)
            .is_some_and(|until| now < *until)
        {
            return Admission::Dropped;
        }
        self.suspended_until.remove(&from);
        let sent = self.sent.entry(from).or_default();
        while sent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= BURST_WINDOW)
        {
            sent.pop_front();
        }
        if sent.len() >= most {
            sent.clear();
            if let Some(until) = now.checked_add(suspension) {
                self.suspended_until.insert(from, until);
            }
            return Admission::Suspended;
        }
        sent.push_back(now);
        Admission::Passed
    }
}

/// `text` cut to the [`LONGEST_TEXT_BYTES`] both grids pass on, at a
/// character's boundary.
fn cut(text: &str) -> String {
    let end = (0..=LONGEST_TEXT_BYTES.min(text.len()))
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    text.get(..end).unwrap_or_default().to_owned()
}

/// The id of what passes between `from` and `to`: the session of the two, or
/// the sender's own id for a message to itself, whose session is nil.
fn session_between(from: AgentKey, to: AgentKey) -> Uuid {
    let session = session_of(from, to);
    if session.is_nil() {
        from.uuid()
    } else {
        session
    }
}

/// The origin, where a message the grid states from nowhere is.
const fn nowhere() -> RegionCoordinates {
    RegionCoordinates::new(0.0, 0.0, 0.0)
}

impl ImPolicy {
    /// `im` of `from`'s as this grid passes it on to the avatar it names.
    pub(crate) fn passed_on(&self, from: &Whereabouts, im: &InstantMessage) -> InstantMessage {
        let to = im.to_agent_id;
        let passed = InstantMessage {
            from_agent_id: from.agent,
            from_agent_name: from.name.clone(),
            to_agent_id: to,
            dialog: im.dialog,
            from_group: false,
            region_id: Some(from.region_id),
            position: from.position,
            offline: false,
            timestamp: im.timestamp,
            id: session_between(from.agent, to),
            parent_estate_id: ESTATE,
            message: cut(&im.message),
            binary_bucket: vec![0],
        };
        match self.statement {
            Statement::Rewritten if im.dialog == ImDialog::DoNotDisturbAutoResponse => {
                InstantMessage {
                    region_id: None,
                    position: nowhere(),
                    parent_estate_id: 0,
                    ..passed
                }
            }
            Statement::Rewritten => passed,
            Statement::Stamped => InstantMessage {
                from_agent_name: im.from_agent_name.clone(),
                position: im.position,
                timestamp: Some(now_epoch_seconds()),
                id: if im.id.is_nil() { passed.id } else { im.id },
                binary_bucket: im.binary_bucket.clone(),
                ..passed
            },
        }
    }

    /// A teleport request of `from`'s as this grid passes it on.
    fn request_passed_on(&self, from: &Whereabouts, im: &InstantMessage) -> InstantMessage {
        match self.statement {
            Statement::Rewritten => InstantMessage {
                from_agent_id: from.agent,
                from_agent_name: from.name.clone(),
                to_agent_id: im.to_agent_id,
                dialog: ImDialog::TeleportRequest,
                from_group: false,
                region_id: None,
                position: im.position,
                offline: false,
                timestamp: im.timestamp,
                id: im.id,
                parent_estate_id: 0,
                message: cut(&im.message),
                binary_bucket: vec![0],
            },
            Statement::Stamped => self.passed_on(from, im),
        }
    }

    /// What the grid says itself to `to`: of `dialog`, from `from` under the
    /// grid's signature, under `id`, from nowhere.
    fn notice(
        &self,
        from: AgentKey,
        to: AgentKey,
        dialog: ImDialog,
        id: Uuid,
        text: &str,
    ) -> InstantMessage {
        let (timestamp, binary_bucket) = match self.statement {
            Statement::Rewritten => (None, vec![0]),
            Statement::Stamped => (Some(now_epoch_seconds()), Vec::new()),
        };
        InstantMessage {
            from_agent_id: from,
            from_agent_name: self.signature.to_owned(),
            to_agent_id: to,
            dialog,
            from_group: false,
            region_id: None,
            position: nowhere(),
            offline: false,
            timestamp,
            id,
            parent_estate_id: 0,
            message: text.to_owned(),
            binary_bucket,
        }
    }
}

/// `passed`, a message of `from`'s, as it is kept for an avatar that is away:
/// over `RetrieveInstantMessages` as it would have arrived, flagged kept,
/// with the time it was sent; as Second Life's `ReadOfflineMsgs` record, from
/// the sender's region and place, estate 1.
fn kept(from: &Whereabouts, passed: InstantMessage) -> KeptMessage {
    let message = InstantMessage {
        offline: true,
        timestamp: Some(now_epoch_seconds()),
        ..passed
    };
    let record = InstantMessage {
        region_id: Some(from.region_id),
        position: from.position,
        parent_estate_id: ESTATE,
        ..message.clone()
    };
    KeptMessage { message, record }
}

/// The per-session responder for instant messages: passes on what its client
/// sends to the avatars it names, until the session closes or the grid shuts
/// down.
pub(crate) async fn run_im_responder(core: Arc<GridCore>, shared: SharedSim) {
    let mut events = shared.subscribe_events();
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
            return;
        }
        let received = tokio::select! {
            received = events.recv() => received,
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    return;
                }
                continue;
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return;
                }
                continue;
            }
        };
        match received {
            Ok(event) => answer(&core, &shared, event).await,
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("instant message responder missed {missed} events");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Answers one thing the client sent.
async fn answer(core: &Arc<GridCore>, shared: &SharedSim, event: ServerEvent) {
    match event {
        ServerEvent::InstantMessage(im) => {
            let policy = core.im_policy;
            match im.dialog {
                ImDialog::Message => message(core, shared, &im).await,
                ImDialog::TypingStart | ImDialog::TypingStop => {
                    pass_on(core, shared, &im).await;
                }
                ImDialog::DoNotDisturbAutoResponse => auto_response(core, shared, &im).await,
                ImDialog::FromTask if policy.viewer_from_task_passed_on => {
                    let (from, passed) = stated(core, shared, &im).await;
                    if !deliver(core, &passed).await {
                        keep(core, &from, passed);
                    }
                }
                ImDialog::TeleportRequest => request(core, shared, &im).await,
                ImDialog::InventoryOffered => crate::gifts::offered(core, shared, &im).await,
                ImDialog::InventoryAccepted | ImDialog::InventoryDeclined
                    if core.gift_policy.answers == crate::gifts::GiftAnswers::Relayed =>
                {
                    pass_on(core, shared, &im).await;
                    if im.dialog == ImDialog::InventoryDeclined {
                        crate::gifts::discard(shared, &im).await;
                    }
                }
                ImDialog::LureDeclined => {
                    // A decline goes nowhere; on a grid that holds its lures
                    // it spends the one it names.
                    let by = shared.state.lock().await.avatar.agent_id;
                    let _spent = core.messages.lock().spend_lure(im.id, by);
                }
                _ => {}
            }
        }
        ServerEvent::LureOffered {
            message, targets, ..
        } => offer_lure(core, shared, &message, &targets).await,
        ServerEvent::RequestUserInfo => user_info(core, shared).await,
        ServerEvent::UpdateUserInfo {
            im_via_email,
            directory_visibility,
        } => {
            let agent = shared.state.lock().await.avatar.agent_id;
            core.messages.lock().preferences.insert(
                agent,
                Preferences {
                    im_via_email,
                    directory_visibility,
                },
            );
        }
        _ => {}
    }
}

/// Where the sender of `im` is, and `im` as the grid passes it on.
async fn stated(
    core: &GridCore,
    shared: &SharedSim,
    im: &InstantMessage,
) -> (Whereabouts, InstantMessage) {
    let from = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let passed = core.im_policy.passed_on(&from, im);
    (from, passed)
}

/// Sends `passed` to the avatar it names; answers whether it is here.
async fn deliver(core: &GridCore, passed: &InstantMessage) -> bool {
    tell(core, passed.to_agent_id, |state, now| {
        warn_on(
            state.sim.send_instant_message(passed, now),
            "an instant message",
        );
    })
    .await
}

/// Passes `im` on where its avatar is here, and drops it where it is not.
async fn pass_on(core: &GridCore, shared: &SharedSim, im: &InstantMessage) {
    if im.to_agent_id.uuid().is_nil() {
        return;
    }
    let (_from, passed) = stated(core, shared, im).await;
    let _here = deliver(core, &passed).await;
}

/// Keeps `passed`, a message of `from`'s, for the account it names, if there
/// is one.
fn keep(core: &GridCore, from: &Whereabouts, passed: InstantMessage) {
    let to = passed.to_agent_id;
    if core.account_by_agent(to).is_some() {
        core.social.lock().keep(to, kept(from, passed));
    }
}

/// Tells the sender of `passed` that the grid kept it: from the recipient's
/// id, under the grid's signature.
async fn tell_kept(core: &GridCore, passed: &InstantMessage) {
    let policy = core.im_policy;
    let (sender, recipient) = (passed.from_agent_id, passed.to_agent_id);
    let told = policy.notice(
        recipient,
        sender,
        ImDialog::Message,
        session_between(sender, recipient),
        policy.kept_text,
    );
    tell(core, sender, |state, now| {
        warn_on(state.sim.send_instant_message(&told, now), "a kept message");
    })
    .await;
}

/// An ordinary message: counted against a burst, passed on, or kept for an
/// avatar that is away and its sender told.
async fn message(core: &GridCore, shared: &SharedSim, im: &InstantMessage) {
    if im.to_agent_id.uuid().is_nil() {
        return;
    }
    let policy = core.im_policy;
    let (from, passed) = stated(core, shared, im).await;
    if let Some(burst) = policy.burst {
        let admission = core.messages.lock().admit(from.agent, shared.now(), burst);
        match admission {
            Admission::Passed => {}
            Admission::Dropped => return,
            Admission::Suspended => {
                let told = policy.notice(
                    AgentKey::from(Uuid::nil()),
                    from.agent,
                    ImDialog::Message,
                    Uuid::nil(),
                    SECOND_LIFE_SUSPENDED_TEXT,
                );
                warn_on(
                    shared
                        .with_sim(|sim| sim.send_instant_message(&told, shared.now()))
                        .await,
                    "a suspension",
                );
                return;
            }
        }
    }
    if deliver(core, &passed).await {
        return;
    }
    tell_kept(core, &passed).await;
    keep(core, &from, passed);
}

/// An automatic reply: passed on, or — for an avatar that is away — kept
/// without a word, or refused.
async fn auto_response(core: &GridCore, shared: &SharedSim, im: &InstantMessage) {
    if im.to_agent_id.uuid().is_nil() {
        return;
    }
    let policy = core.im_policy;
    let (from, passed) = stated(core, shared, im).await;
    if deliver(core, &passed).await {
        return;
    }
    match policy.away_auto_response {
        AwayAutoResponse::Kept => keep(core, &from, passed),
        AwayAutoResponse::Refused => {
            let told = policy.notice(
                from.agent,
                passed.to_agent_id,
                ImDialog::DoNotDisturbAutoResponse,
                session_between(from.agent, passed.to_agent_id),
                OPENSIM_AUTO_RESPONSE_REFUSED,
            );
            warn_on(
                shared
                    .with_sim(|sim| sim.send_instant_message(&told, shared.now()))
                    .await,
                "a refused automatic reply",
            );
        }
    }
}

/// A teleport request: passed on to an avatar that is here, and nothing kept
/// for one that is not.
async fn request(core: &GridCore, shared: &SharedSim, im: &InstantMessage) {
    if im.to_agent_id.uuid().is_nil() {
        return;
    }
    let from = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let passed = core.im_policy.request_passed_on(&from, im);
    let _here = deliver(core, &passed).await;
}

/// The whole metres of a place a lure states, truncated towards zero and kept
/// within what a `u16` holds.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=65535 before the cast; no From impl exists"
)]
fn whole_metres(value: f32) -> u16 {
    value.trunc().clamp(0.0, f32::from(u16::MAX)) as u16
}

/// The bucket of Second Life's offer: the place's region corner in global
/// metres, the landing position and facing in whole metres, and the region's
/// rating, with a padding space and a terminator.
fn lure_bucket(
    place: &Place,
    look_at: &sl_types::lsl::Vector,
    rating: sl_proto::Maturity,
) -> Vec<u8> {
    let (west, south) = place.region_handle.global_coordinates();
    let position = place.position;
    format!(
        "{west}|{south}|{:.0}|{:.0}|{:.0}|{:.0}|{:.0}|{:.0}|{} \0",
        position.x(),
        position.y(),
        position.z(),
        look_at.x,
        look_at.y,
        look_at.z,
        rating.to_login_access().unwrap_or("PG"),
    )
    .into_bytes()
}

/// An avatar offered each of `targets` a teleport to where it stands: an
/// offer to each that is here, and nothing for one that is not.
async fn offer_lure(core: &GridCore, shared: &SharedSim, text: &str, targets: &[AgentKey]) {
    let policy = core.im_policy;
    let (from, look_at, rating) = {
        let state = shared.state.lock().await;
        let rating = core
            .region(state.region)
            .map_or(sl_proto::Maturity::Pg, |region| region.config.maturity);
        (
            Whereabouts::of(&state),
            state.sim.arrival_position().look_at.clone(),
            rating,
        )
    };
    let place = Place {
        region_handle: from.handle,
        position: RegionCoordinates::new(
            from.position.x().trunc(),
            from.position.y().trunc(),
            from.position.z().trunc(),
        ),
    };
    for &to in targets {
        if to.uuid().is_nil() {
            continue;
        }
        let mut offer = InstantMessage {
            from_agent_id: from.agent,
            from_agent_name: from.name.clone(),
            to_agent_id: to,
            dialog: ImDialog::LureUser,
            from_group: false,
            region_id: Some(from.region_id),
            position: nowhere(),
            offline: false,
            timestamp: None,
            id: Uuid::nil(),
            parent_estate_id: ESTATE,
            message: cut(text),
            binary_bucket: Vec::new(),
        };
        match policy.lures {
            Lures::Held => {
                offer.id = core.minter.uuid();
                offer.binary_bucket = lure_bucket(&place, &look_at, rating);
            }
            Lures::Places => {
                offer.id = sl_wire::FakeParcelId {
                    region_handle: from.handle,
                    x: whole_metres(from.position.x()),
                    y: whole_metres(from.position.y()),
                    z: whole_metres(from.position.z()).saturating_add(2),
                }
                .to_uuid();
                offer.position = from.position;
                offer.timestamp = Some(now_epoch_seconds());
            }
        }
        if policy.lures == Lures::Held {
            core.messages
                .lock()
                .lures
                .insert(offer.id, HeldLure { to, place });
        }
        if !deliver(core, &offer).await && policy.lures == Lures::Held {
            core.messages.lock().lures.remove(&offer.id);
        }
    }
}

/// An avatar asked what its account says of messages while it is away.
async fn user_info(core: &GridCore, shared: &SharedSim) {
    let agent = shared.state.lock().await.avatar.agent_id;
    let preferences = core
        .messages
        .lock()
        .preferences
        .get(&agent)
        .copied()
        .unwrap_or_default();
    let info = UserInfo {
        im_via_email: preferences.im_via_email,
        directory_visibility: preferences.directory_visibility,
        email: String::new(),
    };
    warn_on(
        shared
            .with_sim(|sim| sim.send_user_info_reply(&info, shared.now()))
            .await,
        "the account's preferences",
    );
}

#[cfg(test)]
mod tests {
    use super::{
        Admission, ImPolicy, LONGEST_TEXT_BYTES, Messages, Place, SECOND_LIFE_BURST,
        SECOND_LIFE_SUSPENSION, Statement, cut, lure_bucket, session_between,
    };
    use crate::ImitatedGrid;
    use crate::social::Whereabouts;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::{ImDialog, InstantMessage, Maturity};
    use sl_types::key::AgentKey;
    use sl_types::lsl::Vector;
    use sl_types::map::RegionCoordinates;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    /// The avatar that writes.
    fn sender() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0x0f0f))
    }

    /// The avatar written to.
    fn recipient() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0xff00))
    }

    /// Where the sender stands.
    fn where_the_sender_is() -> Whereabouts {
        Whereabouts {
            agent: sender(),
            name: "Avatar Writer".to_owned(),
            region_id: Uuid::from_u128(0xabc),
            position: RegionCoordinates::new(10.0, 20.0, 30.0),
            handle: sl_wire::RegionHandle::from_grid(1000, 1000),
        }
    }

    /// A message as a viewer that makes things up writes it.
    fn claiming(dialog: ImDialog) -> InstantMessage {
        InstantMessage {
            from_agent_id: sender(),
            from_agent_name: "Somebody Else".to_owned(),
            to_agent_id: recipient(),
            dialog,
            from_group: false,
            region_id: Some(Uuid::from_u128(0x77)),
            position: RegionCoordinates::new(11.0, 22.0, 33.0),
            offline: true,
            timestamp: Some(1_234_567),
            id: Uuid::from_u128(0x99),
            parent_estate_id: 77,
            message: "claim".to_owned(),
            binary_bucket: b"bucket".to_vec(),
        }
    }

    #[test]
    fn second_life_rewrites_what_a_viewer_claims_but_the_time() {
        let policy = ImitatedGrid::SecondLife.im_policy();
        let from = where_the_sender_is();
        let passed = policy.passed_on(&from, &claiming(ImDialog::Message));
        assert_eq!(passed.from_agent_name, "Avatar Writer");
        assert_eq!(passed.id, Uuid::from_u128(0xf00f));
        assert_eq!(passed.region_id, Some(from.region_id));
        assert_eq!(passed.position, from.position);
        assert_eq!(passed.timestamp, Some(1_234_567));
        assert_eq!(passed.parent_estate_id, 1);
        assert_eq!(passed.binary_bucket, vec![0]);
        assert!(!passed.offline);
        // An automatic reply is from nowhere.
        let reply = policy.passed_on(&from, &claiming(ImDialog::DoNotDisturbAutoResponse));
        assert_eq!(reply.region_id, None);
        assert_eq!(reply.position, RegionCoordinates::new(0.0, 0.0, 0.0));
        assert_eq!(reply.parent_estate_id, 0);
    }

    #[test]
    fn opensim_passes_what_a_viewer_claims_on_but_the_region_estate_and_time() {
        let policy = ImitatedGrid::OpenSim.im_policy();
        assert_eq!(policy.statement, Statement::Stamped);
        let from = where_the_sender_is();
        let claimed = claiming(ImDialog::Message);
        let passed = policy.passed_on(&from, &claimed);
        assert_eq!(passed.from_agent_name, "Somebody Else");
        assert_eq!(passed.id, claimed.id);
        assert_eq!(passed.region_id, Some(from.region_id));
        assert_eq!(passed.position, claimed.position);
        assert_ne!(passed.timestamp, claimed.timestamp);
        assert_eq!(passed.parent_estate_id, 1);
        assert_eq!(passed.binary_bucket, claimed.binary_bucket);
        assert!(!passed.offline);
        // An id left out is the session of the two.
        let unnamed = InstantMessage {
            id: Uuid::nil(),
            ..claimed
        };
        assert_eq!(
            policy.passed_on(&from, &unnamed).id,
            session_between(sender(), recipient())
        );
    }

    #[test]
    fn a_message_to_oneself_is_under_ones_own_id() {
        assert_eq!(session_between(sender(), sender()), sender().uuid());
    }

    #[test]
    fn a_text_is_cut_at_1023_bytes_on_a_character() {
        assert_eq!(cut(&"x".repeat(3000)).len(), LONGEST_TEXT_BYTES);
        assert_eq!(cut("short"), "short");
        // A two-byte character straddling the cut is left out whole.
        let straddling = format!("{}é", "x".repeat(LONGEST_TEXT_BYTES - 1));
        assert_eq!(cut(&straddling).len(), LONGEST_TEXT_BYTES - 1);
    }

    #[test]
    fn second_life_passes_fourteen_a_second_and_then_suspends() {
        let mut messages = Messages::default();
        let start = Instant::now();
        let burst = (SECOND_LIFE_BURST, SECOND_LIFE_SUSPENSION);
        let answers: Vec<Admission> =
            std::iter::repeat_with(|| messages.admit(sender(), start, burst))
                .take(20)
                .collect();
        let passed = answers
            .iter()
            .filter(|answer| **answer == Admission::Passed)
            .count();
        assert_eq!(passed, SECOND_LIFE_BURST);
        assert_eq!(answers.get(SECOND_LIFE_BURST), Some(&Admission::Suspended));
        assert!(
            answers
                .iter()
                .skip(SECOND_LIFE_BURST + 1)
                .all(|answer| *answer == Admission::Dropped)
        );
        // Another sender is not suspended, and the suspension ends.
        assert_eq!(messages.admit(recipient(), start, burst), Admission::Passed);
        let later = start + SECOND_LIFE_SUSPENSION + Duration::from_millis(1);
        assert_eq!(messages.admit(sender(), later, burst), Admission::Passed);
    }

    #[test]
    fn a_held_lure_is_spent_once_and_by_its_target_alone() {
        let mut messages = Messages::default();
        let place = Place {
            region_handle: sl_wire::RegionHandle::from_grid(1000, 1000),
            position: RegionCoordinates::new(10.0, 10.0, 42.0),
        };
        let id = Uuid::from_u128(0x1e);
        messages.lures.insert(
            id,
            super::HeldLure {
                to: recipient(),
                place,
            },
        );
        assert_eq!(messages.spend_lure(id, sender()), None);
        assert_eq!(messages.spend_lure(id, recipient()), Some(place));
        assert_eq!(messages.spend_lure(id, recipient()), None);
    }

    #[test]
    fn a_second_life_offer_names_the_place_and_its_rating() {
        let place = Place {
            region_handle: sl_wire::RegionHandle::from_grid(997, 1002),
            position: RegionCoordinates::new(10.0, 10.0, 42.0),
        };
        let look = Vector {
            x: -1.0,
            y: 0.0,
            z: -0.0,
        };
        let bucket = lure_bucket(&place, &look, Maturity::Pg);
        assert_eq!(bucket, b"255232|256512|10|10|42|-1|0|-0|PG \0".to_vec());
        assert_eq!(bucket.len(), 35);
        // And the client reads it back.
        let offer = InstantMessage {
            dialog: ImDialog::LureUser,
            binary_bucket: lure_bucket(&place, &look, Maturity::Adult),
            ..claiming(ImDialog::LureUser)
        };
        let destination = offer.lure_destination();
        assert_eq!(
            destination.map(|destination| (destination.region_handle, destination.maturity)),
            Some((place.region_handle, Maturity::Adult))
        );
    }

    #[test]
    fn the_two_flavours_differ_where_they_were_measured_to() {
        let second_life: ImPolicy = ImitatedGrid::SecondLife.im_policy();
        let opensim = ImitatedGrid::OpenSim.im_policy();
        assert_eq!(second_life, ImPolicy::default());
        assert!(!second_life.viewer_from_task_passed_on);
        assert!(opensim.viewer_from_task_passed_on);
        assert!(second_life.burst.is_some());
        assert_eq!(opensim.burst, None);
        assert_ne!(second_life.signature, opensim.signature);
        assert_ne!(second_life.lures, opensim.lures);
    }
}
