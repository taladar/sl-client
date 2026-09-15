//! Telling the simulator when the **own** avatar's animations run out.
//!
//! The simulator starts and lists an avatar's animations but never plays them,
//! so it cannot know when a non-looping one has finished. The viewer that owns
//! the avatar does, and says so — the reference's `LLAgent::requestStopMotion`,
//! called by the motion controller on the first frame past a motion's
//! send-stop timestamp:
//!
//! 1. An **`AgentAnimation` stop** for the motion, for every self-terminating
//!    motion alike, so the simulator drops it from the set it broadcasts.
//! 2. For the few built-ins that **hold the avatar still** until they end — the
//!    `standup` of a hard landing, and `pre_jump` / `land` / `medium_land`
//!    around a jump — the one-shot **`FINISH_ANIM`** control bit
//!    (`LLAgent::onAnimStop`). A Second Life simulator waits for that bit
//!    before leaving those states — the reference notes that withholding it on
//!    a pre-jump can stall a quick jump. OpenSim ends them on its own timer and
//!    never reads the bit.
//!
//! When a motion runs out is decided by the playback clock
//! (`AnimationPlayback::take_run_out`); this module only turns the report into
//! commands.

use bevy::prelude::*;
use sl_client_bevy::{AnimationKey, Command, ControlFlags, SlCommand, SlIdentity, Uuid};

use crate::animations::{AnimationManager, AnimationPlayback, RunOut};
use crate::world_api::AvatarControls;

/// The built-ins the simulator holds the avatar still for until the viewer
/// reports them finished, by their [`sl_anim`] registry names:
/// `ANIM_AGENT_STANDUP`, `ANIM_AGENT_PRE_JUMP`, `ANIM_AGENT_LAND` and
/// `ANIM_AGENT_MEDIUM_LAND`, the ids `LLAgent::onAnimStop` answers with
/// `AGENT_CONTROL_FINISH_ANIM`.
const HOLDING_ANIMATIONS: [&str; 4] = ["standup", "pre_jump", "land", "medium_land"];

/// The landings among [`HOLDING_ANIMATIONS`]: the ones whose finish is withheld
/// just after a jump input, so it cannot cut short the next jump's pre-jump.
const LANDING_ANIMATIONS: [&str; 2] = ["land", "medium_land"];

/// How long after a jump input a landing's finish is withheld, in seconds — the
/// reference's `RecentJumpThresholdSecs` default.
const RECENT_JUMP_THRESHOLD_SECS: f32 = 1.0;

/// The name of the movement-holding built-in `id` is, if it is one.
fn holding_animation(id: Uuid) -> Option<&'static str> {
    sl_anim::builtin_animation(id)
        .map(|builtin| builtin.name)
        .filter(|name| HOLDING_ANIMATIONS.contains(name))
}

/// Whether the end of the holding animation `name` should release the
/// simulator's movement hold, given whether the ascend key is held now and how
/// long ago the last jump input was.
///
/// `LLAgent::onAnimStop`: the end of `standup` always does. The end of a
/// pre-jump or a landing does unless the ascend key is held — the avatar is
/// jumping again, and the simulator carries that on itself — and the end of a
/// landing also not within [`RECENT_JUMP_THRESHOLD_SECS`] of a jump input, so
/// a rapid second jump's pre-jump is not skipped by the first jump's landing.
fn releases_hold(name: &str, ascend_held: bool, since_jump_input: Option<f32>) -> bool {
    if name == "standup" {
        return true;
    }
    let recent_jump = since_jump_input.is_some_and(|since| since < RECENT_JUMP_THRESHOLD_SECS);
    !ascend_held && !(LANDING_ANIMATIONS.contains(&name) && recent_jump)
}

