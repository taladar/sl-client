//! Scripted timelines: "at t = 2 s, move that object; once the client has taken
//! delivery of the marker, kill it".
//!
//! Everything else in this crate answers something the client asked for. A
//! timeline is the other half: content that happens *to* a session because time
//! passed, which is what a viewer test of anything moving needs — a prim that
//! moves, an avatar that starts an animation, a region whose sky changes, an
//! agent walked over a border.
//!
//! # What runs it
//!
//! One task per session (`run_timeline`, spawned when the session is
//! activated). It waits for the agent's arrival, then walks the steps: each
//! waits for its [`At`], claims the step, and runs its [`Action`] through the
//! session's own flush rule. Every wait is a `tokio` sleep and every stamp comes
//! from the grid's injected clock ([`crate::time`]), so a test that pauses
//! tokio's timer pauses the script with it and a scripted five minutes costs no
//! wall-clock time at all.
//!
//! The base of a wait is the grid's clock and the wait itself is tokio's timer,
//! so the two have to be the same clock — which the crate's own pair are
//! ([`system_clock`](crate::system_clock) is what an unpaused tokio timer
//! tracks, and [`tokio_clock`](crate::tokio_clock) *is* that timer). A
//! deliberately skewed clock, of the kind `tests/clock.rs` uses to prove the
//! stamps come from the builder's, would put every scripted deadline that far
//! into the timer's future: fine for what that test measures, and not a clock
//! to run a script on.
//!
//! # Why the cursor travels
//!
//! A script that says "teleport, then move the prim you find there" has to
//! outlive the session it started in: a teleport destination is always a
//! *second* [`SimSession`], and a crossing promotes a circuit the client already
//! held. So the steps that have not run yet are handed to the destination
//! session (`hand_over`, called by `teleport.rs` and `crossing.rs` once the
//! client has actually arrived), and the session left behind keeps only the
//! prefix it already ran. That happens for a **client-initiated** teleport too,
//! which is the point: the script belongs to the avatar, not to the patch of
//! land.
//!
//! A destination whose own region declared a timeline loses it to the incoming
//! one; a script that has already finished hands over nothing, so the
//! destination's own script is left alone. A hand-over wakes the destination's
//! parked runner, so it does not matter whether the script arrives before or
//! after that session's own arrival.
//!
//! # What it is not
//!
//! There is no scheduler and no rewind: the steps of one timeline run strictly
//! in order, and a step waiting for something that never happens stops the
//! script rather than skipping ahead. A timeline is a script, not a simulation.

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_proto::{
    ArrivalPlacement, AvatarAppearance, ChatSource, ChatType, EnvironmentSettings,
    ExperienceEnvironmentPush, InstantMessage, Object, ParcelInfo, PlayingAnimation, RegionLimits,
    RegionLocalObjectId, RegionLocalParcelId, RegionStats, SequenceNumber, ServerEvent, SimSession,
    SimulatorTime, attachment_state_from_point,
};
use sl_types::key::InventoryKey;
use sl_types::lsl::Vector;
use sl_types::map::{RegionCoordinates, TeleportFlags};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use crate::driver::SharedSim;
use crate::error::Error;
use crate::runtime::GridCore;
use crate::scenario::SimHook;
use crate::world::{REAL_TIME_DILATION, RegionChange, SceneFixtures};

/// A test of a drained [`ServerEvent`]: what [`At::OnEvent`] waits for.
pub type EventPredicate = Arc<dyn Fn(&ServerEvent) -> bool + Send + Sync>;

/// An in-place edit of an object the region already holds
/// ([`Action::UpdateObject`]).
pub type ObjectEdit = Arc<dyn Fn(&mut Object) + Send + Sync>;

/// An in-place edit of a parcel record the region already holds
/// ([`Action::ChangeParcel`]).
pub type ParcelEdit = Arc<dyn Fn(&mut ParcelInfo) + Send + Sync>;

/// An in-place edit of the region's own configuration
/// ([`Action::ConfigureRegion`]).
pub type RegionEdit = Arc<dyn Fn(&mut RegionLimits) + Send + Sync>;

/// How long [`At::OnEvent`] waits for its event before giving the script up.
///
/// Generous, because the thing being waited for is usually a viewer doing real
/// work (decoding a scene, finishing a teleport); finite, so a test harness
/// reports a stalled script rather than hanging until its own timeout.
pub const EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`At::OnMarkerAck`] waits for the client's acknowledgement.
///
/// Shorter than [`EVENT_WAIT_TIMEOUT`]: an ack is one round trip over loopback,
/// and unlike an event it is an ordering nicety rather than a precondition — a
/// script whose ack never came carries on (see `wait_for_marker_ack`).
pub const MARKER_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// How often [`At::OnMarkerAck`] re-reads the session's unacknowledged set.
///
/// The session machine has no ack notification to subscribe to, so this is a
/// poll — the same one `teleport.rs` does for its `TeleportStart`.
const MARKER_ACK_POLL: Duration = Duration::from_millis(5);

