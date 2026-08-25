//! Keyboard controls that walk / turn / fly the agent's **own** avatar, distinct
//! from the WASD + mouse debug fly-camera (which only moves the viewpoint).
//!
//! Second Life avatar motion is entirely simulator-authoritative: the client does
//! not move the body itself, it advertises *intent* in the `AgentUpdate` message —
//! a set of [`ControlFlags`] (walk forward / back, ascend / descend, fly) plus the
//! body facing the walk direction follows — and the simulator moves the avatar and
//! streams the resulting motion back as `ObjectUpdate`s. Those updates are exactly
//! what the P31.4 avatar dead-reckoner (`drive_avatar_motion`(crate::physics))
//! extrapolates between, so driving the own avatar with these controls is also how
//! that path is exercised live.
//!
//! The controls live on keys the fly-camera does not use, so both work at once with
//! no mode switch:
//!
//! - **↑ / ↓** — walk forward / back ([`ControlFlags::AT_POS`] / [`AT_NEG`]).
//! - **← / →** — turn the body left / right (client-tracked heading, sent as the
//!   `AgentUpdate` body rotation the walk direction follows).
//! - **PageUp / PageDown** — ascend / descend ([`UP_POS`] / [`UP_NEG`], while flying).
//!   Holding **PageUp** while standing on the ground also *starts* flying (P31.16),
//!   once held past a short threshold and if the region / parcel permit it — a quick
//!   tap does not, matching the reference viewer's hold-to-fly.
//! - **F** — toggle flying ([`ControlFlags::FLY`]). Flight also stops itself on
//!   landing (P31.11): descending onto the ground with no ascend key held drops
//!   the fly intent so the avatar stands rather than hovering; **F** takes off again.
//! - **Shift + ↑ / ↓** — run ([`ControlFlags::FAST_AT`]).
//!
//! There is no stop key: the control flags are recomputed from the currently-held
//! keys every frame, so releasing a key drops its flag and the avatar stops.
//!
//! Because the simulator's keep-alive loop re-sends the last advertised controls
//! automatically, the viewer only emits a command when the intent *changes* — a
//! [`Command::SetControls`] when the flag set changes and a [`Command::SetRotation`]
//! (throttled) while turning — rather than every frame.
//!
//! [`AT_NEG`]: ControlFlags::AT_NEG
//! [`UP_POS`]: ControlFlags::UP_POS
//! [`UP_NEG`]: ControlFlags::UP_NEG

use bevy::prelude::*;
use sl_client_bevy::{Command, ControlFlags, Rotation, SlAgentParcel, SlCommand, SlIdentity};

use crate::camera::CameraAim;
use crate::input_action::Action;
use crate::spacenav::{AvatarAxisSettings, AvatarNavSmoothing, SpacenavInput, avatar_nav_drive};

use crate::world_api::AvatarState;
use crate::world_api::TerrainState;
use crate::world_api::{
    AvatarControls, AvatarMotion, CameraMode, DoubleTapRun, ROTATION_SEND_INTERVAL_SECS,
};

/// How fast the ← / → keys turn the avatar's heading, in radians per second
/// (~183°/s — a brisk turn that feels responsive rather than sluggish).
const TURN_RATE_RAD_PER_SEC: f32 = 3.2;

/// The slack (metres) above the stricter avatar ground floor still counted as
/// "on / very close to the ground" for the P31.11 auto-stop-flying-on-landing
/// rule — a small margin so flight ends as the avatar settles onto the surface
/// rather than only once its reported position reaches the floor exactly.
const LANDING_HEIGHT_MARGIN_M: f32 = 0.5;

/// The vertical speed (metres/second, negative = downward) below which the avatar
/// counts as descending for the P31.11 landing check when no descend key is held.
/// A tiny negative threshold (rather than `< 0.0`) ignores dead-reckoning jitter so
/// level low-altitude flight is not mistaken for a descent onto the ground.
const LANDING_DESCENT_SPEED_MPS: f32 = -0.1;