/// Send the stop, and where it applies the finish, for each of the own
/// avatar's animations that ran out this frame.
///
/// Runs after the animation assets are polled (a motion decoded this frame is
/// checked this frame) and **before** the skeleton driver, whose pruning drops
/// a motion once its ease-out tail has passed: checked after it, a motion first
/// seen past its end — a late decode, a long frame — would be gone before it
/// was ever reported, and its hold never released. Not gated on the world
/// holding the keyboard: a landing ends the same whether or not the chat bar
/// has focus.
pub(crate) fn request_own_motion_stops(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    controls: Res<AvatarControls>,
    manager: Res<AnimationManager>,
    mut playback: ResMut<AnimationPlayback>,
    mut writer: MessageWriter<SlCommand>,
    mut last_jump_input: Local<Option<f32>>,
) {
    let now = time.elapsed_secs();
    let advertised = controls.advertised();
    let ascend_held = advertised.contains(ControlFlags::UP_POS);
    // The reference stamps a jump input whenever the ascend key drives a
    // grounded avatar (`LLAgent::moveUp`); flying ascends are not jumps.
    if ascend_held && !advertised.contains(ControlFlags::FLY) {
        *last_jump_input = Some(now);
    }
    let Some(own) = identity.agent_id else {
        return;
    };
    let since_jump_input = last_jump_input.map(|at| now - at);
    let mut release = false;
    for report in playback.take_run_out(own, now, &manager) {
        let id = match report {
            RunOut::Played(id) => {
                writer.write(SlCommand(Command::StopAnimation(AnimationKey::from(id))));
                id
            }
            // The reference would wait for an animation it could not load for
            // ever, and a stop for a motion nobody played is not ours to send.
            // Only a movement hold is worth breaking that wait for.
            RunOut::Unplayable(id) => {
                let Some(name) = holding_animation(id) else {
                    continue;
                };
                warn!(
                    "own avatar's `{name}` animation has no playable asset; releasing the \
                     simulator's movement hold without playing it"
                );
                id
            }
        };
        if let Some(name) = holding_animation(id)
            && releases_hold(name, ascend_held, since_jump_input)
        {
            debug!("own avatar's `{name}` animation finished; releasing the movement hold");
            release = true;
        }
    }
    if release {
        writer.write(SlCommand(Command::FinishAnimation));
    }
}

#[cfg(test)]
mod tests {
    use super::{HOLDING_ANIMATIONS, holding_animation, releases_hold};
    use pretty_assertions::assert_eq;

    /// Every movement-holding name resolves to a registry built-in, so none of
    /// them silently never matches an id.
    #[test]
    fn every_holding_animation_is_a_builtin() -> Result<(), String> {
        for name in HOLDING_ANIMATIONS {
            let builtin = sl_anim::builtin_animation_by_name(name)
                .ok_or_else(|| format!("`{name}` is not a built-in animation"))?;
            assert_eq!(holding_animation(builtin.id), Some(name));
        }
        let stand = sl_anim::builtin_animation_by_name("stand").ok_or("`stand` built-in")?;
        assert_eq!(holding_animation(stand.id), None);
        Ok(())
    }

    /// A hard landing's recovery always releases the hold, ascend key or not.
    #[test]
    fn standup_always_releases() {
        assert!(releases_hold("standup", false, None));
        assert!(releases_hold("standup", true, Some(0.0)));
    }

    /// A held ascend key withholds the finish of a pre-jump and a landing.
    #[test]
    fn ascend_held_withholds_jump_finishes() {
        for name in ["pre_jump", "land", "medium_land"] {
            assert!(releases_hold(name, false, None), "{name}");
            assert!(!releases_hold(name, true, None), "{name}");
        }
    }

    /// A recent jump input withholds a landing's finish, but not a pre-jump's:
    /// withholding that one would stall a quick tap from standing.
    #[test]
    fn a_recent_jump_withholds_only_landings() {
        assert!(!releases_hold("land", false, Some(0.5)));
        assert!(!releases_hold("medium_land", false, Some(0.5)));
        assert!(releases_hold("pre_jump", false, Some(0.5)));
        assert!(releases_hold("land", false, Some(1.5)));
    }
}
