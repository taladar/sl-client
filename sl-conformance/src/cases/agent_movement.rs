//! How a grid moves an agent: what each control does, how fast, and what the
//! grid says back while it does it.
//!
//! A viewer never moves its own avatar. It states which controls are held in
//! an `AgentUpdate` and the simulator decides where the avatar ends up; the
//! viewer's own prediction is corrected by the updates of the avatar's object
//! that come back. This case holds one control after another and samples
//! every such update — position, velocity, acceleration, rotation, the
//! collision plane — and every change of the avatar's own animation set, so
//! the answer to "how does this grid walk an avatar" is a table rather than
//! a belief (`book/src/gridspec/movement.md`).
//!
//! The legs, in order:
//!
//! - **standing** — the baseline: how often a resting avatar is reported;
//! - **first steps** — the forward key from where the login put the avatar.
//!   An avatar that does not move is tried again after a `FINISH_ANIM`, and
//!   then after a short hop into the air, and what freed it is recorded;
//! - **walking**, three times: the key stated once and left to the session's
//!   once-a-second `AgentUpdate`, re-stated ten times a second, and re-stated
//!   a hundred times a second — the reaction to a high update rate;
//! - **backwards**, **sideways**, the **nudge** and **fast** bits, **always
//!   run**, and the **crouch**;
//! - **turning** on the spot;
//! - **jumping**, with and without telling the simulator its pre-jump and
//!   landing animations have finished, and how long the avatar is then held
//!   before the forward key moves it again;
//! - **flying**: the climb (and whether it meets a ceiling), the hover, level
//!   flight, the descent;
//! - **falling** from where the flight ended, the landing, and the hold that
//!   follows a hard one.
//!
//! What it asserts is little, and the same on every grid: the avatar walks
//! when told to, stops when let go, leaves the ground when told to fly and
//! comes back down. Everything else is recorded.
//!
//! Live grids only. The fake grid decodes an `AgentUpdate` and moves nothing
//! (`server-world-agent-movement`), so there is nothing offline to hold to
//! these answers yet.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Command, ControlFlags, Event, MovementMode, PlayingAnimation, Rotation, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, is_opensim};

/// Where the avatar logs in on OpenSim: the middle of a hundred-metre square
/// of level ground in the block's north-eastern region (flattened for this
/// case with `terrain modify fill 25 -rec=20,130,100,100`; the rest of the
/// block is hills, and a slope changes every speed measured on it).
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// How long the scene is left to settle after the arrival before anything is
/// measured: a region still streaming its objects reports its avatars late.
const SETTLE: Duration = Duration::from_secs(15);

/// How often a held control is re-stated, unless a leg says otherwise.
const RESTATE: Duration = Duration::from_millis(100);

/// The re-statement interval of the high-rate walk: a hundred a second, the
/// order of the reference viewer's ceiling of 125.
const RESTATE_FAST: Duration = Duration::from_millis(10);

/// How long a walking leg lasts.
const WALK: Duration = Duration::from_secs(6);

/// How long the shorter ground legs last.
const SHORT: Duration = Duration::from_secs(4);

/// How long the avatar is watched after a control is let go.
const REST: Duration = Duration::from_secs(3);

/// How long the climb lasts. Long enough to meet a ceiling a few hundred
/// metres up, if the grid has one.
const CLIMB: Duration = Duration::from_secs(30);

/// The width of the windows the climb's vertical speed is reported in.
const CLIMB_WINDOW_SECS: f64 = 5.0;

/// How long the hover lasts.
const HOVER: Duration = Duration::from_secs(6);

/// How long a fall may take before the case gives up on a landing.
const FALL_BUDGET: Duration = Duration::from_secs(120);

/// How long the jump key is held waiting for the avatar to leave the ground.
const JUMP_BUDGET: Duration = Duration::from_secs(5);

/// How long an avatar in the air is given to come back down from a jump.
const LANDING_BUDGET: Duration = Duration::from_secs(8);

/// How long the forward key is held waiting for a held avatar to move.
const HELD_BUDGET: Duration = Duration::from_secs(8);

/// The rise that counts as having left the ground, in metres.
const LIFT_OFF_M: f32 = 0.25;

/// The horizontal distance that counts as having moved, in metres.
const MOVED_M: f32 = 0.5;

/// The speed below which an avatar counts as at rest, in metres a second.
const AT_REST_M_PER_S: f32 = 0.05;

/// The built-ins a Second Life simulator holds the avatar still for until the
/// viewer reports them finished (`LLAgent::onAnimStop`), by their names in
/// [`sl_anim`]'s registry.
pub(crate) const HOLDING_ANIMATIONS: [&str; 4] = ["standup", "pre_jump", "land", "medium_land"];

/// How long a single report of an avatar at rest has to stand uncontradicted
/// before the avatar counts as having stopped, in seconds.
const RESTED_FOR_SECS: f64 = 0.5;

/// How far from every border the legs' home is kept, in metres.
const HOME_MARGIN_M: f32 = 80.0;

/// How far from where it started the avatar may be before a leg heads back
/// there rather than further out, in metres.
const NEAR_HOME_M: f32 = 8.0;

/// The speed that counts as moving for the purposes of a landing, in metres a
/// second: an avatar has to have been seen going at least this fast before
/// coming to rest counts as having landed.
const IN_MOTION_M_PER_S: f32 = 0.5;

/// The case's overall budget: some forty legs of a few seconds each, a
/// half-minute climb and the fall from it.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Holds one control after another and records how the grid moves the avatar.
#[derive(Debug)]
pub struct AgentMovement;

