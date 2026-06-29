//! A second avatar says on the public channel; the primary avatar hears it.

use std::time::Instant;

use sl_client_tokio::{ChatAudible, ChatChannel, ChatSource, ChatType, Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// The public local-chat channel (`0`), the only channel a normal `say` is
/// audible on and thus the only one the simulator relays to other nearby
/// avatars. Higher channels are script-only and never reach an avatar's
/// listener.
const PUBLIC_CHANNEL: ChatChannel = ChatChannel(0);

/// A second avatar says one message on the public channel and confirms the
/// primary avatar hears it as a `ChatFromSimulator` attributed to the speaker.
///
/// Where [`super::chat_self_echo`] proves the outbound chat path round-trips to
/// the speaker itself, this two-avatar case proves the simulator *relays* local
/// chat between distinct agents: the secondary `say`s a marker tagged with its
/// own agent id, and the primary — a separate logged-in session sharing the
/// region — receives the matching [`Event::ChatReceived`] attributed to the
/// secondary's agent, fully audible, at normal volume.
///
/// Matching on both the secondary's agent id as the source and the exact marker
/// text ignores any unrelated background chat (including the speaker's own
/// self-echo, which lands only on the secondary session). Asserting
/// [`ChatAudible::Fully`] confirms the two avatars actually shared the region
/// within `say` range, rather than the simulator emitting an out-of-range stub.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); the Aditi
/// variant is deferred to Phase Z pending a second Aditi avatar. The flow is
/// plain LLUDP `ChatFromViewer`/`ChatFromSimulator`, identical on both grids.
#[derive(Debug)]
pub struct ChatHearOther;

impl GridTest for ChatHearOther {
    fn name(&self) -> &'static str {
        "chat-hear-other"
    }

    fn description(&self) -> &'static str {
        "A second avatar says on the public channel; the primary hears it"
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
            // observed: the secondary to speak, the primary to listen.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // The relayed chat is attributed to the speaker's own agent id, so the
            // case needs it both to recognise the message and to build a marker the
            // simulator cannot confuse with other avatars' chat.
            let speaker = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let message = format!("sl-conformance chat-hear-other {speaker}");

            // The secondary says it on the public channel and we time how long the
            // simulator takes to relay it to the primary.
            let sent_at = Instant::now();
            secondary
                .send(Command::Chat {
                    message: message.clone(),
                    chat_type: ChatType::Normal,
                    channel: PUBLIC_CHANNEL,
                })
                .await?;

            // The primary listens for the relayed message: a `ChatFromSimulator`
            // carrying the exact marker text and attributed to the secondary's
            // agent. Filtering on both the source and the text ignores unrelated
            // background chat.
            let heard = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ChatReceived(chat)
                        if chat.source == ChatSource::Agent(speaker) && chat.message == message =>
                    {
                        Some((**chat).clone())
                    }
                    _ => None,
                })
                .await?;
            let rtt = sent_at.elapsed();

            // The primary heard the secondary's say verbatim, fully audible, as a
            // normal-volume message from the secondary's agent. (Source and text
            // were already matched by the predicate; re-assert them so a regression
            // names the wrong field.)
            check_eq("heard source", &heard.source, &ChatSource::Agent(speaker))?;
            check_eq("heard message", &heard.message, &message)?;
            check_eq("heard chat_type", &heard.chat_type, &ChatType::Normal)?;
            check_eq("heard audible", &heard.audible, &ChatAudible::Fully)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("hear_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
