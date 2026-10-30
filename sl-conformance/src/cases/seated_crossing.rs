//! A vehicle carrying its riders over a region border.
//!
//! An avatar that crosses on foot hands itself over (`region-crossing`). One
//! that crosses *seated* is carried: the thing the grid hands to the
//! neighbour is an **object** with avatars attached, and every region-local
//! id involved changes at the line — the vehicle's, and with it the
//! `ParentID` each rider is sent under. What this case records
//! (`book/src/gridspec/teleport.md`, *A seated crossing*), from both riders'
//! sessions at once:
//!
//! - the `CrossedRegion` each rider's session is told, and when;
//! - the vehicle's copies: its update on the region entered, its removal from
//!   the region left, their ids and their order against the riders';
//! - each rider's own avatar through it — the vehicle it is a child of, on
//!   which circuit — so that an unsit and a resit would show;
//! - the other rider, as the first rider's session sees it;
//! - what the vehicle's script says of itself on the way (its region, its
//!   position, and the permissions a sit implied), and whether the controls
//!   it took are taken again.
//!
//! The vehicle is a cube the first avatar rezzes beside itself, with a script
//! uploaded into it that gives it a sit target and, told to on a chat
//! channel, steps it at the nearest border an announced neighbour shares with
//! `llSetPos` — which carries a non-physical object over a border on OpenSim
//! (`SceneObjectGroup.AbsolutePosition`). Where it does not cross, the script
//! is asked to try again as a physical object pushed with `llSetVelocity`,
//! and which of the two carried it is written down.
//!
//! Live grids only: the fake grid's ridden hand-over is driven by its tests
//! (`sl-fake-grid/tests/client_end_to_end.rs`), since it runs no scripts.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, ChatChannel, ChatSource, ChatType, Command, Event, Object, ObjectKey, PrimShape,
    RegionHandle, RegionLocalObjectId, ScopedObjectId, ScriptControlAction, Uuid, Vector,
};

use crate::cases::logout_seated::{
    DeleteOnDrop, delete_command, look_at, sight_object, trash_folder,
};
use crate::cases::sit_stand::plant_script;
use crate::context::{Session, TestContext, TestFailure};
use crate::crossing::{
    Arrival, Edge, Neighbourhood, nearest_border, slot_offset, watch_neighbours,
};
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, wait_for_own_new_object};

/// Where the avatars log in on OpenSim: twelve metres from the eastern border
/// of the block's south-western region, as `region-crossing` does.
const OPENSIM_START: &str = "uri:Default Region&244&128&25";

/// Where the avatars log in on Second Life: a parcel in Morris that allows
/// building, ten metres from the region's western border with Bonifacio (a
/// two-by-two block of mainland). The Bonifacio side of that border is a
/// no-script parcel.
const ADITI_START: &str = "uri:Morris&10&100&35";

/// How long to watch an arrival for its neighbour announcements.
const NEIGHBOUR_WINDOW: Duration = Duration::from_secs(30);

/// The gap without a new announcement that ends that watch.
const NEIGHBOUR_QUIET: Duration = Duration::from_secs(6);

/// How long to wait for the vehicle to be rezzed, its script to compile and
/// each rider to be seen seated.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How far from the first avatar the vehicle is rezzed, along the border,
/// in metres: beside it rather than between it and the border.
const SEAT_OFFSET_M: f32 = 1.5;

/// How far below a standing avatar's centre the centre of a half-metre cube
/// resting on the same ground is, in metres.
const SEAT_BELOW_AVATAR_M: f32 = 0.6;

/// How far past the border the vehicle is taken, in metres: far enough that
/// the riders are not handed straight back.
const PAST_BORDER_M: f32 = 8.0;

/// The side of a region in metres: both live grids' regions here are the
/// standard size.
const REGION_SIDE_M: f32 = 256.0;

/// How far one step of the script's moves the vehicle, in metres. Matches
/// `STEP` in [`VEHICLE_SCRIPT`].
///
/// Not a round figure, because an avatar may stand at a whole metre: a step
/// that lands *exactly* on the border (x = 256.0) neither moves the vehicle
/// nor carries it over on OpenSim, and every step after it aims at the same
/// spot again (`seated-crossing`, 2026-10-28).
const STEP_M: f32 = 1.9;

