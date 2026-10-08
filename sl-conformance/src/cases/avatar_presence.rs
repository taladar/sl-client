//! Watch one avatar from another's session and record everything the grid
//! says about it: the coarse-location feed, the full and terse object
//! updates, and what arrives when it goes out of sight, leaves and comes
//! back.
//!
//! Two residents stand together; the first (the *mover*) acts and the second
//! (the *watcher*) only listens. The legs, in order:
//!
//! - **idle** — both stand still: how often `CoarseLocationUpdate` comes, who
//!   it lists and in what order, what `You` and `Prey` point at, how a
//!   position is rounded into an entry, and whether an avatar that does
//!   nothing is re-sent;
//! - **track** — the watcher sends `TrackAgent` for the mover and then for
//!   nobody: whether `Prey` follows;
//! - **walk** — the mover paces back and forth: how many updates of it a
//!   second the watcher is sent, and whether the coarse feed quickens;
//! - **range** — the watcher's draw distance goes down to 64 m and the mover
//!   climbs a kilometre away: whether its object is taken away
//!   (`KillObject`), at what distance and how long after, and whether the
//!   coarse feed goes on listing it. Neither live grid takes it away, so
//!   the watcher then leaves for a neighbouring region and comes back, to
//!   see whether an agent arriving anew is sent an avatar that far off. On a
//!   grid that did withhold it the leg goes on to what a larger draw
//!   distance or a camera beside it brings back, and at what distance it
//!   returns on the way down;
//! - **teleport** — the mover teleports to a neighbouring region and back:
//!   when the watcher is told it left, whether it then sees it through the
//!   neighbour's child circuit, and the order of what announces its return;
//! - **logout** — the mover logs out and in again at `last`: the same two
//!   questions for a session ending and beginning.
//!
//! The fake grid's residents are not shown to each other
//! (`server-fake-grid-agent-avatars-shared`), so there the case is the idle
//! leg alone, from the one resident's own session.

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Camera, CircuitId, CoarseLocation, Command, ControlFlags, Distance, Event,
    GridCoordinates, RegionHandle, ScopedObjectId, Uuid, Vector,
};

use crate::context::{LoginAnswer, LoginAttempt, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, is_fake, log_out, secs_metric};
use crate::teleport_trace::{neighbouring_region, request_teleport, watch_teleport};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/avatars.md (avatar-presence, 2026-10-08)";

/// Where both avatars log in on OpenSim, side by side: the spot
/// `agent-movement` flies from, with open sky above it. The middle of the
/// "Default Region" is roofed with the workspace's test objects, and a climb
/// from there goes nowhere.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// How long the scene is left to arrive before anything is measured.
const SETTLE: Duration = Duration::from_secs(20);

/// How long the idle leg listens.
const IDLE: Duration = Duration::from_secs(30);

/// How long the watcher listens after each `TrackAgent`.
const TRACK: Duration = Duration::from_secs(12);

/// How long each stroke of the walk lasts, and how many there are: forwards
/// and back, so the mover ends about where it began.
const STROKE: Duration = Duration::from_secs(3);

/// The number of strokes of the walk.
const STROKES: u8 = 6;

/// The draw distance the watcher drops to for the range leg, in metres.
const NEAR_DRAW_M: f64 = 64.0;

/// The draw distances stepped through once the mover is out of sight.
const DRAW_STEPS_M: [f64; 3] = [128.0, 256.0, 512.0];

/// The draw distance the case leaves the watcher with: the session's own
/// default.
const DEFAULT_DRAW_M: f64 = 256.0;

/// How long the mover climbs at most: at sixteen metres a second, to about
/// 1,100 m — past the 1,020 m a coarse entry can state. It stops as soon as
/// the watcher is told its avatar is gone.
const CLIMB: Duration = Duration::from_secs(70);

/// How long the mover's way down may take.
const DESCENT: Duration = Duration::from_secs(180);

/// How long the watcher waits for a grid to take an avatar out of its stream
/// once it is out of range. Second Life takes fifty seconds to retire a
/// neighbouring region; this allows an avatar as long.
const RETIRE: Duration = Duration::from_secs(75);

/// How long the watcher waits for an avatar to be sent again once it is back
/// in range.
const APPEAR: Duration = Duration::from_secs(30);

/// How long after an arrival the watcher goes on listening, so that
/// everything that announces it is in.
const ARRIVAL: Duration = Duration::from_secs(15);

/// How long the watcher stays in the neighbouring region before it comes
/// back.
const BETWEEN_TELEPORTS: Duration = Duration::from_secs(8);

/// The slice a conditional watch re-checks its condition at.
const SLICE: Duration = Duration::from_secs(1);

/// How long the watcher listens after a departure.
const DEPARTURE: Duration = Duration::from_secs(20);

/// The longest the watcher listens across the mover's second login: a
/// cooldown, the login, and the arrival.
const LOGIN_WINDOW: Duration = Duration::from_secs(6 * 60);

/// How far below the mover the camera of the *camera* step stands, in metres.
const CAMERA_BELOW_M: f32 = 10.0;

/// The height above its start at which the mover's descent counts as over, in
/// metres.
const LANDED_M: f32 = 4.0;

/// The case's overall budget: a dozen watches, two of them retirements, a
/// teleport there and back, and a login cooldown.
const CASE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// Whether a `CoarseLocationUpdate` lists the agent it is sent to.
const LISTS_SELF: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether its `You` index points at that entry.
const YOU_IS_SELF: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// How far apart the coarse updates are, in seconds.
const COARSE_INTERVAL_SECS: Measured<f64> = Measured {
    second_life: 1.333,
    opensim: 4.54,
    source: SOURCE,
};

/// How far from its grid's interval a run's median may be and still be that
/// interval, as a fraction of it.
const INTERVAL_SLACK: f64 = 0.1;

/// Whether an avatar that climbs a kilometre away from a watcher with a draw
/// distance of 64 m is taken out of the watcher's stream.
const FAR_AVATAR_KILLED: Measured<bool> = Measured {
    second_life: false,
    opensim: false,
    source: SOURCE,
};

