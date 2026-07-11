//! Client-side locomotion / state animations for the **own** avatar (P31.6): the
//! built-in walk / run / turn / fly / hover / fall / stand motions an animation
//! overrider (AO) replaces, played from the avatar's movement state as immediate
//! feedback and as a fallback where the simulator does not drive them itself.
//!
//! On the wire these are simulator-authoritative: the sim decides the state
//! animation (e.g. `ANIM_AGENT_WALK`) from the avatar's movement and broadcasts it
//! in an `AvatarAnimation`, which the Phase 18 pipeline already ingests and plays.
//! Two things stop that from being enough for the own avatar on the local grid:
//!
//! 1. **The locomotion built-ins were misclassified** as procedural (no
//!    downloadable asset), so the viewer never fetched them even when the sim
//!    signalled them. That is fixed in [`sl_anim`]'s registry: the reference
//!    viewer's `LLKeyframeWalkMotion` / `LLKeyframeStandMotion` extend
//!    `LLKeyframeMotion`, which downloads the keyframe asset by UUID and only
//!    layers a procedural *adjustment* on top — so walk / run / stand / turn /
//!    crouch are ordinary downloadable `.anim` assets.
//! 2. **OpenSim only broadcasts a presence's animations when it is a root agent**
//!    (`ScenePresenceAnimator` skips child agents). A fresh login to the local
//!    2×2 megaregion lands as a child presence in a login region, so the own
//!    avatar's locomotion is never broadcast back — nothing drives its animation
//!    at all.
//!
//! This module fills that gap: it derives the movement state from the P31.4
//! dead-reckoned velocity ([`AvatarMotion`]) and the P31.5 movement intent
//! ([`AvatarControls`] / the turn keys), maps it to the matching built-in
//! animation, and plays it on the own avatar through a dedicated client-driven
//! slot on [`AnimationPlayback`]. It **defers entirely** to the simulator whenever
//! the sim is driving the avatar (a root presence, or an AO on Second Life), so it
//! only ever fills genuine silence — the two never animate the avatar at once.

use bevy::prelude::*;
use sl_client_bevy::{AssetKey, ControlFlags, SlIdentity};

use crate::animations::{AnimationManager, AnimationPlayback};
use crate::avatars::AvatarState;
use crate::movement::AvatarControls;
use crate::physics::AvatarMotion;

/// Vertical speed (metres/second) beyond which a *flying* avatar counts as
/// ascending / descending rather than hovering.
const VERTICAL_MOVE: f32 = 0.5;

/// Downward speed (metres/second) beyond which a *grounded* (non-flying) avatar
/// counts as falling — well above the gentle vertical drift of walking down a
/// slope, so only a real drop triggers the fall animation.
const FALL_SPEED: f32 = 3.0;

/// The turn intent read from the ← / → keys, for the turn-in-place state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnIntent {
    /// Neither turn key is held (or both are, cancelling out).
    None,
    /// Turning left (`←`).
    Left,
    /// Turning right (`→`).
    Right,
}

/// Map the own avatar's movement state to the short name of the built-in
/// animation that best represents it ([`sl_anim::builtin_animation_by_name`]
/// resolves the name to its fixed asset UUID). Pure, so the state machine is
/// unit-testable without a running app.
///
/// The walk / run / turn / fly-forward states are driven by the avatar's
/// advertised control-flag **intent** rather than its dead-reckoned velocity: the
/// intent clears the instant the user releases the key, whereas the last-reported
/// `AvatarMotion` velocity lingers at walk speed until the simulator sends a
/// corrective zero-velocity update — so keying off velocity leaves the walk
/// animation running after the avatar has stopped. Velocity is consulted only for
/// the two states that have no key: a grounded **fall** and a flying **ascend /
/// descend** confirmation.
///
/// Priority mirrors the reference viewer's `LLAgent` state resolution: flying
/// (with its ascend / descend / hover sub-states) wins over grounded motion, a
/// real fall wins over standing, translation (walk / run) wins over a turn, and
/// standing is the resting default.
#[must_use]
fn locomotion_anim(flags: ControlFlags, vertical_speed: f32, turn: TurnIntent) -> &'static str {
    let translating = flags.contains(ControlFlags::AT_POS) || flags.contains(ControlFlags::AT_NEG);
    if flags.contains(ControlFlags::FLY) {
        if flags.contains(ControlFlags::UP_POS) || vertical_speed > VERTICAL_MOVE {
            return "hover_up";
        }
        if flags.contains(ControlFlags::UP_NEG) || vertical_speed < -VERTICAL_MOVE {
            return "hover_down";
        }
        if translating {
            return "fly";
        }
        return "hover";
    }
    // Grounded. A real fall (no key) wins over intent.
    if vertical_speed < -FALL_SPEED {
        return "falldown";
    }
    if translating {
        if flags.contains(ControlFlags::FAST_AT) {
            return "run";
        }
        return "walk";
    }
    match turn {
        TurnIntent::Left => "turnleft",
        TurnIntent::Right => "turnright",
        TurnIntent::None => "stand",
    }
}