/// The `audible` byte a simulator sends for a line that is fully audible.
const AUDIBLE: u8 = 1;

/// A scripted sequence of grid-side actions for one avatar.
///
/// Stated by a [`Scenario`](crate::Scenario) and run from the moment the agent
/// arrives. Empty by default: a grid whose scenario says nothing about time
/// behaves exactly as it did before timelines existed.
#[derive(Clone, Default)]
pub struct Timeline {
    /// The steps, in the order they run.
    pub steps: Vec<Step>,
}

impl std::fmt::Debug for Timeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeline")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Timeline {
    /// An empty timeline: nothing happens because time passed.
    #[must_use]
    pub const fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Appends a step.
    #[must_use]
    pub fn then(mut self, at: At, action: Action) -> Self {
        self.steps.push(Step { at, action });
        self
    }

    /// Appends a step `after` the previous one — the shape most scripts are
    /// written in.
    #[must_use]
    pub fn after(self, after: Duration, action: Action) -> Self {
        self.then(At::AfterPrevious(after), action)
    }

    /// Whether the timeline says nothing at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// One scripted action and what has to be true before it runs.
#[derive(Clone, Debug)]
pub struct Step {
    /// What is waited for before [`action`](Self::action) runs.
    pub at: At,
    /// What the grid does.
    pub action: Action,
}

/// When a [`Step`] runs.
#[derive(Clone)]
#[non_exhaustive]
pub enum At {
    /// This long after the agent's movement into the region completed.
    ///
    /// The base is the *session's* arrival, so a step handed to a teleport
    /// destination is timed from the arrival there.
    AfterArrival(Duration),
    /// This long after the previous step ran — or after the arrival, for the
    /// first step a session runs.
    AfterPrevious(Duration),
    /// As soon as the client acknowledges the most recent [`Action::Marker`].
    ///
    /// The one wait that is a *happens-before* rather than a duration: a client
    /// acknowledges a packet it has decoded and handled, so once the marker's
    /// ack is in, everything sent before it has already reached the viewer's own
    /// event stream. This is how a script says "not until the viewer has really
    /// seen the last thing" without guessing a number of milliseconds.
    OnMarkerAck,
    /// As soon as the session drains a [`ServerEvent`] the predicate accepts.
    ///
    /// The predicate is tested against every event since the timeline started,
    /// so a step waiting for something that has already happened runs at once.
    OnEvent(EventPredicate),
}

impl std::fmt::Debug for At {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AfterArrival(after) => f.debug_tuple("AfterArrival").field(after).finish(),
            Self::AfterPrevious(after) => f.debug_tuple("AfterPrevious").field(after).finish(),
            Self::OnMarkerAck => f.write_str("OnMarkerAck"),
            Self::OnEvent(_) => f.write_str("OnEvent(<predicate>)"),
        }
    }
}

