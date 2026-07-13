//! The secondary IMs the primary twice so the primary's 1:1 chat session
//! accumulates unread messages; the primary then marks the session read and the
//! unread counter resets to zero.

use std::time::Instant;

use sl_client_tokio::{ChatLifecycleView, ChatSessionInfo, ChatSessionKind, Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, count_metric, secs_metric};

/// The secondary sends two 1:1 IMs to the primary; each inbound message bumps the
/// primary's per-session unread counter, so the primary's `Direct` session reads
/// `unread == 2`. The primary then [`Command::MarkSessionRead`]s the session and
/// the counter resets to zero — the unread → mark-read transition.
///
/// Each chat session carries an unread counter: it is bumped on every inbound
/// message that is not our own echo, and cleared either by our own outbound send
/// (sending implies we have read the conversation) or explicitly by
/// [`Command::MarkSessionRead`] (the viewer "marking a conversation read" without
/// replying). This case exercises the explicit-clear path on a 1:1 `Direct`
/// session: the secondary `InstantMessage`s the primary twice — each tagged with
/// the secondary's agent id so the predicate ignores unrelated background IM — and
/// the primary, a separate session, observes both [`Event::InstantMessageReceived`]
/// and so accumulates two unread on its `Direct { peer: secondary }` session.
///
/// The transition is asserted against the primary's chat-session registry via
/// [`Command::QueryChatSessions`]: before marking, the `Direct` session is present
/// as a [`ChatLifecycleView::Joined`] 1:1 with `unread == 2` (proving the counter
/// *counts*, not merely flips a flag); after [`Command::MarkSessionRead`] the same
/// session is still present and still `Joined` (mark-read clears the badge, it
/// does not close the conversation) but reads `unread == 0`.
///
/// `MarkSessionRead` is a purely local registry operation (no wire send), so it is
/// identical on both grids; only the inbound IMs that seed the unread count touch
/// the wire, and those are plain LLUDP `ImprovedInstantMessage`. `2av`. Runs on
/// OpenSim today (local secondary `Friend Tester`); `[opensim]` only, the Aditi
/// variant deferred to Phase Z pending its Aditi run.
#[derive(Debug)]
pub struct SessionMarkRead;

/// How many IMs the secondary sends to seed the unread count. Two (rather than
/// one) so the assertion proves `unread` is a *count* that accumulates, not a
/// boolean has-unread flag.
const MESSAGES: u32 = 2;

impl GridTest for SessionMarkRead {
    fn name(&self) -> &'static str {
        "session-mark-read"
    }

    fn description(&self) -> &'static str {
        "Secondary IMs the primary twice; the primary marks the session read and unread resets to zero"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before an IM can be routed
            // between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // The unread will accrue on the primary's `Direct` session keyed by the
            // secondary (the sender). Capture the secondary's id while it is
            // borrowed, then release the borrow before reborrowing the primary to
            // capture the primary's id (the IM recipient).
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;
            let direct = ChatSessionKind::Direct { peer: secondary_id };

            // The secondary sends two IMs to the primary; the primary waits for each
            // matching `InstantMessageReceived` (which is emitted *after* the inbound
            // message bumps the session's unread counter, so by the time both events
            // are observed the registry shows `unread == MESSAGES`). Tagging each
            // marker with the sender's id ignores any unrelated background IM.
            let first_sent_at = Instant::now();
            for index in 0..MESSAGES {
                let marker = format!("sl-conformance session-mark-read {secondary_id} {index}");
                ctx.secondary()
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "two-account test ran without a secondary".to_owned(),
                        )
                    })?
                    .send(Command::InstantMessage {
                        to_agent_id: primary_id,
                        message: marker.clone(),
                    })
                    .await?;
                ctx.primary()
                    .wait_for(REPLY_TIMEOUT, move |event| match event {
                        Event::InstantMessageReceived(im)
                            if im.from_agent_id == secondary_id && im.message == marker =>
                        {
                            Some(())
                        }
                        _ => None,
                    })
                    .await?;
            }
            let deliver_rtt = first_sent_at.elapsed();

            // Before marking read: the primary's `Direct` session is a joined 1:1
            // carrying both unread messages.
            let before = require_primary_session(ctx, direct, "before mark-read").await?;
            check_eq(
                "session lifecycle before mark-read",
                &matches!(before.lifecycle, ChatLifecycleView::Joined),
                &true,
            )?;
            check_eq("unread before mark-read", &before.unread, &MESSAGES)?;

            // Mark the session read; the unread counter resets to zero. This is a
            // local registry operation, processed in order ahead of the query that
            // follows it on the same command channel.
            ctx.primary()
                .send(Command::MarkSessionRead { session: direct })
                .await?;

            // After marking read: the session is still present and still joined
            // (mark-read clears the badge, it does not close the conversation) but
            // reads zero unread.
            let after = require_primary_session(ctx, direct, "after mark-read").await?;
            check_eq(
                "session lifecycle after mark-read",
                &matches!(after.lifecycle, ChatLifecycleView::Joined),
                &true,
            )?;
            check_eq("unread after mark-read", &after.unread, &0)?;

            let metrics = ctx.metrics();
            metrics.set(&count_metric("messages_sent"), MESSAGES);
            metrics.set(&count_metric("unread_before"), before.unread);
            metrics.set(&count_metric("unread_after"), after.unread);
            metrics.set_timing(&secs_metric("deliver_rtt"), deliver_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Queries the primary's chat-session registry and returns the entry for `kind`,
/// failing the test if it is absent. `phase` names the moment for the failure
/// message.
async fn require_primary_session(
    ctx: &mut TestContext,
    kind: ChatSessionKind,
    phase: &str,
) -> Result<ChatSessionInfo, TestFailure> {
    ctx.primary().send(Command::QueryChatSessions).await?;
    let sessions = ctx
        .primary()
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ChatSessions(sessions) => {
                Some(std::sync::Arc::<[ChatSessionInfo]>::clone(sessions))
            }
            _ => None,
        })
        .await?;
    sessions
        .iter()
        .find(|info| info.kind == kind)
        .cloned()
        .ok_or_else(|| {
            TestFailure::Assertion(format!(
                "primary's 1:1 session missing from the registry {phase}"
            ))
        })
}
