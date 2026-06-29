//! Set the draw distance and confirm the simulator enables neighbouring regions.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Distance, Event, NeighborInfo};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// The draw distance applied by the case, in metres.
///
/// Deliberately larger than the client default (256 m) so the simulator's agent
/// interest list reaches a full region width past the current region in every
/// direction. At 512 m it enables the adjacent neighbours regardless of where in
/// the 256 m region the avatar happens to stand.
const DRAW_DISTANCE_M: f64 = 512.0;

/// How long to observe the circuit after applying the draw distance.
///
/// `AgentUpdate` (which carries the `Far` draw-distance field) is unreliable and
/// fire-and-forget, so there is no reply to await. Instead the case observes for
/// this window, long enough to (a) catch the `EnableSimulator` announcements the
/// enlarged interest list provokes and (b) span at least one keep-alive ping
/// (≈ 5 s interval) confirming the circuit is still healthy. Kept generous for
/// Aditi network jitter and load.
const OBSERVE_WINDOW: Duration = Duration::from_secs(15);

/// One observation the case collects within its window.
enum Observed {
    /// A neighbouring region was enabled (`EnableSimulator`).
    Neighbor(NeighborInfo),
    /// A keep-alive ping to the root simulator completed; the circuit is live.
    Ping(Duration),
}

/// Sets the draw distance and confirms the simulator enables neighbouring
/// regions in response.
///
/// A viewer advertises its draw distance in the `Far` field of the keep-alive
/// `AgentUpdate`. The simulator folds that, together with the camera, into the
/// agent's interest list and enables the neighbouring regions it reaches —
/// announcing each with an `EnableSimulator`, surfaced here as
/// [`Event::NeighborDiscovered`]. `AgentUpdate` is unreliable and has no reply,
/// so "no error" cannot be read from a reply; it is instead the circuit staying
/// healthy (a keep-alive ping still round-trips) after the change.
///
/// The case applies a 512 m draw distance — double the client default — then
/// observes the circuit for a window, collecting neighbour discoveries (the
/// echoed effect of the enlarged interest list) and the latest ping RTT. On the
/// local OpenSim grid, a 2×2 block of adjacent regions, 512 m always reaches a
/// neighbour, so the case asserts at least one is enabled. On Aditi the landing
/// region may have none within reach, which is recorded `partial` rather than
/// failed. The requested draw distance, neighbour count, and post-change RTT are
/// recorded.
#[derive(Debug)]
pub struct DrawDistance;

impl GridTest for DrawDistance {
    fn name(&self) -> &'static str {
        "draw-distance"
    }

    fn description(&self) -> &'static str {
        "Set the draw distance and confirm the simulator enables neighbouring regions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Apply a draw distance larger than the default so the simulator's
            // interest list reaches into the neighbouring regions. It takes
            // effect on the next keep-alive `AgentUpdate`.
            session
                .send(Command::SetDrawDistance(Distance::new(DRAW_DISTANCE_M)))
                .await?;

            // Observe the circuit for the window, collecting neighbour
            // discoveries (the echoed effect of the enlarged interest list) and
            // the latest keep-alive ping RTT (proof the circuit stayed healthy —
            // the "no error" signal). A `Disconnected` mid-window propagates and
            // fails the case; a per-iteration timeout that consumes the
            // remaining window simply ends the observation.
            let mut neighbors: Vec<NeighborInfo> = Vec::new();
            let mut last_rtt: Option<Duration> = None;
            let start = Instant::now();
            while let Some(remaining) = OBSERVE_WINDOW.checked_sub(start.elapsed()) {
                if remaining.is_zero() {
                    break;
                }
                match session
                    .wait_for(remaining, |event| match event {
                        Event::NeighborDiscovered(info) => Some(Observed::Neighbor(info.clone())),
                        Event::Ping {
                            child: false, rtt, ..
                        } => Some(Observed::Ping(*rtt)),
                        _ => None,
                    })
                    .await
                {
                    Ok(Observed::Neighbor(info)) => neighbors.push(info),
                    Ok(Observed::Ping(rtt)) => last_rtt = Some(rtt),
                    Err(TestFailure::Timeout(_)) => break,
                    Err(other) => return Err(other),
                }
            }

            // An unreliable `AgentUpdate` cannot itself tear the circuit down, so
            // a keep-alive ping answered after the change is the "no error"
            // signal: the command was accepted and the session is still live.
            let rtt = last_rtt.ok_or_else(|| {
                TestFailure::Assertion(
                    "no keep-alive ping observed after setting the draw distance".to_owned(),
                )
            })?;

            let neighbor_count = neighbors.len();
            if is_opensim(grid) {
                // The local OpenSim grid is a 2×2 block of adjacent regions
                // (Default/East/North/Northeast), so a 512 m draw distance always
                // reaches at least one neighbour — assert the echoed effect.
                check(
                    neighbor_count >= 1,
                    "expected the enlarged draw distance to enable at least one neighbouring region",
                )?;
            } else if neighbor_count == 0 {
                // On Aditi the landing region may have no enabled neighbours
                // within reach; that is a legitimately incomplete dataset.
                ctx.mark_partial("landing region has no neighbouring regions within draw distance");
            }

            let metrics = ctx.metrics();
            metrics.set("draw_distance_m", DRAW_DISTANCE_M);
            metrics.set(
                &count_metric("neighbors"),
                i64::try_from(neighbor_count).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
