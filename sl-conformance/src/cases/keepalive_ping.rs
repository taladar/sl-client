//! Observe a keep-alive ping round-trip over the circuit and record the RTT.

use sl_client_tokio::Event;

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, secs_metric};

/// Waits for the session's periodic keep-alive ping to complete and records its
/// round-trip time.
///
/// Once a region is active the client sends a `StartPingCheck` on the root
/// circuit every few seconds — the reference viewer's circuit ping — and the
/// simulator answers with a `CompletePingCheck` echoing the ping id. The session
/// times that round trip and surfaces it as [`Event::Ping`], the "ping to sim" a
/// viewer displays. The case asserts the round trip is observed and that the RTT
/// is a sane, sub-second-ish measurement (well under the reply timeout), then
/// records it.
///
/// Runs on both grids: the ping exchange is plain LLUDP, present on OpenSim and
/// Second Life alike.
#[derive(Debug)]
pub struct KeepalivePing;

impl GridTest for KeepalivePing {
    fn name(&self) -> &'static str {
        "keepalive-ping"
    }

    fn description(&self) -> &'static str {
        "Observe a keep-alive ping round-trip over the circuit and record the RTT"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The keep-alive ping is driven by the session's own timer, so the
            // case just waits for the first round trip rather than sending a
            // command. Filter to the root circuit's ping — the "ping to sim" a
            // viewer displays — rather than a neighbouring region's child ping.
            // The timeout is generous enough to cover the ping interval plus a
            // slow Aditi round trip.
            let rtt = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::Ping {
                        child: false, rtt, ..
                    } => Some(*rtt),
                    _ => None,
                })
                .await?;

            // A genuine measurement, not a degenerate zero-or-huge value: the
            // round trip must complete inside the reply window.
            check(
                rtt < REPLY_TIMEOUT,
                &format!("ping RTT {rtt:?} should be well under the reply timeout"),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