impl GridTest for AgentMovement {
    fn name(&self) -> &'static str {
        "agent-movement"
    }

    fn description(&self) -> &'static str {
        "Walk, run, jump, fly and fall, sampling every update of our own avatar"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let mut watch = Watch::new(agent);
            let mut found = Found::default();
            let outcome = drive_all(session, &mut watch, &mut found).await;
            // Whatever a leg ran into, the avatar is not left with a key held.
            let released = session
                .send(Command::SetControls(ControlFlags::empty()))
                .await;
            let walking = session
                .send(Command::SetAlwaysRun {
                    mode: MovementMode::Walk,
                })
                .await;
            found.record(&watch, ctx.metrics());
            outcome?;
            released?;
            walking?;
            found.check()
        })
    }
}

/// One update of our own avatar's object.
#[derive(Debug, Clone)]
struct Sample {
    /// Seconds from the start of the watch.
    at: f64,
    /// Where the grid put the avatar.
    position: Vector,
    /// The velocity it reported.
    velocity: Vector,
    /// The acceleration it reported.
    acceleration: Vector,
    /// The body rotation it reported.
    rotation: Rotation,
    /// The collision plane it reported.
    plane: Option<[f32; 4]>,
}

impl Sample {
    /// The horizontal speed reported, in metres a second.
    fn ground_speed(&self) -> f32 {
        self.velocity.x.hypot(self.velocity.y)
    }

    /// The whole speed reported, in metres a second.
    fn speed(&self) -> f32 {
        self.ground_speed().hypot(self.velocity.z)
    }

    /// The horizontal distance to `other`, in metres.
    fn ground_distance(&self, other: &Self) -> f32 {
        (self.position.x - other.position.x).hypot(self.position.y - other.position.y)
    }

    /// The turn about the vertical the rotation encodes, in degrees.
    fn yaw_degrees(&self) -> f32 {
        (2.0 * self.rotation.z.atan2(self.rotation.s)).to_degrees()
    }
}

/// Everything the grid said about our own avatar, in order.
#[derive(Debug)]
struct Watch {
    /// Our own avatar.
    agent: AgentKey,
    /// When the watch began.
    started: Instant,
    /// Every update of the avatar's object.
    samples: Vec<Sample>,
    /// Every statement of the avatar's animation set: when, and the set as
    /// names.
    animations: Vec<(f64, BTreeSet<String>)>,
    /// Every alert the grid raised: when, and what.
    alerts: Vec<(f64, String)>,
    /// The holding animations already answered with a `FINISH_ANIM`, by name,
    /// while they stay in the set.
    finished: BTreeSet<&'static str>,
}

/// What an event asked of the leg that saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// The leg's own condition came true.
    Reached,
    /// A holding animation appeared in the avatar's set.
    Holding,
}

impl Watch {
    /// A watch of `agent` that has seen nothing.
    fn new(agent: AgentKey) -> Self {
        Self {
            agent,
            started: Instant::now(),
            samples: Vec::new(),
            animations: Vec::new(),
            alerts: Vec::new(),
            finished: BTreeSet::new(),
        }
    }

    /// Seconds since the watch began.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The latest update of the avatar.
    fn last(&self) -> Option<&Sample> {
        self.samples.last()
    }

    /// The latest update of the avatar, or the failure its absence is.
    fn here(&self) -> Result<Sample, TestFailure> {
        self.last().cloned().ok_or_else(|| {
            TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
        })
    }

    /// The animation set as last stated.
    fn animation_set(&self) -> Option<&BTreeSet<String>> {
        self.animations.last().map(|(_at, set)| set)
    }

    /// The holding animations in the set that have not been answered yet.
    fn unanswered_holds(&self) -> Vec<&'static str> {
        let Some(set) = self.animation_set() else {
            return Vec::new();
        };
        HOLDING_ANIMATIONS
            .into_iter()
            .filter(|name| set.contains(*name) && !self.finished.contains(name))
            .collect()
    }

    /// Folds one event into the watch.
    fn note(&mut self, event: &Event) {
        let at = self.now();
        match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == self.agent.uuid() =>
            {
                tracing::debug!(
                    at,
                    x = object.motion.position.x,
                    y = object.motion.position.y,
                    z = object.motion.position.z,
                    vx = object.motion.velocity.x,
                    vy = object.motion.velocity.y,
                    vz = object.motion.velocity.z,
                    "our avatar"
                );
                self.samples.push(Sample {
                    at,
                    position: object.motion.position.clone(),
                    velocity: object.motion.velocity.clone(),
                    acceleration: object.motion.acceleration.clone(),
                    rotation: object.motion.rotation.clone(),
                    plane: object.motion.collision_plane,
                });
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } if *avatar_id == self.agent => {
                let set = animation_names(animations);
                // A holding animation that left the set may be held for again.
                self.finished.retain(|name| set.contains(*name));
                tracing::info!(at, set = join(&set), "our animation set");
                self.animations.push((at, set));
            }
            Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                tracing::info!(at, ?event, "an alert");
                self.alerts.push((at, format!("{event:?}")));
            }
            _other => {}
        }
    }
}

/// The names of the animations in a set: each built-in's short name, and one
/// `+N` entry counting the rest — the facial expressions and other assets a
/// Second Life simulator lists beside the locomotion state, which say nothing
/// about movement and change from one statement to the next.
pub(crate) fn animation_names(animations: &[PlayingAnimation]) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = animations
        .iter()
        .filter_map(|animation| sl_anim::builtin_animation(animation.anim_id))
        .map(|builtin| builtin.name.to_owned())
        .collect();
    let others = animations.len().saturating_sub(names.len());
    if others > 0 {
        let _new = names.insert(format!("~{others} more"));
    }
    names
}

/// A set as one string.
pub(crate) fn join(set: &BTreeSet<String>) -> String {
    if set.is_empty() {
        "-".to_owned()
    } else {
        set.iter().cloned().collect::<Vec<_>>().join("+")
    }
}

/// A stretch of the watch: one leg.
#[derive(Debug, Clone, Copy)]
struct Span {
    /// Seconds from the start of the watch to the leg's start.
    start: f64,
    /// Seconds from the start of the watch to its end.
    end: f64,
    /// Seconds into the leg at which its condition came true, when it had one
    /// and it did.
    reached: Option<f64>,
}