/// How long the script waits between two steps. Matches its timer.
const STEP_SECS: f64 = 0.5;

/// How long after the last step a crossing is still waited for.
const CROSSING_GRACE: Duration = Duration::from_secs(20);

/// How long to keep watching after both riders have arrived, for what the
/// grid sends once the vehicle has settled next door.
const SETTLE: Duration = Duration::from_secs(12);

/// The chat channel the vehicle listens on, for its owner only.
const CHANNEL: i32 = 7311;

/// The vehicle's script.
///
/// `go <direction> <steps>` steps the vehicle 1.9 m at a time with
/// `llSetPos`; `push <direction> <steps>` makes it physical and weightless
/// and pushes it with `llSetVelocity` instead. Everything it says to its owner
/// starts `SC|`, then what happened, the region it is in and its position.
/// It asks the avatar on its sit target for the two permissions a sit grants
/// without a dialog, takes two controls once granted, and says what it still
/// holds after a change of region and when it stops. A change of region is
/// also carried into the next report: it happens before the riders are root
/// agents in the region entered, where an `llOwnerSay` may not reach them.
const VEHICLE_SCRIPT: &str = r#"integer CHANNEL = 7311;
float STEP = 1.9;
integer steps;
vector dir;
integer pushing;
key rider;
string pending;

report(string what)
{
    llOwnerSay("SC|" + what + pending + "|" + llGetRegionName() + "|" + (string)llGetPos());
    pending = "";
}

default
{
    state_entry()
    {
        llSitTarget(<0.0, 0.0, 1.0>, ZERO_ROTATION);
        llListen(CHANNEL, "", llGetOwner(), "");
        report("ready");
    }

    listen(integer channel, string name, key id, string message)
    {
        list words = llParseString2List(message, [" "], []);
        string verb = llList2String(words, 0);
        if (verb == "go" || verb == "push")
        {
            dir = (vector)llList2String(words, 1);
            steps = (integer)llList2String(words, 2);
            pushing = (verb == "push");
            if (pushing)
            {
                llSetPos(llGetPos() + <0.0, 0.0, 1.0>);
                llSetBuoyancy(1.0);
                llSetStatus(STATUS_PHYSICS, TRUE);
            }
            report(verb);
            llSetTimerEvent(0.5);
        }
    }

    timer()
    {
        if (steps <= 0)
        {
            llSetTimerEvent(0.0);
            if (pushing)
            {
                llSetVelocity(ZERO_VECTOR, FALSE);
                llSetStatus(STATUS_PHYSICS, FALSE);
            }
            report("done " + (string)llGetPermissions() + " " + (string)llGetPermissionsKey());
            return;
        }
        steps = steps - 1;
        if (pushing)
        {
            llSetVelocity(dir * STEP * 2.0, FALSE);
        }
        else
        {
            llSetPos(llGetPos() + dir * STEP);
        }
        report("step");
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            key sitter = llAvatarOnSitTarget();
            report("link " + (string)llGetNumberOfPrims());
            if (sitter != NULL_KEY && sitter != rider)
            {
                rider = sitter;
                llRequestPermissions(sitter, PERMISSION_TAKE_CONTROLS | PERMISSION_TRIGGER_ANIMATION);
            }
        }
        if (change & CHANGED_REGION)
        {
            pending = " (region " + (string)llGetPermissions() + " " + (string)llGetPermissionsKey() + ")";
            report("region");
        }
    }

    run_time_permissions(integer granted)
    {
        report("granted " + (string)granted);
        if (granted & PERMISSION_TAKE_CONTROLS)
        {
            llTakeControls(CONTROL_FWD | CONTROL_BACK, TRUE, TRUE);
        }
    }
}
"#;

/// The case's overall budget: two logins' neighbour watches, a rez, a
/// compile, two sits, up to two crossings and their settles.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// One rider's avatar, as one session saw it at one moment.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    /// Seconds from the start of the ride.
    at: f64,
    /// The region it was reported in.
    region: RegionHandle,
    /// Its id there: the circuit it came down and the region-local id.
    id: ScopedObjectId,
    /// The object it is a child of; zero when it is not seated.
    parent: RegionLocalObjectId,
}

