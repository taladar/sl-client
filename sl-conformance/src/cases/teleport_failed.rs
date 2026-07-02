//! Provoke a failed teleport and assert the session surfaces
//! [`Event::TeleportFailed`] rather than an arrival.

use std::time::Instant;

use sl_client_tokio::{Command, Event, RegionCoordinates, RegionHandle, Vector};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, secs_metric};

/// Grid coordinates (region indices) of the non-existent destination region.
///
/// The local OpenSim standalone hosts a 2×2 block of regions at grid
/// `(1000,1000)`–`(1001,1001)`; `(2000, 2000)` is far outside that block (and
/// well away from any region a real grid would host near the test avatar), so
/// no region occupies the handle and the simulator answers the teleport with
/// `TeleportFailed` ("The region you tried to teleport to was not found")
/// instead of a `TeleportStart` → arrival sequence. Choosing coordinates in the
/// void — rather than reusing the current region's handle with an illegal
/// position — is what forces the *different-region* path in OpenSim's
/// `EntityTransferModule`, where the grid-service lookup returns no region.
const VOID_REGION_GRID: (u32, u32) = (2000, 2000);

/// The region-local landing position of the (doomed) teleport request. The
/// centre of the region at a modest height; the exact value is irrelevant since
/// the destination region does not exist, but a plausible in-region position
/// keeps the request well-formed.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// Drives a teleport to a region that does not exist and asserts the session
/// reports the failure.
///
/// A viewer teleport walks a small state machine announced by the simulator; a
/// teleport whose destination region cannot be located never reaches an arrival
/// phase — instead the simulator answers with `TeleportFailed` carrying a
/// human-readable reason (and, on some grids, a structured `AlertInfo`). The
/// session leaves the teleporting state and stays connected to the current
/// region.
///
/// The case teleports to a region handle in the empty part of the grid, waits
/// for the first terminal teleport event, and asserts it is
/// [`Event::TeleportFailed`] (not an arrival). It records the failure reason,
/// whether a structured alert accompanied it, and the request-to-failure time.
#[derive(Debug)]
pub struct TeleportFailed;

impl GridTest for TeleportFailed {
    fn name(&self) -> &'static str {
        "teleport-failed"
    }

    fn description(&self) -> &'static str {
        "Provoke a teleport to a non-existent region and assert Event::TeleportFailed"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // A region handle in the void: no region occupies these coordinates,
            // so the destination lookup fails and the teleport is refused.
            let (grid_x, grid_y) = VOID_REGION_GRID;
            let region_handle = RegionHandle::from_grid(grid_x, grid_y);
            let (x, y, z) = DESTINATION;

            let started_at = Instant::now();
            session
                .send(Command::Teleport {
                    region_handle,
                    position: RegionCoordinates::new(x, y, z),
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                })
                .await?;

            // Wait for the first *terminal* teleport event. The failure path may
            // (or may not) be preceded by a `TeleportStart`, so ignore
            // `TeleportStarted` / `TeleportProgress` and resolve on the terminal
            // outcome: a `TeleportFailed` (expected), or any arrival event
            // (`TeleportLocal` / `TeleportFinished` / `RegionChanged`) that would
            // mean the teleport unexpectedly succeeded.
            let outcome = session
                .wait_for(REGION_TIMEOUT, |event| match event {
                    Event::TeleportFailed { reason, alert_info } => {
                        Some(Ok((reason.clone(), alert_info.is_some())))
                    }
                    Event::TeleportLocal => Some(Err("TeleportLocal")),
                    Event::TeleportFinished { .. } => Some(Err("TeleportFinished")),
                    Event::RegionChanged { .. } => Some(Err("RegionChanged")),
                    _ => None,
                })
                .await?;
            let elapsed = started_at.elapsed();

            let (reason, has_alert_info) = match outcome {
                Ok(outcome) => outcome,
                Err(arrival) => {
                    return Err(TestFailure::Assertion(format!(
                        "expected the teleport to a non-existent region to fail, but it arrived \
                         ({arrival})"
                    )));
                }
            };

            // A failure reason must accompany the event — an empty string would
            // mean the simulator refused the teleport without telling the viewer
            // why, which no client could surface.
            check(
                !reason.trim().is_empty(),
                "expected TeleportFailed to carry a non-empty failure reason",
            )?;

            let metrics = ctx.metrics();
            metrics.set("failure_reason", reason);
            metrics.set("has_alert_info", has_alert_info);
            metrics.set_timing(&secs_metric("teleport_failure"), elapsed.as_secs_f64());
            Ok(())
        })
    }
}
