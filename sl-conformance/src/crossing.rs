//! What a region's neighbours and a walk over its border looked like from the
//! client: which regions were announced and when, what the hand-over raised,
//! and what became of the circuits and of the avatar's own object on the way.
//!
//! The neighbour cases (`neighbour-child-circuits`, `draw-distance`,
//! `region-crossing`) watch the same handful of events and differ in what they
//! ask of them, so the watching is here once
//! (`book/src/gridspec/teleport.md`, *Neighbours and crossings*).
//!
//! A crossing is the grid's to decide, so on a live grid the avatar is
//! *walked* at a border ([`walk_over_border`]) the way a viewer walks one:
//! face it, hold the forward key, and re-send that every quarter second until
//! the session reports the region changed.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, CircuitId, Command, ControlFlags, Event, NeighborInfo, NeighborRetirement,
    RegionHandle, Rotation, ScopedObjectId, Vector,
};

use crate::context::{Session, TestFailure};
use crate::metrics::Metrics;

/// The side of a region in metres. Both live grids' regions here are the
/// standard size; a variable-sized OpenSim region would need its handshake's.
const REGION_SIZE_M: f32 = 256.0;

/// How long a flight also pushes upwards for, to get off the ground.
const LIFT_OFF: Duration = Duration::from_secs(1);

/// How often the walk re-sends its heading and its forward key.
const STEER_INTERVAL: Duration = Duration::from_millis(250);

/// A neighbouring region as it was announced.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbour {
    /// The announcement.
    pub info: NeighborInfo,
    /// Seconds from the start of the watch to the announcement.
    pub announced: f64,
    /// Seconds from the start of the watch to its seed capability, when one
    /// came.
    pub seeded: Option<f64>,
    /// Seconds from the start of the watch to the first object stamped with
    /// its handle, when one came: the child circuit is carrying the region.
    pub streamed: Option<f64>,
}

/// Our own avatar as the object stream last showed it.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnAvatar {
    /// Its scoped id: a circuit and the region-local id that circuit's
    /// simulator gave it.
    pub id: ScopedObjectId,
    /// The region it was reported in.
    pub region_handle: RegionHandle,
    /// Where in that region.
    pub position: Vector,
}

/// What an arrival's neighbour announcements looked like.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Neighbourhood {
    /// The neighbours, in the order they were announced.
    pub neighbours: Vec<Neighbour>,
    /// Our own avatar, when the object stream showed it.
    pub own: Option<OwnAvatar>,
}