/// Read the ← / → keys into a [`TurnIntent`] (both or neither held ⟹ no turn).
#[must_use]
fn turn_intent(keyboard: &ButtonInput<KeyCode>) -> TurnIntent {
    let left = keyboard.pressed(KeyCode::ArrowLeft);
    let right = keyboard.pressed(KeyCode::ArrowRight);
    match (left, right) {
        (true, false) => TurnIntent::Left,
        (false, true) => TurnIntent::Right,
        _neither_or_both => TurnIntent::None,
    }
}

/// Drive the own avatar's client-side locomotion animation each frame (P31.6):
/// derive the movement state from its dead-reckoned velocity and advertised
/// controls, resolve the matching built-in animation, request its asset, and play
/// it through the client-driven slot on [`AnimationPlayback`].
///
/// It runs only while the own avatar is **rigged** (there is a skeleton to pose)
/// and the **simulator is silent** about it — the moment the sim broadcasts the
/// agent's own animations (a root presence, or an AO on Second Life) the fallback
/// eases its motion out and defers, so it never double-drives the avatar.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading time, keyboard, identity, controls, avatars, motions, and both animation resources"
)]
pub(crate) fn drive_own_locomotion(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    identity: Res<SlIdentity>,
    controls: Res<AvatarControls>,
    avatars: Res<AvatarState>,
    motions: Query<&AvatarMotion>,
    mut manager: ResMut<AnimationManager>,
    mut playback: ResMut<AnimationPlayback>,
    mut last_state: Local<Option<&'static str>>,
) {
    let now = time.elapsed_secs();
    let Some(own) = identity.agent_id else {
        return;
    };
    // Only a rigged own avatar has a skeleton to pose; a placeholder sphere gains
    // nothing, so do not fetch locomotion assets for it.
    if avatars.joint_entities_of(own).is_none() {
        playback.set_client_locomotion(own, None, now);
        log_state(&mut last_state, None);
        return;
    }
    // Defer entirely while the simulator drives the avatar's own animations —
    // unless the debug override forces the client path on (env
    // `SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1`), which lets the fallback be verified on
    // a root presence too (where the sim would otherwise always drive the avatar and
    // hide it). When forced, the client state and the sim's tend to agree, so the
    // pose merge collapses to one animation rather than doubling.
    if !force_client_locomotion() && playback.has_active_sim_animation(own) {
        playback.set_client_locomotion(own, None, now);
        log_state(&mut last_state, Some("<simulator-driven>"));
        return;
    }
    // The own avatar's dead-reckoned vertical speed (for the fall / fly-vertical
    // states; zero when its motion is unknown). The walk / run / turn states come
    // from the control-flag intent, not velocity, so a released key stops them at
    // once — see [`locomotion_anim`].
    let vertical = avatars
        .body_root_of(own)
        .and_then(|anchor| motions.get(anchor).ok())
        .map_or(0.0, AvatarMotion::vertical_speed);
    let name = locomotion_anim(controls.advertised(), vertical, turn_intent(&keyboard));
    let Some(builtin) = sl_anim::builtin_animation_by_name(name) else {
        return;
    };
    // Fetch the asset (idempotent) and play it on the client-driven slot.
    manager.request(AssetKey::from(builtin.id));
    playback.set_client_locomotion(own, Some(builtin.id), now);
    log_state(&mut last_state, Some(name));
}