/// One update of the mover's own avatar, from its own session.
#[derive(Debug, Clone)]
struct Fix {
    /// Seconds from the start of the watch.
    at: f64,
    /// Where it was.
    position: Vector,
    /// How fast it was moving.
    velocity: Vector,
}

/// The mover's side of the watch: where its own session says it is.
#[derive(Debug)]
struct Track {
    /// The mover.
    agent: Uuid,
    /// When the watch began.
    started: Instant,
    /// Every update of its own avatar.
    fixes: Vec<Fix>,
}

impl Track {
    /// Seconds from the start of the watch.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Keeps `event` if it is an update of the mover's own avatar.
    fn note(&mut self, event: &Event) {
        if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event
            && object.full_id.uuid() == self.agent
        {
            self.fixes.push(Fix {
                at: self.now(),
                position: object.motion.position.clone(),
                velocity: object.motion.velocity.clone(),
            });
        }
    }

    /// Where the mover was at `at`: the last update before it, carried on at
    /// that update's velocity. A grid sends none while the velocity holds.
    fn reckon(&self, at: f64) -> Option<Vector> {
        let fix = self
            .fixes
            .iter()
            .rev()
            .find(|fix| fix.at <= at)
            .or_else(|| self.fixes.first())?;
        let since = seconds_f32((at - fix.at).max(0.0));
        Some(Vector {
            x: fix.velocity.x.mul_add(since, fix.position.x),
            y: fix.velocity.y.mul_add(since, fix.position.y),
            z: fix.velocity.z.mul_add(since, fix.position.z),
        })
    }

    /// Where the mover's latest update put it.
    fn latest(&self) -> Option<Vector> {
        self.fixes.last().map(|fix| fix.position.clone())
    }
}

/// A span of seconds as an `f32`, for arithmetic with positions.
fn seconds_f32(seconds: f64) -> f32 {
    Duration::try_from_secs_f64(seconds).map_or(0.0, |span| span.as_secs_f32())
}

/// What the watcher was sent about the mover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    /// Its object, new to the stream: a full update.
    Object,
    /// Its object again: a later full update or a terse one.
    Moved,
    /// Its object's removal.
    Removed,
    /// Its `AvatarAppearance`.
    Appearance,
    /// Its `AvatarAnimation`.
    Animation,
}

impl Seen {
    /// The word a sequence names it by.
    const fn word(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Moved => "moved",
            Self::Removed => "removed",
            Self::Appearance => "appearance",
            Self::Animation => "animation",
        }
    }
}

/// One thing the watcher was sent about the mover.
#[derive(Debug, Clone, Copy)]
struct Sight {
    /// Seconds from the start of the watch.
    at: f64,
    /// What it was.
    what: Seen,
    /// Whether it came on the watcher's own region's circuit rather than a
    /// neighbour's. The appearance and the animations carry no circuit and
    /// count as the region's own.
    root: bool,
}

/// One `CoarseLocationUpdate`.
#[derive(Debug, Clone)]
pub(crate) struct Coarse {
    /// Seconds from the start of the watch.
    at: f64,
    /// Whether it describes the watcher's own region.
    root: bool,
    /// The region it describes.
    region: RegionHandle,
    /// How many avatars it lists.
    entries: usize,
    /// Its `You` index.
    you: Option<usize>,
    /// Its `Prey` index.
    prey: Option<usize>,
    /// The watcher's own entry and where in the list it stands.
    own: Option<(usize, CoarseLocation)>,
    /// The mover's.
    pub(crate) subject: Option<(usize, CoarseLocation)>,
}

/// The watcher's side: everything its session was sent about the mover, and
/// every coarse update.
#[derive(Debug)]
pub(crate) struct Feed {
    /// The watcher.
    agent: Uuid,
    /// The mover, when there is one.
    subject: Option<Uuid>,
    /// The watcher's root circuit.
    root: Option<CircuitId>,
    /// The watcher's region.
    home: Option<RegionHandle>,
    /// When the watch began.
    started: Instant,
    /// Where the watcher's own avatar stands.
    own: Option<Vector>,
    /// The ids the mover's avatar has had in the watcher's stream.
    known: HashSet<ScopedObjectId>,
    /// Whether the mover's object is in the stream of the watcher's own
    /// region now.
    pub(crate) present: bool,
    /// Everything sent about the mover.
    sights: Vec<Sight>,
    /// Every coarse update.
    coarse: Vec<Coarse>,
}

impl Feed {
    /// A feed for `session`, watching `subject`, on the clock that began at
    /// `started`.
    pub(crate) fn new(
        session: &Session,
        agent: Uuid,
        subject: Option<Uuid>,
        started: Instant,
    ) -> Self {
        Self {
            agent,
            subject,
            root: session.circuit_id(),
            home: session.region_handle(),
            started,
            own: None,
            known: HashSet::new(),
            present: false,
            sights: Vec::new(),
            coarse: Vec::new(),
        }
    }

    /// Seconds from the start of the watch.
    pub(crate) fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Records one sight of the mover.
    fn sight(&mut self, what: Seen, root: bool) {
        let at = self.now();
        self.sights.push(Sight { at, what, root });
        if root {
            match what {
                Seen::Object | Seen::Moved => self.present = true,
                Seen::Removed => self.present = false,
                Seen::Appearance | Seen::Animation => {}
            }
        }
    }

