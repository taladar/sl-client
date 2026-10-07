//! Drive a cross-region teleport — to a region *different* from the agent's
//! current one — and assert the circuit handover completes.
//!
//! Unlike an intra-region teleport (which the simulator answers with
//! `TeleportLocal`, keeping the one circuit), a teleport whose destination is a
//! *different* region tears down and re-establishes the root circuit: the source
//! region answers with a `TeleportFinish` (delivered over the CAPS event queue on
//! OpenSim's V2 transfer protocol) carrying the destination simulator's address
//! and seed capability, the client hands the root circuit over to that simulator
//! (`UseCircuitCode` + `CompleteAgentMovement`), and the destination's handshake
//! completes as an [`sl_client_tokio::Event::RegionChanged`]. That handover — a `TeleportFinished`
//! for a *different* region handle followed by a `RegionChanged` to it — is the
//! observable difference between a cross-region and an intra-region teleport.
//!
//! The case:
//!
//! 1. Discovers a neighbouring region via the world map
//!    ([`sl_client_tokio::Command::RequestMapBlocks`] over a one-cell margin around the agent's own
//!    region) and picks the first block whose grid coordinates differ from the
//!    current region — a genuinely *different* destination region.
//! 2. Teleports to the centre of that region ([`sl_client_tokio::Command::Teleport`] with the
//!    destination's region handle).
//! 3. Collects the teleport phases until arrival, asserting the sequence opens
//!    with *Starting* ([`sl_client_tokio::Event::TeleportStarted`]), carries a
//!    [`sl_client_tokio::Event::TeleportFinished`] for the destination handle, and ends at a
//!    [`sl_client_tokio::Event::RegionChanged`] to that same handle — never the intra-region
//!    [`sl_client_tokio::Event::TeleportLocal`], which would mean the teleport did not cross a
//!    region boundary.
//! 4. Confirms the session's current region handle is now the destination.
//!
//! Records the origin and destination grid coordinates, the destination region
//! name and simulator address, the whole trace — order, progress lines, every
//! flags word, whether the client kept the world it left — and the
//! request-to-arrival latency, and holds the grid to the shape it was measured
//! sending: Second Life narrates the teleport with `resolving` and then the
//! sentence `Sending to destination.`; OpenSim sends no progress line at all.
//!
//! `1av`. **OpenSim** hosts a 2×2 block of regions (Default / East / North /
//! Northeast at grid `(1000,1000)`–`(1001,1001)`, loopback ports 9000–9003), so a
//! neighbour is always one map query away and the teleport crosses to a distinct
//! simulator. No new client code — the CAPS `TeleportFinish` handover, the
//! `Command::Teleport` / `Event::TeleportFinished` / `Event::RegionChanged`
//! surface, and the map-block discovery path all existed from earlier teleport and
//! survey work. On **aditi** the case logs in at a mainland region with
//! neighbours (`ADITI_START`): the sandbox the test avatar otherwise stands in
//! has none. The **fake** grid serves two regions and answers the same map
//! query, so this runs offline on every commit; note that its second region is
//! the *neighbour*, so the handover is to a region the client already holds a
//! child circuit for — [`super::region_crossing`] is the case that pins what
//! that distinction means.

use std::time::Instant;

use sl_client_tokio::{GridCoordinates, RegionHandle, TeleportFlags};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, count_metric, secs_metric};
use crate::teleport_trace::{neighbouring_region, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-cross-region, 2026-10-07)";

/// The region-local destination of the teleport: the centre of the destination
/// region at a modest height. The simulator clamps `Z` to ground level, so the
/// exact height only needs to be non-negative; the centre `(128, 128)` is always
/// inside the 256 m destination region.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// Where the case logs in on aditi: a mainland region with neighbours on three
/// sides. The sandbox the test avatars otherwise stand in borders nothing, so a
/// map query around it finds no other region to go to.
pub(crate) const ADITI_START: &str = "uri:Ahern&128&128&40";

/// The progress lines between the `TeleportStart` and the `TeleportFinish` of
/// a teleport to a location: a key and then a sentence on Second Life, nothing
/// on OpenSim.
pub(crate) const PROGRESS_LINES: Measured<&[&str]> = Measured {
    second_life: &["resolving", "Sending to destination."],
    opensim: &[],
    source: SOURCE,
};

/// The order of the messages of an inter-region teleport.
const SEQUENCE: Measured<&str> = Measured {
    second_life: "started,progress,progress,finished,region-changed",
    opensim: "started,finished,region-changed",
    source: SOURCE,
};