/// Everything one rider's session saw of a ride.
#[derive(Debug)]
struct Ride {
    /// The session's own avatar.
    agent: Uuid,
    /// The other rider, whose updates are kept as well.
    other: Option<Uuid>,
    /// The vehicle.
    vehicle: ObjectKey,
    /// The region the ride started in, for slot offsets.
    home: RegionHandle,
    /// When the ride began.
    started: Instant,
    /// The arrival in the next region, when there was one.
    arrival: Option<Arrival>,
    /// Every teleport event: a ride over a border is not a teleport.
    teleports: Vec<(f64, String)>,
    /// The vehicle's copies as they appeared: when, in which region, under
    /// which id.
    vehicle_seen: Vec<(f64, RegionHandle, ScopedObjectId)>,
    /// Removals of a copy of the vehicle: when, and which. The session drops
    /// the region left's copy of an object the moment the region entered
    /// sends its own, so this says when the vehicle changed hands, not whether
    /// the grid sent a `KillObject` (the trace log says that).
    vehicle_removed: Vec<(f64, ScopedObjectId)>,
    /// Every change in where the session's own avatar was reported.
    own: Vec<Seen>,
    /// Every change in where the other rider was reported.
    others: Vec<Seen>,
    /// Removals of a copy of either rider's avatar: when, whose, which.
    riders_removed: Vec<(f64, &'static str, ScopedObjectId)>,
    /// What the vehicle's script said, `SC|` stripped.
    script: Vec<(f64, String)>,
    /// Every `ScriptControlChange`: when, and what it took or released.
    controls: Vec<(f64, String)>,
    /// Every sit response: a resit would show here.
    sit_results: Vec<f64>,
    /// Every permission request: a sit grants its two without one.
    permission_requests: Vec<(f64, String)>,
    /// Every alert.
    alerts: Vec<(f64, String)>,
}

impl Ride {
    /// A ride of `agent` on `vehicle`, starting in `home`.
    fn new(agent: Uuid, other: Option<Uuid>, vehicle: ObjectKey, home: RegionHandle) -> Self {
        Self {
            agent,
            other,
            vehicle,
            home,
            started: Instant::now(),
            arrival: None,
            teleports: Vec::new(),
            vehicle_seen: Vec::new(),
            vehicle_removed: Vec::new(),
            own: Vec::new(),
            others: Vec::new(),
            riders_removed: Vec::new(),
            script: Vec::new(),
            controls: Vec::new(),
            sit_results: Vec::new(),
            permission_requests: Vec::new(),
            alerts: Vec::new(),
        }
    }

    /// Seconds since the ride began.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Whether the session's own avatar has been seen seated on a copy of the
    /// vehicle in the region it arrived in.
    fn aboard_after_arrival(&self) -> bool {
        let Some(arrival) = self.arrival else {
            return false;
        };
        self.own.iter().any(|seen| {
            seen.region == arrival.region_handle
                && self.vehicle_seen.iter().any(|(_at, region, id)| {
                    *region == arrival.region_handle && id.id() == seen.parent
                })
        })
    }

    /// The vehicle's copy in `region`, when one was seen.
    fn vehicle_in(&self, region: RegionHandle) -> Option<ScopedObjectId> {
        self.vehicle_seen
            .iter()
            .rev()
            .find(|(_at, seen_in, _id)| *seen_in == region)
            .map(|(_at, _region, id)| *id)
    }

    /// Folds `seen` into `trail` when it says something the last entry did
    /// not.
    fn push_seen(trail: &mut Vec<Seen>, seen: Seen) {
        let changed = trail.last().is_none_or(|last| {
            last.region != seen.region || last.id != seen.id || last.parent != seen.parent
        });
        if changed {
            trail.push(seen);
        }
    }

