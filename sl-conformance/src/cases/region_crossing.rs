//! Walking over a border is a promotion, not a reconnection.
//!
//! When an avatar walks off the edge of a region the simulator hands it to the
//! neighbour: `CrossedRegion` names the destination's circuit and seed, the
//! client sends its `CompleteAgentMovement` there, and the **child** circuit it
//! has held since arrival becomes its root. Nothing is torn down — no teleport
//! screen, no cleared scene, no fresh circuit — which is the whole difference
//! between a crossing and a teleport to the same place.
//!
//! What this asserts, on every grid, of each crossing:
//!
//! - it raises a [`RegionChanged`](sl_client_tokio::Event::RegionChanged)
//!   naming the destination, on the simulator the neighbour was announced at,
//! - with `world_reset: false` — the client kept and re-based its scene rather
//!   than clearing it,
//! - on a different circuit than the source region's,
//! - and it raised **no** teleport event on the way.
//!
//! What it records (`book/src/gridspec/teleport.md`, *Neighbours and
//! crossings*): the neighbours as they were announced, how long the walk took,
//! where the avatar was put down, and which neighbours were announced or
//! retired once it had settled next door.
//!
//! How the avatar gets over the border depends on who decides a crossing. A
//! live grid decides it from where the avatar is, so there the avatar is
//! *walked*: at the nearest border an announced neighbour shares, a few metres
//! past it, and — once the grid has had time to rearrange its circuits — back
//! again, which is a second crossing and leaves the avatar where it started.
//! The fake grid simulates no movement, so
//! [`FakeGrid::cross_agent`](sl_fake_grid::FakeGrid::cross_agent) is the
//! scripted stand-in for walking, east and one way.

use std::time::Duration;

use sl_client_tokio::{AgentKey, NeighborInfo, RegionCoordinates, RegionHandle, Vector};

use crate::context::{Session, TestContext, TestFailure};
use crate::crossing::{
    Edge, Neighbourhood, OwnAvatar, Steer, Trace, Walk, nearest_border, walk_over_border,
    watch_neighbours,
};
use crate::fake::FakeControl;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, is_aditi, is_opensim};

/// How long to watch an arrival for its neighbour announcements. A crossing
/// into a region the client holds no child circuit for is a different path,
/// and would make this case assert the wrong thing.
const NEIGHBOUR_WINDOW: Duration = Duration::from_secs(30);

/// The gap without a new announcement that ends the watch. OpenSim announces
/// its neighbours half a second apart; Second Life in one batch.
const NEIGHBOUR_QUIET: Duration = Duration::from_secs(6);

/// How long a walk may take to reach its border.
const WALK_BUDGET: Duration = Duration::from_secs(90);

/// How long to keep walking after a crossing: about ten metres at a walk.
const CARRY_ON: Duration = Duration::from_secs(3);

/// How long to watch after stopping. Second Life takes its time over a
/// neighbour that is no longer wanted — fifty seconds after a draw distance
/// stopped reaching one — so a retirement inside this window is one a crossing
/// caused promptly, and the absence of one is not proof that none follows.
const SETTLE: Duration = Duration::from_secs(25);

/// The fake grid's settle: nothing there waits on a clock.
const FAKE_SETTLE: Duration = Duration::from_secs(3);

/// A border further off than this is flown to: whatever stands on the ground
/// in between would otherwise be in the way.
const FLY_BEYOND_M: f32 = 40.0;

/// Where the avatar logs in on OpenSim: twelve metres from the eastern border
/// of the block's south-western region.
const OPENSIM_START: &str = "uri:Default Region&244&128&25";

/// Where in the destination region the fake grid's scripted crossing lands the
/// agent: just over the shared edge, halfway along it, on the stock ground.
const FAKE_ARRIVAL: (f32, f32, f32) = (8.0, 128.0, 26.0);

/// The case's overall budget: two walks, each with its settle, after a
/// neighbour watch.
const CASE_TIMEOUT: Duration = Duration::from_secs(6 * 60);

/// Walks the agent over a region border and checks the client promoted its
/// child circuit rather than rebuilding the world.
#[derive(Debug)]
pub struct RegionCrossing;

