//! Keyboard controls that walk / turn / fly the agent's **own** avatar, distinct
//! from the WASD + mouse debug fly-camera (which only moves the viewpoint).
//!
//! Second Life avatar motion is entirely simulator-authoritative: the client does
//! not move the body itself, it advertises *intent* in the `AgentUpdate` message —
//! a set of [`ControlFlags`] (walk forward / back, ascend / descend, fly) plus the
//! body facing the walk direction follows — and the simulator moves the avatar and
//! streams the resulting motion back as `ObjectUpdate`s. Those updates are exactly
//! what the P31.4 avatar dead-reckoner ([`drive_avatar_motion`](crate::physics))
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
//! - **F** — toggle flying ([`ControlFlags::FLY`]).
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
use sl_client_bevy::{Command, ControlFlags, Rotation, SlCommand, SlIdentity};

use crate::avatars::AvatarState;
use crate::physics::AvatarMotion;

/// How fast the ← / → keys turn the avatar's heading, in radians per second
/// (~183°/s — a brisk turn that feels responsive rather than sluggish).
const TURN_RATE_RAD_PER_SEC: f32 = 3.2;

/// The minimum interval, in seconds, between the body-rotation `AgentUpdate`s sent
/// while turning (~20 Hz), so a held turn key does not flood the circuit — the
/// heading still advances every frame client-side, it is just broadcast at this
/// rate.
const ROTATION_SEND_INTERVAL_SECS: f32 = 0.05;

/// The persistent state of the avatar movement controls: the client-tracked walk
/// heading, whether flying is toggled on, and the bookkeeping that keeps the viewer
/// from re-sending an unchanged intent every frame.
#[derive(Resource)]
pub(crate) struct AvatarControls {
    /// The walk heading (yaw about the Second Life up axis, radians) the body faces;
    /// seeded once from the own avatar's reported facing so the first step does not
    /// snap it.
    yaw: f32,
    /// Whether flying is toggled on ([`ControlFlags::FLY`] is advertised).
    flying: bool,
    /// Whether [`yaw`](Self::yaw) has been seeded from the own avatar yet.
    seeded: bool,
    /// Whether the seeded heading has been advertised to the simulator at least
    /// once, so a walk before the first turn moves in the right direction.
    sent_initial_rotation: bool,
    /// The control-flag set last advertised, so a [`Command::SetControls`] is emitted
    /// only when the flags actually change.
    last_controls: ControlFlags,
    /// Seconds accumulated since the last rotation send, for the turning throttle.
    rotation_send_accum: f32,
}

impl Default for AvatarControls {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            flying: false,
            seeded: false,
            sent_initial_rotation: false,
            last_controls: ControlFlags::empty(),
            rotation_send_accum: ROTATION_SEND_INTERVAL_SECS,
        }
    }
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

/// Read the movement keys each frame and advertise the avatar's intent to the
/// simulator: the [`ControlFlags`] for the held walk / fly keys (emitted only when
/// they change) and, while turning, the body rotation the walk direction follows
/// (throttled). The simulator moves the avatar and streams it back for the P31.4
/// dead-reckoner to extrapolate.
pub(crate) fn drive_avatar_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    motions: Query<&AvatarMotion>,
    mut controls: ResMut<AvatarControls>,
    mut writer: MessageWriter<SlCommand>,
) {
    let dt = time.delta_secs();

    // Seed the walk heading from the own avatar's reported facing the first time it
    // is available, so the first step keeps its orientation instead of snapping.
    if !controls.seeded
        && let Some(own) = identity.agent_id
        && let Some(anchor) = avatars.body_root_of(own)
        && let Ok(motion) = motions.get(anchor)
    {
        controls.yaw = motion.yaw();
        controls.seeded = true;
    }

    // F toggles flying.
    if keyboard.just_pressed(KeyCode::KeyF) {
        controls.flying = !controls.flying;
    }

    // Assemble the control-flag set from the currently-held keys (releasing a key
    // simply drops its flag — no explicit stop).
    let mut flags = ControlFlags::empty();
    if controls.flying {
        flags = flags.union(ControlFlags::FLY);
    }
    let forward = keyboard.pressed(KeyCode::ArrowUp);
    let backward = keyboard.pressed(KeyCode::ArrowDown);
    if forward {
        flags = flags.union(ControlFlags::AT_POS);
    }
    if backward {
        flags = flags.union(ControlFlags::AT_NEG);
    }
    let running = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if running && (forward || backward) {
        flags = flags.union(ControlFlags::FAST_AT);
    }
    if keyboard.pressed(KeyCode::PageUp) {
        flags = flags.union(ControlFlags::UP_POS);
    }
    if keyboard.pressed(KeyCode::PageDown) {
        flags = flags.union(ControlFlags::UP_NEG);
    }

    // Turn the heading with the ← / → keys.
    let mut turning = false;
    if keyboard.pressed(KeyCode::ArrowLeft) {
        controls.yaw += TURN_RATE_RAD_PER_SEC * dt;
        turning = true;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        controls.yaw -= TURN_RATE_RAD_PER_SEC * dt;
        turning = true;
    }
    if turning {
        // Keep the heading in a bounded range so a long session cannot accumulate a
        // huge angle (the quaternion is unaffected, but this keeps `yaw` tidy).
        controls.yaw = wrap_angle(controls.yaw);
    }

    // Emit a `SetControls` only when the flag set changes; the simulator holds the
    // last set via its keep-alive re-sends.
    let controls_changed = flags != controls.last_controls;
    if controls_changed {
        writer.write(SlCommand(Command::SetControls(flags)));
        controls.last_controls = flags;
    }

    // Advertise the body facing: once to seed it, when a walk starts (so it moves in
    // the current heading), and throttled while turning.
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
    use super::{rotation_from_yaw, wrap_angle};
    use sl_client_bevy::Rotation;

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
