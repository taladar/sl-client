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

/// Which of the camera's own axes an [auto-rotation](CameraSpin) spins about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SpinAxis {
    /// About the vertical (Bevy `+Y`) axis — pans left/right (the natural survey
    /// spin; rotates freely).
    #[default]
    Yaw,
    /// About the camera's local right axis — tilts up/down (clamped to the same
    /// `±MAX_PITCH` the mouse-look uses, so it sweeps rather than loops).
    Pitch,
    /// About the camera's local forward axis — rolls the horizon (rotates freely).
    Roll,
}

/// A debug affordance: auto-rotate the camera at a fixed rate about one of its
/// axes, for unattended screenshot sequences that pan across a scene (e.g. to
/// inspect the water / underwater-fog artifacts of R21 around a region edge).
/// Inserted from the `--camera-spin` / `--camera-spin-axis` options; a zero
/// [`rate`](Self::rate) disables it.
#[derive(Resource, Default)]
pub(crate) struct CameraSpin {
    /// Radians per second to auto-rotate; `0.0` disables the spin.
    pub(crate) rate: f32,
    /// Which of the camera's axes the spin rotates about.
    pub(crate) axis: SpinAxis,
}

/// A debug affordance: place the fly-camera at an absolute pose on startup
/// instead of snapping it to the agent on login, so an unattended screenshot
/// capture can frame a specific viewpoint. Inserted from the `--camera-position`
/// / `--camera-look-at` options; a `None` [`position`](Self::position) keeps the
/// default login-snap behaviour.
#[derive(Resource, Default)]
pub(crate) struct CameraStart {
    /// The absolute Bevy-space camera position, or `None` to snap to the agent.
    pub(crate) position: Option<Vec3>,
    /// The Bevy-space look direction (un-normalised is fine), or `None` to keep
    /// the default forward aim.
    pub(crate) look: Option<Vec3>,
}

/// The debug fly-camera: a marker plus its accumulated look angles.
#[derive(Component)]
pub(crate) struct FlyCamera {
    /// Yaw about the Bevy up (`+Y`) axis, in radians.
    yaw: f32,
    /// Pitch about the camera's local right axis, in radians, clamped to
    /// `±MAX_PITCH`.
    pitch: f32,
    /// Roll about the camera's local forward axis, in radians. Only the
    /// [`CameraSpin`] roll mode moves it; mouse-look leaves it at `0`.
    roll: f32,
}

impl Default for FlyCamera {
    /// A level camera looking along `-Z` (Bevy forward).
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }
}

impl FlyCamera {
    /// Aim the camera along `direction` (Bevy Y-up space) by setting the yaw/pitch
    /// the [`fly_camera`] system drives the rotation from, so the aim survives the
    /// next frame's mouse-look re-derivation. A zero direction is ignored.
    ///
    /// Yaw is measured so `direction = -Z` gives yaw `0` (matching
    /// [`Default`](Self::default)); pitch is the elevation of `direction`, clamped
    /// to the same `±MAX_PITCH` the mouse-look uses.
    pub(crate) fn aim_along(&mut self, direction: Vec3) {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        self.yaw = (-dir.x).atan2(-dir.z);
        self.pitch = dir.y.asin().clamp(-MAX_PITCH, MAX_PITCH);
    }
}

/// Drive the fly-camera each frame: apply mouse-look, then translate along the
/// resulting orientation from the WASD / Space / Ctrl keys.
pub(crate) fn fly_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    spin: Res<CameraSpin>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let delta = mouse.delta;
    let dt = time.delta_secs();
    for (mut transform, mut camera) in &mut cameras {
        // Mouse-look: yaw from horizontal motion, pitch from vertical, standard
        // (moving the mouse up looks up).
        camera.yaw -= delta.x * MOUSE_SENSITIVITY;
        camera.pitch = (camera.pitch - delta.y * MOUSE_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
        // Optional auto-rotation (a survey pan for unattended captures): advance
        // the chosen axis by the spin rate. Yaw and roll loop freely; pitch keeps
        // the mouse-look clamp, so a pitch spin sweeps between the poles.
        if spin.rate != 0.0 {
            let step = spin.rate * dt;
            match spin.axis {
                SpinAxis::Yaw => camera.yaw += step,
                SpinAxis::Pitch => {
                    camera.pitch = (camera.pitch + step).clamp(-MAX_PITCH, MAX_PITCH);
                }
                SpinAxis::Roll => camera.roll += step,
            }
        }
        transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, camera.roll);

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