    /// Keeps `event` if it says anything about the mover, the watcher's own
    /// avatar, or the coarse locations.
    pub(crate) fn note(&mut self, event: &Event) {
        match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                let id = object.full_id.uuid();
                if id == self.agent && Some(object.circuit) == self.root {
                    self.own = Some(object.motion.position.clone());
                }
                if Some(id) == self.subject {
                    let _new = self.known.insert(object.scoped_id());
                    let what = if matches!(event, Event::ObjectAdded(_)) {
                        Seen::Object
                    } else {
                        Seen::Moved
                    };
                    self.sight(what, Some(object.circuit) == self.root);
                }
            }
            Event::ObjectRemoved { local_id, .. } if self.known.contains(local_id) => {
                self.sight(Seen::Removed, Some(local_id.circuit) == self.root);
            }
            Event::AvatarAppearance(appearance)
                if Some(appearance.avatar_id.uuid()) == self.subject =>
            {
                self.sight(Seen::Appearance, true);
            }
            Event::AvatarAnimation { avatar_id, .. } if Some(avatar_id.uuid()) == self.subject => {
                self.sight(Seen::Animation, true);
            }
            Event::CoarseLocationUpdate {
                locations,
                you,
                prey,
                region_handle,
            } => {
                let entry = |who: Option<Uuid>| {
                    locations
                        .iter()
                        .enumerate()
                        .find(|(_index, location)| Some(location.agent_id.uuid()) == who)
                        .map(|(index, location)| (index, *location))
                };
                self.coarse.push(Coarse {
                    at: self.now(),
                    root: Some(*region_handle) == self.home,
                    region: *region_handle,
                    entries: locations.len(),
                    you: *you,
                    prey: *prey,
                    own: entry(Some(self.agent)),
                    subject: self.subject.and_then(|subject| entry(Some(subject))),
                });
            }
            _other => {}
        }
    }

    /// The coarse updates of the watcher's own region in `[from, to)`.
    pub(crate) fn root_coarse(&self, from: f64, to: f64) -> impl Iterator<Item = &Coarse> {
        self.coarse
            .iter()
            .filter(move |coarse| coarse.root && coarse.at >= from && coarse.at < to)
    }

    /// The first sight of `what` at or after `from`, on the watcher's own
    /// region's circuit or on a neighbour's.
    pub(crate) fn first(&self, what: Seen, root: bool, from: f64) -> Option<f64> {
        self.sights
            .iter()
            .find(|sight| sight.what == what && sight.root == root && sight.at >= from)
            .map(|sight| sight.at)
    }

    /// How many sights of `what` the watcher's own region sent in `[from, to)`.
    fn count(&self, what: Seen, from: f64, to: f64) -> u32 {
        tally(
            self.sights
                .iter()
                .filter(|sight| {
                    sight.what == what && sight.root && sight.at >= from && sight.at < to
                })
                .count(),
        )
    }

    /// When the coarse feed of the watcher's own region first changed its
    /// mind about listing the mover, at or after `from`.
    fn coarse_turns(&self, listed: bool, from: f64) -> Option<f64> {
        self.coarse
            .iter()
            .find(|coarse| coarse.root && coarse.at >= from && coarse.subject.is_some() == listed)
            .map(|coarse| coarse.at)
    }

    /// When a neighbouring region's coarse feed first listed the mover, at or
    /// after `from`.
    fn neighbour_coarse_lists(&self, from: f64) -> Option<f64> {
        self.coarse
            .iter()
            .find(|coarse| !coarse.root && coarse.at >= from && coarse.subject.is_some())
            .map(|coarse| coarse.at)
    }

    /// The first of each kind of sight at or after `from`, in the order they
    /// came, with the coarse feed's first listing among them: what announces
    /// an avatar.
    fn announcement(&self, from: f64) -> Vec<(f64, &'static str)> {
        let mut firsts: Vec<(f64, &'static str)> =
            [Seen::Object, Seen::Appearance, Seen::Animation]
                .into_iter()
                .filter_map(|what| {
                    self.first(what, true, from)
                        .map(|at| (at - from, what.word()))
                })
                .collect();
        if let Some(at) = self.coarse_turns(true, from) {
            firsts.push((at - from, "coarse"));
        }
        firsts.sort_by(|a, b| a.0.total_cmp(&b.0));
        firsts
    }
}

/// A count as a metric.
fn tally(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// A position as three figures.
fn xyz(position: &Vector) -> String {
    format!("{:.2} {:.2} {:.2}", position.x, position.y, position.z)
}

/// A coarse entry as three figures.
fn entry(location: &CoarseLocation) -> String {
    format!("{} {} {}", location.x, location.y, location.z)
}

/// An index of a coarse update, or `none`.
fn index(index: Option<usize>) -> String {
    index.map_or_else(|| "none".to_owned(), |index| index.to_string())
}

/// The straight-line distance between two region positions, in metres.
fn distance(a: &Vector, b: &Vector) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
}

/// The median of the gaps between consecutive `times`.
fn median_gap(times: &[f64]) -> Option<f64> {
    let mut gaps: Vec<f64> = times
        .iter()
        .zip(times.iter().skip(1))
        .map(|(earlier, later)| later - earlier)
        .collect();
    gaps.sort_by(f64::total_cmp);
    gaps.get(gaps.len() / 2).copied()
}

/// A wait that ran its whole window is what a watch is: only another failure
/// is one.
pub(crate) fn ran_out(outcome: Result<(), TestFailure>) -> Result<(), TestFailure> {
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// The two residents and what each side of the watch has recorded.
struct Pair<'a> {
    /// The avatar that acts.
    mover: &'a mut Session,
    /// The avatar that listens.
    watcher: &'a mut Session,
    /// The mover's own account of where it is.
    track: Track,
    /// What the watcher was sent.
    feed: Feed,
}