/// What a [`Step`] does.
///
/// Every variant is something a simulator does unprompted. The world-changing
/// ones go through the region's shared store and publish to the region's change
/// stream, so a second avatar standing there is told as well — a scripted rez is
/// a rez, not a picture painted on one circuit.
#[derive(Clone)]
#[non_exhaustive]
pub enum Action {
    /// Adds an object to the region and streams it.
    RezObject(Box<Object>),
    /// Moves an object the region already holds.
    MoveObject {
        /// Which object.
        local_id: RegionLocalObjectId,
        /// Where it goes, in region-local metres.
        to: Vector,
    },
    /// Edits an object the region already holds and re-streams it — the general
    /// form of [`MoveObject`](Self::MoveObject).
    UpdateObject {
        /// Which object.
        local_id: RegionLocalObjectId,
        /// The edit, applied under the region lock.
        edit: ObjectEdit,
    },
    /// Removes an object from the region and kills it on the wire.
    KillObject(RegionLocalObjectId),
    /// Rezzes an object **worn** by this session's own avatar.
    ///
    /// The object's parent, attachment-point `state` byte and `AttachItemID`
    /// name-value are filled in here, so a script states the prim, the point and
    /// the item it stands for and nothing about how a simulator encodes an
    /// attachment.
    Attach {
        /// The prim to wear; its position and rotation are the offset from the
        /// attachment point.
        object: Box<Object>,
        /// The `AttachmentPoint` code it hangs from.
        point: u8,
        /// The inventory item the viewer keys the worn object on.
        item: InventoryKey,
    },
    /// Takes a worn object off: the kill an [`Attach`](Self::Attach) is undone
    /// by.
    Detach(RegionLocalObjectId),
    /// Plays (or stops) animations on this session's own avatar.
    ///
    /// The list is the whole set that is playing, as `AvatarAnimation` carries
    /// it: an animation left out of it has stopped.
    AnimateAvatar(Vec<PlayingAnimation>),
    /// Pushes an avatar's appearance — a changed shape, a fresh bake.
    SetAppearance(Box<AvatarAppearance>),
    /// Says something in local chat.
    Chat {
        /// The name the line is attributed to.
        from_name: String,
        /// Who is speaking (an agent, an object, or the system itself).
        source: ChatSource,
        /// The owner id of a speaking object; nil for anything else.
        owner_id: Uuid,
        /// Normal, whisper, shout, or one of the typing markers.
        chat_type: ChatType,
        /// Where in the region it was said.
        position: Vector,
        /// The line itself.
        message: String,
    },
    /// Delivers an instant message.
    Im(Box<InstantMessage>),
    /// Replaces what the session's `ExtEnvironment` capability serves — the
    /// region's sky and water, or one parcel's.
    ///
    /// It changes what a **fetch** answers, and nothing more: there is no
    /// message that carries new environment settings to a viewer already
    /// standing here. The viewer's reason to re-read is a `RegionInfo` —
    /// `LLViewerRegion::processRegionInfo` runs `LLRegionInfoModel`'s update
    /// signal, which `LLEnvironment` has hooked to `requestRegion()`, with no
    /// field compared on the way. So a script that means "the estate changed
    /// the sky" pairs this with [`ConfigureRegion`](Self::ConfigureRegion),
    /// which is what sends that `RegionInfo`.
    ///
    /// The one environment change that *is* pushed is a different action:
    /// [`PushExperienceEnvironment`](Self::PushExperienceEnvironment).
    SetEnvironment(Box<EnvironmentSettings>),
    /// Pushes an environment at the viewer as an **experience** does
    /// (`llSetEnvironment`) — the only live environment change in the protocol.
    ///
    /// Unlike [`SetEnvironment`](Self::SetEnvironment) this needs no
    /// [`ConfigureRegion`](Self::ConfigureRegion) beside it and changes nothing
    /// the region serves: it reaches the viewer at once, layers over the
    /// region's settings, and the
    /// [`Clear`](sl_proto::EnvironmentPushAction::Clear) case takes the layer
    /// away again — at which point the region's own sky is back with no
    /// refetch. A scenario that wants the *estate* to have changed its sky
    /// still wants `SetEnvironment` + `ConfigureRegion`.
    ///
    /// A [`Full`](sl_proto::EnvironmentPushAction::Full) push names a settings
    /// **asset** by id, which the viewer fetches over `ViewerAsset` — so the
    /// scenario has to have put those bytes in the grid's asset store
    /// (`environment_asset_to_bytes`) or the push resolves to nothing.
    PushExperienceEnvironment(Box<ExperienceEnvironmentPush>),
    /// Edits the region's own configuration and sends the `RegionInfo` that
    /// announces it — the estate floater's Region tab, saved by nobody.
    ///
    /// The message matters beyond the fields it carries: a viewer re-reads the
    /// region's environment when one arrives, so this is also how a scripted
    /// [`SetEnvironment`](Self::SetEnvironment) reaches a viewer that is
    /// already in the region.
    ConfigureRegion {
        /// The edit, applied to the region's limits under the region lock.
        edit: RegionEdit,
    },
    /// Edits a parcel record the region already holds and pushes it.
    ChangeParcel {
        /// Which parcel.
        local_id: RegionLocalParcelId,
        /// The edit, applied under the region lock.
        edit: ParcelEdit,
    },
    /// Teleports the avatar, exactly as a lure does
    /// ([`FakeGrid::teleport_agent`](crate::FakeGrid::teleport_agent)). The
    /// steps after it continue in the destination.
    Teleport {
        /// The destination region's name.
        region: String,
        /// Where the avatar lands.
        position: RegionCoordinates,
        /// Which way it faces on arrival.
        look_at: Vector,
    },
    /// Walks the avatar over a border into an adjacent region
    /// ([`FakeGrid::cross_agent`](crate::FakeGrid::cross_agent)). The steps
    /// after it continue on the far side.
    CrossRegion {
        /// The region walked into; it must border the one the avatar is in.
        region: String,
        /// Where in the destination the avatar lands.
        position: RegionCoordinates,
        /// The velocity it carries over, which is what keeps a walk a walk.
        velocity: Vector,
    },
    /// Reports a frame of region statistics.
    SimStats(Box<RegionStats>),
    /// Reports the simulator's world clock and sun state.
    SimulatorTime(Box<SimulatorTime>),
    /// Sends a marker ([`mod@crate::marker`]) a test can wait for, and which
    /// [`At::OnMarkerAck`] then waits for the client to acknowledge.
    Marker(String),
    /// Anything else, against the live session under its own lock.
    Custom(SimHook),
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RezObject(object) => f.debug_tuple("RezObject").field(&object.local_id).finish(),
            Self::MoveObject { local_id, to } => f
                .debug_struct("MoveObject")
                .field("local_id", local_id)
                .field("to", to)
                .finish(),
            Self::UpdateObject { local_id, .. } => f
                .debug_struct("UpdateObject")
                .field("local_id", local_id)
                .finish_non_exhaustive(),
            Self::KillObject(local_id) => f.debug_tuple("KillObject").field(local_id).finish(),
            Self::Attach { point, item, .. } => f
                .debug_struct("Attach")
                .field("point", point)
                .field("item", item)
                .finish_non_exhaustive(),
            Self::Detach(local_id) => f.debug_tuple("Detach").field(local_id).finish(),
            Self::AnimateAvatar(animations) => f
                .debug_tuple("AnimateAvatar")
                .field(&animations.len())
                .finish(),
            Self::SetAppearance(appearance) => f
                .debug_tuple("SetAppearance")
                .field(&appearance.avatar_id)
                .finish(),
            Self::Chat { message, .. } => f.debug_tuple("Chat").field(message).finish(),
            Self::Im(im) => f.debug_tuple("Im").field(&im.message).finish(),
            Self::SetEnvironment(_) => f.write_str("SetEnvironment(<settings>)"),
            Self::PushExperienceEnvironment(push) => f
                .debug_struct("PushExperienceEnvironment")
                .field("experience", &push.experience_id)
                .field("action", &push.action.name())
                .finish_non_exhaustive(),
            Self::ConfigureRegion { .. } => f.write_str("ConfigureRegion(<edit>)"),
            Self::ChangeParcel { local_id, .. } => f
                .debug_struct("ChangeParcel")
                .field("local_id", local_id)
                .finish_non_exhaustive(),
            Self::Teleport {
                region, position, ..
            } => f
                .debug_struct("Teleport")
                .field("region", region)
                .field("position", position)
                .finish_non_exhaustive(),
            Self::CrossRegion {
                region, position, ..
            } => f
                .debug_struct("CrossRegion")
                .field("region", region)
                .field("position", position)
                .finish_non_exhaustive(),
            Self::SimStats(_) => f.write_str("SimStats(<stats>)"),
            Self::SimulatorTime(_) => f.write_str("SimulatorTime(<time>)"),
            Self::Marker(name) => f.debug_tuple("Marker").field(name).finish(),
            Self::Custom(_) => f.write_str("Custom(<closure>)"),
        }
    }
}