impl Neighbourhood {
    /// The announcement offsets, in order, to the hundredth of a second.
    #[must_use]
    pub fn timeline(&self) -> String {
        self.neighbours
            .iter()
            .map(|neighbour| format!("{:.2}", neighbour.announced))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Where each neighbour lies relative to `here`, in region slots, in the
    /// order announced (`-1/0` is the region to the west).
    #[must_use]
    pub fn offsets(&self, here: RegionHandle) -> String {
        self.neighbours
            .iter()
            .map(|neighbour| {
                let (dx, dy) = slot_offset(here, neighbour.info.region_handle);
                format!("{dx}/{dy}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Record the neighbourhood under `prefix`.
    pub fn record(&self, prefix: &str, here: Option<RegionHandle>, metrics: &mut Metrics) {
        let key = |name: &str| format!("{prefix}{name}");
        metrics.set(&key("count"), tally(self.neighbours.len()));
        metrics.set(&key("announced_at"), self.timeline());
        if let Some(here) = here {
            metrics.set(&key("offsets"), self.offsets(here));
        }
        metrics.set(
            &key("seeded"),
            tally(
                self.neighbours
                    .iter()
                    .filter(|neighbour| neighbour.seeded.is_some())
                    .count(),
            ),
        );
        metrics.set(
            &key("streaming"),
            tally(
                self.neighbours
                    .iter()
                    .filter(|neighbour| neighbour.streamed.is_some())
                    .count(),
            ),
        );
    }
}

/// A count as a metric value.
#[must_use]
pub fn tally(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// How many region slots `there` lies east and north of `here`.
#[must_use]
pub fn slot_offset(here: RegionHandle, there: RegionHandle) -> (i64, i64) {
    let (here_x, here_y) = here.grid_coordinates();
    let (there_x, there_y) = there.grid_coordinates();
    (
        i64::from(there_x).saturating_sub(i64::from(here_x)),
        i64::from(there_y).saturating_sub(i64::from(here_y)),
    )
}

/// Watches an arrival's neighbour announcements: every
/// [`Event::NeighborDiscovered`], the seed and the first object of each, and
/// our own avatar, until nothing new has been announced for `quiet` or
/// `window` has passed.
///
/// # Errors
///
/// Propagates [`Session::wait_for`] failures other than its timeout.
pub async fn watch_neighbours(
    session: &mut Session,
    agent: AgentKey,
    window: Duration,
    quiet: Duration,
) -> Result<Neighbourhood, TestFailure> {
    let started = Instant::now();
    let mut found = Neighbourhood::default();
    let mut last_news = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        let idle = quiet.saturating_sub(last_news.elapsed());
        let wait = remaining.min(idle);
        if wait.is_zero() {
            return Ok(found);
        }
        let news = session
            .wait_for(wait, |event| {
                note_neighbour_event(&mut found, agent, started, event)
            })
            .await;
        match news {
            Ok(()) => last_news = Instant::now(),
            Err(TestFailure::Timeout(_)) => return Ok(found),
            Err(other) => return Err(other),
        }
    }
}

/// Folds one event into `found`, returning `Some` when it was news about a
/// neighbour (an announcement, a seed, or a neighbour's first object).
fn note_neighbour_event(
    found: &mut Neighbourhood,
    agent: AgentKey,
    started: Instant,
    event: &Event,
) -> Option<()> {
    let now = started.elapsed().as_secs_f64();
    match event {
        Event::NeighborDiscovered(info) => {
            found.neighbours.push(Neighbour {
                info: info.clone(),
                announced: now,
                seeded: None,
                streamed: None,
            });
            Some(())
        }
        Event::NeighborSeed { sim, .. } => {
            let neighbour = found
                .neighbours
                .iter_mut()
                .find(|neighbour| neighbour.info.sim == *sim && neighbour.seeded.is_none())?;
            neighbour.seeded = Some(now);
            Some(())
        }
        Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
            if object.full_id.uuid() == agent.uuid() {
                found.own = Some(OwnAvatar {
                    id: object.scoped_id(),
                    region_handle: object.region_handle,
                    position: object.motion.position.clone(),
                });
                return None;
            }
            let neighbour = found.neighbours.iter_mut().find(|neighbour| {
                neighbour.info.region_handle == object.region_handle && neighbour.streamed.is_none()
            })?;
            neighbour.streamed = Some(now);
            Some(())
        }
        _other => None,
    }
}

/// A border of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The border at `x` = 256.
    East,
    /// The border at `x` = 0.
    West,
    /// The border at `y` = 256.
    North,
    /// The border at `y` = 0.
    South,
}

impl Edge {
    /// The edge shared with a region `(dx, dy)` slots away, for a region that
    /// shares one (a diagonal neighbour touches at a corner only).
    #[must_use]
    pub const fn towards(dx: i64, dy: i64) -> Option<Self> {
        match (dx, dy) {
            (1, 0) => Some(Self::East),
            (-1, 0) => Some(Self::West),
            (0, 1) => Some(Self::North),
            (0, -1) => Some(Self::South),
            _ => None,
        }
    }

    /// The edge on the other side: the way back.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::East => Self::West,
            Self::West => Self::East,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }

    /// The edge's name, for a metric.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::South => "south",
        }
    }

    /// How far `position` is from this edge, in metres.
    #[must_use]
    pub fn distance_from(self, position: &Vector) -> f32 {
        match self {
            Self::East => REGION_SIZE_M - position.x,
            Self::West => position.x,
            Self::North => REGION_SIZE_M - position.y,
            Self::South => position.y,
        }
    }

    /// The body rotation that faces this edge squarely: a turn about the
    /// vertical from the avatar's rest facing, which is east.
    #[must_use]
    pub fn facing(self) -> Rotation {
        let yaw = match self {
            Self::East => 0.0_f32,
            Self::North => core::f32::consts::FRAC_PI_2,
            Self::West => core::f32::consts::PI,
            Self::South => -core::f32::consts::FRAC_PI_2,
        };
        let half = yaw / 2.0;
        Rotation {
            x: 0.0,
            y: 0.0,
            z: half.sin(),
            s: half.cos(),
        }
    }
}

