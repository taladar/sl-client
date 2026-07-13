//! Two avatars at known separations confirm that whisper and shout carry a
//! different distance than a normal say.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, ChatAudible, ChatChannel, ChatMessage, ChatSource, ChatType, Command, Event,
    RegionCoordinates, RegionHandle, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, secs_metric};

/// The public local-chat channel (`0`), the only channel a normal say, whisper,
/// or shout is audible on and thus the only one the simulator relays between
/// avatars.
const PUBLIC_CHANNEL: ChatChannel = ChatChannel(0);

/// The shared region-local X both avatars stand on, so their separation runs
/// purely along Y and equals the Y offset.
const LANE_X: f32 = 128.0;

/// The primary avatar's anchor Y; the secondary is placed at `ANCHOR_Y + d`.
const ANCHOR_Y: f32 = 100.0;

/// A height well above any terrain. An intra-region teleport only adjusts Z when
/// it falls *below* the ground (OpenSim's `TeleportAgentWithinRegion` clamps up
/// to the heightmap, never down), so placing both avatars at the same high Z
/// keeps their Z identical and makes the 3-D chat-range distance exactly the
/// horizontal Y separation — independent of the region's terrain. With physics
/// disabled (the local grid's default) neither avatar falls before it speaks.
const LANE_Z: f32 = 1000.0;

/// A separation between whisper range (10 m) and normal-say range (20 m): a say
/// reaches, a whisper does not.
const NEAR_SEPARATION: f32 = 15.0;

/// A separation between normal-say range (20 m) and shout range (100 m): a shout
/// reaches, a say does not.
const FAR_SEPARATION: f32 = 60.0;

/// How long to keep watching for a message that should NOT arrive, after the
/// in-range sentinel that the secondary sent *after* it has already been heard.
/// The sentinel is a causal fence (same circuit, sent later), so this only
/// guards against a reordered late delivery; loopback relay RTT is ~1 ms, so the
/// window is ample without slowing the case much.
const SILENCE_GRACE: Duration = Duration::from_secs(2);

/// What the primary heard while watching its chat stream for two tagged markers.
enum Heard {
    /// The message that should have been out of range arrived — a failure.
    OutOfRange,
    /// The in-range sentinel arrived (the expected outcome), carried out for
    /// field assertions.
    Sentinel(ChatMessage),
}

/// Verifies that whisper carries a shorter distance, and shout a longer
/// distance, than a normal say, by separating the two avatars to a known
/// distance and observing which chat types still reach.
///
/// Where [`super::chat_hear_other`] proves the simulator relays local chat
/// between distinct agents at all, this case proves the relay is *range-gated*
/// by volume. OpenSim drops an out-of-range message outright rather than marking
/// it less audible (`ChatModule.TrySendChatMessage` returns without sending once
/// the squared distance exceeds the type's range, and any delivered message is
/// always `Fully` audible), so the observable is simply whether the matching
/// [`Event::ChatReceived`] arrives.
///
/// The case anchors the primary and teleports the secondary to two separations:
///
/// - **15 m** (between whisper's 10 m and say's 20 m): a whisper does not reach
///   but a normal say does, so whisper range < say range.
/// - **60 m** (between say's 20 m and shout's 100 m): a say does not reach but a
///   shout does, so say range < shout range.
///
/// At each separation the secondary says the out-of-range message immediately
/// followed by an in-range sentinel of a louder type; the primary must hear the
/// sentinel but never the out-of-range message. Both avatars are placed with an
/// intra-region [`Command::Teleport`] so the separation is exact and independent
/// of where they logged in.
///
/// `2av`. Runs on OpenSim today (local secondary `Friend Tester`); the Aditi
/// variant is deferred to Phase Z pending its Aditi run. The flow is
/// plain LLUDP `ChatFromViewer`/`ChatFromSimulator` and `TeleportLocationRequest`,
/// identical on both grids.
#[derive(Debug)]
pub struct ChatWhisperShoutRange;