/// Hands the steps `source` has not run yet to `destination`, and wakes the
/// destination's parked runner.
///
/// Called once the client has actually arrived in the destination — by
/// `teleport.rs` and by `crossing.rs`, so a client-initiated hop carries the
/// script as surely as a scripted one does. A source with nothing left hands
/// over nothing, which is what leaves a destination region's own timeline alone.
pub(crate) async fn hand_over(source: &SharedSim, destination: &SharedSim) {
    let remaining = {
        let mut state = source.state.lock().await;
        let cursor = state.timeline_cursor;
        if cursor >= state.timeline.len() {
            return;
        }
        let rest = state.timeline.split_off(cursor);
        // The prefix stays behind as the record of what this session ran; the
        // cursor at its end is what makes the source's own runner park.
        state.timeline_cursor = state.timeline.len();
        rest
    };
    let steps = remaining.len();
    {
        let mut state = destination.state.lock().await;
        state.timeline = remaining;
        state.timeline_cursor = 0;
        state.timeline_generation = state.timeline_generation.saturating_add(1);
    }
    destination.timeline_notify.notify_one();
    tracing::debug!("{steps} scripted step(s) handed to the destination session");
}

/// What a wait ended in.
enum Wait {
    /// The step may run.
    Ready,
    /// The script is over: the session closed, the grid is shutting down, or
    /// what the step waited for never happened.
    Stop,
}

/// Where a runner is in a script: which step of *which* script.
///
/// The generation is what makes the pair meaningful. A runner peeks a step,
/// waits for its `at`, and only then claims it — and in that window a hand-over
/// may have replaced the whole script, at which point index 2 names a different
/// step than the one that was waited for.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cursor {
    /// Which script: bumped by every hand-over into this session.
    generation: u64,
    /// Which step of it.
    index: usize,
}

/// What claiming the next step ended in.
enum Claim {
    /// The step is this runner's to execute.
    Took(Step),
    /// Something else moved the cursor (a hand-over): look again at whatever
    /// the session now holds.
    Moved,
    /// The script is over for this session.
    Stop,
}