impl Span {
    /// How long the leg lasted, in seconds.
    fn secs(&self) -> f64 {
        self.end - self.start
    }

    /// The updates that arrived during the leg.
    fn samples<'a>(&self, watch: &'a Watch) -> impl Iterator<Item = &'a Sample> + 'a {
        let (start, end) = (self.start, self.end);
        watch
            .samples
            .iter()
            .filter(move |sample| sample.at >= start && sample.at <= end)
    }

    /// The updates that arrived during the leg's second half: the leg at its
    /// steady pace, past whatever it took to get there.
    ///
    /// A grid that reports an avatar only when its motion changes sends none
    /// at a steady pace, so where the second half is silent the leg's last
    /// update stands for it: that is the motion the avatar was left in.
    fn steady<'a>(&self, watch: &'a Watch) -> Vec<&'a Sample> {
        let halfway = self.start + self.secs() / 2.0;
        let late: Vec<&Sample> = self
            .samples(watch)
            .filter(|sample| sample.at >= halfway)
            .collect();
        if late.is_empty() {
            self.samples(watch).last().into_iter().collect()
        } else {
            late
        }
    }

    /// The animation sets stated during the leg, each with its offset into
    /// it, as one string.
    fn animations(&self, watch: &Watch) -> String {
        let stated: Vec<String> = watch
            .animations
            .iter()
            .filter(|(at, _set)| *at >= self.start && *at <= self.end)
            .map(|(at, set)| format!("{:.2}:{}", at - self.start, join(set)))
            .collect();
        if stated.is_empty() {
            "none".to_owned()
        } else {
            stated.join(" ")
        }
    }
}

/// The median of `values`, or zero of none.
fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut sorted: Vec<f32> = values.collect();
    sorted.sort_by(f32::total_cmp);
    sorted.get(sorted.len().midpoint(0)).copied().unwrap_or(0.0)
}

/// The largest of `values`, or zero of none.
fn largest(values: impl Iterator<Item = f32>) -> f32 {
    values.fold(0.0, f32::max)
}

/// How a leg holds its controls.
#[derive(Debug, Clone, Copy)]
struct Hold {
    /// The controls held.
    controls: ControlFlags,
    /// How long for, at most.
    budget: Duration,
    /// How often the controls are re-stated. `None` states them once and
    /// leaves the rest to the session's own once-a-second `AgentUpdate`.
    restate: Option<Duration>,
    /// Whether a holding animation that appears is answered with a
    /// `FINISH_ANIM`, as a viewer that played it to its end would.
    finish: bool,
}

impl Hold {
    /// `controls` for `budget`, re-stated at the usual rate.
    const fn of(controls: ControlFlags, budget: Duration) -> Self {
        Self {
            controls,
            budget,
            restate: Some(RESTATE),
            finish: false,
        }
    }

    /// The same, answering holding animations.
    const fn finishing(mut self) -> Self {
        self.finish = true;
        self
    }

    /// The same, re-stated every `restate` (or never).
    const fn restated(mut self, restate: Option<Duration>) -> Self {
        self.restate = restate;
        self
    }
}

/// Holds `hold`'s controls for its budget, or until `reached` says the leg
/// has seen what it was waiting for, folding every event into `watch`.
async fn drive(
    session: &mut Session,
    watch: &mut Watch,
    hold: Hold,
    mut reached: impl FnMut(&Watch) -> bool,
) -> Result<Span, TestFailure> {
    let began = Instant::now();
    let start = watch.now();
    let mut reached_at = None;
    session.send(Command::SetControls(hold.controls)).await?;
    loop {
        let remaining = hold.budget.saturating_sub(began.elapsed());
        if remaining.is_zero() {
            break;
        }
        let wait = hold.restate.map_or(remaining, |every| every.min(remaining));
        let woke = session
            .wait_for(wait, |event| {
                watch.note(event);
                if reached(watch) {
                    Some(Wake::Reached)
                } else if hold.finish && !watch.unanswered_holds().is_empty() {
                    Some(Wake::Holding)
                } else {
                    None
                }
            })
            .await;
        match woke {
            Ok(Wake::Reached) => {
                reached_at = Some(watch.now() - start);
                break;
            }
            Ok(Wake::Holding) => {
                for name in watch.unanswered_holds() {
                    tracing::info!(name, "answering a holding animation with FINISH_ANIM");
                    let _new = watch.finished.insert(name);
                }
                session.send(Command::FinishAnimation).await?;
            }
            Err(TestFailure::Timeout(_)) => {
                // A condition that time alone makes true (an avatar at rest
                // long enough) is not waited for an event to notice.
                if reached(watch) {
                    reached_at = Some(watch.now() - start);
                    break;
                }
                if hold.restate.is_some() && began.elapsed() < hold.budget {
                    session.send(Command::SetControls(hold.controls)).await?;
                }
            }
            Err(other) => return Err(other),
        }
    }
    Ok(Span {
        start,
        end: watch.now(),
        reached: reached_at,
    })
}

/// Holds `hold`'s controls for the whole of its budget.
async fn hold_for(
    session: &mut Session,
    watch: &mut Watch,
    hold: Hold,
) -> Result<Span, TestFailure> {
    drive(session, watch, hold, |_watch| false).await
}

/// Lets every control go and watches for `duration`.
async fn rest(
    session: &mut Session,
    watch: &mut Watch,
    duration: Duration,
) -> Result<Span, TestFailure> {
    hold_for(
        session,
        watch,
        Hold::of(ControlFlags::empty(), duration).restated(None),
    )
    .await
}

/// The body rotation that turns the avatar `yaw` radians about the vertical
/// from its rest facing, which is east.
fn facing(yaw: f32) -> Rotation {
    let half = yaw / 2.0;
    Rotation {
        x: 0.0,
        y: 0.0,
        z: half.sin(),
        s: half.cos(),
    }
}