impl GridTest for ChatWhisperShoutRange {
    fn name(&self) -> &'static str {
        "chat-whisper-shout-range"
    }

    fn description(&self) -> &'static str {
        "Whisper and shout carry a different range than a normal say"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be active before either can be repositioned: the
            // primary supplies the region they share, the secondary speaks.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let region = ctx.primary().region_handle().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report a region handle".to_owned())
            })?;

            // The relayed chat is attributed to the speaker's own agent id, so
            // the case needs it both to recognise messages and to build markers
            // the simulator cannot confuse with other avatars' chat.
            let speaker = {
                let secondary = ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?;
                secondary.wait_for_region(REGION_TIMEOUT).await?;
                secondary.agent_id().ok_or_else(|| {
                    TestFailure::Assertion("secondary login did not report an agent id".to_owned())
                })?
            };

            // Anchor the primary; it stays put for the rest of the case.
            teleport_within(ctx.primary(), region, LANE_X, ANCHOR_Y, LANE_Z).await?;

            // Near separation: a normal say reaches, a whisper does not.
            let (say, say_rtt) = range_phase(
                ctx,
                region,
                speaker,
                NEAR_SEPARATION,
                "near",
                ChatType::Whisper,
                ChatType::Normal,
            )
            .await?;
            check_eq("say source", &say.source, &ChatSource::Agent(speaker))?;
            check_eq("say chat_type", &say.chat_type, &ChatType::Normal)?;
            check_eq("say audible", &say.audible, &ChatAudible::Fully)?;

            // Far separation: a shout reaches, a normal say does not.
            let (shout, shout_rtt) = range_phase(
                ctx,
                region,
                speaker,
                FAR_SEPARATION,
                "far",
                ChatType::Normal,
                ChatType::Shout,
            )
            .await?;
            check_eq("shout source", &shout.source, &ChatSource::Agent(speaker))?;
            check_eq("shout chat_type", &shout.chat_type, &ChatType::Shout)?;
            check_eq("shout audible", &shout.audible, &ChatAudible::Fully)?;

            let metrics = ctx.metrics();
            metrics.set("near_separation_m", f64::from(NEAR_SEPARATION));
            metrics.set("far_separation_m", f64::from(FAR_SEPARATION));
            metrics.set_timing(&secs_metric("say_rtt"), say_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("shout_rtt"), shout_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Teleport `session` to the region-local `(x, y, z)` within `region` and wait
/// for the teleport to complete.
///
/// Within the current region the simulator answers with `TeleportLocal`
/// ([`Event::TeleportLocal`]); if the avatar logged in to a neighbouring region
/// the same request is a cross-region teleport that completes with an
/// [`Event::RegionChanged`]. Either confirms the avatar now stands at the
/// requested position in `region`, so the case tolerates both.
async fn teleport_within(
    session: &mut Session,
    region: RegionHandle,
    x: f32,
    y: f32,
    z: f32,
) -> Result<(), TestFailure> {
    session
        .send(Command::Teleport {
            region_handle: region,
            position: RegionCoordinates::new(x, y, z),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    session
        .wait_for(REGION_TIMEOUT, |event| {
            matches!(event, Event::TeleportLocal | Event::RegionChanged { .. }).then_some(())
        })
        .await
}

/// Run one separation phase: place the secondary `separation` metres from the
/// anchored primary, have it say an out-of-range message of `silent_type`
/// immediately followed by an in-range sentinel of `sentinel_type`, and assert
/// the primary hears only the sentinel.
///
/// `tag` distinguishes the two phases' marker text. Returns the heard sentinel
/// (for field assertions) and its round-trip time.
async fn range_phase(
    ctx: &mut TestContext,
    region: RegionHandle,
    speaker: AgentKey,
    separation: f32,
    tag: &str,
    silent_type: ChatType,
    sentinel_type: ChatType,
) -> Result<(ChatMessage, Duration), TestFailure> {
    let silent = format!("sl-conformance chat-range {speaker} {tag}-silent");
    let sentinel = format!("sl-conformance chat-range {speaker} {tag}-sentinel");

    // Position the secondary and let it speak: the out-of-range message first,
    // then the in-range sentinel, both on the same circuit so the sentinel is a
    // causal fence after the silent one.
    let sent_at = {
        let secondary = ctx.secondary().ok_or_else(|| {
            TestFailure::Assertion("two-account test ran without a secondary".to_owned())
        })?;
        teleport_within(secondary, region, LANE_X, ANCHOR_Y + separation, LANE_Z).await?;
        secondary
            .send(Command::Chat {
                message: silent.clone(),
                chat_type: silent_type,
                channel: PUBLIC_CHANNEL,
            })
            .await?;
        let sent_at = Instant::now();
        secondary
            .send(Command::Chat {
                message: sentinel.clone(),
                chat_type: sentinel_type,
                channel: PUBLIC_CHANNEL,
            })
            .await?;
        sent_at
    };

    expect_in_range_only(ctx.primary(), speaker, &silent, &sentinel, sent_at).await
}

/// Assert the primary hears `sentinel` but never `silent`, both attributed to
/// `speaker`.
///
/// The secondary sent `silent` before `sentinel` over the same circuit, so once
/// the sentinel arrives the out-of-range message — had it been delivered — would
/// already have arrived too; seeing it before the sentinel fails the phase. A
/// short [`SILENCE_GRACE`] after the sentinel guards against a reordered late
/// delivery. Returns the heard sentinel and its round-trip time, measured at the
/// moment of receipt (before the grace window).
async fn expect_in_range_only(
    primary: &mut Session,
    speaker: AgentKey,
    silent: &str,
    sentinel: &str,
    sentinel_sent_at: Instant,
) -> Result<(ChatMessage, Duration), TestFailure> {
    let heard = match primary
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ChatReceived(chat) if chat.source == ChatSource::Agent(speaker) => {
                if chat.message == silent {
                    Some(Heard::OutOfRange)
                } else if chat.message == sentinel {
                    Some(Heard::Sentinel((**chat).clone()))
                } else {
                    None
                }
            }
            _ => None,
        })
        .await?
    {
        Heard::OutOfRange => {
            return Err(TestFailure::Assertion(format!(
                "out-of-range message was heard: {silent:?}"
            )));
        }
        Heard::Sentinel(chat) => chat,
    };
    let rtt = sentinel_sent_at.elapsed();

    // Confirm the out-of-range message does not arrive after the sentinel.
    match primary
        .wait_for(SILENCE_GRACE, |event| match event {
            Event::ChatReceived(chat)
                if chat.source == ChatSource::Agent(speaker) && chat.message == silent =>
            {
                Some(())
            }
            _ => None,
        })
        .await
    {
        Ok(()) => Err(TestFailure::Assertion(format!(
            "out-of-range message arrived after the sentinel: {silent:?}"
        ))),
        Err(TestFailure::Timeout(_)) => Ok((heard, rtt)),
        Err(other) => Err(other),
    }
}