/// The per-session timeline task: waits for the agent to arrive, then runs the
/// session's script, parking on the hand-over notification whenever it runs out
/// of steps rather than exiting — a script may still be walked over to it.
///
/// Exits when the session closes (a logout, or retirement after a teleport),
/// when the agent stops being the root agent here (a crossing made it a child),
/// or when the grid shuts down.
///
/// Boxed for the reason the teleport responder is: a scripted teleport activates
/// a session, and activating a session spawns one of these, so the future's type
/// would otherwise be infinitely recursive.
pub(crate) fn run_timeline(
    core: Arc<GridCore>,
    shared: SharedSim,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let mut events = shared.subscribe_events();
        let mut closed_rx = shared.closed_tx.subscribe();
        let mut shutdown_rx = shared.shutdown_rx.clone();
        if !wait_for_arrival(&mut events, &mut closed_rx, &mut shutdown_rx).await {
            return;
        }
        let arrival = shared.now();
        // The base of the first `AfterPrevious`: a script whose first step is
        // "half a second after the previous one" and has no previous step means
        // half a second after the avatar got here.
        let mut previous = arrival;
        let mut marker: Option<SequenceNumber> = None;
        loop {
            let Some((cursor, step)) = peek(&shared).await else {
                // Nothing left to run. Park rather than exit: a teleport or a
                // crossing may still hand this session a script.
                if park(&shared, &mut closed_rx, &mut shutdown_rx).await {
                    continue;
                }
                return;
            };
            let waited = wait_until(
                &shared,
                &step.at,
                arrival,
                previous,
                marker,
                &mut events,
                &mut closed_rx,
                &mut shutdown_rx,
            )
            .await;
            if matches!(waited, Wait::Stop) {
                return;
            }
            let step = match claim(&shared, cursor).await {
                Claim::Took(step) => step,
                Claim::Moved => continue,
                Claim::Stop => return,
            };
            previous = shared.now();
            marker = execute(&core, &shared, &step.action).await.or(marker);
        }
    })
}

/// The next step and its index, without claiming it.
///
/// Deliberately not claimed until after its wait: a hand-over during a long wait
/// must be able to take the step with it, which it cannot if the runner has
/// already consumed it.
async fn peek(shared: &SharedSim) -> Option<(Cursor, Step)> {
    let state = shared.state.lock().await;
    let cursor = Cursor {
        generation: state.timeline_generation,
        index: state.timeline_cursor,
    };
    let step = state.timeline.get(cursor.index).cloned();
    drop(state);
    Some((cursor, step?))
}

/// Takes ownership of the step at `cursor`, if it is still the next step of the
/// same script and this session is still the one running it.
async fn claim(shared: &SharedSim, cursor: Cursor) -> Claim {
    let mut state = shared.state.lock().await;
    if state.sim.is_closed() {
        return Claim::Stop;
    }
    if !state.sim.is_root_agent() {
        // A crossing made this session a child: the avatar is next door, and the
        // script went with it.
        return Claim::Stop;
    }
    if state.timeline_generation != cursor.generation || state.timeline_cursor != cursor.index {
        return Claim::Moved;
    }
    let Some(step) = state.timeline.get(cursor.index).cloned() else {
        return Claim::Moved;
    };
    state.timeline_cursor = cursor.index.saturating_add(1);
    drop(state);
    Claim::Took(step)
}

/// Resolves when this session's script must stop — the session closed or the
/// grid shut down — and otherwise never resolves.
///
/// Every wait below selects on this rather than on the two watch channels
/// directly, because a `changed()` that reports a *false* flag must go back to
/// waiting rather than let the wait fall through.
async fn stopped(closed_rx: &mut watch::Receiver<bool>, shutdown_rx: &mut watch::Receiver<bool>) {
    loop {
        if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
            return;
        }
        tokio::select! {
            changed = closed_rx.changed() => if changed.is_err() { return },
            changed = shutdown_rx.changed() => if changed.is_err() { return },
        }
    }
}