/// How long (seconds) the ascend key must be held while standing before flight
/// auto-engages (P31.16), matching the reference viewer's `FLY_TIME` — a quick tap
/// is a jump / hop, a sustained hold takes off. It also debounces the take-off
/// from the P31.11 auto-land, so a landing does not instantly re-launch.
const TAKE_OFF_HOLD_SECS: f32 = 0.5;

/// The window (seconds) within which a second tap of the same walk key counts as
/// a double-tap for tap-tap-hold-to-run. Deliberately its own constant — the
/// *mouse* double-click interval has its own consolidation task
/// (`viewer-consolidate-double-click-interval`) and this is a keyboard gesture.
const DOUBLE_TAP_RUN_WINDOW_SECS: f32 = 0.3;

/// The user-tunable movement parameters, refreshed every frame from the typed
/// settings store by the camera & movement preferences tab
/// (`crate::preferences_camera_move`). The defaults reproduce the module
/// constants / established behaviour, so a run without a settings store (the
/// gallery, headless tests) behaves as before — except
/// [`allow_tap_tap_hold_run`](Self::allow_tap_tap_hold_run), a new gesture that
/// ships enabled to match the reference default.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MovementTuning {
    /// How fast the ← / → keys turn the avatar's heading, radians per second —
    /// replaces the fixed `TURN_RATE_RAD_PER_SEC`.
    pub turn_rate_rad_per_sec: f32,
    /// Whether double-tapping and holding a walk key runs — the reference's
    /// `AllowTapTapHoldRun`.
    pub allow_tap_tap_hold_run: bool,
    /// Whether holding the ascend key while standing auto-engages flight
    /// (P31.16) — the reference's `AutomaticFly`. Off, only the explicit fly
    /// toggle starts flying; auto-land (P31.11) stays on either way, as the
    /// reference does.
    pub automatic_fly: bool,
}

impl Default for MovementTuning {
    /// Today's constants / behaviour (tap-tap-hold-run enabled, the reference
    /// default).
    fn default() -> Self {
        Self {
            turn_rate_rad_per_sec: TURN_RATE_RAD_PER_SEC,
            allow_tap_tap_hold_run: true,
            automatic_fly: true,
        }
    }
}

/// Advance one walk key's tap-tap-hold-to-run state by a frame and return
/// whether it is running: a fresh press within
/// [`DOUBLE_TAP_RUN_WINDOW_SECS`] of the previous one latches the run, and the
/// latch holds until the key is released. Pure, so the gesture is
/// unit-testable frame by frame.
fn double_tap_run(state: &mut DoubleTapRun, just_pressed: bool, held: bool, dt: f32) -> bool {
    if !held {
        state.latched = false;
    }
    if just_pressed {
        if state.since_last_tap <= DOUBLE_TAP_RUN_WINDOW_SECS {
            state.latched = true;
        }
        state.since_last_tap = 0.0;
    } else {
        state.since_last_tap += dt;
    }
    state.latched && held
}

/// A Second Life body [`Rotation`] for a heading `yaw` (radians about the up axis):
/// a unit quaternion turning about Second Life's Z.
#[must_use]
fn rotation_from_yaw(yaw: f32) -> Rotation {
    let (sin, cos) = (yaw * 0.5).sin_cos();
    Rotation {
        x: 0.0,
        y: 0.0,
        z: sin,
        s: cos,
    }
}