/// Turns the avatar to `yaw`.
async fn face(session: &Session, yaw: f32) -> Result<(), TestFailure> {
    let rotation = facing(yaw);
    session
        .send(Command::SetRotation {
            body: rotation.clone(),
            head: rotation,
        })
        .await
}

/// One leg as it was measured.
#[derive(Debug, Clone)]
struct Leg {
    /// The leg's name: the prefix of its metrics.
    name: &'static str,
    /// The stretch of the watch it covers.
    span: Span,
}

/// What freed an avatar the forward key alone did not move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freed {
    /// Nothing had to: the forward key moved it.
    Unheld,
    /// A `FINISH_ANIM`.
    FinishAnimation,
    /// A hop into the air and back.
    Hop,
    /// Nothing the case tried.
    Never,
}

impl Freed {
    /// The answer as a metric value.
    const fn label(self) -> &'static str {
        match self {
            Self::Unheld => "not held",
            Self::FinishAnimation => "FINISH_ANIM",
            Self::Hop => "a hop into the air",
            Self::Never => "nothing tried",
        }
    }
}

/// One jump as it was measured.
#[derive(Debug, Clone)]
struct Jump {
    /// The prefix of its metrics.
    name: &'static str,
    /// The stretch the jump key was held for.
    launch: Span,
    /// The stretch from letting the key go to being back on the ground.
    flight: Span,
    /// The stretch the forward key was then held for, until the avatar moved.
    held: Span,
    /// The height the avatar stood at before the jump.
    ground: f32,
}

/// Everything the case measured.
#[derive(Debug, Default)]
struct Found {
    /// The legs, in the order they were driven.
    legs: Vec<Leg>,
    /// What freed the avatar at its first steps.
    first_steps: Option<Freed>,
    /// The yaw asked for in the turning leg, in degrees.
    turned_to: Option<f32>,
    /// The jumps.
    jumps: Vec<Jump>,
    /// The height the avatar stood at before it flew.
    ground: Option<f32>,
    /// The fall: the stretch from letting go of flight to the landing.
    fall: Option<Span>,
    /// What freed the avatar after the fall's landing.
    after_fall: Option<(Freed, Span)>,
}

impl Found {
    /// Remembers a leg.
    fn leg(&mut self, name: &'static str, span: Span) -> Span {
        self.legs.push(Leg { name, span });
        span
    }

    /// The leg called `name`.
    fn named(&self, name: &str) -> Option<&Leg> {
        self.legs.iter().find(|leg| leg.name == name)
    }

    /// Holds the run to what every grid has to get right.
    fn check(&self) -> Result<(), TestFailure> {
        check(
            self.first_steps.is_some_and(|freed| freed != Freed::Never),
            "the forward key never moved the avatar, with or without a FINISH_ANIM or a hop",
        )?;
        check(
            self.fall.is_some_and(|fall| fall.reached.is_some()),
            "the avatar never came back down after it stopped flying",
        )
    }

    /// Records everything under the legs' names.
    fn record(&self, watch: &Watch, metrics: &mut Metrics) {
        for leg in &self.legs {
            record_leg(leg, watch, metrics);
        }
        if let Some(freed) = self.first_steps {
            metrics.set("first_steps_freed_by", freed.label());
        }
        if let (Some(asked), Some(leg)) = (self.turned_to, self.named("turn")) {
            metrics.set("turn_asked_yaw_deg", f64::from(asked));
            if let Some(sample) = leg.span.samples(watch).last() {
                let reported = sample.yaw_degrees();
                metrics.set("turn_reported_yaw_deg", f64::from(reported));
                metrics.set(
                    "turn_error_deg",
                    f64::from((reported - asked + 180.0).rem_euclid(360.0) - 180.0),
                );
            }
        }
        for jump in &self.jumps {
            record_jump(jump, watch, metrics);
        }
        self.record_flight(watch, metrics);
        metrics.set("alerts_count", count(watch.alerts.len()));
        metrics.set(
            "alerts",
            watch
                .alerts
                .iter()
                .map(|(at, alert)| format!("{at:.1}:{alert}"))
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }

    /// Records the climb's profile, the fall and the landing.
    fn record_flight(&self, watch: &Watch, metrics: &mut Metrics) {
        let ground = self.ground.unwrap_or(0.0);
        if let Some(climb) = self.named("climb") {
            // The hover that follows is part of the reading: a grid that
            // reports only changes says where the climb got to when it ends.
            let until = self
                .named("hover")
                .map_or(climb.span.end, |hover| hover.span.end);
            let samples: Vec<&Sample> = watch
                .samples
                .iter()
                .filter(|sample| sample.at >= climb.span.start && sample.at <= until)
                .collect();
            let top = largest(samples.iter().map(|sample| sample.position.z));
            metrics.set("climb_height_gained_m", f64::from(top - ground));
            metrics.set("climb_top_z_m", f64::from(top));
            // The vertical speed last reported by the end of each window: a
            // ceiling shows as a window that ends at rest with the key held.
            let mut profile = Vec::new();
            let mut until = CLIMB_WINDOW_SECS;
            while until <= climb.span.secs() + 0.5 {
                let latest = climb
                    .span
                    .samples(watch)
                    .take_while(|sample| sample.at - climb.span.start <= until)
                    .last();
                profile.push(latest.map_or_else(
                    || "?".to_owned(),
                    |sample| format!("{:.1}", sample.velocity.z),
                ));
                until += CLIMB_WINDOW_SECS;
            }
            metrics.set("climb_vertical_speed_by_window", profile.join(" "));
        }
        if let Some(hover) = self.named("hover") {
            let samples: Vec<&Sample> = hover.span.samples(watch).collect();
            if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
                metrics.set(
                    "hover_drift_z_m",
                    f64::from(last.position.z - first.position.z),
                );
            }
        }
        if let Some(fall) = &self.fall {
            metrics.set("fall_landed", fall.reached.is_some());
            if let Some(secs) = fall.reached {
                metrics.set("fall_secs", secs);
            }
            let samples: Vec<&Sample> = fall.samples(watch).collect();
            metrics.set(
                "fall_fastest_m_per_s",
                f64::from(largest(samples.iter().map(|sample| -sample.velocity.z))),
            );
            if let Some(first) = samples.first() {
                metrics.set("fall_from_height_m", f64::from(first.position.z - ground));
            }
            metrics.set("fall_animations", fall.animations(watch));
        }
        if let Some((freed, span)) = &self.after_fall {
            metrics.set("after_fall_freed_by", freed.label());
            metrics.set("after_fall_animations", span.animations(watch));
            if let Some(secs) = span.reached {
                metrics.set("after_fall_moved_after_secs", secs);
            }
        }
    }
}