/// Waits until a script is handed to this session, or it is over. Returns
/// whether there is something to look at again.
async fn park(
    shared: &SharedSim,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> bool {
    tokio::select! {
        () = shared.timeline_notify.notified() => true,
        () = stopped(closed_rx, shutdown_rx) => false,
    }
}

/// Waits for this session's `AgentArrived`, which is where every script starts.
///
/// A child circuit never sees one until a crossing promotes it, so a neighbour's
/// runner simply waits here for as long as the circuit lives.
async fn wait_for_arrival(
    events: &mut broadcast::Receiver<ServerEvent>,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> bool {
    loop {
        let received = tokio::select! {
            received = events.recv() => received,
            () = stopped(closed_rx, shutdown_rx) => return false,
        };
        match received {
            Ok(ServerEvent::AgentArrived) => return true,
            Ok(ServerEvent::Disconnected | ServerEvent::LoggedOut)
            | Err(broadcast::error::RecvError::Closed) => return false,
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("a timeline runner missed {missed} events waiting to start");
            }
        }
    }
}

/// Waits for a step's [`At`].
#[expect(
    clippy::too_many_arguments,
    reason = "one argument per thing an `At` variant measures itself against, each a different \
              type; bundling them would name the same six values twice"
)]
async fn wait_until(
    shared: &SharedSim,
    at: &At,
    arrival: Instant,
    previous: Instant,
    marker: Option<SequenceNumber>,
    events: &mut broadcast::Receiver<ServerEvent>,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Wait {
    match at {
        At::AfterArrival(after) => sleep_past(arrival, *after, closed_rx, shutdown_rx).await,
        At::AfterPrevious(after) => sleep_past(previous, *after, closed_rx, shutdown_rx).await,
        At::OnMarkerAck => wait_for_marker_ack(shared, marker, closed_rx, shutdown_rx).await,
        At::OnEvent(predicate) => wait_for_event(predicate, events, closed_rx, shutdown_rx).await,
    }
}

/// Sleeps until `after` past `base`, on tokio's timer — the same one the grid's
/// clock reads when it is [`tokio_clock`](crate::tokio_clock), so a paused-time
/// test advances the script by hand.
async fn sleep_past(
    base: Instant,
    after: Duration,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Wait {
    // A deadline that cannot be represented is one no `Instant` could reach: run
    // the step now rather than never.
    let Some(deadline) = base.checked_add(after) else {
        return Wait::Ready;
    };
    tokio::select! {
        () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Wait::Ready,
        () = stopped(closed_rx, shutdown_rx) => Wait::Stop,
    }
}

/// Waits for the client to acknowledge the packet the last [`Action::Marker`]
/// went out as.
///
/// A missing ack does **not** stop the script. The ack orders two things the
/// client would otherwise see in either order, and an ordering nicety that could
/// strand a run would be worse than the reordering it prevents — the call
/// `teleport.rs` makes about its own `TeleportStart`. A step with no marker
/// behind it has nothing to wait for, and says so.
async fn wait_for_marker_ack(
    shared: &SharedSim,
    marker: Option<SequenceNumber>,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Wait {
    let Some(sequence) = marker else {
        tracing::warn!(
            "a timeline step waits on a marker acknowledgement, but no step before it sent a \
             marker; running it straight away"
        );
        return Wait::Ready;
    };
    let Some(deadline) = tokio::time::Instant::now().checked_add(MARKER_ACK_TIMEOUT) else {
        return Wait::Ready;
    };
    loop {
        if !shared.state.lock().await.sim.is_awaiting_ack(sequence) {
            return Wait::Ready;
        }
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => {
                tracing::warn!(
                    "no marker acknowledgement within {MARKER_ACK_TIMEOUT:?}; running the next \
                     scripted step regardless"
                );
                return Wait::Ready;
            }
            () = tokio::time::sleep(MARKER_ACK_POLL) => {}
            () = stopped(closed_rx, shutdown_rx) => return Wait::Stop,
        }
    }
}

/// Waits for an event the predicate accepts.
///
/// Unlike the ack, an event **is** a precondition — it is the whole content of
/// the step's `at` — so one that never comes stops the script rather than letting
/// the rest of it run against a world it was not written for.
async fn wait_for_event(
    predicate: &EventPredicate,
    events: &mut broadcast::Receiver<ServerEvent>,
    closed_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Wait {
    let Some(deadline) = tokio::time::Instant::now().checked_add(EVENT_WAIT_TIMEOUT) else {
        return Wait::Stop;
    };
    loop {
        let received = tokio::select! {
            received = tokio::time::timeout_at(deadline, events.recv()) => received,
            () = stopped(closed_rx, shutdown_rx) => return Wait::Stop,
        };
        match received {
            Ok(Ok(event)) => {
                if predicate(&event) {
                    return Wait::Ready;
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(missed))) => {
                tracing::warn!(
                    "a timeline runner missed {missed} events while waiting for one of its own; \
                     the event it is waiting for may have been among them"
                );
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => return Wait::Stop,
            Err(_elapsed) => {
                tracing::warn!(
                    "no matching event within {EVENT_WAIT_TIMEOUT:?}; abandoning the rest of the \
                     script"
                );
                return Wait::Stop;
            }
        }
    }
}

/// Runs one action, returning the sequence number an [`Action::Marker`] went out
/// as (what [`At::OnMarkerAck`] then waits for).
async fn execute(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    action: &Action,
) -> Option<SequenceNumber> {
    let now = shared.now();
    match action {
        Action::RezObject(object) => {
            let rezzed = (**object).clone();
            shared
                .with_region(move |world, sim, now| {
                    world.objects.push(rezzed.clone());
                    push_object(sim, &rezzed, now);
                    ((), vec![RegionChange::Rezzed(Box::new(rezzed))])
                })
                .await;
        }
        Action::MoveObject { local_id, to } => {
            let (local_id, to) = (*local_id, to.clone());
            shared
                .with_region(move |world, sim, now| {
                    edit_object(world, sim, now, local_id, |object| {
                        object.motion.position = to;
                    })
                })
                .await;
        }
        Action::UpdateObject { local_id, edit } => {
            let (local_id, edit) = (*local_id, Arc::clone(edit));
            shared
                .with_region(move |world, sim, now| {
                    edit_object(world, sim, now, local_id, |object| edit(object))
                })
                .await;
        }
        Action::KillObject(local_id) | Action::Detach(local_id) => {
            let local_id = *local_id;
            shared
                .with_region(move |world, sim, now| {
                    if world.remove_object(local_id).is_none() {
                        tracing::warn!(
                            "a scripted kill named {local_id:?}, which this region does not have"
                        );
                        return ((), Vec::new());
                    }
                    if let Err(error) = sim.send_kill_object(&[local_id], now) {
                        tracing::warn!("a scripted kill failed to send: {error}");
                    }
                    ((), vec![RegionChange::Killed(local_id)])
                })
                .await;
        }
        Action::Attach {
            object,
            point,
            item,
        } => {
            let (object, point, item) = ((**object).clone(), *point, *item);
            shared
                .with_region(move |world, sim, now| {
                    let mut worn = object;
                    worn.parent_id = world.avatar_local_id;
                    worn.state = attachment_state_from_point(point);
                    let attach = format!("AttachItemID STRING RW SV {}", item.uuid());
                    worn.name_value = if worn.name_value.is_empty() {
                        attach
                    } else {
                        format!("{}\n{attach}", worn.name_value)
                    };
                    world.objects.push(worn.clone());
                    push_object(sim, &worn, now);
                    ((), vec![RegionChange::Rezzed(Box::new(worn))])
                })
                .await;
        }
        Action::AnimateAvatar(animations) => {
            let animations = animations.clone();
            shared
                .with_state(move |state| {
                    let avatar = state.avatar.agent_id;
                    if let Err(error) = state.sim.send_avatar_animation(avatar, &animations, now) {
                        tracing::warn!("a scripted animation failed to send: {error}");
                    }
                })
                .await;
        }
        Action::SetAppearance(appearance) => {
            let appearance = appearance.clone();
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_avatar_appearance(&appearance, now) {
                        tracing::warn!("a scripted appearance failed to send: {error}");
                    }
                })
                .await;
        }
        Action::Chat {
            from_name,
            source,
            owner_id,
            chat_type,
            position,
            message,
        } => {
            let (from_name, source, owner_id) = (from_name.clone(), *source, *owner_id);
            let (chat_type, position, message) = (*chat_type, position.clone(), message.clone());
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_chat_from_simulator(
                        &from_name, source, owner_id, chat_type, AUDIBLE, position, &message, now,
                    ) {
                        tracing::warn!("a scripted chat line failed to send: {error}");
                    }
                })
                .await;
        }
        Action::Im(im) => {
            let im = im.clone();
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_instant_message(&im, now) {
                        tracing::warn!("a scripted instant message failed to send: {error}");
                    }
                })
                .await;
        }
        Action::SetEnvironment(environment) => {
            let environment = (**environment).clone();
            shared
                .with_sim(move |sim| sim.set_environment(environment))
                .await;
        }
        Action::PushExperienceEnvironment(push) => {
            let push = (**push).clone();
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_experience_environment_push(&push, now) {
                        tracing::warn!(
                            "a scripted experience environment push failed to send: {error}"
                        );
                    }
                })
                .await;
        }
        Action::ConfigureRegion { edit } => {
            let edit = Arc::clone(edit);
            shared
                .with_state(move |state| {
                    // Session lock first, then the region's -- the crate's one
                    // lock order, and the same one `answer_estate_request`
                    // takes for this very edit.
                    let mut world = state.world.lock();
                    edit(world.limits_mut(&state.identity));
                    let limits = world.limits(&state.identity);
                    if let Err(error) = state.sim.send_region_info(&limits, now) {
                        tracing::warn!("a scripted region configuration failed to send: {error}");
                    }
                    drop(state.changes.send(crate::world::RegionUpdate {
                        source: state.seq,
                        change: RegionChange::RegionConfigured(Box::new(limits)),
                    }));
                    drop(world);
                })
                .await;
        }
        Action::ChangeParcel { local_id, edit } => {
            let (local_id, edit) = (*local_id, Arc::clone(edit));
            shared
                .with_region(move |world, sim, now| {
                    let Some(parcel) = world.parcel_mut(local_id) else {
                        tracing::warn!(
                            "a scripted parcel edit named {local_id:?}, which this region does \
                             not have"
                        );
                        return ((), Vec::new());
                    };
                    edit(parcel);
                    let record = parcel.clone();
                    if let Err(error) = sim.send_parcel_properties(&record, now) {
                        tracing::warn!("a scripted parcel push failed to send: {error}");
                    }
                    ((), vec![RegionChange::ParcelChanged(Box::new(record))])
                })
                .await;
        }
        Action::Teleport {
            region,
            position,
            look_at,
        } => {
            if let Err(error) =
                teleport_named(core, shared, region, *position, look_at.clone()).await
            {
                tracing::warn!("a scripted teleport to {region:?} failed: {error}");
            }
        }
        Action::CrossRegion {
            region,
            position,
            velocity,
        } => {
            if let Err(error) = cross_named(core, shared, region, *position, velocity.clone()).await
            {
                tracing::warn!("a scripted crossing into {region:?} failed: {error}");
            }
        }
        Action::SimStats(stats) => {
            let stats = stats.clone();
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_sim_stats(&stats, now) {
                        tracing::warn!("a scripted stats frame failed to send: {error}");
                    }
                })
                .await;
        }
        Action::SimulatorTime(time) => {
            let time = time.clone();
            shared
                .with_sim(move |sim| {
                    if let Err(error) = sim.send_simulator_time(&time, now) {
                        tracing::warn!("a scripted simulator time failed to send: {error}");
                    }
                })
                .await;
        }
        Action::Marker(name) => {
            let name = name.clone();
            return shared
                .with_sim(move |sim| {
                    let sequence = sim.next_outgoing_sequence();
                    match sim.send_generic_message(&crate::marker::marker(&name), now) {
                        Ok(()) => Some(sequence),
                        Err(error) => {
                            tracing::warn!("a scripted marker failed to send: {error}");
                            None
                        }
                    }
                })
                .await;
        }
        Action::Custom(hook) => {
            let hook = Arc::clone(hook);
            shared.with_sim(move |sim| hook(sim, now)).await;
        }
    }
    None
}

