//! The primary sends a direct (1:1) instant message; the secondary receives it
//! and replies, and the primary receives the reply.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImDialog};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// The primary sends a 1:1 instant message to the secondary, the secondary
/// receives it and replies, and the primary receives the reply — a full IM
/// round-trip between two distinct agents.
///
/// A direct IM is an `ImprovedInstantMessage` with the `IM_NOTHING_SPECIAL`
/// dialog ([`ImDialog::Message`]), routed by the grid's instant-message service
/// rather than broadcast to the region like local chat. Where
/// [`super::chat_hear_other`] proves the simulator relays a spoken *local-chat*
/// message between nearby avatars, this case proves the IM service delivers a
/// *targeted* message to a named recipient regardless of proximity, and that the
/// recipient's reply travels back the same way: the primary
/// [`Command::InstantMessage`]s the secondary, the secondary — a separate
/// session — observes the matching [`Event::InstantMessageReceived`] attributed
/// to the primary, then `InstantMessage`s a reply, and the primary observes its
/// own matching [`Event::InstantMessageReceived`] attributed to the secondary.
///
/// Each direction's message is tagged with the *sender's* agent id so the
/// predicate matches the exact message and ignores any unrelated background IM
/// (a friendship/inventory offer, a system notice). Asserting
/// [`ImDialog::Message`] confirms an ordinary 1:1 IM rather than one of the many
/// other `ImprovedInstantMessage` sub-types, and asserting `to_agent_id`
/// confirms the IM was addressed to this agent rather than broadcast.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); the Aditi
/// variant is deferred to Phase Z pending a second Aditi avatar. The flow is
/// plain LLUDP `ImprovedInstantMessage` in both directions, identical on both
/// grids.
#[derive(Debug)]
pub struct Im1to1;

impl GridTest for Im1to1 {
    fn name(&self) -> &'static str {
        "im-1to1"
    }

    fn description(&self) -> &'static str {
        "Primary sends a 1:1 IM; secondary receives and replies; primary receives the reply"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before an IM can be
            // routed between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Each direction's IM is attributed to its sender and addressed to
            // its recipient; capture both agent ids to build unambiguous markers
            // and to assert delivery. Take the secondary's id while it is borrowed,
            // then release the borrow before reborrowing the primary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            let outbound = format!("sl-conformance im-1to1 ping {primary_id}");
            let reply = format!("sl-conformance im-1to1 pong {secondary_id}");

            // The primary sends the IM to the secondary; time the delivery.
            let sent_at = Instant::now();
            ctx.primary()
                .send(Command::InstantMessage {
                    to_agent_id: secondary_id,
                    message: outbound.clone(),
                })
                .await?;

            // The secondary receives the matching 1:1 IM from the primary.
            // Filtering on both the sender and the exact text ignores any
            // unrelated background IM.
            let received = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id && im.message == outbound =>
                    {
                        Some((**im).clone())
                    }
                    _ => None,
                })
                .await?;
            let deliver_rtt = sent_at.elapsed();

            // It was an ordinary 1:1 IM addressed to the secondary. (Sender and
            // text were already matched by the predicate; re-assert them so a
            // regression names the wrong field.)
            check_eq("received dialog", &received.dialog, &ImDialog::Message)?;
            check_eq(
                "received from_agent_id",
                &received.from_agent_id,
                &primary_id,
            )?;
            check_eq("received to_agent_id", &received.to_agent_id, &secondary_id)?;
            check_eq("received message", &received.message, &outbound)?;

            // The secondary replies; the primary should receive it the same way.
            let replied_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::InstantMessage {
                    to_agent_id: primary_id,
                    message: reply.clone(),
                })
                .await?;

            let reply_received = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id && im.message == reply =>
                    {
                        Some((**im).clone())
                    }
                    _ => None,
                })
                .await?;
            let reply_rtt = replied_at.elapsed();

            check_eq("reply dialog", &reply_received.dialog, &ImDialog::Message)?;
            check_eq(
                "reply from_agent_id",
                &reply_received.from_agent_id,
                &secondary_id,
            )?;
            check_eq(
                "reply to_agent_id",
                &reply_received.to_agent_id,
                &primary_id,
            )?;
            check_eq("reply message", &reply_received.message, &reply)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("deliver_rtt"), deliver_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("reply_rtt"), reply_rtt.as_secs_f64());
            Ok(())
        })
    }
}