/// A count as a metric value.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Records one leg under its name.
fn record_leg(leg: &Leg, watch: &Watch, metrics: &mut Metrics) {
    let key = |name: &str| format!("{}_{name}", leg.name);
    let span = leg.span;
    let all: Vec<&Sample> = span.samples(watch).collect();
    let steady = span.steady(watch);
    metrics.set(&key("updates_count"), count(all.len()));
    if span.secs() > 0.0 {
        metrics.set(
            &key("updates_per_sec"),
            f64::from(count(all.len())) / span.secs(),
        );
    }
    metrics.set(
        &key("ground_speed_m_per_s"),
        f64::from(median(steady.iter().map(|sample| sample.ground_speed()))),
    );
    metrics.set(
        &key("vertical_speed_m_per_s"),
        f64::from(median(steady.iter().map(|sample| sample.velocity.z))),
    );
    metrics.set(
        &key("acceleration_m_per_s2"),
        f64::from(median(steady.iter().map(|sample| {
            sample
                .acceleration
                .x
                .hypot(sample.acceleration.y)
                .hypot(sample.acceleration.z)
        }))),
    );
    // The speed the positions themselves show, which is the one that matters
    // to a viewer predicting from the reported velocity: where the two differ
    // the prediction is corrected at every update.
    if let (Some(first), Some(last)) = (steady.first(), steady.last())
        && last.at - first.at >= 1.0
    {
        metrics.set(
            &key("travelled_m_per_s"),
            f64::from(last.ground_distance(first)) / (last.at - first.at),
        );
    }
    if let (Some(first), Some(last)) = (all.first(), all.last()) {
        metrics.set(&key("travelled_m"), f64::from(last.ground_distance(first)));
        metrics.set(
            &key("rose_m"),
            f64::from(last.position.z - first.position.z),
        );
        metrics.set(
            &key("collision_plane"),
            last.plane.map_or_else(
                || "none".to_owned(),
                |plane| {
                    let [x, y, z, d] = plane;
                    format!("{x:.2} {y:.2} {z:.2} {d:.2}")
                },
            ),
        );
        // How long after the leg began the avatar was last reported moving:
        // for a rest, the time it took to come to a stop.
        if let Some(moving) = all
            .iter()
            .rev()
            .find(|sample| sample.speed() > AT_REST_M_PER_S)
        {
            metrics.set(&key("last_moving_at_secs"), moving.at - span.start);
        }
    }
    metrics.set(&key("animations"), span.animations(watch));
}

/// Records one jump under its name.
fn record_jump(jump: &Jump, watch: &Watch, metrics: &mut Metrics) {
    let key = |name: &str| format!("{}_{name}", jump.name);
    metrics.set(&key("left_ground"), jump.launch.reached.is_some());
    if let Some(secs) = jump.launch.reached {
        metrics.set(&key("left_ground_after_secs"), secs);
    }
    // A jump that never left the ground has no apex to report, and on a grid
    // that reports a resting avatar only once it may have no update at all.
    if jump.launch.reached.is_some() {
        let airborne = || jump.launch.samples(watch).chain(jump.flight.samples(watch));
        let top = largest(airborne().map(|sample| sample.position.z));
        metrics.set(&key("apex_m"), f64::from(top - jump.ground));
        metrics.set(
            &key("launch_speed_m_per_s"),
            f64::from(largest(airborne().map(|sample| sample.velocity.z))),
        );
    }
    metrics.set(&key("landed"), jump.flight.reached.is_some());
    if let Some(secs) = jump.flight.reached {
        metrics.set(&key("airborne_after_release_secs"), secs);
    }
    metrics.set(&key("moved_after_landing"), jump.held.reached.is_some());
    if let Some(secs) = jump.held.reached {
        metrics.set(&key("moved_after_landing_secs"), secs);
    }
    let whole = Span {
        start: jump.launch.start,
        end: jump.held.end,
        reached: None,
    };
    metrics.set(&key("animations"), whole.animations(watch));
}

/// Whether the avatar has moved `MOVED_M` over the ground from `from`.
fn moved_from(watch: &Watch, from: &Sample) -> bool {
    watch
        .last()
        .is_some_and(|sample| sample.ground_distance(from) >= MOVED_M)
}

/// Whether the avatar is back on the ground and still: since `since` it has
/// been reported in motion, and it is now at rest — its latest two reports
/// say so, or its latest one does and nothing has contradicted it for
/// [`RESTED_FOR_SECS`] (a grid that reports an avatar only when something
/// changes says "stopped" once).
///
/// The motion is required because an avatar let go in the air is at rest for
/// a moment before it falls, and one at the top of a jump for another.
fn landed(watch: &Watch, since: f64) -> bool {
    let recent: Vec<&Sample> = watch
        .samples
        .iter()
        .filter(|sample| sample.at > since)
        .collect();
    let mut latest = recent.iter().rev();
    let at_rest = match (latest.next(), latest.next()) {
        (Some(last), before) => {
            last.speed() < AT_REST_M_PER_S
                && (before.is_some_and(|before| before.speed() < AT_REST_M_PER_S)
                    || watch.now() - last.at >= RESTED_FOR_SECS)
        }
        (None, _) => false,
    };
    at_rest
        && recent
            .iter()
            .any(|sample| sample.speed() >= IN_MOTION_M_PER_S)
}