/// Read the movement **actions** ([`crate::input_action`]) each frame and advertise
/// the avatar's intent to the simulator: the [`ControlFlags`] for the held walk /
/// fly actions (emitted only when they change) and, while turning, the body
/// rotation the walk direction follows (throttled). The simulator moves the avatar
/// and streams it back for the P31.4 dead-reckoner to extrapolate.
///
/// The actions mean different things by camera mode, deliberately close to the
/// reference so scripted vehicles behave:
///
/// - **Flycam** — the actions drive the *camera* not the avatar, so this stops the
///   body (once) and bows out.
/// - **Seated on a vehicle** — left / right (or the SpaceNavigator's twist) send the
///   **yaw** control bits that *steer the vehicle*, never turning the avatar body,
///   while forward / back and up / down send their control bits unchanged — so the
///   device drives a vehicle through the very same `AgentUpdate` a script sees from
///   the keys. Crucially this holds through a region / corner crossing: our session
///   keeps the seat across the border (unlike the reference, whose transient unseat
///   there is what flips the keys back to avatar-turn and orbits the camera), so
///   [`SlAgentParcel::seated_on`] stays set and the steering never reverts
///   mid-crossing.
/// - **Mouselook** — the mouse turns the body (its heading follows
///   [`CameraAim`]), so left / right *strafe* instead.
/// - **Third person** — left / right turn the avatar heading, the classic default.
///
/// The **SpaceNavigator** ([`crate::spacenav`]) composes with the keyboard here the
/// way the reference `LLViewerJoystick::moveAvatar` composes with the keys: its
/// forward axis walks (either source moving the avatar, neither blocking the other),
/// its up axis flies up / down exactly as PageUp / PageDown do, and its twist turns
/// the body. It only drives the avatar when flycam is off (in flycam it drives the
/// camera instead, via `crate::camera::drive_flycam`).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading time, the actions, the camera mode / aim, the user movement \
              tuning, identity, avatars, terrain, the fly permission + seat, the SpaceNavigator \
              input + settings + smoothing, and the avatar motions plus the controls state and \
              command writer"
)]
pub fn drive_avatar_controls(
    actions: Res<ButtonInput<Action>>,
    mode: Res<CameraMode>,
    camera_aim: Res<CameraAim>,
    tuning: Res<MovementTuning>,
    time: Res<Time>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    terrain: Res<TerrainState>,
    agent: Res<SlAgentParcel>,
    spacenav: Res<SpacenavInput>,
    avatar_axes: Res<AvatarAxisSettings>,
    mut nav_smoothing: ResMut<AvatarNavSmoothing>,
    motions: Query<&AvatarMotion>,
    presence: Option<Res<crate::world_api::PresenceState>>,
    mut controls: ResMut<AvatarControls>,
    mut writer: MessageWriter<SlCommand>,
) {
    // The away bit rides along with the movement bits: the reference keeps
    // `AGENT_CONTROL_AWAY` in the same control word across its per-frame reset
    // (`LLAgent::resetControlFlags`), so it is folded in here rather than sent
    // by a second writer that would fight this one for the field.
    let away_bit = if presence.is_some_and(|presence| presence.is_away()) {
        ControlFlags::AWAY
    } else {
        ControlFlags::empty()
    };
    // The reference clamps the frame time so a big frame-rate drop does not make a
    // huge feathered turn jump.
    let dt = time.delta_secs().min(0.2);

    // In flycam the movement actions drive the camera (`crate::camera::drive_flycam`),
    // not the avatar. Park the body: drop every *movement* bit so it stops walking /
    // ascending, but keep the `FLY` flag if it was flying — clearing it would make
    // the simulator land the avatar, so a hovering avatar would plummet the moment
    // the camera switched to flycam. With `FLY` set and no motion bits the avatar
    // just hovers in place, which is what a spectator flycam wants.
    if *mode == CameraMode::Flycam {
        let parked = away_bit.union(if controls.flying {
            ControlFlags::FLY
        } else {
            ControlFlags::empty()
        });
        if controls.last_controls != parked {
            writer.write(SlCommand(Command::SetControls(parked)));
            controls.last_controls = parked;
        }
        return;
    }

    // The SpaceNavigator's walk / fly / turn contribution this frame (empty when no
    // device is connected — the axes are zero), composed with the keyboard below.
    let nav = avatar_nav_drive(&spacenav, &avatar_axes, &mut nav_smoothing, dt);

    let seated = agent.seated_on.is_some();
    let mouselook = *mode == CameraMode::Mouselook;

    // The own avatar's authoritative motion (facing, vertical speed, ground floor),
    // used to seed the walk heading and to auto-stop flying on landing.
    let own_motion = identity
        .agent_id
        .and_then(|own| avatars.body_root_of(own))
        .and_then(|anchor| motions.get(anchor).ok());

    // Seed the walk heading from the own avatar's reported facing the first time it
    // is available, so the first step keeps its orientation instead of snapping.
    if !controls.seeded
        && let Some(motion) = own_motion
    {
        controls.yaw = motion.yaw();
        controls.seeded = true;
    }

    // Ascend / descend from PageUp / PageDown or the SpaceNavigator's up axis (a
    // lift ascends, a press descends — the same intent as the keys), so the two
    // sources compose and either flies the avatar up or down.
    let ascend = actions.pressed(Action::MoveUp) || nav.vertical > 0;
    let descend = actions.pressed(Action::MoveDown) || nav.vertical < 0;

    // Flight is an avatar concern, not a vehicle one: skip the whole fly toggle /
    // take-off / auto-land machinery while seated (the vehicle owns vertical motion
    // via the up/down control bits below).
    if !seated {
        // The fly action toggles flying.
        if actions.just_pressed(Action::ToggleFly) {
            controls.flying = !controls.flying;
        }

        // Auto-take-off (P31.16): holding ascend while standing engages flight once
        // held past the threshold, if flying is permitted here. A quick tap does not
        // (that is a jump); the hold also keeps the P31.11 auto-land from firing.
        if !controls.flying && ascend {
            controls.ascend_hold_secs += dt;
        } else {
            controls.ascend_hold_secs = 0.0;
        }
        let grounded = own_motion.is_none_or(|motion| {
            crate::physics::avatar_at_ground_floor(motion, &terrain, LANDING_HEIGHT_MARGIN_M)
        });
        if should_take_off(
            tuning.automatic_fly,
            controls.flying,
            grounded,
            controls.ascend_hold_secs,
            agent.can_fly,
        ) {
            controls.flying = true;
            controls.ascend_hold_secs = 0.0;
        }

        // Auto-stop flying on landing (P31.11): descending onto the ground with no
        // ascend held drops the fly intent so the avatar stands rather than hovering.
        if let Some(motion) = own_motion
            && should_auto_stop_flying(
                controls.flying,
                ascend,
                descend,
                motion.vertical_speed(),
                crate::physics::avatar_at_ground_floor(motion, &terrain, LANDING_HEIGHT_MARGIN_M),
            )
        {
            controls.flying = false;
        }
    } else {
        // A seated avatar is never flying; keep the state tidy so standing up later
        // starts from a clean slate.
        controls.flying = false;
        controls.ascend_hold_secs = 0.0;
    }

    // Assemble the control-flag set from the currently-held actions (releasing an
    // action simply drops its flag — no explicit stop).
    let mut flags = ControlFlags::empty();
    if controls.flying {
        flags = flags.union(ControlFlags::FLY);
    }
    // Walk forward / back from the keys or the SpaceNavigator's forward axis (push
    // walks forward, pull back walks back).
    let forward = actions.pressed(Action::MoveForward) || nav.forward > 0;
    let backward = actions.pressed(Action::MoveBackward) || nav.forward < 0;
    if forward {
        flags = flags.union(ControlFlags::AT_POS);
    }
    if backward {
        flags = flags.union(ControlFlags::AT_NEG);
    }
    // Tap-tap-hold-to-run: double-tapping and holding a walk key latches a run
    // for as long as it stays held. The detectors advance every frame (keeping
    // their state fresh) and the preference gates only whether the latch counts.
    let tap_forward = double_tap_run(
        &mut controls.tap_run_forward,
        actions.just_pressed(Action::MoveForward),
        actions.pressed(Action::MoveForward),
        dt,
    );
    let tap_backward = double_tap_run(
        &mut controls.tap_run_backward,
        actions.just_pressed(Action::MoveBackward),
        actions.pressed(Action::MoveBackward),
        dt,
    );
    let tap_run = tuning.allow_tap_tap_hold_run && (tap_forward || tap_backward);
    // Run from Shift, a forward push past the SpaceNavigator run threshold, or a
    // latched double-tap.
    if (actions.pressed(Action::Run) || nav.run || tap_run) && (forward || backward) {
        flags = flags.union(ControlFlags::FAST_AT);
    }
    if ascend {
        flags = flags.union(ControlFlags::UP_POS);
    }
    if descend {
        flags = flags.union(ControlFlags::UP_NEG);
    }

    // Left / right: steer a vehicle, strafe in mouselook, or turn the avatar in
    // third person. `turning` marks that a body rotation should be advertised.
    let left = actions.pressed(Action::MoveLeft);
    let right = actions.pressed(Action::MoveRight);
    let mut turning = false;
    if seated {
        // Steer the vehicle with the yaw bits; never turn the avatar body (which is
        // what the reference bug does after a laggy crossing). The SpaceNavigator's
        // twist steers it the same way the keys do (a positive twist turns left, like
        // MoveLeft), so a vehicle can be flown with the device — the forward / back
        // and up / down bits already come through the uniform flags above, so the
        // whole device drives the same control message a script sees from the keys.
        if left || nav.yaw_delta > 0.0 {
            flags = flags.union(ControlFlags::YAW_POS);
        }
        if right || nav.yaw_delta < 0.0 {
            flags = flags.union(ControlFlags::YAW_NEG);
        }
    } else if mouselook {
        // The mouse turns the body (heading follows the camera aim); left / right
        // strafe.
        if left {
            flags = flags.union(ControlFlags::LEFT_POS);
        }
        if right {
            flags = flags.union(ControlFlags::LEFT_NEG);
        }
        controls.yaw = camera_aim.sl_yaw;
        turning = true;
    } else {
        if left {
            controls.yaw += tuning.turn_rate_rad_per_sec * dt;
            turning = true;
        }
        if right {
            controls.yaw -= tuning.turn_rate_rad_per_sec * dt;
            turning = true;
        }
        // The SpaceNavigator's twist turns the body too (feathered per frame); it
        // composes with the keys and, like them, only turns in third person (in
        // mouselook the camera aim owns the heading, seated the vehicle does).
        if nav.yaw_delta != 0.0 {
            controls.yaw += nav.yaw_delta;
            turning = true;
        }
    }
    if turning {
        // Keep the heading bounded so a long session cannot accumulate a huge angle.
        controls.yaw = wrap_angle(controls.yaw);
    }

    // Emit a `SetControls` only when the flag set changes; the simulator holds the
    // last set via its keep-alive re-sends.
    let flags = flags.union(away_bit);
    let controls_changed = flags != controls.last_controls;
    if controls_changed {
        writer.write(SlCommand(Command::SetControls(flags)));
        controls.last_controls = flags;
    }

    // Advertise the body facing — but never while seated: the vehicle owns the
    // avatar's orientation, and sending a body rotation would fight it (the other
    // half of the reference's arrow-key-orbits-the-vehicle bug).
    if !seated {
        controls.rotation_send_accum += dt;
        let starting_walk = controls_changed
            && (flags.contains(ControlFlags::AT_POS) || flags.contains(ControlFlags::AT_NEG));
        let send_rotation = controls.seeded
            && (!controls.sent_initial_rotation
                || starting_walk
                || (turning && controls.rotation_send_accum >= ROTATION_SEND_INTERVAL_SECS));
        if send_rotation {
            let body = rotation_from_yaw(controls.yaw);
            writer.write(SlCommand(Command::SetRotation {
                body: body.clone(),
                head: body,
            }));
            controls.sent_initial_rotation = true;
            controls.rotation_send_accum = 0.0;
        }
    }
}