/// The announced neighbour whose shared border is nearest `own`, with that
/// border and the distance to it. `None` when no announced neighbour shares
/// an edge with `here`.
#[must_use]
pub fn nearest_border(
    found: &Neighbourhood,
    here: RegionHandle,
    own: &Vector,
) -> Option<(NeighborInfo, Edge, f32)> {
    found
        .neighbours
        .iter()
        .filter_map(|neighbour| {
            let (dx, dy) = slot_offset(here, neighbour.info.region_handle);
            let edge = Edge::towards(dx, dy)?;
            Some((neighbour.info.clone(), edge, edge.distance_from(own)))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// The arrival a crossing ended in ([`Event::RegionChanged`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrival {
    /// The region arrived in.
    pub region_handle: RegionHandle,
    /// Its simulator.
    pub sim: SocketAddr,
    /// The root circuit after the crossing.
    pub circuit: CircuitId,
    /// Whether the client dropped the world it left. A crossing keeps it.
    pub world_reset: bool,
    /// Seconds from the start of the watch to the arrival.
    pub at: f64,
}

/// A neighbour's circuit going away ([`Event::NeighborRetired`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Retired {
    /// The region, when the session knew its handle.
    pub region_handle: Option<RegionHandle>,
    /// Why it went.
    pub reason: NeighborRetirement,
    /// Seconds from the start of the watch.
    pub at: f64,
}

/// Everything a walk over a border raised, from the first step to the end of
/// the settle that follows the arrival.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    /// The arrival, when the walk got that far.
    pub arrival: Option<Arrival>,
    /// The first teleport event seen on the way, if any: a crossing is not a
    /// teleport, and a client that raised one treated it as one.
    pub teleport: Option<String>,
    /// Seconds to the removal of our own avatar's object from the region left
    /// behind. This is the client's doing as much as the grid's: the session
    /// drops the old region's copy of an avatar the moment the new region's
    /// arrives, so it says when the avatar changed hands, not whether the grid
    /// sent a `KillObject` (the trace log says that).
    pub own_removed_from_source: Option<f64>,
    /// Seconds to our own avatar's first appearance on the destination's
    /// circuit, and where.
    pub own_on_destination: Option<(f64, Vector)>,
    /// Neighbours announced during or after the crossing.
    pub announced: Vec<(NeighborInfo, f64)>,
    /// Neighbours retired during or after the crossing.
    pub retired: Vec<Retired>,
}

