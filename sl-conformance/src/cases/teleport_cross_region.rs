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
//! completes as an [`Event::RegionChanged`]. That handover — a `TeleportFinished`
//! for a *different* region handle followed by a `RegionChanged` to it — is the
//! observable difference between a cross-region and an intra-region teleport.
//!
//! The case:
//!
//! 1. Discovers a neighbouring region via the world map
//!    ([`Command::RequestMapBlocks`] over a one-cell margin around the agent's own
//!    region) and picks the first block whose grid coordinates differ from the
//!    current region — a genuinely *different* destination region.
//! 2. Teleports to the centre of that region ([`Command::Teleport`] with the
//!    destination's region handle).
//! 3. Collects the teleport phases until arrival, asserting the sequence opens
//!    with *Starting* ([`Event::TeleportStarted`]), carries a
//!    [`Event::TeleportFinished`] for the destination handle, and ends at a
//!    [`Event::RegionChanged`] to that same handle — never the intra-region
//!    [`Event::TeleportLocal`], which would mean the teleport did not cross a
//!    region boundary.
//! 4. Confirms the session's current region handle is now the destination.
//!
//! Records the origin and destination grid coordinates, the destination region
//! name and simulator address, the observed phase sequence and progress-update
//! count, and the request-to-arrival latency.
//!
//! `1av`. **OpenSim** hosts a 2×2 block of regions (Default / East / North /
//! Northeast at grid `(1000,1000)`–`(1001,1001)`, loopback ports 9000–9003), so a
//! neighbour is always one map query away and the teleport crosses to a distinct
//! simulator. No new client code — the CAPS `TeleportFinish` handover, the
//! `Command::Teleport` / `Event::TeleportFinished` / `Event::RegionChanged`
//! surface, and the map-block discovery path all existed from earlier teleport and
//! survey work. The aditi run is deferred with the batch (SL's grid also answers a
//! cross-region teleport with a `TeleportFinish` handover to a different
//! simulator).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, GridCoordinates, MapRegionInfo, RegionCoordinates, RegionHandle, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, secs_metric};

/// The region-local destination of the teleport: the centre of the destination
/// region at a modest height. The simulator clamps `Z` to ground level, so the
/// exact height only needs to be non-negative; the centre `(128, 128)` is always
/// inside the 256 m destination region.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// How many grid cells to pad the map-block request rectangle by on each side of
/// the agent's own region. A margin of one covers every immediate neighbour —
/// enough to reach any region in OpenSim's 2×2 test block regardless of which one
/// the avatar logged in to.
const BLOCK_MARGIN: u32 = 1;

/// The quiet gap (no further `MapBlockReply`) that marks the block reply fully
/// drained. OpenSim's world-map worker batches regions with a ~50 ms sleep between
/// batches, so this stays comfortably above that cadence.
const BLOCK_DRAIN_QUIET: Duration = Duration::from_secs(2);

/// Drives a teleport to a different region and asserts the cross-region circuit
/// handover completes.
#[derive(Debug)]
pub struct TeleportCrossRegion;

