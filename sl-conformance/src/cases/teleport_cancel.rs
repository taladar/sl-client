//! Cancel a teleport on the heels of asking for it and hold the grid to what
//! it was measured doing about it.

use sl_client_tokio::{Command, GridCoordinates, RegionHandle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};
use crate::teleport_trace::{neighbouring_region, request_teleport, watch_teleport};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-cancel, 2026-10-07)";

/// Where the agent asks to land in the neighbouring region.
const DESTINATION: (f32, f32, f32) = (128.0, 128.0, 30.0);

/// How a teleport cancelled straight after its request ends: Second Life
/// abandons it and reports it failed; OpenSim moves the agent regardless.
const OUTCOME: Measured<&str> = Measured {
    second_life: "failed",
    opensim: "moved",
    source: SOURCE,
};

/// The reason and alert key of the failure a cancelled teleport is reported
/// with, on the grid that reports one.
const CANCELLED: (&str, &str) = ("Teleport cancelled.", "TPCancelled");

/// Sends a `TeleportCancel` immediately behind a `TeleportLocationRequest` to
/// a neighbouring region and watches how the teleport ends.
///
/// A viewer's cancel button sends `TeleportCancel` and drops the teleport on
/// its own side; what the grid then does is the grid's. **Second Life** honours
/// the cancel: the `TeleportStart` and the first progress line still arrive —
/// they were on the wire already — and then a `TeleportFailed` over the event
/// queue, reason `Teleport cancelled.`, alert `TPCancelled`. The agent stays
/// where it was. **OpenSim** honours a cancel only in the few tens of
/// milliseconds between creating the agent at the destination and sending the
/// `TeleportFinish`, and then silently; a cancel sent with the request arrives
/// before that window opens and is lost, so the teleport completes and the
/// client that cancelled finds itself in the destination all the same.
///
/// Either way the client has to follow the grid: a session that treated its own
/// cancel as the end of the matter would ignore OpenSim's `TeleportFinish` and
/// be stranded in a region it had left.
#[derive(Debug)]
pub struct TeleportCancel;

impl GridTest for TeleportCancel {
    fn name(&self) -> &'static str {
        "teleport-cancel"
    }

    fn description(&self) -> &'static str {
        "Cancel a teleport straight after asking for it and hold the outcome to the grid's own"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
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
            let target = neighbouring_region(session, GridCoordinates::from(origin_handle)).await?;
            let target_handle = RegionHandle::from(target.grid_coordinates);

            // The two go out back to back: the cancel is on the wire before the
            // grid has answered the request at all.
            request_teleport(session, target_handle, DESTINATION, (1.0, 0.0, 0.0)).await?;
            session.send(Command::CancelTeleport).await?;
            let trace = watch_teleport(session, REGION_TIMEOUT).await?;
            let now_in = session.region_handle();

            let metrics = ctx.metrics();
            trace.record("", metrics);

            OUTCOME.check(
                "how a teleport cancelled straight after its request ends",
                grid,
                &trace.outcome(),
            )?;
            match (&trace.failure, &trace.arrival) {
                (Some(failure), _) => {
                    let (reason, alert) = CANCELLED;
                    check_eq("cancel_reason", &failure.reason.as_str(), &reason)?;
                    check_eq(
                        "cancel_alert",
                        &failure.alert.as_ref().map(|alert| alert.message.as_str()),
                        &Some(alert),
                    )?;
                    check(
                        now_in == Some(origin_handle),
                        "a cancelled teleport left the session naming another region",
                    )
                }
                (None, Some(arrival)) => {
                    check_eq("arrived_in", &arrival.region_handle, &target_handle)?;
                    check(
                        now_in == Some(target_handle),
                        "the session did not follow a teleport the grid completed after a cancel",
                    )
                }
                (None, None) => Err(TestFailure::Assertion(format!(
                    "the cancelled teleport ended as [{}]",
                    trace.sequence()
                ))),
            }
        })
    }
}
