//! Apply a bandwidth `Throttle` preset and confirm the simulator accepts it.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Diagnostic, Event, Throttle};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, secs_metric};

/// How long to keep observing the circuit after applying the throttle.
///
/// `AgentThrottle` is sent reliably and has no protocol reply, so the only
/// confirmation of acceptance is that the simulator acks the packet rather than
/// letting our client retransmit it to exhaustion. The client's retransmit
/// budget is `MAX_RESEND_ATTEMPTS` (4) attempts at a round-trip-derived timeout
/// of at most 10 s (five times the 2 s ceiling on the averaged ping), so 40 s
/// bounds it however slow the circuit is; a healthy circuit's timeout settles
/// toward its 1 s floor and gives up far sooner. This window covers that bound
/// with margin, so a silently-dropped throttle has surfaced its
/// retransmit-exhaustion diagnostic before we conclude it was accepted.
const ACCEPT_WINDOW: Duration = Duration::from_secs(45);

/// Per-ping timeout while spanning the [`ACCEPT_WINDOW`].
///
/// The keep-alive ping fires every `PING_INTERVAL` (5 s), so a healthy circuit
/// always answers well inside this; it is kept generous to tolerate Aditi
/// network jitter and load. A ping that fails to arrive within it — or a
/// `Disconnected` — fails the wait, which is exactly the un-accepted-throttle
/// signal we are looking for.
const PING_WAIT: Duration = Duration::from_secs(20);

/// Applies a bandwidth throttle preset and confirms the simulator accepts it.
///
/// A viewer tells the simulator how to split its UDP send bandwidth across the
/// seven traffic categories with an `AgentThrottle` message. It is fire-and-
/// forget: the simulator simply re-weights its outboxes and never replies. So
/// "accepted" cannot be asserted from a reply — instead it is the *absence* of a
/// failure: `AgentThrottle` is sent reliably, and an accepted packet is acked by
/// the sim's reliable-UDP layer rather than retransmitted to exhaustion (which,
/// in our client, abandons the packet and records a diagnostic naming it).
///
/// The case applies the 500 kbps preset (a deliberate change from the default
/// 1000 kbps), then watches the circuit for longer than the retransmit budget by
/// awaiting keep-alive ping round-trips. Those pings keep the circuit's health
/// under test throughout; the throttle itself is judged by the absence of an
/// `AgentThrottle` retransmit-exhaustion diagnostic once the budget has run out.
/// The requested total bandwidth and the post-throttle RTT are recorded.
///
/// Runs on every grid: `AgentThrottle` is plain LLUDP, handled by OpenSim,
/// Second Life and the offline fake grid alike. It is also the longest of the
/// offline cases, and unavoidably so: the assertion *is* the wait, because the
/// only evidence a throttle was accepted is a retransmit budget that went
/// unspent.
#[derive(Debug)]
pub struct ThrottleSet;

impl GridTest for ThrottleSet {
    fn name(&self) -> &'static str {
        "throttle-set"
    }

    fn description(&self) -> &'static str {
        "Apply a bandwidth Throttle preset and confirm the simulator accepts it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // A deliberate change from the client's default (the 1000 kbps
            // preset) so the simulator actually re-weights its outboxes.
            let throttle = Throttle::preset_500();
            let total_kbps = f64::from(throttle.total());

            let start = Instant::now();
            session.send(Command::SetThrottle(throttle)).await?;

            // Keep observing the circuit past the reliable-retransmit budget by
            // awaiting consecutive keep-alive pings, continuously, until the
            // accept window has elapsed. Each ping confirms the circuit is still
            // live, so a link that dies under us fails here rather than being
            // mistaken for an accepted throttle. A generous per-ping timeout
            // (rather than the shrinking window remainder) avoids a spurious
            // timeout on the final sliver of the window while still failing if a
            // ping genuinely stops arriving.
            let mut last_rtt = None;
            while start.elapsed() < ACCEPT_WINDOW {
                let rtt = session
                    .wait_for(PING_WAIT, |event| match event {
                        Event::Ping {
                            child: false, rtt, ..
                        } => Some(*rtt),
                        _ => None,
                    })
                    .await?;
                last_rtt = Some(rtt);
            }

            // The verdict: the reliable AgentThrottle must not have been
            // retransmitted to exhaustion. Losing an ordinary reliable packet
            // does not fail the session — it is reported as this diagnostic and
            // the circuit carries on — so this, not the circuit's survival, is
            // what says the simulator took the throttle.
            let throttle_dropped = session.diagnostics().iter().any(|diagnostic| {
                matches!(
                    diagnostic,
                    Diagnostic::ExpectedReplyMissing { request, .. }
                        if request == "AgentThrottle"
                )
            });
            check(
                !throttle_dropped,
                "AgentThrottle was retransmitted to exhaustion (never acked by the simulator)",
            )?;

            let rtt = last_rtt.ok_or_else(|| {
                crate::context::TestFailure::Assertion(
                    "no keep-alive ping observed after applying the throttle".to_owned(),
                )
            })?;

            let metrics = ctx.metrics();
            metrics.set("throttle_total_kbps", total_kbps);
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