impl Pair<'_> {
    /// Listens on both sessions for `window`.
    async fn watch(&mut self, window: Duration) -> Result<(), TestFailure> {
        let track = &mut self.track;
        let feed = &mut self.feed;
        let (mover, watcher) = tokio::join!(
            self.mover.wait_for(window, |event| {
                track.note(event);
                None::<()>
            }),
            self.watcher.wait_for(window, |event| {
                feed.note(event);
                None::<()>
            }),
        );
        ran_out(mover).and_then(|()| ran_out(watcher))
    }

    /// Listens on both sessions for as long as `pending` holds, and no
    /// longer than `window`.
    async fn watch_while(
        &mut self,
        window: Duration,
        pending: impl Fn(&Track, &Feed) -> bool,
    ) -> Result<(), TestFailure> {
        let began = Instant::now();
        while pending(&self.track, &self.feed) {
            let remaining = window.saturating_sub(began.elapsed());
            if remaining.is_zero() {
                break;
            }
            self.watch(remaining.min(SLICE)).await?;
        }
        Ok(())
    }

    /// Seconds from the start of the watch.
    fn now(&self) -> f64 {
        self.feed.now()
    }

    /// How far apart the two avatars were at `at`, by the mover's own account
    /// of where it was and the watcher's of where it stands.
    fn separation(&self, at: f64) -> Option<f32> {
        let mover = self.track.reckon(at)?;
        let watcher = self.feed.own.as_ref()?;
        Some(distance(&mover, watcher))
    }

    /// Sets the watcher's draw distance.
    async fn draw(&self, metres: f64) -> Result<(), TestFailure> {
        self.watcher
            .send(Command::SetDrawDistance(Distance::new(metres)))
            .await
    }

    /// Puts the watcher's camera at `eye`, looking at `target`.
    async fn camera(&self, eye: Vector, target: Vector) -> Result<(), TestFailure> {
        self.watcher
            .send(Command::SetCamera(Camera::looking_at(eye, target)))
            .await
    }

    /// Puts the watcher's camera back at its own avatar, looking east.
    async fn camera_home(&self) -> Result<(), TestFailure> {
        let Some(own) = self.feed.own.clone() else {
            return Ok(());
        };
        self.camera(
            Vector {
                x: own.x,
                y: own.y,
                z: own.z + 1.0,
            },
            Vector {
                x: own.x + 10.0,
                y: own.y,
                z: own.z + 1.0,
            },
        )
        .await
    }
}

/// What the coarse feed of the watching session's own region looked like in
/// `[from, to)`.
struct Listing {
    /// Whether the session itself is listed.
    lists_self: bool,
    /// Whether `You` points at that entry.
    you_is_self: bool,
    /// The median gap between two updates, in seconds.
    interval: Option<f64>,
}

/// Records the coarse feed's shape over `[from, to)`.
fn record_coarse(feed: &Feed, from: f64, to: f64, metrics: &mut Metrics) -> Option<Listing> {
    let times: Vec<f64> = feed.root_coarse(from, to).map(|coarse| coarse.at).collect();
    metrics.set(&count_metric("coarse_idle"), tally(times.len()));
    let interval = median_gap(&times);
    if let Some(gap) = interval {
        metrics.set(&secs_metric("coarse_interval_idle"), gap);
    }
    let neighbours: BTreeSet<RegionHandle> = feed
        .coarse
        .iter()
        .filter(|coarse| !coarse.root)
        .map(|coarse| coarse.region)
        .collect();
    metrics.set(
        &count_metric("coarse_neighbour_feeds"),
        tally(neighbours.len()),
    );
    let last = feed.root_coarse(from, to).last()?;
    metrics.set(&count_metric("coarse_entries"), tally(last.entries));
    metrics.set("coarse_you", index(last.you));
    metrics.set("coarse_prey", index(last.prey));
    metrics.set(
        "coarse_self_index",
        index(last.own.as_ref().map(|(index, _)| *index)),
    );
    if let Some((_index, location)) = &last.own {
        metrics.set("coarse_self_entry", entry(location));
    }
    if let Some(own) = &feed.own {
        metrics.set("coarse_self_position", xyz(own));
    }
    if feed.subject.is_some() {
        metrics.set(
            "coarse_other_index",
            index(last.subject.as_ref().map(|(index, _)| *index)),
        );
        if let Some((_index, location)) = &last.subject {
            metrics.set("coarse_other_entry", entry(location));
        }
    }
    Some(Listing {
        lists_self: last.own.is_some(),
        you_is_self: last.you.is_some() && last.you == last.own.as_ref().map(|(index, _)| *index),
        interval,
    })
}

/// Holds the coarse feed's shape to what each grid was measured sending.
fn check_listing(grid: Grid, listing: Option<&Listing>) -> Result<(), TestFailure> {
    let listing = listing.ok_or_else(|| {
        TestFailure::Assertion("no CoarseLocationUpdate arrived in the idle watch".to_owned())
    })?;
    LISTS_SELF.check(
        "whether a coarse update lists the agent it is sent to",
        grid,
        &listing.lists_self,
    )?;
    YOU_IS_SELF.check(
        "whether a coarse update's You index points at the agent's own entry",
        grid,
        &listing.you_is_self,
    )?;
    let expected = *COARSE_INTERVAL_SECS.on(grid);
    let interval = listing.interval.ok_or_else(|| {
        TestFailure::Assertion("fewer than two coarse updates arrived in the idle watch".to_owned())
    })?;
    check(
        (interval - expected).abs() <= expected * INTERVAL_SLACK,
        &format!(
            "the coarse updates came {interval:.2} s apart on {grid}, where {expected} s was \
             measured ({})",
            COARSE_INTERVAL_SECS.source
        ),
    )
}