/// Drives every leg, in order.
async fn drive_all(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
) -> Result<(), TestFailure> {
    let settled = rest(session, watch, SETTLE).await?;
    tracing::info!(
        updates = settled.samples(watch).count(),
        "the arrival settled"
    );
    let here = watch.here()?;
    let mut heading = Course::from(&here);
    face(session, heading.faced).await?;
    let standing = rest(session, watch, Duration::from_secs(5)).await?;
    let _span = found.leg("standing", standing);

    first_steps(session, watch, found).await?;
    ground_legs(session, watch, found, &mut heading).await?;
    turn(session, watch, found, &heading).await?;
    jumps(session, watch, found, &mut heading).await?;
    flight(session, watch, found, &mut heading).await
}

/// Where the legs take the avatar: out from where it started and back, so
/// that a few dozen legs leave it where they found it and none of them meets
/// a border.
#[derive(Debug, Clone, Copy)]
struct Course {
    /// Where the avatar started, along the ground.
    home: (f32, f32),
    /// The direction of travel of a leg that starts at home, in radians:
    /// towards the middle of the region.
    out: f32,
    /// The yaw the avatar was last turned to.
    faced: f32,
}

impl Course {
    /// A course out from `home`.
    fn from(start: &Sample) -> Self {
        // Home is where the avatar started, unless that is near a border: the
        // longest leg is a level flight of some sixty metres, and a leg that
        // overshoots home by that much must still end inside the region. An
        // avatar that starts in a corner spends its first legs getting there.
        let home = (
            start.position.x.clamp(HOME_MARGIN_M, 256.0 - HOME_MARGIN_M),
            start.position.y.clamp(HOME_MARGIN_M, 256.0 - HOME_MARGIN_M),
        );
        let out = (128.0 - home.1).atan2(128.0 - home.0);
        Self {
            home,
            out,
            faced: out,
        }
    }

    /// The direction the next leg travels in from `here`: home when the
    /// avatar is away from it, out when it is there.
    fn travel(&self, here: &Sample) -> f32 {
        let (dx, dy) = (self.home.0 - here.position.x, self.home.1 - here.position.y);
        if dx.hypot(dy) > NEAR_HOME_M {
            dy.atan2(dx)
        } else {
            self.out
        }
    }

    /// Turns the avatar so that holding `controls` takes it the way the next
    /// leg should go.
    async fn aim(
        &mut self,
        session: &Session,
        watch: &Watch,
        controls: ControlFlags,
    ) -> Result<(), TestFailure> {
        self.faced = self.travel(&watch.here()?) - travel_offset(controls);
        face(session, self.faced).await
    }
}

/// The direction `controls` move an avatar in, relative to the way it faces,
/// in radians anticlockwise: backwards for the backward key, a quarter turn
/// either way for the sideways ones.
fn travel_offset(controls: ControlFlags) -> f32 {
    if controls.contains(ControlFlags::AT_NEG) {
        core::f32::consts::PI
    } else if controls.contains(ControlFlags::LEFT_POS) {
        core::f32::consts::FRAC_PI_2
    } else if controls.contains(ControlFlags::LEFT_NEG) {
        -core::f32::consts::FRAC_PI_2
    } else {
        0.0
    }
}

/// The forward key from where the login put the avatar, and what it took to
/// get an avatar that did not move moving.
async fn first_steps(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
) -> Result<(), TestFailure> {
    let from = watch.here()?;
    let forwards = Hold::of(ControlFlags::AT_POS, SHORT);
    let plain = drive(session, watch, forwards, |watch| moved_from(watch, &from)).await?;
    let _span = found.leg("first_steps", plain);
    let mut freed = if plain.reached.is_some() {
        Freed::Unheld
    } else {
        Freed::Never
    };
    if freed == Freed::Never {
        session.send(Command::FinishAnimation).await?;
        let finished = drive(session, watch, forwards, |watch| moved_from(watch, &from)).await?;
        let _span = found.leg("first_steps_after_finish", finished);
        if finished.reached.is_some() {
            freed = Freed::FinishAnimation;
        }
    }
    if freed == Freed::Never {
        let _up = hold_for(
            session,
            watch,
            Hold::of(
                ControlFlags::FLY | ControlFlags::UP_POS,
                Duration::from_secs(1),
            ),
        )
        .await?;
        let released = watch.now();
        let _down = drive(
            session,
            watch,
            Hold::of(ControlFlags::empty(), LANDING_BUDGET),
            |watch| landed(watch, released),
        )
        .await?;
        let from = watch.here()?;
        let hopped = drive(session, watch, forwards, |watch| moved_from(watch, &from)).await?;
        let _span = found.leg("first_steps_after_hop", hopped);
        if hopped.reached.is_some() {
            freed = Freed::Hop;
        }
    }
    found.first_steps = Some(freed);
    let _rest = rest(session, watch, REST).await?;
    Ok(())
}

