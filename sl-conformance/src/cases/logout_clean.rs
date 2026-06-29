//! Request logout and assert a clean `LogoutReply` / shutdown.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Diagnostic, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, secs_metric};

/// How long to let the diagnostic channel settle after `LoggedOut` arrives, so
/// a `LogoutReply`-timeout diagnostic (recorded on a background task in the
/// same run-loop tick) is visible before we read it.
const DIAGNOSTIC_GRACE: Duration = Duration::from_millis(500);

/// Logs out and asserts the session shuts down cleanly via `LoggedOut`,
/// recording whether the grid actually answered with a `LogoutReply`.
///
/// Both a real `LogoutReply` and the client's logout-timeout fallback surface
/// the same [`Event::LoggedOut`]; only a [`Diagnostic::ExpectedReplyMissing`]
/// for `"Logout"` distinguishes them, so the case inspects the diagnostics to
/// tell them apart.
///
/// # Grid behaviour
///
/// Second Life answers promptly with a real `LogoutReply` (observed
/// `logout_secs ≈ 0.18`), so the run is `complete` there.
///
/// OpenSim, by contrast, never gets the reply onto the wire — it is *queued*
/// but then dropped by the synchronous close path, so our client falls back to
/// its 5 s logout timeout and the run is recorded as **partial** (a graceful
/// shutdown without the reply, not a failure). Traced through the OpenSim
/// source (commit current at investigation time):
///
/// 1. `LLUDPServer.LogoutHandler` calls `client.SendLogoutPacket()` — which
///    only `OutPacket`s the `LogoutReply` onto the `Task` throttle outbox
///    (async send) — and then synchronously `Scene.CloseAgent(...)`.
/// 2. `CloseAgent` → `LLClientView.CloseWithoutChecks` calls
///    `m_udpServer.Flush(m_udpClient)`, but `LLUDPServer.Flush` is an
///    unimplemented stub (`// FIXME: Implement?`), so nothing is flushed.
/// 3. It then calls `m_udpClient.Shutdown()`, which sets `IsConnected = false`
///    and `Clear()`s every throttle outbox — discarding the still-queued
///    `LogoutReply` before the async sender drains it. (`SendPacketFinal` also
///    drops packets once `IsConnected` is false.)
///
/// Our client is conformant on both: the run loop keeps the circuit open and
/// reads inbound for the full timeout window, and `LogoutReply` dispatch is not
/// state-gated — a reply on the wire would be decoded. Nothing arrives on
/// OpenSim because the reply is never transmitted.
#[derive(Debug)]
pub struct LogoutClean;

impl GridTest for LogoutClean {
    fn name(&self) -> &'static str {
        "logout-clean"
    }

    fn description(&self) -> &'static str {
        "Request logout and assert a clean LogoutReply / shutdown"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Drive the logout from inside the case so we observe the shutdown.
            // `wait_for` treats an intervening `Disconnected` as a failure
            // unless the predicate consumes it, so a logout answered by a bare
            // unsolicited disconnect (rather than a clean `LoggedOut`) fails.
            let start = Instant::now();
            session.send(Command::Logout).await?;
            session
                .wait_for(REPLY_TIMEOUT, |event| {
                    matches!(event, Event::LoggedOut).then_some(())
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            // The timeout-fallback diagnostic is recorded just before
            // `LoggedOut` on a separate task; let it settle before reading.
            tokio::time::sleep(DIAGNOSTIC_GRACE).await;
            let reply_missing = session.diagnostics().iter().any(|diagnostic| {
                matches!(
                    diagnostic,
                    Diagnostic::ExpectedReplyMissing { request, .. } if request == "Logout"
                )
            });
            let reply_received = !reply_missing;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("logout"), elapsed);
            metrics.set("logout_reply_received", reply_received);

            if reply_missing {
                ctx.mark_partial(
                    "grid did not send a LogoutReply; logged out via timeout fallback",
                );
            }
            Ok(())
        })
    }
}