    /// Folds one event into the ride.
    fn note(&mut self, event: &Event) {
        let at = self.now();
        match event {
            Event::RegionChanged {
                region_handle,
                sim,
                circuit,
                world_reset,
            } if self.arrival.is_none() => {
                tracing::info!(at, ?region_handle, "the ride arrived next door");
                self.arrival = Some(Arrival {
                    region_handle: *region_handle,
                    sim: *sim,
                    circuit: *circuit,
                    world_reset: *world_reset,
                    at,
                });
            }
            Event::TeleportStarted { .. }
            | Event::TeleportFinished { .. }
            | Event::TeleportFailed { .. }
            | Event::TeleportLocal { .. } => {
                self.teleports.push((at, format!("{event:?}")));
            }
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                self.note_object(at, object);
            }
            Event::ObjectRemoved { local_id, .. } => {
                if self
                    .vehicle_seen
                    .iter()
                    .any(|(_at, _region, id)| id == local_id)
                {
                    tracing::info!(at, ?local_id, "a copy of the vehicle went");
                    self.vehicle_removed.push((at, *local_id));
                }
                let rider = |trail: &[Seen]| trail.iter().any(|seen| seen.id == *local_id);
                if rider(&self.own) {
                    self.riders_removed.push((at, "own", *local_id));
                } else if rider(&self.others) {
                    self.riders_removed.push((at, "other", *local_id));
                }
            }
            Event::ChatReceived(chat)
                if chat.chat_type == ChatType::Owner
                    && chat.source == ChatSource::Object(self.vehicle) =>
            {
                if let Some(said) = chat.message.strip_prefix("SC|") {
                    tracing::info!(at, said, "the vehicle");
                    self.script.push((at, said.to_owned()));
                }
            }
            Event::ScriptControlChange(controls) => {
                let said = controls
                    .iter()
                    .map(|control| {
                        format!(
                            "{}:{:#x}{}",
                            match control.action {
                                ScriptControlAction::Take => "take",
                                ScriptControlAction::Release => "release",
                            },
                            control.controls.bits(),
                            if control.pass_to_agent { "+pass" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                tracing::info!(at, said, "script controls");
                self.controls.push((at, said));
            }
            Event::SitResult { .. } => {
                tracing::info!(at, "a sit response");
                self.sit_results.push(at);
            }
            Event::ScriptPermissionRequest(request) => {
                tracing::info!(at, ?request, "a permission request");
                self.permission_requests
                    .push((at, format!("{:?}", request.permissions)));
            }
            Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
                tracing::info!(at, message, "an alert");
                self.alerts.push((at, message.trim().to_owned()));
            }
            _other => {}
        }
    }

    /// Folds an object update into the ride.
    fn note_object(&mut self, at: f64, object: &Object) {
        if object.full_id == self.vehicle {
            let id = object.scoped_id();
            if !self
                .vehicle_seen
                .iter()
                .any(|(_at, _region, seen)| *seen == id)
            {
                tracing::info!(at, ?id, region = ?object.region_handle, "a copy of the vehicle");
                self.vehicle_seen.push((at, object.region_handle, id));
            }
            return;
        }
        let uuid = object.full_id.uuid();
        let seen = Seen {
            at,
            region: object.region_handle,
            id: object.scoped_id(),
            parent: object.parent_id,
        };
        if uuid == self.agent {
            Self::push_seen(&mut self.own, seen);
        } else if Some(uuid) == self.other {
            Self::push_seen(&mut self.others, seen);
        }
    }

    /// Watches `session` for `budget`, or until `until` holds, and says
    /// whether it came to hold.
    async fn observe(
        &mut self,
        session: &mut Session,
        budget: Duration,
        until: impl Fn(&Self) -> bool + Send + Sync,
    ) -> Result<bool, TestFailure> {
        let started = Instant::now();
        loop {
            if until(self) {
                return Ok(true);
            }
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(false);
            }
            let woken = session
                .wait_for(remaining, |event| {
                    self.note(event);
                    until(self).then_some(())
                })
                .await;
            match woken {
                Ok(()) => {}
                Err(TestFailure::Timeout(_)) => return Ok(until(self)),
                Err(other) => return Err(other),
            }
        }
    }

    /// Where `region` lies from the region the ride started in.
    fn slots(&self, region: RegionHandle) -> String {
        let (dx, dy) = slot_offset(self.home, region);
        format!("{dx}/{dy}")
    }

    /// A trail of an avatar as one string: `at@region:local^parent`.
    fn trail(&self, trail: &[Seen], offset: f64) -> String {
        joined(trail.iter().map(|seen| {
            format!(
                "{:.2}@{}:{}^{}",
                seen.at - offset,
                self.slots(seen.region),
                seen.id.id().0,
                seen.parent.0
            )
        }))
    }

    /// Writes the ride down under `prefix`. Times are seconds from the
    /// arrival, where there was one, else from the start of the ride.
    fn record(&self, prefix: &str, metrics: &mut Metrics) {
        let key = |name: &str| format!("{prefix}{name}");
        let offset = self.arrival.map_or(0.0, |arrival| arrival.at);
        metrics.set(&key("crossed"), self.arrival.is_some());
        if let Some(arrival) = &self.arrival {
            metrics.set(&key("arrived_secs"), arrival.at);
            metrics.set(&key("direction"), self.slots(arrival.region_handle));
            metrics.set(&key("world_reset"), arrival.world_reset);
            metrics.set(&key("aboard_after"), self.aboard_after_arrival());
        }
        metrics.set(
            &key("teleport_events"),
            joined(
                self.teleports
                    .iter()
                    .map(|(at, what)| format!("{:.2}:{what}", at - offset)),
            ),
        );
        metrics.set(
            &key("vehicle_seen"),
            joined(self.vehicle_seen.iter().map(|(at, region, id)| {
                format!("{:.2}@{}:{}", at - offset, self.slots(*region), id.id().0)
            })),
        );
        metrics.set(
            &key("vehicle_removed"),
            joined(
                self.vehicle_removed
                    .iter()
                    .map(|(at, id)| format!("{:.2}:{}", at - offset, id.id().0)),
            ),
        );
        metrics.set(&key("own"), self.trail(&self.own, offset));
        if self.other.is_some() {
            metrics.set(&key("other"), self.trail(&self.others, offset));
        }
        metrics.set(
            &key("riders_removed"),
            joined(
                self.riders_removed
                    .iter()
                    .map(|(at, whose, id)| format!("{:.2}:{whose}:{}", at - offset, id.id().0)),
            ),
        );
        if !self.script.is_empty() {
            metrics.set(
                &key("script"),
                joined(
                    self.script
                        .iter()
                        .map(|(at, said)| format!("{:.2}:{said}", at - offset)),
                ),
            );
        }
        metrics.set(
            &key("controls"),
            joined(
                self.controls
                    .iter()
                    .map(|(at, said)| format!("{:.2}:{said}", at - offset)),
            ),
        );
        metrics.set(
            &key("sit_results"),
            joined(
                self.sit_results
                    .iter()
                    .map(|at| format!("{:.2}", at - offset)),
            ),
        );
        metrics.set(
            &key("permission_requests"),
            joined(
                self.permission_requests
                    .iter()
                    .map(|(at, what)| format!("{:.2}:{what}", at - offset)),
            ),
        );
        metrics.set(
            &key("alerts"),
            joined(
                self.alerts
                    .iter()
                    .map(|(at, what)| format!("{:.2}:{what}", at - offset)),
            ),
        );
    }
}

/// `items` joined by spaces, or `none`.
fn joined(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(" ")
    }
}

/// The unit direction out of the region over `edge`, as an LSL vector.
const fn direction(edge: Edge) -> &'static str {
    match edge {
        Edge::East => "<1,0,0>",
        Edge::West => "<-1,0,0>",
        Edge::North => "<0,1,0>",
        Edge::South => "<0,-1,0>",
    }
}