/// The legs on the ground: each control held for a few seconds, then let go.
async fn ground_legs(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
    heading: &mut Course,
) -> Result<(), TestFailure> {
    let walks = [
        ("walk_stated_once", None),
        ("walk", Some(RESTATE)),
        ("walk_100hz", Some(RESTATE_FAST)),
    ];
    for (name, restate) in walks {
        heading.aim(session, watch, ControlFlags::AT_POS).await?;
        let hold = Hold::of(ControlFlags::AT_POS, WALK).restated(restate);
        let span = hold_for(session, watch, hold).await?;
        let _span = found.leg(name, span);
        let stopped = rest(session, watch, REST).await?;
        if name == "walk" {
            let _span = found.leg("walk_stop", stopped);
        }
    }
    let short = [
        ("backwards", ControlFlags::AT_NEG),
        ("strafe_left", ControlFlags::LEFT_POS),
        ("strafe_right", ControlFlags::LEFT_NEG),
        ("nudge", ControlFlags::NUDGE_AT_POS),
        ("fast_bit", ControlFlags::AT_POS | ControlFlags::FAST_AT),
        ("crouch", ControlFlags::UP_NEG),
        ("crouch_walk", ControlFlags::UP_NEG | ControlFlags::AT_POS),
    ];
    for (name, controls) in short {
        heading.aim(session, watch, controls).await?;
        let span = hold_for(session, watch, Hold::of(controls, SHORT)).await?;
        let _span = found.leg(name, span);
        let _rest = rest(session, watch, REST).await?;
    }

    session
        .send(Command::SetAlwaysRun {
            mode: MovementMode::AlwaysRun,
        })
        .await?;
    heading.aim(session, watch, ControlFlags::AT_POS).await?;
    let run = hold_for(session, watch, Hold::of(ControlFlags::AT_POS, WALK)).await?;
    let _span = found.leg("run", run);
    let stopped = rest(session, watch, REST).await?;
    let _span = found.leg("run_stop", stopped);
    heading.aim(session, watch, ControlFlags::AT_NEG).await?;
    let back = hold_for(session, watch, Hold::of(ControlFlags::AT_NEG, SHORT)).await?;
    let _span = found.leg("run_backwards", back);
    let _rest = rest(session, watch, REST).await?;
    session
        .send(Command::SetAlwaysRun {
            mode: MovementMode::Walk,
        })
        .await?;
    Ok(())
}

/// A quarter turn on the spot, with no key held: whether the grid reports the
/// rotation back, and how.
async fn turn(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
    heading: &Course,
) -> Result<(), TestFailure> {
    let asked = heading.faced + core::f32::consts::FRAC_PI_2;
    found.turned_to = Some(asked.to_degrees());
    let start = watch.now();
    face(session, asked).await?;
    let mut span = rest(session, watch, REST).await?;
    span.start = start;
    let _span = found.leg("turn", span);
    face(session, heading.faced).await?;
    let _rest = rest(session, watch, Duration::from_secs(1)).await?;
    Ok(())
}

/// Three jumps: one the simulator is told nothing about, one whose pre-jump
/// animation is reported finished the moment it appears, and one whose
/// landing animation is too.
async fn jumps(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
    heading: &mut Course,
) -> Result<(), TestFailure> {
    // Whether the pre-jump is reported finished, and whether the landing is.
    let variants = [
        ("jump", false, false),
        ("jump_launch_finished", true, false),
        ("jump_finishing", true, true),
    ];
    for (name, finish_launch, finish_landing) in variants {
        heading.aim(session, watch, ControlFlags::AT_POS).await?;
        let _rest = rest(session, watch, REST).await?;
        let from = watch.here()?;
        let ground = from.position.z;
        let answering = |finish: bool, hold: Hold| if finish { hold.finishing() } else { hold };
        let launch = drive(
            session,
            watch,
            answering(finish_launch, Hold::of(ControlFlags::UP_POS, JUMP_BUDGET)),
            |watch| {
                watch
                    .last()
                    .is_some_and(|sample| sample.position.z - ground >= LIFT_OFF_M)
            },
        )
        .await?;
        let released = watch.now();
        let flight = drive(
            session,
            watch,
            answering(
                finish_landing,
                Hold::of(ControlFlags::empty(), LANDING_BUDGET),
            ),
            |watch| landed(watch, released),
        )
        .await?;
        // The forward key the moment the avatar is down: how long the landing
        // holds it.
        let down = watch.here()?;
        let held = drive(
            session,
            watch,
            answering(finish_landing, Hold::of(ControlFlags::AT_POS, HELD_BUDGET)),
            |watch| moved_from(watch, &down),
        )
        .await?;
        found.jumps.push(Jump {
            name,
            launch,
            flight,
            held,
            ground,
        });
        let _rest = rest(session, watch, REST).await?;
        if held.reached.is_none() {
            // Not left held for the next leg.
            session.send(Command::FinishAnimation).await?;
            let _rest = rest(session, watch, REST).await?;
        }
    }
    Ok(())
}