impl Trace {
    /// Record the trace under `prefix`. `from` is the region left, for the
    /// offsets of what was announced and retired.
    pub fn record(&self, prefix: &str, from: RegionHandle, metrics: &mut Metrics) {
        let key = |name: &str| format!("{prefix}{name}");
        metrics.set(&key("crossed"), self.arrival.is_some());
        if let Some(arrival) = &self.arrival {
            metrics.set(&key("world_reset"), arrival.world_reset);
            metrics.set_timing(&key("walk_secs"), arrival.at);
            let slots = |handle: RegionHandle| {
                let (dx, dy) = slot_offset(arrival.region_handle, handle);
                format!("{dx}/{dy}")
            };
            metrics.set(
                &key("announced_after"),
                self.announced
                    .iter()
                    .map(|(info, at)| {
                        format!("{}@{:.1}", slots(info.region_handle), at - arrival.at)
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            metrics.set(
                &key("retired_after"),
                self.retired
                    .iter()
                    .map(|retired| {
                        // The reason matters: a neighbour the grid retired
                        // and one that simply fell silent are different
                        // findings.
                        format!(
                            "{}@{:.1}:{:?}",
                            retired.region_handle.map_or_else(|| "?".to_owned(), slots),
                            retired.at - arrival.at,
                            retired.reason
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            if let Some(at) = self.own_removed_from_source {
                metrics.set(&key("own_left_source_after_secs"), at - arrival.at);
            }
            if let Some((at, position)) = &self.own_on_destination {
                metrics.set(&key("own_on_destination_after_secs"), at - arrival.at);
                metrics.set(
                    &key("arrived_at"),
                    format!("{:.1} {:.1} {:.1}", position.x, position.y, position.z),
                );
            }
        }
        let (dx, dy) = self
            .arrival
            .map_or((0, 0), |arrival| slot_offset(from, arrival.region_handle));
        metrics.set(&key("direction"), format!("{dx}/{dy}"));
        metrics.set(
            &key("teleport_event"),
            self.teleport.clone().unwrap_or_else(|| "none".to_owned()),
        );
    }
}

/// Which way a walk steers, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Steer {
    /// The border to walk at.
    pub edge: Edge,
    /// Whether to fly rather than walk: for a border far enough away that
    /// whatever stands on the ground in between would be in the way.
    pub fly: bool,
}

/// How a walk is carried out.
#[derive(Debug, Clone, Copy)]
pub struct Walk {
    /// Where to steer the avatar. `None` when something else moves it — the
    /// fake grid simulates no movement, so a test asks it for the crossing —
    /// and the walk only watches.
    pub steer: Option<Steer>,
    /// How long to keep trying to reach the border.
    pub budget: Duration,
    /// How long to keep walking after the arrival, to get clear of the border
    /// before stopping (an avatar left standing on one is handed back and
    /// forth).
    pub carry_on: Duration,
    /// How long to keep watching after stopping, for what the grid does with
    /// the circuits once the avatar has settled.
    pub settle: Duration,
}

/// Walks our own avatar at a border until the session reports the region
/// changed, carries on a little, stops, and watches for [`Walk::settle`].
///
/// Not crossing is an answer rather than a failure: the trace then has no
/// arrival, and the caller decides what that means.
///
/// `own` is our avatar as last seen on the region being left, so its removal
/// there can be told from any other object's.
///
/// # Errors
///
/// Propagates the sends' failures and [`Session::wait_for`]'s other than its
/// timeout.
pub async fn walk_over_border(
    session: &mut Session,
    agent: AgentKey,
    own: Option<&OwnAvatar>,
    walk: Walk,
) -> Result<Trace, TestFailure> {
    let started = Instant::now();
    let source_circuit = session.circuit_id();
    let mut source_id = own.map(|own| own.id);
    let mut trace = Trace::default();
    let steering = walk.steer.map(|steer| {
        let forwards = if steer.fly {
            ControlFlags::AT_POS | ControlFlags::FLY
        } else {
            ControlFlags::AT_POS
        };
        tracing::info!(
            edge = steer.edge.label(),
            fly = steer.fly,
            "walking at a border"
        );
        (steer.edge.facing(), forwards)
    });
    // The three phases are one loop with one predicate, so no event between
    // them is lost: the walk's own waits would otherwise discard what the grid
    // sends in the very moment of the hand-over.
    let mut stop_at: Option<Instant> = None;
    let mut stopped_at: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if stopped_at.is_none() {
            let give_up = trace.arrival.is_none() && started.elapsed() >= walk.budget;
            let done = stop_at.is_some_and(|at| now >= at);
            if give_up || done {
                if steering.is_some() {
                    session
                        .send(Command::SetControls(ControlFlags::empty()))
                        .await?;
                }
                stopped_at = Some(now);
                if give_up {
                    tracing::info!("the walk never reached another region");
                    return Ok(trace);
                }
            } else if let Some((facing, forwards)) = &steering {
                session
                    .send(Command::SetRotation {
                        body: facing.clone(),
                        head: facing.clone(),
                    })
                    .await?;
                // A flight starts by leaving the ground: an avatar that the
                // ground holds stays held with the forward key alone.
                let lifting = forwards.contains(ControlFlags::FLY) && started.elapsed() < LIFT_OFF;
                let controls = if lifting {
                    *forwards | ControlFlags::UP_POS
                } else {
                    *forwards
                };
                session.send(Command::SetControls(controls)).await?;
            }
        }
        if stopped_at.is_some_and(|at| at.elapsed() >= walk.settle) {
            return Ok(trace);
        }
        let wait = stopped_at.map_or(STEER_INTERVAL, |at| {
            walk.settle.saturating_sub(at.elapsed()).min(STEER_INTERVAL)
        });
        let outcome = session
            .wait_for(wait, |event| {
                note_crossing_event(
                    &mut trace,
                    &mut source_id,
                    source_circuit,
                    agent,
                    started,
                    event,
                );
                None::<()>
            })
            .await;
        match outcome {
            Ok(()) | Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
        if trace.arrival.is_some() && stop_at.is_none() {
            // An addition that overflows the clock stops the walk now.
            let now = Instant::now();
            stop_at = Some(now.checked_add(walk.carry_on).unwrap_or(now));
        }
    }
}

/// Folds one event into a crossing's trace.
fn note_crossing_event(
    trace: &mut Trace,
    source_id: &mut Option<ScopedObjectId>,
    source_circuit: Option<CircuitId>,
    agent: AgentKey,
    started: Instant,
    event: &Event,
) {
    let now = started.elapsed().as_secs_f64();
    match event {
        Event::RegionChanged {
            region_handle,
            sim,
            circuit,
            world_reset,
        } if trace.arrival.is_none() => {
            trace.arrival = Some(Arrival {
                region_handle: *region_handle,
                sim: *sim,
                circuit: *circuit,
                world_reset: *world_reset,
                at: now,
            });
        }
        Event::TeleportStarted { .. }
        | Event::TeleportFinished { .. }
        | Event::TeleportFailed { .. }
        | Event::TeleportLocal { .. } => {
            let _first = trace.teleport.get_or_insert_with(|| format!("{event:?}"));
        }
        Event::NeighborDiscovered(info) => trace.announced.push((info.clone(), now)),
        Event::NeighborRetired {
            region_handle,
            reason,
            ..
        } => trace.retired.push(Retired {
            region_handle: *region_handle,
            reason: *reason,
            at: now,
        }),
        Event::ObjectAdded(object) | Event::ObjectUpdated(object)
            if object.full_id.uuid() == agent.uuid() =>
        {
            let id = object.scoped_id();
            if Some(id.circuit()) == source_circuit {
                // Still (or again) on the region being left: this is the id
                // its removal will name.
                *source_id = Some(id);
            } else if trace.own_on_destination.is_none() {
                trace.own_on_destination = Some((now, object.motion.position.clone()));
            }
        }
        Event::ObjectRemoved { local_id, .. }
            if Some(*local_id) == *source_id && trace.own_removed_from_source.is_none() =>
        {
            trace.own_removed_from_source = Some(now);
        }
        _other => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{Edge, Neighbour, Neighbourhood, nearest_border, slot_offset};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{GridCoordinates, NeighborInfo, RegionHandle, Vector};
    use std::net::{Ipv4Addr, SocketAddr};

    /// A neighbour of the region at 1000/1000, `(dx, dy)` slots away.
    fn neighbour(dx: i64, dy: i64) -> Result<Neighbour, String> {
        let x = u32::try_from(1000_i64.saturating_add(dx)).map_err(|error| error.to_string())?;
        let y = u32::try_from(1000_i64.saturating_add(dy)).map_err(|error| error.to_string())?;
        Ok(Neighbour {
            info: NeighborInfo {
                region_handle: RegionHandle::from_grid(x, y),
                sim: SocketAddr::from((Ipv4Addr::LOCALHOST, 9001)),
                grid_coordinates: GridCoordinates::new(x, y),
            },
            announced: 0.0,
            seeded: None,
            streamed: None,
        })
    }

    #[test]
    fn the_nearest_border_is_an_edge_and_never_a_corner() -> Result<(), String> {
        let here = RegionHandle::from_grid(1000, 1000);
        // Standing in the south-west corner, as the aditi avatar does: the
        // diagonal neighbour is the closest region and shares no edge.
        let own = Vector {
            x: 7.7,
            y: 10.1,
            z: 40.0,
        };
        let found = Neighbourhood {
            neighbours: vec![neighbour(-1, -1)?, neighbour(0, -1)?, neighbour(-1, 0)?],
            own: None,
        };
        let (info, edge, distance) =
            nearest_border(&found, here, &own).ok_or("no border was chosen")?;
        assert_eq!(edge, Edge::West);
        assert_eq!(slot_offset(here, info.region_handle), (-1, 0));
        assert!((distance - 7.7).abs() < 1e-4, "got {distance}");
        Ok(())
    }

    #[test]
    fn a_region_with_only_a_diagonal_neighbour_has_no_border_to_walk_at() -> Result<(), String> {
        let here = RegionHandle::from_grid(1000, 1000);
        let own = Vector {
            x: 128.0,
            y: 128.0,
            z: 30.0,
        };
        let found = Neighbourhood {
            neighbours: vec![neighbour(1, 1)?],
            own: None,
        };
        assert_eq!(nearest_border(&found, here, &own), None);
        Ok(())
    }

    #[test]
    fn each_edge_faces_out_of_the_region_and_back_is_its_opposite() {
        for edge in [Edge::East, Edge::West, Edge::North, Edge::South] {
            let facing = edge.facing();
            // The avatar's forward axis is +X; rotate it about Z by the yaw the
            // quaternion encodes.
            let yaw = 2.0 * facing.z.atan2(facing.s);
            let (x, y) = (yaw.cos(), yaw.sin());
            let expected = match edge {
                Edge::East => (1.0, 0.0),
                Edge::West => (-1.0, 0.0),
                Edge::North => (0.0, 1.0),
                Edge::South => (0.0, -1.0),
            };
            assert!(
                (x - expected.0).abs() < 1e-5 && (y - expected.1).abs() < 1e-5,
                "{edge:?} faces {x}/{y}"
            );
            assert_eq!(edge.opposite().opposite(), edge);
        }
    }
}