/// The spot beside `standing`, along `edge` rather than towards it, on the
/// ground the avatar stands on.
fn beside(standing: &Vector, edge: Edge) -> Vector {
    let (dx, dy) = match edge {
        Edge::East | Edge::West => (0.0, SEAT_OFFSET_M),
        Edge::North | Edge::South => (SEAT_OFFSET_M, 0.0),
    };
    Vector {
        x: standing.x + dx,
        y: standing.y + dy,
        z: standing.z - SEAT_BELOW_AVATAR_M,
    }
}

/// Where the vehicle at `seat` ends up once over `edge`, in the coordinates
/// of the region across it: [`PAST_BORDER_M`] beyond the line.
fn across(edge: Edge, seat: &Vector) -> Vector {
    let (x, y) = match edge {
        Edge::East => (PAST_BORDER_M, seat.y),
        Edge::West => (REGION_SIDE_M - PAST_BORDER_M, seat.y),
        Edge::North => (seat.x, PAST_BORDER_M),
        Edge::South => (seat.x, REGION_SIDE_M - PAST_BORDER_M),
    };
    Vector { x, y, z: seat.z }
}

/// How many steps of [`STEP_M`] take the vehicle from `seat` over `edge` and
/// [`PAST_BORDER_M`] beyond it.
fn steps_over(edge: Edge, seat: &Vector) -> u32 {
    let metres = edge.distance_from(seat) + PAST_BORDER_M;
    let mut steps = 0_u32;
    let mut covered = 0.0_f32;
    while covered < metres {
        steps = steps.saturating_add(1);
        covered += STEP_M;
    }
    steps
}