impl GridTest for RegionCrossing {
    fn name(&self) -> &'static str {
        "region-crossing"
    }

    fn description(&self) -> &'static str {
        "Walk over a region border and check the child circuit was promoted, not replaced"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
            let grid = ctx.grid();
            // Taken by value up front: everything below borrows the context
            // mutably to drive the session, and the grid-side handle is a
            // cheap clone of two `Arc`s.
            let control = ctx.fake().cloned();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let home = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login established no region handle".to_owned())
            })?;

            // The child circuit has to exist before the crossing, or the
            // hand-over is a different path entirely (a fresh circuit to an
            // unconnected region, which *does* reset the world).
            let found = watch_neighbours(session, agent, NEIGHBOUR_WINDOW, NEIGHBOUR_QUIET).await?;

            let (out, back) = match &control {
                Some(control) => {
                    let out = cross_scripted(session, control, agent, &found, home).await?;
                    (out, None)
                }
                None => {
                    let Some((out, back)) =
                        walk_there_and_back(session, grid, agent, &found, home).await?
                    else {
                        found.record("neighbours_", Some(home), ctx.metrics());
                        ctx.mark_partial(
                            "no announced neighbour shares a border with the region the avatar \
                             stands in, so there is nothing to walk over",
                        );
                        return Ok(());
                    };
                    (out, Some(back))
                }
            };

            let metrics = ctx.metrics();
            found.record("neighbours_", Some(home), metrics);
            out.trace.record("out_", home, metrics);
            if let Some(back) = &back {
                back.trace
                    .record("back_", out.destination.region_handle, metrics);
            }
            Ok(())
        })
    }
}

/// One crossing that was carried out and checked.
struct Crossed {
    /// The neighbour crossed into, as it was announced.
    destination: NeighborInfo,
    /// Everything the crossing raised.
    trace: Trace,
}

/// Holds one crossing to what every grid has to get right, and returns it.
///
/// `from` is the root circuit before the crossing and `destination` the
/// neighbour the avatar was sent at.
fn checked(
    session: &Session,
    from: Option<sl_client_tokio::CircuitId>,
    destination: NeighborInfo,
    trace: Trace,
) -> Result<Crossed, TestFailure> {
    let arrival = trace.arrival.ok_or_else(|| {
        TestFailure::Assertion(
            "the avatar never arrived in the region across the border".to_owned(),
        )
    })?;
    check_eq(
        "the region crossed into",
        &arrival.region_handle,
        &destination.region_handle,
    )?;
    check_eq("the destination simulator", &arrival.sim, &destination.sim)?;
    check(
        !arrival.world_reset,
        "the crossing reset the client's world — it rebuilt the scene instead of re-basing the \
         one it already held",
    )?;
    check(
        Some(arrival.circuit) != from,
        "the crossing kept the source region's circuit as the root",
    )?;
    check_eq(
        "the region the session is in after the crossing",
        &session.region_handle(),
        &Some(destination.region_handle),
    )?;
    check(
        trace.teleport.is_none(),
        &format!(
            "a border crossing raised {}, so the client did not treat it as a hand-over",
            trace.teleport.as_deref().unwrap_or("a teleport event")
        ),
    )?;
    Ok(Crossed { destination, trace })
}

/// Whether the avatar flies to a border `distance` metres away rather than
/// walking there.
///
/// Always on Second Life, and not for the view: an avatar that arrives at a
/// region's landing point is held in its landing animation until a viewer
/// reports it finished (`FINISH_ANIM`), and this client plays no animations.
/// Ninety seconds of the forward key moved it two centimetres on 2026-10-07; a
/// flight, which ends the landing, crossed the border eight metres away in
/// four seconds (`book/src/gridspec/movement.md`).
fn flies(grid: Grid, distance: f32) -> bool {
    is_aditi(grid) || distance > FLY_BEYOND_M
}

