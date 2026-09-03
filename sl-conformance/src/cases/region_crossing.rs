//! Walking over a border is a promotion, not a reconnection.
//!
//! When an avatar walks off the edge of a region the simulator hands it to the
//! neighbour: `CrossedRegion` names the destination's circuit and seed, the
//! client sends its `CompleteAgentMovement` there, and the **child** circuit it
//! has held since arrival becomes its root. Nothing is torn down — no teleport
//! screen, no cleared scene, no fresh circuit — which is the whole difference
//! between a crossing and a teleport to the same place.
//!
//! What this asserts, in one sequence:
//!
//! - the crossing raises an [`Event::RegionChanged`] naming the destination, on
//!   the simulator the neighbour was announced at,
//! - with `world_reset: false` — the client kept and re-based its scene rather
//!   than clearing it,
//! - on a different circuit than the source region's,
//! - and it raised **no** teleport event on the way.
//!
//! The crossing itself is the grid's decision to make, so this case makes it:
//! the fake grid simulates no movement, and
//! [`FakeGrid::cross_agent`](sl_fake_grid::FakeGrid::cross_agent) is the
//! scripted stand-in for walking. That is why this is a fake-grid case and not a
//! live one — plus the plainer reason that neither live grid reliably offers two
//! adjacent regions this workspace's avatar may walk between.

use std::time::Duration;

use sl_client_tokio::{Event, RegionCoordinates, Vector};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, secs_metric};

/// How long to wait for the neighbour's child circuit to be announced before
/// crossing into it. A crossing into a region the client holds no child circuit
/// for is a different path, and would make this case assert the wrong thing.
const NEIGHBOUR_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for the hand-over to complete once the grid has started it.
const CROSSING_TIMEOUT: Duration = Duration::from_secs(60);

/// Where in the destination region the agent lands: just over the shared edge,
/// halfway along it, standing on the stock ground.
const ARRIVAL: (f32, f32, f32) = (8.0, 128.0, 26.0);

/// Walks the agent over the eastern border and checks the client promoted its
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
        &[Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Taken by value up front: everything below borrows the context
            // mutably to drive the session, and the grid-side handle is a
            // cheap clone of two `Arc`s.
            let control = ctx.fake().cloned().ok_or_else(|| {
                TestFailure::Assertion("this case runs on the fake grid only".to_owned())
            })?;
            let east = control
                .grid()
                .region_handle(crate::fake::EAST_REGION)
                .ok_or_else(|| {
                    TestFailure::Assertion(format!(
                        "the grid serves no region called {:?}",
                        crate::fake::EAST_REGION
                    ))
                })?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let from_circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;

            // The child circuit has to exist before the crossing, or the
            // hand-over is a different path entirely (a fresh circuit to an
            // unconnected region, which *does* reset the world).
            let child_sim = session
                .wait_for(NEIGHBOUR_TIMEOUT, |event| match event {
                    Event::NeighborDiscovered(info) if info.region_handle == east => Some(info.sim),
                    _other => None,
                })
                .await?;

            let started = std::time::Instant::now();
            // The grid's decision, because the grid is where a crossing is
            // decided. Placed rather than walked: the fake grid runs no
            // physics, so there is no momentum to carry over the border.
            let crossing = control.grid().cross_agent(
                control.agent(),
                crate::fake::EAST_REGION,
                RegionCoordinates::new(ARRIVAL.0, ARRIVAL.1, ARRIVAL.2),
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            );
            // A teleport event seen *while* the hand-over runs, not after it:
            // the wait consumes every event it passes over, so the only place
            // to notice one is inside the predicate.
            let mut teleported: Option<String> = None;
            // The client's `CompleteAgentMovement` is what completes the
            // crossing, and it only sends one while its run loop is draining —
            // so the grid's future and the session's wait have to run together
            // rather than one after the other.
            let (crossed, changed) = tokio::join!(
                crossing,
                session.wait_for(CROSSING_TIMEOUT, |event| {
                    match event {
                        Event::TeleportStarted
                        | Event::TeleportFinished { .. }
                        | Event::TeleportFailed { .. } => {
                            let _first = teleported.get_or_insert_with(|| format!("{event:?}"));
                            None
                        }
                        Event::RegionChanged {
                            region_handle,
                            sim,
                            circuit,
                            world_reset,
                        } if *region_handle == east => Some((*sim, *circuit, *world_reset)),
                        _other => None,
                    }
                })
            );
            let _destination = crossed.map_err(|error| {
                TestFailure::Assertion(format!("the grid could not hand the agent over: {error}"))
            })?;
            let (sim, circuit, world_reset) = changed?;
            let elapsed = started.elapsed().as_secs_f64();

            check_eq("the destination simulator", &sim, &child_sim)?;
            check(
                !world_reset,
                "the crossing reset the client's world — it rebuilt the scene instead of \
                 re-basing the one it already held",
            )?;
            check(
                circuit != from_circuit,
                "the crossing kept the source region's circuit as the root",
            )?;
            check_eq(
                "the region the session is in after the crossing",
                &session.region_handle(),
                &Some(east),
            )?;
            check(
                teleported.is_none(),
                &format!(
                    "a border crossing raised {}, so the client did not treat it as a hand-over",
                    teleported.as_deref().unwrap_or("a teleport event")
                ),
            )?;

            ctx.metrics().set_timing(&secs_metric("crossing"), elapsed);
            Ok(())
        })
    }
}