/// The flight: up, a hover, level flight out and back, a little way down, and
/// the fall from there.
async fn flight(
    session: &mut Session,
    watch: &mut Watch,
    found: &mut Found,
    heading: &mut Course,
) -> Result<(), TestFailure> {
    let fly = ControlFlags::FLY;
    found.ground = Some(watch.here()?.position.z);
    let climb = hold_for(session, watch, Hold::of(fly | ControlFlags::UP_POS, CLIMB)).await?;
    let _span = found.leg("climb", climb);
    let hover = hold_for(session, watch, Hold::of(fly, HOVER)).await?;
    let _span = found.leg("hover", hover);
    let level = [
        ("fly", fly | ControlFlags::AT_POS),
        (
            "fly_fast_bit",
            fly | ControlFlags::AT_POS | ControlFlags::FAST_AT,
        ),
    ];
    for (name, controls) in level {
        heading.aim(session, watch, controls).await?;
        let span = hold_for(session, watch, Hold::of(controls, SHORT)).await?;
        let _span = found.leg(name, span);
        let stopped = hold_for(session, watch, Hold::of(fly, REST)).await?;
        if name == "fly" {
            let _span = found.leg("fly_stop", stopped);
        }
    }
    let descent = hold_for(session, watch, Hold::of(fly | ControlFlags::UP_NEG, SHORT)).await?;
    let _span = found.leg("descent", descent);
    let _hover = hold_for(session, watch, Hold::of(fly, REST)).await?;

    // Letting go of flight in the air: the fall, and the landing.
    let released = watch.now();
    let fall = drive(
        session,
        watch,
        Hold::of(ControlFlags::empty(), FALL_BUDGET),
        |watch| landed(watch, released),
    )
    .await?;
    found.fall = Some(fall);

    // The forward key the moment the avatar is down, as after a jump.
    heading.aim(session, watch, ControlFlags::AT_POS).await?;
    let down = watch.here()?;
    let forwards = Hold::of(ControlFlags::AT_POS, HELD_BUDGET);
    let plain = drive(session, watch, forwards, |watch| moved_from(watch, &down)).await?;
    let mut after = (Freed::Unheld, plain);
    if plain.reached.is_none() {
        session.send(Command::FinishAnimation).await?;
        let finished = drive(session, watch, forwards, |watch| moved_from(watch, &down)).await?;
        let freed = if finished.reached.is_some() {
            Freed::FinishAnimation
        } else {
            Freed::Never
        };
        after = (
            freed,
            Span {
                start: plain.start,
                end: finished.end,
                reached: finished
                    .reached
                    .map(|secs| secs + (finished.start - plain.start)),
            },
        );
    }
    found.after_fall = Some(after);
    let _rest = rest(session, watch, REST).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Course, HOLDING_ANIMATIONS, Sample, Span, Watch, facing, landed, median, travel_offset,
    };
    use pretty_assertions::assert_eq;
    use sl_client_tokio::ControlFlags;
    use sl_client_tokio::{AgentKey, Rotation, Uuid, Vector};

    /// Some agent.
    fn agent() -> AgentKey {
        AgentKey::from(Uuid::from_u128(1))
    }

    /// A sample at `at` seconds, at `x` along the ground and `z` up, moving at
    /// `speed` along the ground.
    fn sample(at: f64, x: f32, z: f32, speed: f32) -> Sample {
        Sample {
            at,
            position: Vector { x, y: 0.0, z },
            velocity: Vector {
                x: speed,
                y: 0.0,
                z: 0.0,
            },
            acceleration: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            plane: None,
        }
    }

    #[test]
    fn the_holding_animations_are_built_ins() {
        for name in HOLDING_ANIMATIONS {
            assert!(
                sl_anim::builtin_animation_by_name(name).is_some(),
                "{name} is no built-in"
            );
        }
    }

    #[test]
    fn a_legs_steady_half_leaves_out_the_time_it_took_to_get_going() {
        let mut watch = Watch::new(agent());
        watch.samples = vec![
            sample(9.0, 0.0, 20.0, 9.0),
            sample(10.0, 0.0, 20.0, 0.0),
            sample(11.0, 1.0, 20.0, 1.0),
            sample(12.5, 4.0, 20.0, 3.0),
            sample(13.5, 7.0, 20.0, 3.0),
            sample(14.5, 9.0, 20.0, 9.0),
        ];
        let span = Span {
            start: 10.0,
            end: 14.0,
            reached: None,
        };
        assert_eq!(span.samples(&watch).count(), 4);
        let steady = span.steady(&watch);
        assert_eq!(steady.len(), 2);
        let speed = median(steady.iter().map(|sample| sample.ground_speed()));
        assert!((speed - 3.0).abs() < 1e-6, "got {speed}");
    }

    #[test]
    fn a_landing_is_two_reports_at_rest_after_being_seen_in_motion() {
        let mut watch = Watch::new(agent());
        watch.samples = vec![sample(1.0, 0.0, 20.0, 0.0), sample(2.0, 0.0, 20.0, 0.0)];
        // Both reports predate the release: the avatar has not been seen since.
        assert!(!landed(&watch, 2.5));
        // At rest in the air, before the fall begins.
        watch.samples.push(sample(2.6, 0.0, 40.0, 0.0));
        watch.samples.push(sample(2.7, 0.0, 40.0, 0.0));
        assert!(!landed(&watch, 2.5));
        watch.samples.push(sample(3.0, 0.0, 22.0, 2.0));
        watch.samples.push(sample(4.0, 0.0, 20.0, 0.0));
        assert!(!landed(&watch, 2.5));
        watch.samples.push(sample(5.0, 0.0, 20.0, 0.0));
        assert!(landed(&watch, 2.5));
    }

    #[test]
    fn a_leg_heads_home_when_away_and_out_when_there() {
        let at = |x: f32, y: f32| {
            let mut here = sample(0.0, x, 20.0, 0.0);
            here.position.y = y;
            here
        };
        // Home is south-west of the middle of the region, so out is north-east.
        let course = Course::from(&at(100.0, 90.0));
        let out = course.travel(&at(102.0, 90.0));
        assert!(out > 0.0 && out < core::f32::consts::FRAC_PI_2, "{out}");
        // Twenty metres east of home, the way back is west.
        let back = course.travel(&at(120.0, 90.0));
        assert!((back.abs() - core::f32::consts::PI).abs() < 1e-4, "{back}");
        // An avatar that starts in a corner has its home moved clear of the
        // borders, and heads there first.
        let cornered = Course::from(&at(8.0, 42.0));
        let inwards = cornered.travel(&at(8.0, 42.0));
        assert!(
            inwards > 0.0 && inwards < core::f32::consts::FRAC_PI_2,
            "{inwards}"
        );
        // The backward key travels the way the avatar does not face.
        assert!((travel_offset(ControlFlags::AT_NEG) - core::f32::consts::PI).abs() < 1e-6);
        assert!(travel_offset(ControlFlags::LEFT_POS) > 0.0);
        assert!(travel_offset(ControlFlags::LEFT_NEG) < 0.0);
    }

    #[test]
    fn a_reported_rotation_reads_back_as_the_yaw_it_was_built_from() {
        for degrees in [0.0_f32, 45.0, 90.0, -120.0] {
            let mut turned = sample(0.0, 0.0, 0.0, 0.0);
            turned.rotation = facing(degrees.to_radians());
            let read = turned.yaw_degrees();
            assert!((read - degrees).abs() < 1e-3, "{degrees} read as {read}");
        }
    }
}