/// The live grids' crossing: walk at the nearest shared border, then back.
/// `None` when no announced neighbour shares a border with `home`.
async fn walk_there_and_back(
    session: &mut Session,
    grid: Grid,
    agent: AgentKey,
    found: &Neighbourhood,
    home: RegionHandle,
) -> Result<Option<(Crossed, Crossed)>, TestFailure> {
    let own = found.own.clone().ok_or_else(|| {
        TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
    })?;
    let Some((destination, edge, distance)) = nearest_border(found, home, &own.position) else {
        return Ok(None);
    };
    tracing::info!(
        edge = edge.label(),
        distance,
        x = own.position.x,
        y = own.position.y,
        "the nearest border a neighbour shares"
    );
    let home_circuit = session.circuit_id();
    let home_sim = found_root_sim(session);
    let fly = flies(grid, distance);
    let out = walk(session, agent, Some(&own), edge, fly, SETTLE).await?;
    let out = checked(session, home_circuit, destination, out)?;

    // The way back is a second crossing, into the region just left — which is
    // now a neighbour of the one the avatar stands in, on the circuit that was
    // its root.
    let away_circuit = session.circuit_id();
    let back = walk(session, agent, None, edge.opposite(), fly, SETTLE).await?;
    let home_info = NeighborInfo {
        region_handle: home,
        sim: home_sim.unwrap_or(out.destination.sim),
        grid_coordinates: sl_client_tokio::GridCoordinates::new(
            home.grid_coordinates().0,
            home.grid_coordinates().1,
        ),
    };
    let back = match home_sim {
        Some(_sim) => checked(session, away_circuit, home_info, back)?,
        None => {
            return Err(TestFailure::Assertion(
                "the session reported no simulator for the region the avatar logged in to"
                    .to_owned(),
            ));
        }
    };
    Ok(Some((out, back)))
}

/// The simulator of the region the session is rooted in, from the login.
fn found_root_sim(session: &Session) -> Option<std::net::SocketAddr> {
    session
        .login_success()
        .map(|login| std::net::SocketAddr::new(login.sim_ip.into(), login.sim_port))
}

/// One walk at `edge`.
async fn walk(
    session: &mut Session,
    agent: AgentKey,
    own: Option<&OwnAvatar>,
    edge: Edge,
    fly: bool,
    settle: Duration,
) -> Result<Trace, TestFailure> {
    walk_over_border(
        session,
        agent,
        own,
        Walk {
            steer: Some(Steer { edge, fly }),
            budget: WALK_BUDGET,
            carry_on: CARRY_ON,
            settle,
        },
    )
    .await
}

/// The fake grid's crossing: the grid is asked for it, because the grid is
/// where a crossing is decided and this one simulates no movement.
async fn cross_scripted(
    session: &mut Session,
    control: &FakeControl,
    agent: AgentKey,
    found: &Neighbourhood,
    home: RegionHandle,
) -> Result<Crossed, TestFailure> {
    let east = control
        .grid()
        .region_handle(crate::fake::EAST_REGION)
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "the grid serves no region called {:?}",
                crate::fake::EAST_REGION
            ))
        })?;
    let destination = found
        .neighbours
        .iter()
        .find(|neighbour| neighbour.info.region_handle == east)
        .map(|neighbour| neighbour.info.clone())
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "the region to the east was never announced as a neighbour of {home:?}"
            ))
        })?;
    let from = session.circuit_id();
    // Placed rather than walked: the fake grid runs no physics, so there is no
    // momentum to carry over the border.
    let crossing = control.grid().cross_agent(
        control.agent(),
        crate::fake::EAST_REGION,
        RegionCoordinates::new(FAKE_ARRIVAL.0, FAKE_ARRIVAL.1, FAKE_ARRIVAL.2),
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    );
    // The client's `CompleteAgentMovement` is what completes the crossing, and
    // it only sends one while its run loop is draining — so the grid's future
    // and the session's watch have to run together rather than one after the
    // other.
    let (crossed, trace) = tokio::join!(
        crossing,
        walk_over_border(
            session,
            agent,
            found.own.as_ref(),
            Walk {
                steer: None,
                budget: WALK_BUDGET,
                carry_on: Duration::ZERO,
                settle: FAKE_SETTLE,
            },
        )
    );
    let _destination = crossed.map_err(|error| {
        TestFailure::Assertion(format!("the grid could not hand the agent over: {error}"))
    })?;
    checked(session, from, destination, trace?)
}