/// Applies `edit` to an object the region holds, streams it to this session and
/// reports the change for the region's other viewers.
fn edit_object(
    world: &mut SceneFixtures,
    sim: &mut SimSession,
    now: Instant,
    local_id: RegionLocalObjectId,
    edit: impl FnOnce(&mut Object),
) -> ((), Vec<RegionChange>) {
    world.record_undo(local_id);
    let Some(object) = world.object_mut(local_id) else {
        tracing::warn!("a scripted edit named {local_id:?}, which this region does not have");
        return ((), Vec::new());
    };
    edit(object);
    let changed = object.clone();
    push_object(sim, &changed, now);
    ((), vec![RegionChange::Updated(Box::new(changed))])
}

/// Streams one object to this session's client, logging a failure rather than
/// failing the step.
fn push_object(sim: &mut SimSession, object: &Object, now: Instant) {
    if let Err(error) =
        sim.send_object_update(std::slice::from_ref(object), REAL_TIME_DILATION, now)
    {
        tracing::warn!("streaming a scripted object failed: {error}");
    }
}

/// Teleports the session's agent to the region called `region`.
async fn teleport_named(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    region: &str,
    position: RegionCoordinates,
    look_at: Vector,
) -> Result<(), Error> {
    let index = core
        .region_by_name(region)
        .ok_or_else(|| Error::UnknownRegion {
            region: region.to_owned(),
        })?;
    let request = crate::teleport::TeleportRequest {
        region: index,
        arrival: ArrivalPlacement { position, look_at },
        flags: TeleportFlags::VIA_LOCATION,
        progress: sl_proto::teleport_strings::SENDING_DEST,
    };
    let _outcome = crate::teleport::teleport_session(core, shared, request).await?;
    Ok(())
}

