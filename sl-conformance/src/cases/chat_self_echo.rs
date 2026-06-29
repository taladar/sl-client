//! Say on the public channel and observe the simulator echo the avatar's own
//! chat back to it.

use std::time::Instant;

use sl_client_tokio::{ChatChannel, ChatSource, ChatType, Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// The public local-chat channel (`0`), the only channel a normal `say` is
/// audible on — and so the only one the simulator echoes back to nearby avatars,
/// including the speaker itself. Higher channels are script-only and never reach
/// an avatar's listener.
const PUBLIC_CHANNEL: ChatChannel = ChatChannel(0);

/// Says one message on the public channel and confirms the simulator echoes it
/// straight back to the speaker as a `ChatFromSimulator`.
///
/// When a viewer sends `ChatFromViewer` on channel 0, the simulator broadcasts
/// the resulting `ChatFromSimulator` to every nearby agent — the speaker
/// included. That self-echo is what a viewer renders in its own local-chat
/// history, and observing it is the cleanest single-avatar confirmation that the
/// outbound chat path round-trips through the region: the message text comes back
/// verbatim, attributed to the speaker's own agent id.
///
/// The case sends a marker message tagged with the agent's own id (so it cannot
/// be confused with unrelated background chat), then waits for the matching
/// [`Event::ChatReceived`] whose source is the speaker's own agent. It asserts
/// the echoed text, source, and chat type, and records the echo round-trip time.
///
/// Runs on both grids: `ChatFromViewer`/`ChatFromSimulator` is plain LLUDP,
/// present on OpenSim and Second Life alike, and needs only the one logged-in
/// avatar.
#[derive(Debug)]
pub struct ChatSelfEcho;

impl GridTest for ChatSelfEcho {
    fn name(&self) -> &'static str {
        "chat-self-echo"
    }

    fn description(&self) -> &'static str {
        "Say on the public channel and observe the simulator echo own chat"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The echo is attributed to the speaker's own agent id, so the case
            // needs it both to recognise the reply and to build a marker the
            // simulator cannot confuse with other avatars' chat.
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("login did not report an agent id".to_owned())
            })?;
            let message = format!("sl-conformance chat-self-echo {own}");

            // Say it on the public channel and time how long the simulator takes
            // to echo it back. A normal `say` on channel 0 is heard by every
            // nearby agent, the speaker included.
            let sent_at = Instant::now();
            session
                .send(Command::Chat {
                    message: message.clone(),
                    chat_type: ChatType::Normal,
                    channel: PUBLIC_CHANNEL,
                })
                .await?;

            // Wait for the self-echo: a `ChatFromSimulator` carrying our exact
            // marker text and attributed to our own agent. Filtering on both the
            // source and the text ignores any unrelated background chat.
            let echo = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ChatReceived(chat)
                        if chat.source == ChatSource::Agent(own) && chat.message == message =>
                    {
                        Some(chat.clone())
                    }
                    _ => None,
                })
                .await?;
            let rtt = sent_at.elapsed();

            // The simulator echoes the say verbatim, as a normal-volume message
            // from our own agent. (Source and text were already matched by the
            // predicate; re-assert them so a regression names the wrong field.)
            check_eq("echo source", &echo.source, &ChatSource::Agent(own))?;
            check_eq("echo message", &echo.message, &message)?;
            check_eq("echo chat_type", &echo.chat_type, &ChatType::Normal)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("echo_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
