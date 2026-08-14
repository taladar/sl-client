//! Provoke a server-side kick and assert the session surfaces
//! [`Event::Kicked`] and closes with the kicked disconnect reason.
//!
//! The one kick a single avatar can deterministically provoke is the estate
//! self-kick: the estate owner issues `EstateOwnerMessage`/`kickestate`
//! naming itself as prey. OpenSim answers with a `KickUser` carrying
//! "You have been kicked" and then closes the agent, which the session
//! surfaces as [`Event::Kicked`] followed by [`Event::Disconnected`] with
//! `DisconnectReason::Kicked`. Like the other estate cases, on OpenSim this
//! must run as the **estate-owner** avatar (`--avatar estate-owner`) —
//! `kickestate` needs estate-manager/owner rights and is silently refused
//! otherwise. On Second Life the test avatar holds no estate power over the
//! region, so the request is silently refused and no kick can be provoked
//! with one avatar — the case records that honestly as a partial run. (SL's other kick path — logging the same account in
//! elsewhere — needs a second concurrent login of one account, which the
//! harness deliberately does not do.)
//!
//! Whatever the live outcome, the `KickUser` parse and the
//! kicked-disconnect transition are guaranteed by the in-process client ↔
//! `SimSession` round-trip (`sl-proto/tests/sim_session.rs`,
//! `kick_user_reaches_client_and_disconnects`).

use std::time::Instant;

use sl_client_tokio::{Command, DisconnectReason, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim, secs_metric};

/// Issues an estate self-kick and asserts the kicked session observes the
/// `KickUser` and the kicked disconnect, or records the grid's refusal.
#[derive(Debug)]
pub struct KickUser;

impl GridTest for KickUser {
    fn name(&self) -> &'static str {
        "kick-user"
    }

    fn description(&self) -> &'static str {
        "Estate self-kick: assert Event::Kicked and the kicked disconnect transition"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent_id = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // The estate self-kick: the estate owner (the test avatar on the
            // local grid) kicks itself. On a grid where the avatar holds no
            // estate power the simulator silently ignores the request.
            let started_at = Instant::now();
            session
                .send(Command::KickEstateUser { target: agent_id })
                .await?;

            let kick = match session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::Kicked(kick) => Some(kick.clone()),
                    _ => None,
                })
                .await
            {
                Ok(kick) => kick,
                Err(TestFailure::Timeout(_)) => {
                    ctx.metrics().set("kicked", false);
                    if is_opensim(grid) {
                        // The estate owner kicking itself must work on OpenSim;
                        // silence is a real defect, not a documented gap.
                        return Err(TestFailure::Assertion(
                            "estate self-kick drew no KickUser on OpenSim".to_owned(),
                        ));
                    }
                    ctx.mark_partial(
                        "test avatar holds no estate power on this grid, so no 1av action \
                         can provoke a KickUser (the kickestate request is silently \
                         refused); the KickUser parse and kicked-disconnect transition \
                         are covered by the sl-proto sim_session round-trip test",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            let kick_elapsed = started_at.elapsed();

            check(
                !kick.reason.trim().is_empty(),
                "expected KickUser to carry a non-empty reason",
            )?;
            check_eq("kicked agent", &kick.agent, &agent_id)?;

            // The kick also drives the session to its terminal disconnected
            // state; the predicate resolves on the Disconnected event itself so
            // an unexpected reason is reported rather than swallowed.
            let disconnect_reason = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::Disconnected(reason) => Some(reason.clone()),
                    _ => None,
                })
                .await?;
            check(
                matches!(disconnect_reason, DisconnectReason::Kicked { .. }),
                &format!("expected DisconnectReason::Kicked, got {disconnect_reason:?}"),
            )?;

            let metrics = ctx.metrics();
            metrics.set("kicked", true);
            metrics.set("kick_reason", kick.reason);
            metrics.set_timing(&secs_metric("kick_reply"), kick_elapsed.as_secs_f64());
            Ok(())
        })
    }
}
