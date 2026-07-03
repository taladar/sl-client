//! The debug fly-camera: WASD translation (Shift for a speed boost, Space /
//! Ctrl for vertical), mouse-look on a captured cursor.
//!
//! The camera holds its own yaw/pitch so mouse deltas accumulate into a stable
//! orientation independent of the rendered `Transform`. All state is in Bevy's
//! Y-up space; the viewer converts the agent's Second Life login position
//! through [`sl_to_bevy_vec`](crate::coords::sl_to_bevy_vec) before seeding the
//! camera.
//!
//! The cursor is captured via the window's `CursorOptions`, set at plugin
//! configuration time in `main`.
//!
//! The translation math is kept in per-component `f32` (rather than `glam`
//! vector operators) so it stays clear of the workspace `arithmetic_side_effects`
//! restriction lint, which does not apply to plain floating-point arithmetic.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;

/// Base translation speed, in metres per second.
const BASE_SPEED: f32 = 10.0;

/// Multiplier applied to [`BASE_SPEED`] while a Shift key is held.
const FAST_MULTIPLIER: f32 = 4.0;

/// Radians of yaw/pitch per pixel of mouse motion.
const MOUSE_SENSITIVITY: f32 = 0.003;

/// Pitch clamp (just under a quarter turn) so the view never flips over the
/// pole.
const MAX_PITCH: f32 = 1.54;

/// The debug fly-camera: a marker plus its accumulated look angles.
#[derive(Component)]
pub(crate) struct FlyCamera {
    /// Yaw about the Bevy up (`+Y`) axis, in radians.
    yaw: f32,
    /// Pitch about the camera's local right axis, in radians, clamped to
    /// `±MAX_PITCH`.
    pitch: f32,
}

impl Default for FlyCamera {
    /// A level camera looking along `-Z` (Bevy forward).
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// Drive the fly-camera each frame: apply mouse-look, then translate along the
/// resulting orientation from the WASD / Space / Ctrl keys.
pub(crate) fn fly_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let delta = mouse.delta;
    let dt = time.delta_secs();
    for (mut transform, mut camera) in &mut cameras {
        // Mouse-look: yaw from horizontal motion, pitch from vertical, standard
        // (moving the mouse up looks up).
        camera.yaw -= delta.x * MOUSE_SENSITIVITY;
        camera.pitch = (camera.pitch - delta.y * MOUSE_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);

        // Assemble a movement direction from the pressed keys, in the camera's
        // now-current basis, accumulated per-component so the arithmetic stays
        // in plain `f32`. Normalise so diagonals are not faster.
        let forward = *transform.forward();
        let right = *transform.right();
        let mut dx = 0.0_f32;
        let mut dy = 0.0_f32;
        let mut dz = 0.0_f32;
        if keyboard.pressed(KeyCode::KeyW) {
            dx += forward.x;
            dy += forward.y;
            dz += forward.z;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            dx -= forward.x;
            dy -= forward.y;
            dz -= forward.z;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            dx += right.x;
            dy += right.y;
            dz += right.z;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            dx -= right.x;
            dy -= right.y;
            dz -= right.z;
        }
        if keyboard.pressed(KeyCode::Space) {
            dy += 1.0;
        }
        if keyboard.pressed(KeyCode::ControlLeft) {
            dy -= 1.0;
        }
        let length_squared = dx * dx + dy * dy + dz * dz;
        if length_squared > 0.0 {
            let boost = if keyboard.pressed(KeyCode::ShiftLeft) {
                FAST_MULTIPLIER
            } else {
                1.0
            };
            // A single scale folds normalisation (`/ len`) with the frame's
            // speed step, so each axis is one `f32` multiply-add.
            let step = BASE_SPEED * boost * dt / length_squared.sqrt();
            transform.translation.x += dx * step;
            transform.translation.y += dy * step;
            transform.translation.z += dz * step;
        }
    }
}