/// The flags every message of a teleport to a location carries, on both
/// grids: the start, each progress line and the finish.
const FLAGS: Measured<u32> = Measured {
    second_life: TeleportFlags::VIA_LOCATION,
    opensim: TeleportFlags::VIA_LOCATION,
    source: SOURCE,
};

/// Drives a teleport to a different region and asserts the cross-region circuit
/// handover completes.
#[derive(Debug)]
pub struct TeleportCrossRegion;

impl GridTest for TeleportCrossRegion {
    fn name(&self) -> &'static str {
        "teleport-cross-region"
    }

    fn description(&self) -> &'static str {
        "Teleport to a different region and hold the handover's order, lines and flags to the grid"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => ADITI_START,
            Grid::Opensim | Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let origin_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;
            let origin = GridCoordinates::from(origin_handle);

            // Discover a neighbouring region via the world map — a genuinely
            // different destination that forces the cross-region path.
            let target = neighbouring_region(session, origin).await?;
            let target_grid = target.grid_coordinates;
            let target_handle = RegionHandle::from(target_grid);
            let target_name = target
                .name
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();

            // A different region must map to a different handle; otherwise the
            // teleport would be intra-region and could not exercise the handover.
            check(
                target_handle != origin_handle,
                "the chosen destination region resolved to the agent's own region handle",
            )?;

            let started_at = Instant::now();
            request_teleport(session, target_handle, DESTINATION, (1.0, 0.0, 0.0)).await?;
            let trace = watch_teleport(session, REGION_TIMEOUT).await?;
            let elapsed = started_at.elapsed();
            let current = session.region_handle();

            let metrics = ctx.metrics();
            trace.record("", metrics);
            metrics.set("phase_sequence", trace.sequence());
            metrics.set(
                &count_metric("progress_updates"),
                i64::try_from(trace.progress.len()).unwrap_or(-1),
            );
            metrics.set("origin_grid_x", i64::from(origin.x()));
            metrics.set("origin_grid_y", i64::from(origin.y()));
            metrics.set("destination_grid_x", i64::from(target_grid.x()));
            metrics.set("destination_grid_y", i64::from(target_grid.y()));
            metrics.set("destination_region", target_name);
            metrics.set_timing(&secs_metric("teleport"), elapsed.as_secs_f64());

            if let Some(failure) = &trace.failure {
                return Err(TestFailure::Assertion(format!(
                    "cross-region teleport failed: {}",
                    failure.reason
                )));
            }
            // A TeleportLocal here would mean the teleport did not cross a
            // region boundary, which contradicts a distinct destination region.
            let arrival = trace.arrival.ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "expected a cross-region handover but the teleport ended as [{}]",
                    trace.sequence()
                ))
            })?;
            ctx.metrics()
                .set("destination_sim", arrival.sim.to_string());

            // A TeleportFinished must have carried the destination handle: the
            // protocol-level completion that names the region we are handing over
            // to, distinguishing a real cross-region teleport from a local one.
            let finish = trace.finish.ok_or_else(|| {
                TestFailure::Assertion(
                    "the cross-region teleport never surfaced a TeleportFinished with the \
                     destination handle"
                        .to_owned(),
                )
            })?;
            check_eq(
                "teleport_finished_handle",
                &finish.region_handle,
                &target_handle,
            )?;

            // ... and the handover (the RegionChanged terminal phase) must have
            // completed at the destination region, which the session now names
            // as its own.
            check_eq(
                "region_changed_handle",
                &arrival.region_handle,
                &target_handle,
            )?;
            let current = current.ok_or_else(|| {
                TestFailure::Assertion(
                    "no region handle after the cross-region handover".to_owned(),
                )
            })?;
            check_eq("current_region_handle", &current, &target_handle)?;

            // The shape around the handover, which is what the grids disagree
            // on.
            SEQUENCE.check(
                "the order of an inter-region teleport's messages",
                grid,
                &trace.sequence().as_str(),
            )?;
            PROGRESS_LINES.check(
                "the progress lines of a teleport to a location",
                grid,
                &trace.lines().as_slice(),
            )?;
            for flags in &trace.starts {
                FLAGS.check("the TeleportStart flags", grid, flags)?;
            }
            for (_line, flags) in &trace.progress {
                FLAGS.check("a TeleportProgress line's flags", grid, flags)?;
            }
            FLAGS.check("the TeleportFinish flags", grid, &finish.flags)?;
            Ok(())
        })
    }
}
