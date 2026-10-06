//! Log out twice — once settled in world, once the moment the region is up —
//! and record what the grid answers each time, and the login in between.
//!
//! A logout is one reliable `LogoutRequest` and, when the grid answers, one
//! `LogoutReply`. Two things about it can differ between grids, and the case
//! measures each:
//!
//! 1. **Whether the reply comes.** Both a real `LogoutReply` and the client's
//!    logout-timeout fallback surface the same [`Event::LoggedOut`]; only a
//!    diagnostic tells them apart ([`log_out`]).
//!    The case logs out **settled** (after twelve seconds in world, with whatever
//!    child circuits the region opened) and then **early** (a second login,
//!    logged out as soon as its region handshake completes), because the two
//!    are answered differently where the reply is a race.
//! 2. **What the next login is answered with** — whether the logout left a
//!    presence behind for the next login to trip over.
//!
//! # Grid behaviour
//!
//! Second Life answers every logout with a `LogoutReply` within a fraction of
//! a second.
//!
//! OpenSim answers *sometimes*. `LLUDPServer.LogoutHandler` queues the reply
//! on the `Task` throttle's outbox and then closes the agent synchronously;
//! `LLClientView.CloseWithoutChecks` calls `LLUDPServer.Flush` — an
//! unimplemented stub — and then `LLUDPClient.Shutdown`, which clears every
//! outbox and the pending acks. The reply reaches the wire only when the
//! outgoing-packet thread happens to drain the outbox between those two
//! steps, so a live OpenSim run is recorded, never asserted. The close itself
//! always happens: the presence is gone and the next login gets in.
//!
//! The fake grid imitating OpenSim takes the half a client has to survive —
//! the reply is withheld — so the offline suite exercises the timeout
//! fallback on one flavour and the reply on the other.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use sl_client_tokio::Event;

use crate::context::{LoginAnswer, LoginAttempt, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{Logout, REGION_TIMEOUT, check, count_metric, log_out, secs_metric};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/session.md § Logout (logout-clean, 2026-10-06)";

/// How long the first session stays in world before it logs out: long enough
/// for the arrival burst to drain and the neighbours' child circuits to open.
const SETTLE: Duration = Duration::from_secs(12);

/// The case's overall budget: two logins, and on a grid that rate-limits
/// logins the cooldown waited out between them.
const CASE_TIMEOUT: Duration = Duration::from_secs(6 * 60);

/// Whether a logout is answered with a `LogoutReply`. OpenSim's answer is the
/// one its fake flavour gives; the live grid's is a race and is not held to
/// it.
const REPLY_RECEIVED: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Whether the login after a logout gets in.
const NEXT_LOGIN_ADMITTED: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Stay in world for [`SETTLE`], counting the neighbours whose child circuits
/// the region opened meanwhile.
async fn settle(session: &mut Session) -> Result<usize, TestFailure> {
    let mut neighbours: HashSet<SocketAddr> = HashSet::new();
    let watched = session
        .wait_for(SETTLE, |event| {
            if let Event::NeighborDiscovered(info) = event {
                let _new = neighbours.insert(info.sim);
            }
            None::<()>
        })
        .await;
    match watched {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(neighbours.len()),
        Err(other) => Err(other),
    }
}

/// Hold a logout's reply to the measured answer — except on the live OpenSim,
/// where it is a race the case can only record.
fn check_reply(grid: Grid, what: &str, logout: Logout) -> Result<(), TestFailure> {
    if matches!(grid, Grid::Opensim) {
        return Ok(());
    }
    REPLY_RECEIVED.check(what, grid, &logout.reply_received)
}

/// Logs out settled and early, recording each logout's answer and the login
/// between them.
#[derive(Debug)]
pub struct LogoutClean;

impl GridTest for LogoutClean {
    fn name(&self) -> &'static str {
        "logout-clean"
    }

    fn description(&self) -> &'static str {
        "Log out settled and early; record the LogoutReply, its timing and the next login"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // 1. The settled logout.
            let (neighbours, settled) = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                let neighbours = settle(session).await?;
                (neighbours, log_out(session).await?)
            };
            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("child_circuits"),
                u32::try_from(neighbours).unwrap_or(u32::MAX),
            );
            metrics.set_timing(&secs_metric("logout"), settled.seconds);
            metrics.set("logout_reply_received", settled.reply_received);

            // 2. The next login, exactly one request: a presence the logout
            //    left behind would refuse it.
            let answer = ctx
                .primary()
                .attempt_login(&LoginAttempt::default())
                .await?;
            let mut next = match answer {
                LoginAnswer::Admitted(next) => {
                    ctx.metrics().set("next_login", "admitted");
                    next
                }
                LoginAnswer::Refused { failure, .. } => {
                    let metrics = ctx.metrics();
                    metrics.set("next_login", format!("refused: {}", failure.reason));
                    metrics.set("next_login_message", failure.message);
                    return NEXT_LOGIN_ADMITTED.check(
                        "whether the login after a logout gets in",
                        grid,
                        &false,
                    );
                }
                LoginAnswer::Challenged(challenge) => {
                    return Err(TestFailure::Assertion(format!(
                        "the login after the logout was challenged: {}",
                        challenge.message
                    )));
                }
            };

            // 3. The early logout: the moment that login's region is up.
            next.wait_for_region(REGION_TIMEOUT).await?;
            let early = log_out(&mut next).await?;
            // The session is closed either way; this joins its run loop.
            next.logout().await?;
            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("early_logout"), early.seconds);
            metrics.set("early_logout_reply_received", early.reply_received);

            check(
                settled.reply_received || settled.seconds >= 1.0,
                "a logout with no LogoutReply ended before the client could have timed out",
            )?;
            check_reply(grid, "whether a settled logout is answered", settled)?;
            check_reply(grid, "whether an early logout is answered", early)?;
            NEXT_LOGIN_ADMITTED.check("whether the login after a logout gets in", grid, &true)
        })
    }
}