/// Sits `session`'s avatar on `vehicle` and waits until its own avatar is
/// sent as a child of it.
async fn sit_on(
    session: &mut Session,
    ride: &mut Ride,
    vehicle: &Object,
) -> Result<bool, TestFailure> {
    session
        .send(Command::Sit {
            target: vehicle.full_id,
            offset: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let local = vehicle.local_id;
    ride.observe(session, STEP_TIMEOUT, |ride| {
        ride.own.last().is_some_and(|seen| seen.parent == local)
    })
    .await
}

/// Which way the vehicle was carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carried {
    /// Stepped with `llSetPos`.
    Stepped,
    /// Pushed as a physical object with `llSetVelocity`.
    Pushed,
}

impl Carried {
    /// The script's verb for it.
    const fn verb(self) -> &'static str {
        match self {
            Self::Stepped => "go",
            Self::Pushed => "push",
        }
    }
}

/// Tells the vehicle to set off and watches both riders until each has
/// arrived next door, aboard, and the settle has passed — or until the
/// steps and their grace have run out.
async fn ride_over(
    first: &mut Session,
    second: Option<&mut Session>,
    rides: (&mut Ride, Option<&mut Ride>),
    how: Carried,
    edge: Edge,
    seat: &Vector,
    steps: u32,
) -> Result<(), TestFailure> {
    first
        .send(Command::Chat {
            message: format!("{} {} {steps}", how.verb(), direction(edge)),
            chat_type: ChatType::Normal,
            channel: ChatChannel(CHANNEL),
        })
        .await?;
    let stepping = Duration::from_secs_f64(f64::from(steps) * STEP_SECS);
    let budget = stepping.saturating_add(CROSSING_GRACE);
    let (first_ride, second_ride) = rides;
    let landing = across(edge, seat);
    let first_watch = watch_ride(first, first_ride, budget, &landing);
    match (second, second_ride) {
        (Some(session), Some(ride)) => {
            let (one, two) = tokio::join!(first_watch, watch_ride(session, ride, budget, &landing));
            one?;
            two?;
        }
        _alone => first_watch.await?,
    }
    Ok(())
}

/// Watches one rider until it has arrived aboard, then for the settle. The
/// camera is aimed at `landing` the moment the crossing arrives: its
/// region-local numbers carry over the border unchanged, and would otherwise
/// look at the far side of the region entered.
async fn watch_ride(
    session: &mut Session,
    ride: &mut Ride,
    budget: Duration,
    landing: &Vector,
) -> Result<(), TestFailure> {
    let started = Instant::now();
    let crossed = ride
        .observe(session, budget, |ride| ride.arrival.is_some())
        .await?;
    if crossed {
        look_at(session, landing).await?;
    }
    let left = budget.saturating_sub(started.elapsed());
    let _arrived = ride
        .observe(session, left, Ride::aboard_after_arrival)
        .await?;
    let _settled = ride.observe(session, SETTLE, |_ride| false).await?;
    Ok(())
}

/// Rides the vehicle over a border with two avatars aboard and records what
/// each rider's session saw.
#[derive(Debug)]
pub struct SeatedCrossing;