/// Whether the auto-stop-flying-on-landing rule (P31.11) fires this frame: the
/// avatar is `flying`, is not being held aloft (`ascend_key`, i.e. PageUp), is
/// descending (`descend_key` / PageDown held, or moving downward faster than
/// [`LANDING_DESCENT_SPEED_MPS`]), and is `at_ground_floor` (on / very close to the
/// ground). Requiring a descent — not merely the absence of lift — means pressing
/// **F** to take off from the ground does not immediately re-land the avatar. Pure
/// so the decision is unit-testable without a live terrain / avatar.
#[must_use]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the landing decision is a conjunction of independent binary conditions — flying, ascend / descend key held, and at the ground floor — that read clearest as the flags they are"
)]
fn should_auto_stop_flying(
    flying: bool,
    ascend_key: bool,
    descend_key: bool,
    vertical_speed: f32,
    at_ground_floor: bool,
) -> bool {
    let descending = descend_key || vertical_speed < LANDING_DESCENT_SPEED_MPS;
    flying && !ascend_key && descending && at_ground_floor
}

/// Whether the auto-take-off rule (P31.16) fires this frame: the feature is on
/// (`automatic_fly`, the user preference), the avatar is not already `flying`,
/// is `grounded` (standing on the ground), flying is permitted here (`can_fly`
/// — the region + parcel decision from the session), and the ascend key has
/// been held for at least [`TAKE_OFF_HOLD_SECS`]. The hold requirement is what
/// makes a quick tap a jump but a sustained press a take-off, and debounces it
/// from the P31.11 auto-land. Pure so the decision is unit-testable without a
/// live terrain / avatar.
#[must_use]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the take-off decision is a conjunction of independent binary conditions — the \
              preference, flying, grounded, and fly permission — that read clearest as the flags \
              they are"
)]
fn should_take_off(
    automatic_fly: bool,
    flying: bool,
    grounded: bool,
    ascend_held_secs: f32,
    can_fly: bool,
) -> bool {
    automatic_fly && !flying && grounded && can_fly && ascend_held_secs >= TAKE_OFF_HOLD_SECS
}