impl GridTest for TeleportCrossRegion {
    fn name(&self) -> &'static str {
        "teleport-cross-region"
    }

    fn description(&self) -> &'static str {
        "Teleport to a different region and assert the TeleportFinished -> RegionChanged handover"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let origin_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;
            let origin = GridCoordinates::from(origin_handle);

            // Discover a neighbouring region via the world map and pick the first
            // one whose grid coordinates differ from the current region — a
            // genuinely different destination that forces the cross-region path.
            session
                .send(Command::RequestMapBlocks {
                    min_x: origin.x().saturating_sub(BLOCK_MARGIN),
                    max_x: origin.x().saturating_add(BLOCK_MARGIN),
                    min_y: origin.y().saturating_sub(BLOCK_MARGIN),
                    max_y: origin.y().saturating_add(BLOCK_MARGIN),
                })
                .await?;
            let blocks = drain_map_blocks(session, BLOCK_DRAIN_QUIET).await?;
            let target = blocks
                .into_iter()
                .find(|block| block.grid_coordinates != origin)
                .ok_or_else(|| {
                    TestFailure::Assertion(
                        "the world map reported no region other than the agent's own, so there is \
                         no different region to teleport to"
                            .to_owned(),
                    )
                })?;
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

            let (x, y, z) = DESTINATION;
            let started_at = Instant::now();
            session
                .send(Command::Teleport {
                    region_handle: target_handle,
                    position: RegionCoordinates::new(x, y, z),
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                })
                .await?;

            // Collect the teleport phases until the destination handshake arrives
            // (a RegionChanged, the cross-region terminal) or the teleport fails.
            // A TeleportLocal here would mean the teleport did not cross a region
            // boundary, which contradicts a distinct destination region.
            let mut phases: Vec<&'static str> = Vec::new();
            let mut progress_updates: usize = 0;
            let mut finished: Option<(RegionHandle, SocketAddr)> = None;
            let (changed_handle, changed_sim) = loop {
                let step = session
                    .wait_for(REGION_TIMEOUT, |event| match event {
                        Event::TeleportStarted => Some(Step::Started),
                        Event::TeleportProgress { .. } => Some(Step::Progress),
                        Event::TeleportFinished {
                            region_handle, sim, ..
                        } => Some(Step::Finished(*region_handle, *sim)),
                        Event::RegionChanged {
                            region_handle, sim, ..
                        } => Some(Step::Changed(*region_handle, *sim)),
                        Event::TeleportLocal => Some(Step::Local),
                        Event::TeleportFailed { reason, .. } => Some(Step::Failed(reason.clone())),
                        _ => None,
                    })
                    .await?;
                match step {
                    Step::Started => phases.push("started"),
                    Step::Progress => {
                        progress_updates = progress_updates.saturating_add(1);
                        phases.push("progress");
                    }
                    Step::Finished(handle, sim) => {
                        finished = Some((handle, sim));
                        phases.push("finished");
                    }
                    Step::Changed(handle, sim) => {
                        phases.push("region-changed");
                        break (handle, sim);
                    }
                    Step::Local => {
                        return Err(TestFailure::Assertion(
                            "expected a cross-region handover but the teleport completed locally \
                             (TeleportLocal), so it did not cross a region boundary"
                                .to_owned(),
                        ));
                    }
                    Step::Failed(reason) => {
                        return Err(TestFailure::Assertion(format!(
                            "cross-region teleport failed: {reason}"
                        )));
                    }
                }
            };
            let elapsed = started_at.elapsed();

            // The sequence must open with the Starting phase.
            check(
                phases.first() == Some(&"started"),
                "expected the teleport to begin with a Starting (TeleportStart) phase",
            )?;

            // A TeleportFinished must have carried the destination handle: the
            // protocol-level completion that names the region we are handing over
            // to, distinguishing a real cross-region teleport from a local one.
            let (finished_handle, _finished_sim) = finished.ok_or_else(|| {
                TestFailure::Assertion(
                    "the cross-region teleport never surfaced a TeleportFinished with the \
                     destination handle"
                        .to_owned(),
                )
            })?;
            check_eq("teleport_finished_handle", &finished_handle, &target_handle)?;

            // ... and the handover (the RegionChanged terminal phase) must have
            // completed at the destination region.
            check_eq("region_changed_handle", &changed_handle, &target_handle)?;

            // The session's current region handle must now be the destination.
            let current = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion(
                    "no region handle after the cross-region handover".to_owned(),
                )
            })?;
            check_eq("current_region_handle", &current, &target_handle)?;

            let sequence = phases.join(",");
            let metrics = ctx.metrics();
            metrics.set("phase_sequence", sequence);
            metrics.set(
                &count_metric("progress_updates"),
                i64::try_from(progress_updates).unwrap_or(-1),
            );
            metrics.set("origin_grid_x", i64::from(origin.x()));
            metrics.set("origin_grid_y", i64::from(origin.y()));
            metrics.set("destination_grid_x", i64::from(target_grid.x()));
            metrics.set("destination_grid_y", i64::from(target_grid.y()));
            metrics.set("destination_region", target_name);
            metrics.set("destination_sim", changed_sim.to_string());
            metrics.set_timing(&secs_metric("teleport"), elapsed.as_secs_f64());
            Ok(())
        })
    }
}

/// One step observed on the circuit between the teleport request and arrival.
///
/// Carries the destination handle and simulator address for the two handover
/// frames ([`Event::TeleportFinished`] and [`Event::RegionChanged`]) so the case
/// can assert both name the region it teleported to.
enum Step {
    /// The simulator acknowledged the request and began the teleport
    /// (`TeleportStart`).
    Started,
    /// A progress update arrived mid-teleport (`TeleportProgress`).
    Progress,
    /// The protocol-level completion arrived (`TeleportFinish`), naming the
    /// destination region handle and simulator address.
    Finished(RegionHandle, SocketAddr),
    /// The destination region's handshake completed (`RegionChanged`) — the
    /// cross-region terminal.
    Changed(RegionHandle, SocketAddr),
    /// The teleport completed intra-region (`TeleportLocal`) — unexpected for a
    /// distinct destination region.
    Local,
    /// The teleport failed (`TeleportFailed` or a timeout), carrying its reason.
    Failed(String),
}

/// Drains the [`Event::MapBlock`] entries a `MapBlockReply` yields until none
/// arrives for `quiet`, returning every region reported.
///
/// The first block is awaited with the full [`REPLY_TIMEOUT`] (OpenSim queues the
/// request onto a worker thread); once one is in hand, further ones follow closely
/// or not at all.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect, or times out if not even the
/// first block arrives.
async fn drain_map_blocks(
    session: &mut Session,
    quiet: Duration,
) -> Result<Vec<MapRegionInfo>, TestFailure> {
    let mut blocks = Vec::new();
    loop {
        let timeout = if blocks.is_empty() {
            REPLY_TIMEOUT
        } else {
            quiet
        };
        match session
            .wait_for(timeout, |event| match event {
                Event::MapBlock(region) => Some((**region).clone()),
                _ => None,
            })
            .await
        {
            Ok(region) => blocks.push(region),
            Err(TestFailure::Timeout(_)) => return Ok(blocks),
            Err(other) => return Err(other),
        }
    }
}