/// Both stand still: the coarse feed, and whether an idle avatar is re-sent.
async fn idle(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<Option<Listing>, TestFailure> {
    let from = pair.now();
    pair.watch(IDLE).await?;
    let to = pair.now();
    let listing = record_coarse(&pair.feed, from, to, metrics);
    if let Some(mover) = pair.track.latest() {
        metrics.set("coarse_other_position", xyz(&mover));
    }
    metrics.set(
        &count_metric("idle_other_updates"),
        pair.feed.count(Seen::Moved, from, to),
    );
    metrics.set(
        &count_metric("idle_other_animations"),
        pair.feed.count(Seen::Animation, from, to),
    );
    metrics.set(
        &count_metric("idle_other_appearances"),
        pair.feed.count(Seen::Appearance, from, to),
    );
    Ok(listing)
}

/// The watcher tracks the mover, then nobody: whether `Prey` follows.
async fn track(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let mover = AgentKey::from(pair.track.agent);
    let asked = pair.now();
    pair.watcher
        .send(Command::TrackAgent { prey_id: mover })
        .await?;
    pair.watch(TRACK).await?;
    let tracked = pair.now();
    let points_at_mover = |coarse: &Coarse| {
        coarse.prey.is_some() && coarse.prey == coarse.subject.as_ref().map(|(index, _)| *index)
    };
    let set = pair
        .feed
        .root_coarse(asked, tracked)
        .find(|coarse| points_at_mover(coarse))
        .map(|coarse| coarse.at - asked);
    metrics.set("track_sets_prey", set.is_some());
    if let Some(after) = set {
        metrics.set_timing(&secs_metric("track_prey"), after);
    }
    metrics.set(
        "track_prey_values",
        pair.feed
            .root_coarse(asked, tracked)
            .map(|coarse| index(coarse.prey))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(" "),
    );
    // Nobody: the nil key is how a viewer stops tracking.
    pair.watcher
        .send(Command::TrackAgent {
            prey_id: AgentKey::from(Uuid::nil()),
        })
        .await?;
    pair.watch(TRACK).await?;
    if set.is_some() {
        let cleared = pair
            .feed
            .root_coarse(tracked, pair.now())
            .last()
            .is_some_and(|coarse| coarse.prey.is_none());
        metrics.set("untrack_clears_prey", cleared);
    }
    Ok(())
}

/// The mover paces: how often the watcher hears of it.
async fn walk(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // Second Life holds an avatar that has just landed until its landing
    // animation is declared over (`book/src/gridspec/movement.md`).
    pair.mover.send(Command::FinishAnimation).await?;
    let from = pair.now();
    let began = pair.track.reckon(from);
    let mut furthest: f32 = 0.0;
    for stroke in 0..STROKES {
        let controls = if stroke % 2 == 0 {
            ControlFlags::AT_POS
        } else {
            ControlFlags::AT_NEG
        };
        pair.mover.send(Command::SetControls(controls)).await?;
        pair.watch(STROKE).await?;
        if let (Some(began), Some(latest)) = (&began, pair.track.latest()) {
            furthest = furthest.max(distance(began, &latest));
        }
    }
    let to = pair.now();
    pair.mover
        .send(Command::SetControls(ControlFlags::empty()))
        .await?;
    pair.watch(STROKE).await?;
    let walked = to - from;
    metrics.set("walk_furthest_m", f64::from(furthest));
    metrics.set(
        "walk_other_updates_per_sec",
        f64::from(pair.feed.count(Seen::Moved, from, to)) / walked,
    );
    metrics.set(
        "walk_own_updates_per_sec",
        f64::from(tally(
            pair.track
                .fixes
                .iter()
                .filter(|fix| fix.at >= from && fix.at < to)
                .count(),
        )) / walked,
    );
    let times: Vec<f64> = pair
        .feed
        .root_coarse(from, to)
        .map(|coarse| coarse.at)
        .collect();
    if let Some(gap) = median_gap(&times) {
        metrics.set(&secs_metric("coarse_interval_moving"), gap);
    }
    Ok(())
}

/// The mover climbs out of the watcher's draw distance and comes down again.
///
/// Returns whether the climb took the mover out of the watcher's stream.
async fn range(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<bool, TestFailure> {
    pair.draw(NEAR_DRAW_M).await?;
    pair.camera_home().await?;
    pair.watch(SETTLE).await?;
    metrics.set("near_present_at_64m", pair.feed.present);
    let ground = pair
        .track
        .latest()
        .ok_or_else(|| TestFailure::Assertion("the mover never saw its own avatar".to_owned()))?;

    let fly = ControlFlags::FLY;
    let climb_from = pair.now();
    pair.mover
        .send(Command::SetControls(fly | ControlFlags::UP_POS))
        .await?;
    pair.watch_while(CLIMB, |_track, feed| feed.present).await?;
    metrics.set("far_killed_while_climbing", !pair.feed.present);
    pair.mover.send(Command::SetControls(fly)).await?;
    pair.watch_while(RETIRE, |_track, feed| feed.present)
        .await?;
    // Past the kill, so the coarse feed has spoken again since.
    pair.watch(ARRIVAL).await?;
    let top = pair.now();
    if let Some(apart) = pair.separation(top) {
        metrics.set("far_distance_m", f64::from(apart));
    }
    if let Some(mover) = pair.track.latest() {
        metrics.set("far_other_position", xyz(&mover));
    }
    if let Some((_index, location)) = pair
        .feed
        .root_coarse(climb_from, top)
        .last()
        .and_then(|coarse| coarse.subject.as_ref())
    {
        metrics.set("far_coarse_entry", entry(location));
    }
    metrics.set(
        "far_coarse_lists_other",
        pair.feed
            .root_coarse(climb_from, top)
            .last()
            .is_some_and(|coarse| coarse.subject.is_some()),
    );
    metrics.set(
        &count_metric("climb_other_updates"),
        pair.feed.count(Seen::Moved, climb_from, top),
    );
    let kill = pair.feed.first(Seen::Removed, true, climb_from);
    metrics.set("far_killed", kill.is_some());
    if let Some(kill) = kill {
        metrics.set_timing(&secs_metric("far_kill"), kill - climb_from);
        if let Some(apart) = pair.separation(kill) {
            metrics.set("far_kill_distance_m", f64::from(apart));
        }
        metrics.set(
            &count_metric("far_updates_after_kill"),
            pair.feed.count(Seen::Moved, kill, top),
        );
        out_of_sight(pair, metrics).await?;
    } else if fresh_look(pair, "fresh", metrics).await? {
        // A grid that leaves an avatar it once sent in the stream may still
        // not send one it has yet to: the watcher has now arrived anew, and
        // was not sent it.
        out_of_sight(pair, metrics).await?;
        if pair.feed.present {
            // Sent after all, by a larger draw distance or the camera: out of
            // the stream again, so the way down says how near it has to come.
            let _absent = fresh_look(pair, "fresh_again", metrics).await?;
        }
    }

    // Down again, with the watcher's draw distance and camera as they were
    // for the climb.
    pair.draw(NEAR_DRAW_M).await?;
    pair.camera_home().await?;
    let absent = !pair.feed.present;
    let descent_from = pair.now();
    pair.mover
        .send(Command::SetControls(fly | ControlFlags::UP_NEG))
        .await?;
    let floor = ground.z + LANDED_M;
    pair.watch_while(DESCENT, |track, _feed| {
        track
            .reckon(track.now())
            .is_none_or(|position| position.z > floor)
    })
    .await?;
    pair.mover
        .send(Command::SetControls(ControlFlags::empty()))
        .await?;
    pair.watch(ARRIVAL).await?;
    pair.mover.send(Command::FinishAnimation).await?;
    if absent {
        let back = pair.feed.first(Seen::Object, true, descent_from);
        metrics.set("near_returns", back.is_some());
        if let Some(back) = back {
            metrics.set_timing(&secs_metric("near_return"), back - descent_from);
            if let Some(apart) = pair.separation(back) {
                metrics.set("near_return_distance_m", f64::from(apart));
            }
            metrics.set("near_return_announcement", sequence(&pair.feed, back - 1.0));
        }
    }
    pair.draw(DEFAULT_DRAW_M).await?;
    Ok(kill.is_some())
}

/// The watcher leaves for a neighbouring region and comes straight back,
/// with the mover still where it was: a region's scene is sent afresh to an
/// agent that arrives, so this asks whether an avatar this far off is among
/// what is sent. Returns whether it was left out; `false` too where there is
/// no neighbour to leave for.
async fn fresh_look(
    pair: &mut Pair<'_>,
    prefix: &str,
    metrics: &mut Metrics,
) -> Result<bool, TestFailure> {
    let Some(home) = pair.feed.home else {
        return Ok(false);
    };
    let Some(stood) = pair.feed.own.clone() else {
        return Ok(false);
    };
    let neighbour = match neighbouring_region(pair.watcher, GridCoordinates::from(home)).await {
        Ok(neighbour) => neighbour,
        Err(TestFailure::Assertion(_)) => return Ok(false),
        Err(other) => return Err(other),
    };
    let away = RegionHandle::from(neighbour.grid_coordinates);
    for (region, position, stay) in [
        // OpenSim does not take a second teleport on the heels of the first:
        // one asked for at once timed out.
        (away, (128.0, 128.0, 40.0), BETWEEN_TELEPORTS),
        // Nothing is listened to between the arrival and the reset below: what
        // comes in that time is the scene this asks about.
        (home, (stood.x, stood.y, stood.z), Duration::ZERO),
    ] {
        request_teleport(pair.watcher, region, position, (1.0, 0.0, 0.0)).await?;
        let trace = watch_teleport(pair.watcher, REGION_TIMEOUT).await?;
        if let Some(failure) = trace.failure {
            return Err(TestFailure::Assertion(format!(
                "the watcher's teleport failed: {}",
                failure.reason
            )));
        }
        if !stay.is_zero() {
            pair.watch(stay).await?;
        }
    }
    // The watcher's stream starts over: a new circuit, and nothing in it yet.
    pair.feed.root = pair.watcher.circuit_id();
    pair.feed.known.clear();
    pair.feed.present = false;
    pair.feed.own = None;
    let back = pair.now();
    pair.draw(NEAR_DRAW_M).await?;
    pair.watch_while(SETTLE, |_track, feed| feed.own.is_none())
        .await?;
    pair.camera_home().await?;
    pair.watch_while(APPEAR, |_track, feed| !feed.present)
        .await?;
    pair.watch(ARRIVAL).await?;
    metrics.set(&format!("{prefix}_look_present"), pair.feed.present);
    if let Some(apart) = pair.separation(pair.now()) {
        metrics.set(&format!("{prefix}_look_distance_m"), f64::from(apart));
    }
    if let Some(seen) = pair.feed.first(Seen::Object, true, back) {
        metrics.set_timing(&secs_metric(&format!("{prefix}_look_appear")), seen - back);
    }
    metrics.set(
        &format!("{prefix}_look_coarse_lists_other"),
        pair.feed
            .root_coarse(back, pair.now())
            .last()
            .is_some_and(|coarse| coarse.subject.is_some()),
    );
    Ok(!pair.feed.present)
}

/// The words of an announcement, in order.
fn sequence(feed: &Feed, from: f64) -> String {
    feed.announcement(from)
        .iter()
        .map(|(_at, word)| *word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The mover hovers out of the watcher's sight: what brings it back.
async fn out_of_sight(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // A larger draw distance, a step at a time.
    for metres in DRAW_STEPS_M {
        let asked = pair.now();
        pair.draw(metres).await?;
        pair.watch_while(APPEAR, |_track, feed| !feed.present)
            .await?;
        let key = format!("draw_{metres:.0}m");
        metrics.set(&format!("{key}_present"), pair.feed.present);
        if pair.feed.present {
            pair.watch(ARRIVAL).await?;
            if let Some(back) = pair.feed.first(Seen::Object, true, asked) {
                metrics.set_timing(&secs_metric(&format!("{key}_appear")), back - asked);
            }
            metrics.set(&format!("{key}_announcement"), sequence(&pair.feed, asked));
            break;
        }
    }
    // And back down: is it taken away again, and how soon.
    if pair.feed.present {
        let asked = pair.now();
        pair.draw(NEAR_DRAW_M).await?;
        pair.watch_while(RETIRE, |_track, feed| feed.present)
            .await?;
        metrics.set("draw_back_to_64m_kills", !pair.feed.present);
        if let Some(kill) = pair.feed.first(Seen::Removed, true, asked) {
            metrics.set_timing(&secs_metric("draw_back_to_64m_kill"), kill - asked);
        }
    }
    if pair.feed.present {
        return Ok(());
    }
    // The camera beside it, the avatar where it was: a grid that streams by
    // where the camera is sends it again.
    let Some(mover) = pair.track.latest() else {
        return Ok(());
    };
    let asked = pair.now();
    pair.camera(
        Vector {
            x: mover.x,
            y: mover.y,
            z: mover.z - CAMERA_BELOW_M,
        },
        mover,
    )
    .await?;
    pair.watch_while(APPEAR, |_track, feed| !feed.present)
        .await?;
    metrics.set("camera_beside_present", pair.feed.present);
    if let Some(back) = pair.feed.first(Seen::Object, true, asked) {
        metrics.set_timing(&secs_metric("camera_beside_appear"), back - asked);
    }
    if pair.feed.present {
        let asked = pair.now();
        pair.camera_home().await?;
        pair.watch_while(RETIRE, |_track, feed| feed.present)
            .await?;
        metrics.set("camera_home_kills", !pair.feed.present);
        if let Some(kill) = pair.feed.first(Seen::Removed, true, asked) {
            metrics.set_timing(&secs_metric("camera_home_kill"), kill - asked);
        }
    }
    Ok(())
}

/// Records, under `prefix`, what told the watcher the mover had left since
/// `asked`.
fn record_departure(feed: &Feed, prefix: &str, asked: f64, metrics: &mut Metrics) {
    let kill = feed.first(Seen::Removed, true, asked);
    metrics.set(&format!("{prefix}_kills"), kill.is_some());
    if let Some(kill) = kill {
        metrics.set_timing(&secs_metric(&format!("{prefix}_kill")), kill - asked);
    }
    // The feed listed it when it left, so the first update that does not is
    // the feed letting go.
    let gone = feed.coarse_turns(false, asked);
    metrics.set(&format!("{prefix}_coarse_drops"), gone.is_some());
    if let Some(gone) = gone {
        metrics.set_timing(&secs_metric(&format!("{prefix}_coarse_drop")), gone - asked);
    }
}

/// Records, under `prefix`, what announced the mover to the watcher since
/// `asked`, and when.
fn record_arrival(feed: &Feed, prefix: &str, asked: f64, metrics: &mut Metrics) {
    let announcement = feed.announcement(asked);
    metrics.set(
        &format!("{prefix}_announcement"),
        announcement
            .iter()
            .map(|(_at, word)| *word)
            .collect::<Vec<_>>()
            .join(" "),
    );
    for (after, word) in announcement {
        metrics.set_timing(&secs_metric(&format!("{prefix}_{word}")), after);
    }
    metrics.set(
        &count_metric(&format!("{prefix}_appearances")),
        feed.count(Seen::Appearance, asked, feed.now()),
    );
    metrics.set(
        &count_metric(&format!("{prefix}_animations")),
        feed.count(Seen::Animation, asked, feed.now()),
    );
}

/// One teleport of the mover, watched from the other session until it has
/// arrived and `after` has passed. Returns how long the teleport took.
async fn teleport(
    pair: &mut Pair<'_>,
    region: RegionHandle,
    position: (f32, f32, f32),
    after: Duration,
) -> Result<f64, TestFailure> {
    request_teleport(pair.mover, region, position, (1.0, 0.0, 0.0)).await?;
    let asked = Instant::now();
    let done = AtomicBool::new(false);
    let feed = &mut pair.feed;
    let mover = &mut *pair.mover;
    let (took, watched) = tokio::join!(
        async {
            let trace = watch_teleport(mover, REGION_TIMEOUT).await;
            let took = asked.elapsed().as_secs_f64();
            tokio::time::sleep(after).await;
            done.store(true, Ordering::Relaxed);
            trace.map(|trace| (trace, took))
        },
        pair.watcher
            .wait_for(REGION_TIMEOUT.saturating_add(after), |event| {
                feed.note(event);
                done.load(Ordering::Relaxed).then_some(())
            }),
    );
    ran_out(watched)?;
    let (trace, took) = took?;
    if let Some(failure) = trace.failure {
        return Err(TestFailure::Assertion(format!(
            "the mover's teleport failed: {}",
            failure.reason
        )));
    }
    Ok(took)
}

/// The mover teleports to a neighbouring region and back.
async fn teleports(pair: &mut Pair<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some(home) = pair.feed.home else {
        return Ok(());
    };
    let neighbour = match neighbouring_region(pair.mover, GridCoordinates::from(home)).await {
        Ok(neighbour) => neighbour,
        Err(TestFailure::Assertion(_)) => {
            metrics.set("teleport_leg", "no neighbouring region");
            return Ok(());
        }
        Err(other) => return Err(other),
    };
    metrics.set("teleport_leg", "run");
    let there = RegionHandle::from(neighbour.grid_coordinates);
    let asked = pair.now();
    let took = teleport(pair, there, (128.0, 128.0, 40.0), DEPARTURE).await?;
    metrics.set_timing(&secs_metric("depart_teleport"), took);
    record_departure(&pair.feed, "depart", asked, metrics);
    let through = pair.feed.first(Seen::Object, false, asked);
    metrics.set("depart_seen_through_neighbour", through.is_some());
    if let Some(through) = through {
        metrics.set_timing(&secs_metric("depart_neighbour_object"), through - asked);
    }
    let listed = pair.feed.neighbour_coarse_lists(asked);
    metrics.set("depart_neighbour_coarse_lists", listed.is_some());
    if let Some(listed) = listed {
        metrics.set_timing(&secs_metric("depart_neighbour_coarse"), listed - asked);
    }

    // And back, to beside the watcher.
    let beside = pair.feed.own.clone().map_or((128.0, 128.0, 40.0), |own| {
        (own.x + 3.0, own.y, own.z + 1.0)
    });
    let asked = pair.now();
    let took = teleport(pair, home, beside, ARRIVAL).await?;
    metrics.set_timing(&secs_metric("return_teleport"), took);
    record_arrival(&pair.feed, "return", asked, metrics);
    if let Some(kill) = pair.feed.first(Seen::Removed, false, asked) {
        metrics.set_timing(&secs_metric("return_neighbour_kill"), kill - asked);
    }
    Ok(())
}

/// The mover logs out and in again at `last`, the watcher listening
/// throughout. Leaves the mover's new session in the context.
async fn logout_and_login(
    ctx: &mut TestContext,
    feed: &mut Feed,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let (mover, watcher) = ctx.primary_and_secondary().ok_or_else(|| {
        TestFailure::Assertion("this case needs a second avatar (--secondary)".to_owned())
    })?;
    let asked = feed.now();
    let (logout, watched) = tokio::join!(
        log_out(mover),
        watcher.wait_for(DEPARTURE, |event| {
            feed.note(event);
            None::<()>
        }),
    );
    ran_out(watched)?;
    let logout = logout?;
    metrics.set_timing(&secs_metric("logout"), logout.seconds);
    metrics.set("logout_reply_received", logout.reply_received);
    record_departure(feed, "logout", asked, metrics);

    let done = AtomicBool::new(false);
    let started = feed.started;
    let (login, watched) = tokio::join!(
        async {
            let outcome = async {
                let answer = mover
                    .attempt_login(&LoginAttempt {
                        start: Some("last".to_owned()),
                        ..LoginAttempt::default()
                    })
                    .await?;
                let admitted = started.elapsed().as_secs_f64();
                let LoginAnswer::Admitted(mut next) = answer else {
                    return Err(TestFailure::Assertion(
                        "the mover's second login was not admitted".to_owned(),
                    ));
                };
                next.wait_for_region(REGION_TIMEOUT).await?;
                let ready = started.elapsed().as_secs_f64();
                tokio::time::sleep(ARRIVAL).await;
                Ok((next, admitted, ready))
            }
            .await;
            done.store(true, Ordering::Relaxed);
            outcome
        },
        watcher.wait_for(LOGIN_WINDOW, |event| {
            feed.note(event);
            done.load(Ordering::Relaxed).then_some(())
        }),
    );
    let (next, admitted, ready) = login?;
    ran_out(watched)?;
    *mover = *next;
    metrics.set_timing(&secs_metric("login_region_ready"), ready - admitted);
    record_arrival(feed, "login", admitted, metrics);
    Ok(())
}

/// The agent id of `session`.
pub(crate) fn agent_of(session: &Session) -> Result<Uuid, TestFailure> {
    session
        .agent_id()
        .map(|agent| agent.uuid())
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
}

/// The fake grid's one resident: its own coarse feed.
async fn alone(ctx: &mut TestContext) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let session = ctx.primary();
    session.wait_for_region(REGION_TIMEOUT).await?;
    let mut feed = Feed::new(session, agent_of(session)?, None, Instant::now());
    let from = feed.now();
    ran_out(
        session
            .wait_for(IDLE, |event| {
                feed.note(event);
                None::<()>
            })
            .await,
    )?;
    let mut metrics = Metrics::new();
    let listing = record_coarse(&feed, from, feed.now(), &mut metrics);
    ctx.metrics().merge(metrics);
    check_listing(grid, listing.as_ref())
}

/// Watches one avatar from another's session.
#[derive(Debug)]
pub struct AvatarPresence;

impl GridTest for AvatarPresence {
    fn name(&self) -> &'static str {
        "avatar-presence"
    }

    fn description(&self) -> &'static str {
        "Watch another avatar: the coarse feed, its range, and what announces and removes it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim => OPENSIM_START,
            Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            if is_fake(grid) {
                return alone(ctx).await;
            }
            let mut metrics = Metrics::new();
            let started = Instant::now();
            let (listing, killed, mut feed) = {
                let (mover, watcher) = ctx.primary_and_secondary().ok_or_else(|| {
                    TestFailure::Assertion(
                        "this case needs a second avatar (--secondary)".to_owned(),
                    )
                })?;
                mover.wait_for_region(REGION_TIMEOUT).await?;
                watcher.wait_for_region(REGION_TIMEOUT).await?;
                let (mover_id, watcher_id) = (agent_of(mover)?, agent_of(watcher)?);
                check(
                    mover.region_handle() == watcher.region_handle(),
                    "the two avatars did not log in to the same region",
                )?;
                let mut pair = Pair {
                    track: Track {
                        agent: mover_id,
                        started,
                        fixes: Vec::new(),
                    },
                    feed: Feed::new(watcher, watcher_id, Some(mover_id), started),
                    mover,
                    watcher,
                };
                pair.watch(SETTLE).await?;
                check(
                    pair.feed.present,
                    "the watcher was never sent the mover's avatar",
                )?;
                if let Some(apart) = pair.separation(pair.now()) {
                    metrics.set("start_distance_m", f64::from(apart));
                }
                let listing = idle(&mut pair, &mut metrics).await?;
                track(&mut pair, &mut metrics).await?;
                walk(&mut pair, &mut metrics).await?;
                let killed = range(&mut pair, &mut metrics).await?;
                teleports(&mut pair, &mut metrics).await?;
                (listing, killed, pair.feed)
            };
            let outcome = logout_and_login(ctx, &mut feed, &mut metrics).await;
            ctx.metrics().merge(metrics);
            outcome?;
            if feed.home.is_none() {
                ctx.mark_partial("the watcher's region handle was never established");
            }
            check_listing(grid, listing.as_ref())?;
            FAR_AVATAR_KILLED.check(
                "whether an avatar a kilometre off is taken out of the stream",
                grid,
                &killed,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Fix, Track, median_gap};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{Uuid, Vector};
    use std::time::Instant;

    /// The median gap of an evenly spaced feed is its spacing, and one late
    /// update does not move it.
    #[test]
    fn the_median_gap_ignores_one_late_update() {
        assert_eq!(median_gap(&[0.0, 1.0, 2.0, 3.0, 10.0]), Some(1.0));
        assert_eq!(median_gap(&[4.0]), None);
        assert_eq!(median_gap(&[]), None);
    }

    /// Between two updates the mover is reckoned on from the earlier one at
    /// its velocity.
    #[test]
    fn a_position_is_reckoned_from_the_last_update_before_it() -> Result<(), String> {
        let at_rest = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let track = Track {
            agent: Uuid::nil(),
            started: Instant::now(),
            fixes: vec![
                Fix {
                    at: 1.0,
                    position: Vector {
                        x: 10.0,
                        y: 20.0,
                        z: 30.0,
                    },
                    velocity: Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 16.0,
                    },
                },
                Fix {
                    at: 11.0,
                    position: Vector {
                        x: 10.0,
                        y: 20.0,
                        z: 190.0,
                    },
                    velocity: at_rest,
                },
            ],
        };
        let climbing = track.reckon(3.5).ok_or("no position while climbing")?;
        assert_eq!(climbing.z.to_bits(), 70.0_f32.to_bits());
        let hovering = track.reckon(60.0).ok_or("no position while hovering")?;
        assert_eq!(hovering.z.to_bits(), 190.0_f32.to_bits());
        // Before the first update there is only the first update to go by.
        let before = track.reckon(0.0).ok_or("no position before the first")?;
        assert_eq!(before.z.to_bits(), 30.0_f32.to_bits());
        Ok(())
    }
}