/// Wrap an angle (radians) into `(-π, π]`, keeping the tracked heading bounded over
/// a long session.
#[must_use]
fn wrap_angle(angle: f32) -> f32 {
    let two_pi = core::f32::consts::TAU;
    let wrapped = angle.rem_euclid(two_pi);
    if wrapped > core::f32::consts::PI {
        wrapped - two_pi
    } else {
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LANDING_DESCENT_SPEED_MPS, TAKE_OFF_HOLD_SECS, rotation_from_yaw, should_auto_stop_flying,
        should_take_off, wrap_angle,
    };
    use sl_client_bevy::Rotation;

    /// Holding the ascend key past the threshold while standing with fly permission
    /// takes off; a short hold, being airborne, no permission, already flying, or
    /// the preference switched off does not.
    #[test]
    fn auto_take_off_needs_a_sustained_grounded_permitted_ascend() {
        // Enabled, not flying, grounded, permitted, held past the threshold → take
        // off.
        assert!(should_take_off(true, false, true, TAKE_OFF_HOLD_SECS, true));
        assert!(should_take_off(
            true,
            false,
            true,
            TAKE_OFF_HOLD_SECS + 1.0,
            true
        ));

        // Held, but not long enough yet → keep standing (a tap is a jump).
        assert!(!should_take_off(
            true,
            false,
            true,
            TAKE_OFF_HOLD_SECS - 0.01,
            true
        ));
        // Flying disallowed here (region / parcel) → no take-off.
        assert!(!should_take_off(
            true,
            false,
            true,
            TAKE_OFF_HOLD_SECS,
            false
        ));
        // Already flying → nothing to start.
        assert!(!should_take_off(true, true, true, TAKE_OFF_HOLD_SECS, true));
        // Airborne (not standing) → the hold-to-fly is a standing gesture.
        assert!(!should_take_off(
            true,
            false,
            false,
            TAKE_OFF_HOLD_SECS,
            true
        ));
        // The `AutomaticFly` preference off → an arbitrarily long hold never takes
        // off (only the explicit fly toggle starts flight).
        assert!(!should_take_off(
            false,
            false,
            true,
            TAKE_OFF_HOLD_SECS + 60.0,
            true
        ));
    }

    /// The tap-tap-hold-to-run gesture: a second tap within the window latches a
    /// run for as long as the key stays held; a slow second tap is just walking,
    /// and releasing the key drops the latch.
    #[test]
    fn double_tap_run_latches_within_the_window_only() {
        use super::{DOUBLE_TAP_RUN_WINDOW_SECS, DoubleTapRun, double_tap_run};

        let dt = 1.0 / 60.0;

        // Tap, release briefly, tap again inside the window and hold → running,
        // and it stays latched while held.
        let mut state = DoubleTapRun::default();
        assert!(
            !double_tap_run(&mut state, true, true, dt),
            "first tap walks"
        );
        assert!(!double_tap_run(&mut state, false, false, dt), "released");
        assert!(
            double_tap_run(&mut state, true, true, dt),
            "second tap within the window runs"
        );
        assert!(
            double_tap_run(&mut state, false, true, dt),
            "held: still running"
        );
        // Releasing drops the latch; holding again (no fresh double-tap) walks.
        assert!(
            !double_tap_run(&mut state, false, false, dt),
            "released: latch drops"
        );

        // A second tap *after* the window is just another first tap.
        let mut slow = DoubleTapRun::default();
        assert!(!double_tap_run(&mut slow, true, true, dt));
        assert!(!double_tap_run(&mut slow, false, false, dt));
        // Let more than the window elapse.
        let mut elapsed = 0.0;
        while elapsed <= DOUBLE_TAP_RUN_WINDOW_SECS {
            assert!(!double_tap_run(&mut slow, false, false, dt));
            elapsed += dt;
        }
        assert!(
            !double_tap_run(&mut slow, true, true, dt),
            "a slow second tap walks"
        );

        // The very first tap of a session can never pair with "before the session".
        let mut fresh = DoubleTapRun::default();
        assert!(!double_tap_run(&mut fresh, true, true, dt));
    }

    /// Descending onto the ground with no ascend key held stops flight; the same
    /// situation while ascending, while airborne, or while not flying does not.
    #[test]
    fn auto_stop_flying_only_on_a_grounded_descent() {
        // Flying, descending (downward speed past the threshold), at the ground,
        // ascend key up → land.
        assert!(should_auto_stop_flying(true, false, false, -1.0, true));
        // The descend key counts as descending even with no downward speed reported.
        assert!(should_auto_stop_flying(true, false, true, 0.0, true));

        // Not flying → nothing to stop.
        assert!(!should_auto_stop_flying(false, false, false, -1.0, true));
        // Holding the ascend key keeps the avatar aloft even at the ground floor,
        // so pressing F to take off is not immediately undone.
        assert!(!should_auto_stop_flying(true, true, false, -1.0, true));
        // Level / rising flight near the ground (no descent) keeps flying.
        assert!(!should_auto_stop_flying(true, false, false, 0.0, true));
        assert!(!should_auto_stop_flying(true, false, false, 5.0, true));
        // Descending but still high above the ground keeps flying.
        assert!(!should_auto_stop_flying(true, false, false, -5.0, false));
        // A downward drift slower than the threshold is jitter, not a landing.
        assert!(!should_auto_stop_flying(
            true,
            false,
            false,
            LANDING_DESCENT_SPEED_MPS + 0.01,
            true
        ));
    }

    /// A zero heading is the identity rotation; a quarter turn about the up axis is a
    /// unit quaternion with the expected Z / W components.
    #[test]
    fn rotation_from_yaw_builds_a_z_axis_turn() {
        let Rotation { x, y, z, s } = rotation_from_yaw(0.0);
        assert!(x.abs() < 1.0e-6 && y.abs() < 1.0e-6 && z.abs() < 1.0e-6);
        assert!((s - 1.0).abs() < 1.0e-6);

        let quarter = core::f32::consts::FRAC_PI_2;
        let turned = rotation_from_yaw(quarter);
        let expected = (quarter * 0.5).sin();
        assert!((turned.z - expected).abs() < 1.0e-6);
        assert!((turned.s - expected).abs() < 1.0e-6);
        // A unit quaternion.
        let norm_sq =
            turned.x * turned.x + turned.y * turned.y + turned.z * turned.z + turned.s * turned.s;
        assert!((norm_sq - 1.0).abs() < 1.0e-6);
    }

    /// Angles past ±π wrap back into `(-π, π]`.
    #[test]
    fn wrap_angle_bounds_the_heading() {
        let pi = core::f32::consts::PI;
        assert!((wrap_angle(0.0)).abs() < 1.0e-6);
        assert!((wrap_angle(pi) - pi).abs() < 1.0e-4);
        // 3π wraps to π.
        assert!((wrap_angle(3.0 * pi) - pi).abs() < 1.0e-4);
        // -3π/2 wraps to +π/2.
        assert!((wrap_angle(-1.5 * pi) - 0.5 * pi).abs() < 1.0e-4);
    }
}