/// The debug override (env `SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1`) that keeps the
/// client-side locomotion fallback driving even when the simulator is animating the
/// avatar — so the fallback can be exercised and verified on a root presence,
/// without needing to land as an OpenSim child agent.
#[must_use]
fn force_client_locomotion() -> bool {
    std::env::var("SL_VIEWER_FORCE_CLIENT_LOCOMOTION").as_deref() == Ok("1")
}

/// Edge-triggered live diagnostic (env `SL_VIEWER_LOG_LOCOMOTION=1`): log the own
/// avatar's client-locomotion state only when it *changes*, so a live run can
/// confirm the fallback drives the expected state (and, via `<simulator-driven>`,
/// whether the grid is broadcasting the agent's own animations at all — the P31.6
/// investigation) without flooding the log every frame.
fn log_state(last: &mut Local<Option<&'static str>>, state: Option<&'static str>) {
    if **last == state {
        return;
    }
    **last = state;
    if std::env::var("SL_VIEWER_LOG_LOCOMOTION").as_deref() == Ok("1") {
        match state {
            Some(name) => info!("P31.6 own locomotion state -> {name}"),
            None => info!("P31.6 own locomotion inactive (no rigged own avatar)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TurnIntent, locomotion_anim};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::ControlFlags;

    /// A still, grounded avatar with no intent and no turn is standing.
    #[test]
    fn idle_is_stand() {
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), 0.0, TurnIntent::None),
            "stand"
        );
    }

    /// Turning in place (no translation intent) plays the matching turn animation.
    #[test]
    fn turn_in_place() {
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), 0.0, TurnIntent::Left),
            "turnleft"
        );
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), 0.0, TurnIntent::Right),
            "turnright"
        );
    }

    /// A translation *intent* is a walk (a run with the run flag); it wins over a
    /// simultaneous turn. The key regression: with the walk intent dropped, the
    /// state falls straight back to `stand` regardless of any lingering reported
    /// velocity — velocity no longer enters the walk decision at all.
    #[test]
    fn walk_and_run() {
        assert_eq!(
            locomotion_anim(ControlFlags::AT_POS, 0.0, TurnIntent::Left),
            "walk"
        );
        assert_eq!(
            locomotion_anim(ControlFlags::AT_NEG, 0.0, TurnIntent::None),
            "walk"
        );
        assert_eq!(
            locomotion_anim(
                ControlFlags::AT_POS.union(ControlFlags::FAST_AT),
                0.0,
                TurnIntent::None
            ),
            "run"
        );
        // Intent released ⟹ back to standing immediately (the stop-when-you-stop fix).
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), 0.0, TurnIntent::None),
            "stand"
        );
    }

    /// A hard drop while grounded is a fall; a gentle downhill drift is not.
    #[test]
    fn falling() {
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), -5.0, TurnIntent::None),
            "falldown"
        );
        assert_eq!(
            locomotion_anim(ControlFlags::empty(), -1.0, TurnIntent::None),
            "stand"
        );
    }

    /// Flying resolves the hover / ascend / descend / fly sub-states, and a fall
    /// while flying is *not* triggered (the fly branch owns the vertical axis).
    #[test]
    fn flying_states() {
        let fly = ControlFlags::FLY;
        assert_eq!(locomotion_anim(fly, 0.0, TurnIntent::None), "hover");
        // Forward intent while flying is the fly animation.
        assert_eq!(
            locomotion_anim(fly.union(ControlFlags::AT_POS), 0.0, TurnIntent::None),
            "fly"
        );
        assert_eq!(
            locomotion_anim(fly.union(ControlFlags::UP_POS), 0.0, TurnIntent::None),
            "hover_up"
        );
        assert_eq!(locomotion_anim(fly, 2.0, TurnIntent::None), "hover_up");
        assert_eq!(locomotion_anim(fly, -2.0, TurnIntent::None), "hover_down");
        // A steep dive while flying is a descend, never a ground fall.
        assert_eq!(locomotion_anim(fly, -9.0, TurnIntent::None), "hover_down");
    }
}