impl GridTest for SeatedCrossing {
    fn name(&self) -> &'static str {
        "seated-crossing"
    }

    fn description(&self) -> &'static str {
        "Ride a scripted vehicle over a region border with two avatars aboard"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            ADITI_START
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut guard = DeleteOnDrop {
                commander: ctx.primary().commander(),
                trash: None,
                seat: None,
            };
            let outcome = drive(ctx, &mut guard).await;
            // Whatever went wrong, nobody is left aboard and the vehicle goes.
            let stood = ctx.primary().send(Command::Stand).await;
            if let Some(second) = ctx.secondary() {
                let _stood = second.send(Command::Stand).await;
            }
            let deleted = match (guard.seat.take(), guard.trash) {
                (Some(seat), Some(trash)) => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    ctx.primary().send(delete_command(seat, trash)).await
                }
                _nothing => Ok(()),
            };
            outcome?;
            stood?;
            deleted
        })
    }
}

/// Brings both avatars aboard and rides them over the border.
async fn drive(ctx: &mut TestContext, guard: &mut DeleteOnDrop) -> Result<(), TestFailure> {
    let first = ctx.primary();
    let agent: AgentKey = first
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let folders = first
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _other => None,
        })
        .await?;
    guard.trash = trash_folder(&folders);
    first.wait_for_region(REGION_TIMEOUT).await?;
    let first = ctx.primary();
    let home = first
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("login established no region handle".to_owned()))?;
    // The child circuits have to exist before the crossing, as on foot.
    let found: Neighbourhood = watch_neighbours(first, NEIGHBOUR_WINDOW, NEIGHBOUR_QUIET).await?;
    found.record("neighbours_", Some(home), ctx.metrics());
    let own = found.own.clone().ok_or_else(|| {
        TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
    })?;
    let Some((destination, edge, distance)) = nearest_border(&found, home, &own.position) else {
        ctx.mark_partial(
            "no announced neighbour shares a border with the region the avatars stand in, so \
             there is nothing to ride over",
        );
        return Ok(());
    };
    tracing::info!(edge = edge.label(), distance, "the border to ride over");
    let metrics = ctx.metrics();
    metrics.set("edge", edge.label());
    metrics.set("edge_distance_m", f64::from(distance));

    let seat_at = beside(&own.position, edge);
    // A grid streams by the camera, and a session's camera starts at the
    // region's centre: in a busy region a vehicle rezzed far from it is
    // queued behind the whole scene, and was not sent within half a minute
    // (aditi's Morris sandbox, 2026-10-28). Both riders look at the seat.
    look_at(ctx.primary(), &seat_at).await?;
    if let Some(second) = ctx.secondary() {
        look_at(second, &seat_at).await?;
    }
    let second_agent = ctx.secondary().and_then(|second| second.agent_id());
    let first = ctx.primary();
    first
        .send(Command::RezObject {
            shape: PrimShape::cube(seat_at.clone()),
            group_id: None,
        })
        .await?;
    let vehicle = match wait_for_own_new_object(first, &HashSet::default(), STEP_TIMEOUT).await? {
        Ok(vehicle) => vehicle,
        Err(reason) => {
            ctx.metrics().set("rez_refused", reason.clone());
            ctx.mark_partial(&format!(
                "the vehicle could not be rezzed beside the border: {reason}"
            ));
            return Ok(());
        }
    };
    guard.seat = Some(vehicle.scoped_id());
    let compiled = plant_script(first, &vehicle, VEHICLE_SCRIPT).await?;
    ctx.metrics().set("script_compiled", compiled);
    if !compiled {
        return Err(TestFailure::Assertion(
            "the vehicle's script did not compile".to_owned(),
        ));
    }

    let mut first_ride = Ride::new(
        agent.uuid(),
        second_agent.map(|second| second.uuid()),
        vehicle.full_id,
        home,
    );
    let mut second_ride = second_agent
        .map(|second| Ride::new(second.uuid(), Some(agent.uuid()), vehicle.full_id, home));
    // The first avatar on the sit target; the second wherever the grid puts
    // a rider on a taken target (`book/src/gridspec/movement.md`, Sitting).
    let seated = sit_on(ctx.primary(), &mut first_ride, &vehicle).await?;
    ctx.metrics().set("first_seated", seated);
    if !seated {
        return Err(TestFailure::Assertion(
            "the first avatar was never seen seated on the vehicle".to_owned(),
        ));
    }
    if let (Some(second), Some(ride)) = (ctx.secondary(), second_ride.as_mut()) {
        let _seen = sight_object(second, vehicle.full_id, STEP_TIMEOUT).await;
        let seated = sit_on(second, ride, &vehicle).await?;
        ctx.metrics().set("second_seated", seated);
        if !seated {
            second_ride = None;
        }
    }
    // The permission grants and the controls taken have arrived by now; what
    // the crossing does to them starts from here.
    let steps = steps_over(edge, &seat_at);
    ctx.metrics().set("steps", steps);
    ctx.metrics().set("destination", {
        let (dx, dy) = slot_offset(home, destination.region_handle);
        format!("{dx}/{dy}")
    });

    let mut carried = None;
    for how in [Carried::Stepped, Carried::Pushed] {
        let (first, second) = match second_ride {
            Some(_) => {
                let (first, second) = ctx.primary_and_secondary().ok_or_else(|| {
                    TestFailure::Assertion("the second avatar's session is gone".to_owned())
                })?;
                (first, Some(second))
            }
            None => (ctx.primary(), None),
        };
        ride_over(
            first,
            second,
            (&mut first_ride, second_ride.as_mut()),
            how,
            edge,
            &seat_at,
            steps,
        )
        .await?;
        if first_ride.arrival.is_some() {
            carried = Some(how);
            break;
        }
        tracing::info!(?how, "the vehicle did not carry the riders over");
    }
    if let Some(copy) = first_ride
        .arrival
        .and_then(|arrival| first_ride.vehicle_in(arrival.region_handle))
    {
        guard.seat = Some(copy);
    }
    let metrics = ctx.metrics();
    metrics.set(
        "carried_by",
        match carried {
            Some(Carried::Stepped) => "llSetPos",
            Some(Carried::Pushed) => "llSetVelocity",
            None => "nothing",
        },
    );
    first_ride.record("first_", metrics);
    if let Some(ride) = &second_ride {
        ride.record("second_", metrics);
    }
    if carried.is_none() {
        ctx.mark_partial(
            "neither llSetPos nor a physical push carried the vehicle over the border",
        );
        return Ok(());
    }
    if !first_ride.teleports.is_empty() {
        return Err(TestFailure::Assertion(format!(
            "a seated crossing raised a teleport event: {}",
            first_ride
                .teleports
                .first()
                .map_or("", |(_at, what)| what.as_str())
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Edge, PAST_BORDER_M, STEP_M, across, beside, steps_over};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Vector;

    #[test]
    fn the_steps_take_the_vehicle_past_the_border() {
        let seat = Vector {
            x: 244.0,
            y: 129.5,
            z: 24.4,
        };
        let steps = steps_over(Edge::East, &seat);
        assert_eq!(steps, 11);
        let travelled = f32::from(u16::try_from(steps).unwrap_or(u16::MAX)) * STEP_M;
        assert!(
            seat.x + travelled > 256.0 + 6.0,
            "{travelled} m from {}",
            seat.x
        );
    }

    #[test]
    fn the_landing_is_past_the_border_in_the_region_entered() {
        let seat = Vector {
            x: 9.9,
            y: 101.5,
            z: 34.4,
        };
        let landing = across(Edge::West, &seat);
        // West out of one region is in at the east of the next.
        assert!(Edge::East.distance_from(&landing) > 0.0);
        assert!((Edge::East.distance_from(&landing) - PAST_BORDER_M).abs() < 1e-4);
        assert!((landing.y - seat.y).abs() < 1e-4);
    }

    #[test]
    fn the_seat_is_beside_the_avatar_and_no_nearer_the_border() {
        let standing = Vector {
            x: 8.0,
            y: 10.0,
            z: 30.0,
        };
        for edge in [Edge::East, Edge::West, Edge::North, Edge::South] {
            let seat = beside(&standing, edge);
            assert!(
                (edge.distance_from(&seat) - edge.distance_from(&standing)).abs() < 1e-4,
                "{edge:?}"
            );
        }
    }
}
