//! A second avatar starts and stops typing in local chat; the primary observes
//! both transitions.

use std::time::Instant;

use sl_client_tokio::{Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// A second avatar toggles its local-chat typing indicator on, then off, and
/// the primary avatar observes each transition as an [`Event::ChatTyping`]
/// attributed to the typist.
///
/// The local-chat typing indicator is a `ChatFromViewer` with no text and a
/// `StartTyping`/`StopTyping` chat type (the animation trigger a viewer fires
/// while the user edits the chat bar). The simulator broadcasts it to nearby
/// avatars as a `ChatFromSimulator` of the same type, which the client surfaces
/// as [`Event::ChatTyping`] rather than an empty chat line. Where
/// [`super::chat_hear_other`] proves the simulator relays a spoken *message*
/// between distinct agents, this case proves it relays the typing *signal*: the
/// secondary [`Command::Typing(true)`](Command::Typing) then
/// [`Command::Typing(false)`](Command::Typing), and the primary — a separate
/// session sharing the region — sees `typing: true` then `typing: false`, both
/// attributed to the secondary's agent id.
///
/// Matching on the secondary's agent id as `source_id` ignores any unrelated
/// background typing. Unlike a spoken `say` (gated by the say/whisper/shout
/// distance), OpenSim delivers a `StartTyping`/`StopTyping` with no distance
/// check, so the relay does not depend on how close the two avatars logged in.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); the Aditi
/// variant is deferred to Phase Z pending its Aditi run. The flow is
/// plain LLUDP `ChatFromViewer`/`ChatFromSimulator`, identical on both grids.
#[derive(Debug)]
pub struct TypingIndicator;

impl GridTest for TypingIndicator {
    fn name(&self) -> &'static str {
        "typing-indicator"
    }

    fn description(&self) -> &'static str {
        "A second avatar starts/stops typing in local chat; the primary observes both"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be active in the region before the relay can be
            // observed: the secondary to type, the primary to listen.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // The relayed typing signal is attributed to the typist's own agent
            // id, which the primary uses to recognise it and ignore any unrelated
            // background typing. Capture it now so the secondary borrow is released
            // before the primary is reborrowed below.
            let typist = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;

            // The secondary starts typing; the primary should see a `ChatTyping`
            // with `typing: true` attributed to the typist. Re-acquire the
            // secondary per phase so its borrow does not overlap the primary's.
            let started_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::Typing(true))
                .await?;
            let start = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ChatTyping {
                        source_id, typing, ..
                    } if *source_id == typist.uuid() => Some(*typing),
                    _ => None,
                })
                .await?;
            let start_rtt = started_at.elapsed();

            // The secondary stops typing; the primary should see the matching
            // `ChatTyping` with `typing: false`.
            let stopped_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::Typing(false))
                .await?;
            let stop = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ChatTyping {
                        source_id, typing, ..
                    } if *source_id == typist.uuid() => Some(*typing),
                    _ => None,
                })
                .await?;
            let stop_rtt = stopped_at.elapsed();

            // The primary observed the start as a `typing: true` and the stop as a
            // `typing: false`, both from the typist's agent. (The source id was
            // already matched by the predicates; the `typing` flag is the
            // transition under test.)
            check_eq("start typing flag", &start, &true)?;
            check_eq("stop typing flag", &stop, &false)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("start_rtt"), start_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("stop_rtt"), stop_rtt.as_secs_f64());
            Ok(())
        })
    }
}
