//! The primary starts and stops typing in a 1:1 IM session; the secondary
//! observes both transitions.

use std::time::Instant;

use sl_client_tokio::{ChatSessionKind, Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// The primary toggles its instant-message typing indicator on, then off,
/// addressed to the secondary, and the secondary observes each transition as an
/// [`Event::ImTyping`] attributed to the primary within their shared 1:1 IM
/// session.
///
/// IM typing is an `ImprovedInstantMessage` with an `IM_TYPING_START` /
/// `IM_TYPING_STOP` dialog and the literal text `"typing"` (the notification a
/// viewer fires while the user edits an IM window). Unlike the local-chat
/// typing of [`super::typing_indicator`] — a `ChatFromViewer` the simulator
/// broadcasts to *nearby* avatars — this is routed by the grid's IM service to
/// a single named recipient and carries the canonical 1:1 session id
/// (`agent_id XOR to_agent_id`), exactly as the IM in [`super::im_1to1`] does.
/// Where `im-1to1` proves the IM service relays a targeted *message*, this case
/// proves it relays the typing *signal* over the same session: the primary
/// [`Command::ImTyping`]s `typing: true` then `typing: false` to the secondary,
/// and the secondary — a separate session — sees `typing: true` then
/// `typing: false`, both attributed to the primary's agent id.
///
/// Matching on the primary's agent id as `from_agent_id` ignores any unrelated
/// background typing. The case also asserts the `session_id` equals the
/// canonical id of the secondary's `Direct` session with the primary, proving
/// the signal arrived on the targeted 1:1 session rather than as a stray
/// broadcast.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); the Aditi
/// variant is deferred to Phase Z pending a second Aditi avatar. The flow is
/// plain LLUDP `ImprovedInstantMessage` typing dialogs, identical on both grids.
#[derive(Debug)]
pub struct ImTyping;

impl GridTest for ImTyping {
    fn name(&self) -> &'static str {
        "im-typing"
    }

    fn description(&self) -> &'static str {
        "Primary starts/stops typing in a 1:1 IM session; the secondary observes both"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before a typing
            // notification can be routed between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // The typing notification is attributed to the primary and addressed
            // to the secondary; capture both agent ids to build the marker and to
            // assert delivery. Take the secondary's id while it is borrowed, then
            // release the borrow before reborrowing the primary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // The 1:1 IM session id is symmetric (`agent_id XOR to_agent_id`), so
            // the id the secondary observes is the canonical id of its own
            // `Direct` session with the primary.
            let session_id =
                ChatSessionKind::Direct { peer: primary_id }.canonical_session_id(secondary_id);

            // The primary starts typing; the secondary should see an `ImTyping`
            // with `typing: true` from the primary on the shared session.
            let started_at = Instant::now();
            ctx.primary()
                .send(Command::ImTyping {
                    to_agent_id: secondary_id,
                    typing: true,
                })
                .await?;
            let start = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ImTyping {
                        from_agent_id,
                        session_id: sid,
                        typing,
                        ..
                    } if *from_agent_id == primary_id => Some((*sid, *typing)),
                    _ => None,
                })
                .await?;
            let start_rtt = started_at.elapsed();

            // The primary stops typing; the secondary should see the matching
            // `ImTyping` with `typing: false`.
            let stopped_at = Instant::now();
            ctx.primary()
                .send(Command::ImTyping {
                    to_agent_id: secondary_id,
                    typing: false,
                })
                .await?;
            let stop = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ImTyping {
                        from_agent_id,
                        session_id: sid,
                        typing,
                        ..
                    } if *from_agent_id == primary_id => Some((*sid, *typing)),
                    _ => None,
                })
                .await?;
            let stop_rtt = stopped_at.elapsed();

            // The secondary observed the start as a `typing: true` and the stop as
            // a `typing: false`, both from the primary on the canonical 1:1
            // session. (The sender was already matched by the predicates; the
            // session id and the `typing` flag are the transitions under test.)
            let (start_session, start_typing) = start;
            let (stop_session, stop_typing) = stop;
            check_eq("start session id", &start_session, &session_id)?;
            check_eq("start typing flag", &start_typing, &true)?;
            check_eq("stop session id", &stop_session, &session_id)?;
            check_eq("stop typing flag", &stop_typing, &false)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("start_rtt"), start_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("stop_rtt"), stop_rtt.as_secs_f64());
            Ok(())
        })
    }
}