/// Walks the session's agent into the region called `region`.
async fn cross_named(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    region: &str,
    position: RegionCoordinates,
    velocity: Vector,
) -> Result<(), Error> {
    let index = core
        .region_by_name(region)
        .ok_or_else(|| Error::UnknownRegion {
            region: region.to_owned(),
        })?;
    let _destination =
        crate::crossing::cross_session(core, shared, index, position, velocity).await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{Action, At, Timeline};

    /// The builder appends in order, and `after` is the `AfterPrevious` spelling
    /// of the same step.
    #[test]
    fn a_timeline_keeps_the_order_it_was_written_in() {
        let timeline = Timeline::new()
            .then(
                At::AfterArrival(Duration::from_millis(10)),
                Action::Marker("one".to_owned()),
            )
            .after(Duration::from_millis(20), Action::Marker("two".to_owned()))
            .then(At::OnMarkerAck, Action::Marker("three".to_owned()));
        assert_eq!(timeline.steps.len(), 3);
        assert!(matches!(
            timeline.steps.first().map(|step| &step.at),
            Some(&At::AfterArrival(_))
        ));
        assert!(matches!(
            timeline.steps.get(1).map(|step| &step.at),
            Some(&At::AfterPrevious(_))
        ));
        assert!(matches!(
            timeline.steps.get(2).map(|step| &step.at),
            Some(&At::OnMarkerAck)
        ));
    }

    /// An empty timeline is what a scenario that says nothing about time
    /// carries, and the runner reads that as "no script".
    #[test]
    fn the_default_timeline_is_empty() {
        assert!(Timeline::default().is_empty());
        assert!(
            !Timeline::new()
                .after(Duration::ZERO, Action::Marker(String::new()))
                .is_empty()
        );
    }
}
