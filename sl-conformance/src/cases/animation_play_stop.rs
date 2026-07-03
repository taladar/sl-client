//! Play one of the agent's own animations, then stop it — the animation
//! signalling round trip.
//!
//! A viewer starts and stops an avatar's animations with the `AgentAnimation`
//! message: a list of `(anim_id, start)` pairs the simulator folds into that
//! avatar's animation set. The simulator then broadcasts the *complete*
//! authoritative set back to every viewer in view (including the agent's own) as
//! an `AvatarAnimation` — an animation that stops simply drops out of a later
//! update rather than being signalled individually. This case exercises that
//! full loop against a well-known built-in animation:
//!
//! 1. **Play** a built-in gesture animation (`ANIM_AGENT_CLAP`) with
//!    [`Command::PlayAnimation`], then wait for the [`Event::AvatarAnimation`]
//!    for *this* agent whose set now contains that animation. Matching it back to
//!    the agent's own id (not merely that *some* avatar started clapping) and
//!    finding the played id in the authoritative set is the proof the play took
//!    effect; its per-avatar `sequence_id` (which the simulator bumps on each
//!    (re)start) is recorded.
//! 2. **Stop** it with [`Command::StopAnimation`], then wait for the next
//!    [`Event::AvatarAnimation`] for this agent whose set no longer contains the
//!    played id — the drop-out that signals the stop. Because [`Session::wait_for`]
//!    consumes events in order, the channel position is already past the play
//!    confirmation, so an "absent" set here is genuinely post-stop and not a stale
//!    pre-play baseline.
//!
//! `1av`, `[both]`. Self-contained on either grid: no inventory, permissions or
//! peer avatar is needed — the agent drives its own animation set and observes
//! its own broadcast. On OpenSim the avatar is forced into the "Default Region"
//! so it is a root (not child) presence — `ScenePresenceAnimator` refuses to add
//! or broadcast animations for a child agent — where `AddAnimation` accepts an
//! arbitrary animation UUID and `SendAnimPack` echoes the set back to the agent's
//! own client. On Second Life (aditi) central simulators signal the same way;
//! nearby avatars' own `AvatarAnimation` updates are filtered out by matching the
//! event's avatar id to this agent.

use std::time::Instant;

use sl_client_tokio::{AgentKey, AnimationKey, Command, Event, PlayingAnimation};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, fixtures, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, so the
/// agent is a root presence there (the simulator will not add or broadcast
/// animations for a child agent). On Second Life the avatar keeps `"last"`.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The built-in animation to play and stop: `ANIM_AGENT_CLAP`, a gesture
/// animation from the reference viewer's `llanimationstates` table. A gesture
/// (rather than a locomotion/posture default such as `STAND`) is chosen because
/// it is not part of the avatar's baseline set, so its appearance in — and later
/// disappearance from — the authoritative `AvatarAnimation` set is an unambiguous
/// signal of the play and the stop.
const CLAP_ANIMATION: &str = "9b0c1c4e-8ac7-7969-1494-28c874c4f668";

/// Plays one of the agent's own animations and stops it again, verifying each
/// step by the simulator's authoritative `AvatarAnimation` broadcast for this
/// agent.
#[derive(Debug)]
pub struct AnimationPlayStop;

impl GridTest for AnimationPlayStop {
    fn name(&self) -> &'static str {
        "animation-play-stop"
    }

    fn description(&self) -> &'static str {
        "Play one of the agent's own animations, then stop it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let anim_id = AnimationKey::from(fixtures::uuid(CLAP_ANIMATION)?);

            // The agent's own id — the animation broadcast we care about is the
            // one describing this avatar's set, filtered out from any nearby
            // avatars also animating.
            let me = ctx
                .primary()
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("no agent id after login".to_owned()))?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // 1. Play the animation and wait for the authoritative set for this
            //    agent to include it.
            let play_started = Instant::now();
            session.send(Command::PlayAnimation(anim_id)).await?;
            let playing = wait_for_self_set(session, me, |animations| {
                contains(animations, anim_id)
            })
            .await?
            .ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "no AvatarAnimation for this agent listed the played animation {anim_id} \
                     after PlayAnimation"
                ))
            })?;
            let play_rtt = play_started.elapsed();
            let played = playing
                .iter()
                .find(|animation| animation.anim_id == anim_id.uuid())
                .ok_or_else(|| {
                    TestFailure::Assertion("played animation vanished from its own set".to_owned())
                })?;
            let played_sequence = played.sequence_id;
            let animations_while_playing = playing.len();
            check(
                played_sequence > 0,
                "the played animation carried a non-positive sequence id",
            )?;

            // 2. Stop the animation and wait for the next authoritative set for
            //    this agent that no longer includes it (the drop-out).
            let stop_started = Instant::now();
            session.send(Command::StopAnimation(anim_id)).await?;
            let stopped = wait_for_self_set(session, me, |animations| {
                !contains(animations, anim_id)
            })
            .await?
            .ok_or_else(|| {
                TestFailure::Assertion(format!(
                    "no AvatarAnimation for this agent dropped the animation {anim_id} after \
                     StopAnimation"
                ))
            })?;
            let stop_rtt = stop_started.elapsed();
            let animations_after_stop = stopped.len();

            let metrics = ctx.metrics();
            metrics.set("animation_id", anim_id.to_string());
            metrics.set("played_sequence_id", i64::from(played_sequence));
            metrics.set(
                "animations_while_playing",
                i64::try_from(animations_while_playing).unwrap_or(i64::MAX),
            );
            metrics.set(
                "animations_after_stop",
                i64::try_from(animations_after_stop).unwrap_or(i64::MAX),
            );
            metrics.set_timing(&secs_metric("play_rtt"), play_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("stop_rtt"), stop_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Whether an authoritative animation set lists `anim_id`.
fn contains(animations: &[PlayingAnimation], anim_id: AnimationKey) -> bool {
    animations
        .iter()
        .any(|animation| animation.anim_id == anim_id.uuid())
}

/// Waits for the next [`Event::AvatarAnimation`] describing this agent (`me`)
/// whose animation set satisfies `accept`, returning that set. Returns `None` if
/// none arrives within [`REPLY_TIMEOUT`]; other avatars' updates are ignored.
async fn wait_for_self_set(
    session: &mut Session,
    me: AgentKey,
    mut accept: impl FnMut(&[PlayingAnimation]) -> bool,
) -> Result<Option<Vec<PlayingAnimation>>, TestFailure> {
    match session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } if *avatar_id == me && accept(animations) => Some(animations.clone()),
            _ => None,
        })
        .await
    {
        Ok(animations) => Ok(Some(animations)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}
