//! The viewer camera: **one** main camera entity ([`ViewerCamera`]) driven by a
//! [`CameraMode`] state machine, modelled closely on the reference viewer's
//! `LLAgentCamera`.
//!
//! # One camera, three modes
//!
//! The reference viewer positions a single camera (`LLViewerCamera`) through
//! `LLAgentCamera`; mouselook, third-person and the joystick flycam are *modes*
//! of it, and focus-on-object is third-person with the focus point moved off the
//! avatar. We mirror that: one entity carrying [`ViewerCamera`] (the marker every
//! other system means when it says "the main camera", as distinct from the
//! reflection-probe / mirror / minimap cameras), a [`CameraMode`] resource, and a
//! [`CameraRig`] holding the drivable state. The modes bleed into each other the
//! way they do in Second Life — zoom third-person in past the head and it becomes
//! mouselook; drop into flycam and it keeps the pose it had — which is exactly why
//! one continuous transform, not a camera per mode, is the right model.
//!
//! - **Third person** ([`CameraMode::ThirdPerson`]) orbits a focus point. The
//!   focus is the avatar by default ([`FocusTarget::Avatar`]) but can be a picked
//!   point ([`FocusTarget::Point`], the alt-zoom of `lltoolfocus`). The geometry
//!   reproduces the reference's `CameraOffsetRearView` `(-3, 0, 0.75)` /
//!   `FocusOffsetRearView` `(1, 0, 1)` in the agent's own frame, so a scripted
//!   vehicle camera (`llSetCameraParams`, a later task) composes against the same
//!   numbers it does in the reference.
//! - **Mouselook** ([`CameraMode::Mouselook`]) sits at the avatar's eyes; the
//!   mouse aims and the cursor is captured (by [`crate::input_context`], which
//!   grabs the pointer in this mode and nowhere else).
//! - **Flycam** ([`CameraMode::Flycam`]) is the free 6-DOF spectator camera, the
//!   promotion of the old debug fly-camera. It is what the SpaceNavigator
//!   ([`crate::spacenav`]) drives, and what the "Stop flycam" button leaves.
//!
//! # Two reference bugs deliberately not reproduced
//!
//! The reference viewer has two long-standing camera glitches this design is
//! immune to *by construction*, and the immunity is worth stating so a later
//! change does not quietly reintroduce them:
//!
//! 1. **Sideways camera after a region crossing.** Third person derives its whole
//!    pose from the live avatar transform every frame, and the flycam is only
//!    *translated* by the origin shift ([`crate::terrain::recenter_terrain`]),
//!    never rotated — so a crossing can never leave the view yawed.
//! 2. **Vehicle camera orbiting the avatar on the arrow keys** after a laggy
//!    crossing. The arrow keys are avatar-movement *actions*
//!    ([`crate::input_action`]) routed only to [`crate::movement`], never to camera
//!    orbit (orbit is a mouse-drag alone), and third person always follows the live
//!    avatar / seat — so the camera cannot end up spinning around a frozen avatar.
//!
//! Reference (Firestorm, read-only): `indra/newview/llagentcamera.cpp/h`
//! (`calcCameraPositionTargetGlobal`, the mode machine, orbit / zoom smoothing),
//! `indra/newview/lltoolfocus.cpp` (alt-zoom), `indra/newview/llviewerjoystick`
//! (the flycam).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::avatars::{AvatarBody, AvatarState};
use crate::coords::sl_to_bevy_vec;
use crate::input_action::{Action, InputMode};
use crate::input_context::InputContext;
use crate::physics::AvatarMotion;
use crate::spacenav::{FlycamAxisSettings, SpacenavInput};
use crate::water::{WaterOcean, WaterRegionPlane};
use sl_client_bevy::{SlIdentity, Vector};

/// The agent-frame focus offset (forward, left, up metres) used only as the
/// **fallback** third-person focus for a placeholder-sphere avatar with no head
/// joint — one metre ahead of and above the anchor. A rigged avatar focuses on its
/// actual head instead ([`third_person_focus`]).
const FOCUS_OFFSET: Vec3 = Vec3::new(1.0, 0.0, 1.0);

/// The agent-frame rear-view camera offset (forward, left, up metres), the
/// reference's `CameraOffsetRearView`: three metres behind and 0.75 m above the
/// focus. Its length is the default zoom distance and its elevation the default
/// tilt.
const CAMERA_OFFSET: Vec3 = Vec3::new(-3.0, 0.0, 0.75);

/// The closest the third-person camera zooms before it crosses into mouselook —
/// the reference's `LAND_MIN_ZOOM`, near enough to the head that the transition
/// reads as "stepping inside".
const MOUSELOOK_CROSS_DISTANCE: f32 = 0.5;

/// The farthest the third-person camera zooms from the avatar
/// (`MAX_CAMERA_DISTANCE_FROM_AGENT`).
const MAX_DISTANCE: f32 = 50.0;

/// The mouselook eye offsets: `x` is the forward nudge (metres) from the head
/// joint so the view looks out past the face rather than through it; `z` is the
/// fallback head height above the body-root anchor used only for a
/// placeholder-sphere avatar with no head joint.
const MOUSELOOK_EYE_OFFSET: Vec3 = Vec3::new(0.1, 0.0, 1.2);

/// Radians of orbit per pixel of alt-drag mouse motion (azimuth and, under Ctrl,
/// elevation) — kept gentle so a small drag does not whip the camera around.
const MOUSE_SENSITIVITY: f32 = 0.003;

/// Wheel-notch-equivalent zoom per pixel of vertical alt-drag, so an alt-drag up /
/// down zooms the third-person camera in / out at a brisk rate.
const DRAG_ZOOM_RATE: f32 = 0.05;

/// Pixels of a `Pixel`-unit scroll that count as one `Line`-unit notch, so the
/// wheel zoom behaves the same whether the platform reports line or pixel scroll
/// deltas (a pixel-reporting device otherwise gives tiny per-notch deltas that
/// never zoom far enough to cross into mouselook).
const PIXELS_PER_LINE: f32 = 20.0;

/// Radians of yaw/pitch per pixel of mouse motion in mouselook / flycam look —
/// finer than the orbit rate so aiming is steady.
const AIM_SENSITIVITY: f32 = 0.003;

/// Pitch clamp (just under a quarter turn) so the view never flips over the pole.
const MAX_PITCH: f32 = 1.54;

/// The multiplicative zoom step per mouse-wheel notch (scroll in shrinks the
/// distance by this factor), matching the reference's geometric zoom.
const ZOOM_STEP: f32 = 0.9;

/// Base flycam translation speed, metres per second.
const FLYCAM_SPEED: f32 = 10.0;

/// Multiplier applied to [`FLYCAM_SPEED`] while [`Action::Run`] is held.
const FLYCAM_FAST: f32 = 4.0;

/// How much of the remaining gap the smoothed pose closes each frame's worth of a
/// ~0.1 s half-life, so mode transitions glide rather than snap. Applied as
/// `1 - 0.5^(dt / HALF_LIFE)`.
const SMOOTH_HALF_LIFE: f32 = 0.1;

/// The mouselook eye's smoothing half-life (seconds) — short, so the first-person
/// aim stays responsive, but enough to filter out the animated head joint's
/// per-frame vibration (the idle-animation micro-motion that otherwise shakes the
/// whole view).
const MOUSELOOK_EYE_HALF_LIFE: f32 = 0.06;

/// The clearance kept between the third-person camera and an obstruction it would
/// otherwise clip through, so the pulled-in camera sits just short of the wall.
const COLLISION_PADDING: f32 = 0.2;

/// Which of the camera's own axes an [auto-rotation](CameraSpin) spins about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SpinAxis {
    /// About the vertical (Bevy `+Y`) axis — pans left/right.
    #[default]
    Yaw,
    /// About the camera's local right axis — tilts up/down (clamped to
    /// `±MAX_PITCH`).
    Pitch,
    /// About the camera's local forward axis — rolls the horizon.
    Roll,
}

/// A debug affordance: auto-rotate the flycam at a fixed rate for unattended
/// screenshot pans. Inserted from `--camera-spin` / `--camera-spin-axis`; a zero
/// [`rate`](Self::rate) disables it.
#[derive(Resource, Default)]
pub(crate) struct CameraSpin {
    /// Radians per second to auto-rotate; `0.0` disables the spin.
    pub(crate) rate: f32,
    /// Which of the camera's axes the spin rotates about.
    pub(crate) axis: SpinAxis,
}

/// A debug affordance: place the camera at an absolute flycam pose on startup
/// (from `--camera-position` / `--camera-look-at`) so an unattended capture frames
/// a fixed viewpoint. A `None` [`position`](Self::position) keeps the default
/// third-person follow.
#[derive(Resource, Default)]
pub(crate) struct CameraStart {
    /// The absolute Bevy-space camera position, or `None` to follow the agent.
    pub(crate) position: Option<Vec3>,
    /// The Bevy-space look direction (un-normalised is fine), or `None` to keep
    /// the default forward aim.
    pub(crate) look: Option<Vec3>,
}

/// The camera mode: one of the three the [`ViewerCamera`] cycles between. See the
/// [module documentation](self).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum CameraMode {
    /// First-person: at the eyes, mouse aims, cursor captured.
    Mouselook,
    /// Orbiting third-person around a [`FocusTarget`] (the default).
    #[default]
    ThirdPerson,
    /// Free 6-DOF spectator camera (the promoted debug fly-camera).
    Flycam,
}

/// What the third-person camera orbits around.
///
/// The alt-zoom focus tool (`lltoolfocus`) moves this off the avatar onto a picked
/// object; a vehicle would move it onto the seat. Defaults to the avatar.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub(crate) enum FocusTarget {
    /// The agent's own avatar (rear-view follow).
    #[default]
    Avatar,
    /// A fixed world point (the alt-click focus of `lltoolfocus`).
    Point(Vec3),
}

/// The marker on the one main viewer camera entity — the camera every world
/// system means by "the camera", as opposed to the reflection-probe, mirror and
/// minimap cameras that also carry `Camera3d`. Mode-agnostic: the same entity is
/// the camera in mouselook, third person and flycam.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(crate) struct ViewerCamera;

/// The drivable state of the [`ViewerCamera`], shared by every mode.
///
/// Third person reads the orbit fields ([`azimuth`](Self::azimuth) /
/// [`elevation`](Self::elevation) / [`distance`](Self::distance)); mouselook and
/// flycam read the aim fields ([`yaw`](Self::yaw) / [`pitch`](Self::pitch) /
/// [`roll`](Self::roll)). The flycam's *position* is the entity `Transform`'s
/// translation, not stored here — so a debug focus system that writes the
/// transform moves the flycam directly. The smoothed pose eases toward the mode's
/// desired pose so mode changes glide.
#[derive(Component, Debug, Clone)]
pub(crate) struct CameraRig {
    /// Third-person horizontal orbit offset from dead-behind the avatar, radians
    /// (`0` = rear view). Only a mouse-drag moves it — never the arrow keys.
    azimuth: f32,
    /// Third-person vertical orbit angle, radians (positive looks down onto the
    /// avatar). Seeded from [`CAMERA_OFFSET`]'s elevation.
    elevation: f32,
    /// Third-person camera distance from the focus, metres, clamped to
    /// `[MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE]`.
    distance: f32,
    /// Mouselook / flycam yaw about Bevy up (`+Y`), radians.
    yaw: f32,
    /// Mouselook / flycam pitch about the camera's local right, radians, clamped
    /// to `±MAX_PITCH`.
    pitch: f32,
    /// Flycam roll about the camera's local forward, radians (only [`CameraSpin`]
    /// roll moves it).
    roll: f32,
    /// The world-space offset from a [`FocusTarget::Point`] focus to the camera
    /// eye, used only in focus-on-object. Captured at alt-click so the camera does
    /// not jump, and orbited / zoomed since. Unlike the avatar rear-view orbit
    /// (which follows the heading) this is fixed in the world, as the reference's
    /// object focus is.
    point_offset: Vec3,
    /// The last rendered eye position, eased toward the mode's desired eye.
    smoothed_eye: Vec3,
    /// The last rendered look-at point, eased toward the mode's desired focus.
    smoothed_focus: Vec3,
    /// Whether the smoothed pose has been seeded yet (so the first valid frame
    /// snaps rather than gliding in from an arbitrary origin).
    seeded: bool,
}

impl Default for CameraRig {
    /// The reference rear-view orbit: dead behind, tilted and distanced by
    /// [`CAMERA_OFFSET`].
    fn default() -> Self {
        let horizontal =
            (CAMERA_OFFSET.x * CAMERA_OFFSET.x + CAMERA_OFFSET.y * CAMERA_OFFSET.y).sqrt();
        Self {
            azimuth: 0.0,
            elevation: CAMERA_OFFSET.z.atan2(horizontal),
            distance: CAMERA_OFFSET.length(),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            point_offset: Vec3::ZERO,
            smoothed_eye: Vec3::ZERO,
            smoothed_focus: Vec3::ZERO,
            seeded: false,
        }
    }
}

impl CameraRig {
    /// Reset the third-person orbit to the default rear view — the reference's
    /// `Escape` "reset camera". Leaves the aim / smoothing alone (the caller snaps
    /// via the mode change).
    fn reset_orbit(&mut self) {
        let default = Self::default();
        self.azimuth = default.azimuth;
        self.elevation = default.elevation;
        self.distance = default.distance;
    }

    /// Seed the third-person orbit from the debug framing environment variables,
    /// so the offline screenshot harness can frame the avatar from a chosen angle
    /// (the same `SL_VIEWER_CAMERA_*` knobs the old login-snap read). A no-op when
    /// none are set — the default rear view stands.
    ///
    /// `SL_VIEWER_CAMERA_ORBIT_DEG` swings the azimuth (90 = a side view),
    /// `_ELEV_DEG` the elevation (positive looks down), `_DISTANCE` the zoom.
    pub(crate) fn seed_orbit_from_env(&mut self) {
        let env_f32 = |key: &str| -> Option<f32> {
            std::env::var(key).ok().and_then(|value| value.parse().ok())
        };
        if let Some(orbit) = env_f32("SL_VIEWER_CAMERA_ORBIT_DEG") {
            self.azimuth = orbit.to_radians();
        }
        if let Some(elevation) = env_f32("SL_VIEWER_CAMERA_ELEV_DEG") {
            self.elevation = elevation.to_radians().clamp(-MAX_PITCH, MAX_PITCH);
        }
        if let Some(distance) = env_f32("SL_VIEWER_CAMERA_DISTANCE") {
            self.distance = distance.clamp(MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Reset the smoothing so the next frame snaps to the mode's desired pose
    /// rather than gliding — called after a region-origin shift
    /// ([`crate::terrain::recenter_terrain`]) so the eased pose does not drift
    /// across the 256 m rebase (the reference's sideways-after-crossing bug).
    pub(crate) const fn resnap(&mut self) {
        self.seeded = false;
    }

    /// Aim the flycam / mouselook along `direction` (Bevy Y-up space) by setting
    /// the yaw/pitch, so the aim survives the next frame's re-derivation. A zero
    /// direction is ignored. Yaw is measured so `-Z` gives yaw `0`; pitch is the
    /// elevation, clamped to `±MAX_PITCH`.
    pub(crate) fn aim_along(&mut self, direction: Vec3) {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        self.yaw = (-dir.x).atan2(-dir.z);
        self.pitch = dir.y.asin().clamp(-MAX_PITCH, MAX_PITCH);
    }
}

/// The feathering state of the SpaceNavigator flycam: the per-axis smoothed
/// per-frame delta (`sDelta` in the reference `moveFlycam`), in flycam-function
/// order `[forward, strafe, up, roll, pitch, yaw]`. Each frame it eases toward the
/// dead-zoned, scaled input and is then applied to the camera, so the flycam ramps
/// up and down rather than snapping.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct FlycamSmoothing {
    /// The smoothed per-frame deltas in flycam-function order.
    delta: [f32; 6],
}

/// The avatar heading the camera aims at in mouselook, published for
/// [`crate::movement`] so the body faces where the mouse looks.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct CameraAim {
    /// The Second Life heading (yaw about the SL up axis, radians) the mouselook
    /// camera is pointed along; the avatar body follows it while in mouselook.
    pub(crate) sl_yaw: f32,
    /// Whether the camera is in mouselook this frame.
    pub(crate) mouselook: bool,
}

/// Component-wise vector add (`a + b`), avoiding the glam `+` operator the
/// workspace `arithmetic_side_effects` lint trips on.
const fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Component-wise vector subtract (`a - b`).
const fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector scale (`v * s`).
const fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// Linear interpolation from `a` to `b` by `t`.
const fn vlerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    vadd(a, vscale(vsub(b, a), t))
}

/// The avatar's flattened Bevy-space facing (horizontal forward) from its Second
/// Life heading `sl_yaw`. A Second Life avatar faces its region `+X` turned by the
/// heading; the axis map `(x, y, z) -> (x, z, -y)` carries that into Bevy.
fn facing_from_yaw(sl_yaw: f32) -> Vec3 {
    let (sin, cos) = sl_yaw.sin_cos();
    sl_to_bevy_vec(&Vector {
        x: cos,
        y: sin,
        z: 0.0,
    })
}

/// The third-person eye (Bevy world) given the focus point, the avatar's flattened
/// facing, and the orbit state — the camera sits `distance` back along the rear
/// direction (dead-behind `-flat` rotated by `azimuth` about vertical and tilted up
/// by `elevation`). Pure, so the reference rear-view offset is unit-testable.
fn third_person_eye(
    focus: Vec3,
    facing: Vec3,
    azimuth: f32,
    elevation: f32,
    distance: f32,
) -> Vec3 {
    let flat = flatten(facing);
    let behind = Quat::from_rotation_y(azimuth).mul_vec3(vscale(flat, -1.0));
    let (sin_elev, cos_elev) = elevation.sin_cos();
    let dir = vadd(vscale(behind, cos_elev), Vec3::new(0.0, sin_elev, 0.0));
    vadd(focus, vscale(dir, distance))
}

/// The third-person **focus point**: the avatar's head (so orbit and zoom pivot on
/// the back of the head, as the reference does), or a head-height offset above the
/// body-root `anchor` when no head joint is available (a placeholder sphere).
fn third_person_focus(head: Option<Vec3>, anchor: Vec3, facing: Vec3) -> Vec3 {
    match head {
        Some(head) => head,
        None => {
            let flat = flatten(facing);
            vadd(
                anchor,
                vadd(
                    vscale(flat, FOCUS_OFFSET.x),
                    Vec3::new(0.0, FOCUS_OFFSET.z, 0.0),
                ),
            )
        }
    }
}

/// Flatten a direction onto the horizontal plane and normalise it, falling back to
/// Bevy `-Z` (north-ish) for a straight-up/down input so the camera never loses
/// its heading.
fn flatten(direction: Vec3) -> Vec3 {
    Vec3::new(direction.x, 0.0, direction.z)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z)
}

/// The wheel scroll in `Line`-unit notches, normalising a pixel-unit scroll (a
/// touchpad / high-resolution wheel) so the zoom behaves the same on any platform.
fn scroll_notches(wheel: &AccumulatedMouseScroll) -> f32 {
    match wheel.unit {
        MouseScrollUnit::Pixel => wheel.delta.y / PIXELS_PER_LINE,
        // `Line` (and any future unit) is already in notches.
        _other => wheel.delta.y,
    }
}

/// The camera plugin: the mode machine, the per-mode drivers, and the final pose.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CameraPlugin;

impl Plugin for CameraPlugin {
    /// Wire the camera systems. `sync_input_mode` runs in `PreUpdate` so the action
    /// map's profile matches this frame's mode; the drivers and [`position_camera`]
    /// run in `Update`, in order, so every `.after(position_camera)` consumer (sky,
    /// water, particles, interest reporting) reads the finished pose.
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .init_resource::<FocusTarget>()
            .init_resource::<CameraAim>()
            .init_resource::<FlycamSmoothing>()
            .add_systems(PreUpdate, sync_input_mode)
            .add_systems(
                Update,
                (
                    switch_camera_mode,
                    reset_camera_view,
                    orbit_third_person,
                    aim_look,
                    focus_on_object,
                    drive_flycam,
                    position_camera,
                )
                    .chain(),
            )
            .add_systems(Update, update_camera_cursor);
    }
}

/// Derive the action-map [`InputMode`] from the [`CameraMode`], so a key resolves
/// against the profile that matches the camera.
pub(crate) fn sync_input_mode(mode: Res<CameraMode>, mut input_mode: ResMut<InputMode>) {
    let next = match *mode {
        CameraMode::Mouselook => InputMode::Mouselook,
        CameraMode::ThirdPerson => InputMode::ThirdPerson,
        CameraMode::Flycam => InputMode::Flycam,
    };
    if *input_mode != next {
        *input_mode = next;
    }
}

/// Handle the mode-toggle actions and the seamless zoom-through transitions, and
/// auto-enter flycam on SpaceNavigator input.
///
/// Toggling seeds the rig so the new mode picks up where the old one left off — a
/// dropped-into flycam keeps the current aim, and leaving mouselook restores an
/// orbit just outside the head — which is what makes the transitions
/// ([`position_camera`]'s smoothing does the visual glide) seamless.
pub(crate) fn switch_camera_mode(
    actions: Res<ButtonInput<Action>>,
    spacenav: Res<SpacenavInput>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<(&Transform, &mut CameraRig), With<ViewerCamera>>,
) {
    let Ok((transform, mut rig)) = cameras.single_mut() else {
        return;
    };

    // Mouselook toggle: into mouselook seeds the aim from the current forward;
    // out of it drops to a third-person orbit just outside the head.
    if actions.just_pressed(Action::ToggleMouselook) {
        match *mode {
            CameraMode::Mouselook => {
                *mode = CameraMode::ThirdPerson;
                rig.distance = rig.distance.max(MOUSELOOK_CROSS_DISTANCE);
                *focus = FocusTarget::Avatar;
            }
            CameraMode::ThirdPerson | CameraMode::Flycam => {
                rig.aim_along(transform.forward().as_vec3());
                *mode = CameraMode::Mouselook;
            }
        }
    }

    // Flycam toggle: into flycam keeps the current pose (the entity transform is
    // already the eye; seed the aim from the forward); out of it returns to
    // third-person. The `ToggleFlycam` action and the SpaceNavigator's **first
    // button** both toggle it — matching the reference, where the joystick's flycam
    // button enters and leaves flycam.
    if actions.just_pressed(Action::ToggleFlycam) || spacenav.toggle_flycam {
        toggle_flycam(&mut mode, &mut focus, &mut rig, transform);
    }
}

/// Enter or leave flycam, seeding the aim from the current forward so the pose is
/// continuous across the switch.
fn toggle_flycam(
    mode: &mut CameraMode,
    focus: &mut FocusTarget,
    rig: &mut CameraRig,
    transform: &Transform,
) {
    match *mode {
        CameraMode::Flycam => {
            *mode = CameraMode::ThirdPerson;
            *focus = FocusTarget::Avatar;
            // Leaving flycam **warps** to the third-person view rather than gliding
            // back, matching the reference (the flycam pose and the follow pose are
            // unrelated, so an interpolation between them just flies through the
            // scene).
            rig.resnap();
        }
        CameraMode::Mouselook | CameraMode::ThirdPerson => {
            rig.aim_along(transform.forward().as_vec3());
            *mode = CameraMode::Flycam;
        }
    }
}

/// Reset the camera to the default third-person rear view on `Escape` in the
/// world — the reference's "reset camera" (`Escape` recentres behind the avatar).
///
/// Returns from mouselook / flycam, re-centres the focus on the avatar and resets
/// the orbit; the smoothing then glides the view back rather than snapping. Only
/// in the world context — a focused UI's `Escape` releases focus
/// ([`crate::input_context`]) instead, and quit is now `Ctrl+Q`
/// ([`crate::session::handle_quit_input`]), so `Escape` is free to mean this.
pub(crate) fn reset_camera_view(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    if !context.is_world() || !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    *mode = CameraMode::ThirdPerson;
    *focus = FocusTarget::Avatar;
    if let Ok(mut rig) = cameras.single_mut() {
        rig.reset_orbit();
    }
    info!("camera: reset to third-person rear view");
}

/// Swap the mouse cursor to signal the third-person camera gesture the modifiers
/// arm, before the click — matching the reference: **Alt** shows the zoom cursor,
/// **Ctrl+Alt** the orbit cursor, and anything else the default arrow.
///
/// Only in third person with a free cursor; mouselook captures the cursor and
/// flycam does not use the modifiers, so both keep the default.
pub(crate) fn update_camera_cursor(
    mode: Res<CameraMode>,
    context: Res<InputContext>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut last: Local<Option<SystemCursorIcon>>,
    mut commands: Commands,
) {
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let desired = if *mode == CameraMode::ThirdPerson && context.is_world() && alt {
        if ctrl {
            // The orbit gesture — a grab/hand cursor, clearly distinct from the zoom
            // magnifier.
            SystemCursorIcon::Grab
        } else {
            SystemCursorIcon::ZoomIn
        }
    } else {
        SystemCursorIcon::Default
    };
    // Only write when the icon actually changes, so this is idle most frames.
    if *last == Some(desired) {
        return;
    }
    *last = Some(desired);
    if let Ok(entity) = windows.single() {
        commands.entity(entity).insert(CursorIcon::System(desired));
    }
}

/// Third-person camera control from the mouse, matching Second Life:
///
/// - **Alt + left-drag** orbits — horizontal motion swings the azimuth, vertical
///   motion **zooms** in / out.
/// - **Ctrl + Alt + left-drag** orbits — horizontal is still azimuth, but vertical
///   is the **elevation** (over / under) instead of zoom.
/// - The **wheel** always zooms, and zooming in past [`MOUSELOOK_CROSS_DISTANCE`]
///   crosses into mouselook.
///
/// The camera orbits only under `Alt` (a plain left-click is a *touch*, handled by
/// `crate::hud_pick`) and never on the arrow keys — so a vehicle's arrow-key
/// steering can never be mistaken for a camera orbit (reference bug #2 in the
/// module docs). The alt-drag focus point is set by [`focus_on_object`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode / focus / \
              context state, the Alt / Ctrl modifiers, the mouse button and motion, the wheel, and \
              the camera rig"
)]
pub(crate) fn orbit_third_person(
    mut mode: ResMut<CameraMode>,
    focus: Res<FocusTarget>,
    context: Res<InputContext>,
    keyboard: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    let scroll = scroll_notches(&wheel);
    if *mode != CameraMode::ThirdPerson || !context.is_world() {
        return;
    }
    let Ok(mut rig) = cameras.single_mut() else {
        return;
    };
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    // Only an Alt-held left-drag orbits; otherwise the mouse motion is left alone.
    let drag = if alt && buttons.pressed(MouseButton::Left) {
        motion.delta
    } else {
        Vec2::ZERO
    };

    let azimuth_delta = drag.x * MOUSE_SENSITIVITY;
    // Vertical drag is elevation while Ctrl is held, else zoom (up = closer). The
    // wheel always adds to the zoom.
    let elevation_delta = if ctrl {
        drag.y * MOUSE_SENSITIVITY
    } else {
        0.0
    };
    let zoom_in = scroll + if ctrl { 0.0 } else { -drag.y * DRAG_ZOOM_RATE };

    match *focus {
        // Rear-view orbit around the avatar: the orbit is avatar-relative
        // (azimuth / elevation / distance), so it follows the heading.
        FocusTarget::Avatar => {
            rig.azimuth += azimuth_delta;
            rig.elevation = (rig.elevation + elevation_delta).clamp(-MAX_PITCH, MAX_PITCH);
            if zoom_in != 0.0 {
                let next = rig.distance * ZOOM_STEP.powf(zoom_in);
                if zoom_in > 0.0 && next < MOUSELOOK_CROSS_DISTANCE {
                    // A zoom-in that would cross the minimum distance steps into
                    // mouselook — seeding the aim from the current view direction so
                    // the transition is continuous.
                    let forward = vsub(rig.smoothed_focus, rig.smoothed_eye);
                    rig.aim_along(forward);
                    *mode = CameraMode::Mouselook;
                } else {
                    rig.distance = next.clamp(MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE);
                }
            }
        }
        // Focus on a point: orbit / zoom the fixed world offset around the point,
        // so the object stays put and the camera swings around it.
        FocusTarget::Point(_point) => {
            if azimuth_delta != 0.0 || elevation_delta != 0.0 {
                rig.point_offset = orbit_offset(rig.point_offset, azimuth_delta, elevation_delta);
            }
            if zoom_in != 0.0 {
                let factor = ZOOM_STEP.powf(zoom_in);
                let length = rig.point_offset.length();
                if length > 1.0e-4 {
                    let next = (length * factor).clamp(MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE);
                    rig.point_offset = vscale(rig.point_offset, next / length);
                }
            }
        }
    }
}

/// Orbit a world-space camera offset around the focus: `azimuth` yaws it about
/// vertical, `elevation` tilts it about the horizontal axis perpendicular to the
/// offset — pivoting the camera around the focus point without changing its
/// distance.
fn orbit_offset(offset: Vec3, azimuth: f32, elevation: f32) -> Vec3 {
    let yawed = Quat::from_rotation_y(azimuth).mul_vec3(offset);
    if elevation == 0.0 {
        return yawed;
    }
    // The horizontal axis to tilt about: perpendicular to the offset's horizontal
    // projection. A near-vertical offset has no stable axis, so skip the tilt then.
    let horizontal = Vec3::new(yawed.x, 0.0, yawed.z);
    match Dir3::new(Vec3::Y.cross(horizontal)) {
        Ok(axis) => Quat::from_axis_angle(axis.as_vec3(), elevation).mul_vec3(yawed),
        Err(_degenerate) => yawed,
    }
}

/// The water-surface entities (endless ocean + per-region planes) excluded from
/// the alt-click focus pick.
type WaterQuery<'world, 'state> =
    Query<'world, 'state, Entity, Or<(With<WaterOcean>, With<WaterRegionPlane>)>>;

/// Alt-click **focus-on-object** (`lltoolfocus`): with `Alt` held, a left click in
/// third person focuses the camera on the picked world point, so orbit and zoom
/// pivot around it instead of the avatar. Any avatar-movement action returns the
/// focus to the avatar — the reference's `setFocusOnAvatar(true)` on move.
///
/// Reuses the world camera's perspective ray through the cursor (the same pick the
/// `P` crosshair tool casts).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode / context \
              gate, the Alt modifier and mouse button, the movement actions that reset the focus, \
              the window and camera to cast from, the ray caster, and the focus target it sets"
)]
pub(crate) fn focus_on_object(
    mode: Res<CameraMode>,
    context: Res<InputContext>,
    keyboard: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    actions: Res<ButtonInput<Action>>,
    windows: Query<&Window>,
    water: WaterQuery,
    mut cameras: Query<(&Camera, &GlobalTransform, &mut CameraRig), With<ViewerCamera>>,
    mut ray_cast: MeshRayCast,
    mut focus: ResMut<FocusTarget>,
) {
    // Moving the avatar returns the focus to it (and pre-empts a focus this frame).
    let moving = actions.pressed(Action::MoveForward)
        || actions.pressed(Action::MoveBackward)
        || actions.pressed(Action::MoveLeft)
        || actions.pressed(Action::MoveRight);
    if moving {
        if !matches!(*focus, FocusTarget::Avatar) {
            *focus = FocusTarget::Avatar;
        }
        return;
    }
    if *mode != CameraMode::ThirdPerson || !context.is_world() {
        return;
    }
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    if !alt || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window
        .cursor_position()
        .unwrap_or_else(|| Vec2::new(window.width() * 0.5, window.height() * 0.5));
    let Ok((camera, camera_transform, mut rig)) = cameras.single_mut() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    // The water surface (endless ocean + per-region planes) is not a focus target —
    // it covers the whole scene, so without this every alt-click would focus a
    // distant sea-level point instead of the object under the cursor. Exclude those
    // entities from the pick.
    let water_entities: std::collections::HashSet<Entity> = water.iter().collect();
    let filter = |entity: Entity| !water_entities.contains(&entity);
    let settings = MeshRayCastSettings::default().with_filter(&filter);
    if let Some((_entity, hit)) = ray_cast.cast_ray(ray, &settings).first() {
        // Keep the camera exactly where it is and re-pivot around the picked point:
        // store the world offset from the point to the current eye, so the eye does
        // not jump (the reference does not move the camera on an alt-click focus).
        rig.point_offset = vsub(camera_transform.translation(), hit.point);
        *focus = FocusTarget::Point(hit.point);
        info!("camera: focus on {:?}", hit.point);
    }
}

/// Mouselook aim from the (captured) mouse: raw motion aims the first-person view,
/// and scrolling out returns to third person. Flycam aim is handled in
/// [`drive_flycam`] (with a local-frame quaternion, so it has no gimbal lock).
pub(crate) fn aim_look(
    mut mode: ResMut<CameraMode>,
    context: Res<InputContext>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    let scroll = scroll_notches(&wheel);
    if *mode != CameraMode::Mouselook || !context.is_world() {
        return;
    }
    let Ok(mut rig) = cameras.single_mut() else {
        return;
    };
    let delta = motion.delta;
    rig.yaw -= delta.x * AIM_SENSITIVITY;
    rig.pitch = (rig.pitch - delta.y * AIM_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    // Scroll out of mouselook back into third person, dropping just outside the head.
    if scroll < 0.0 {
        *mode = CameraMode::ThirdPerson;
    }
}

/// Drive the flycam's free position and orientation from the movement actions, the
/// SpaceNavigator, a right-drag mouse-look, and the [`CameraSpin`] auto-rotation.
///
/// The flycam eye is the entity `Transform`'s translation; the WASD actions,
/// `Space`/`Ctrl` and the 6-DOF device translate it along the current basis. The
/// orientation is composed **incrementally in the camera's local frame** (a
/// quaternion multiply), not rebuilt from accumulated Euler angles — so it is true
/// 6-DOF with no gimbal lock looking straight up or down, and rotations act on the
/// camera's own axes, as the reference flycam does. Only runs in flycam.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode gate, the \
              movement actions, the device state, the auto-spin, time, the right-drag mouse-look \
              (motion + button + focus context) and the camera transform"
)]
pub(crate) fn drive_flycam(
    mode: Res<CameraMode>,
    actions: Res<ButtonInput<Action>>,
    spacenav: Res<SpacenavInput>,
    flycam_settings: Res<FlycamAxisSettings>,
    spin: Res<CameraSpin>,
    time: Res<Time>,
    motion: Res<AccumulatedMouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    context: Res<InputContext>,
    mut smoothing: ResMut<FlycamSmoothing>,
    mut cameras: Query<&mut Transform, With<ViewerCamera>>,
) {
    if *mode != CameraMode::Flycam {
        return;
    }
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    // The reference clamps the frame time so a big frame-rate drop does not make a
    // huge jump.
    let dt = time.delta_secs().min(0.2);

    // The SpaceNavigator feathering pipeline (reference `moveFlycam`): per flycam
    // function, apply the soft dead-zone (subtract, so it ramps from zero), the
    // per-axis scale, the frame time, and then ease the smoothed per-frame delta
    // toward it at the feathering rate. The smoothed delta is what actually drives
    // the camera each frame, so it ramps up on push and down on release.
    let feather = flycam_settings.feathering;
    for (index, smoothed) in smoothing.delta.iter_mut().enumerate() {
        let raw = spacenav.axes.get(index).copied().unwrap_or(0.0);
        let dead_zone = flycam_settings.dead_zone.get(index).copied().unwrap_or(0.0);
        let scale = flycam_settings.scale.get(index).copied().unwrap_or(0.0);
        let deadzoned = if raw > 0.0 {
            (raw - dead_zone).max(0.0)
        } else {
            (raw + dead_zone).min(0.0)
        };
        let target = deadzoned * scale * dt;
        *smoothed += (target - *smoothed) * dt * feather;
    }
    let [
        nav_forward,
        nav_strafe,
        nav_up,
        nav_roll,
        nav_pitch,
        nav_yaw,
    ] = smoothing.delta;

    // Rotation: the device's feathered roll / pitch / yaw, plus a right-drag
    // mouse-look and the auto-spin, composed as one **local-frame** delta
    // (right-multiply) — true 6-DOF, gimbal-free looking straight up or down, and
    // an enabled roll axis rolls the camera. Composing local yaw and pitch drifts
    // roll in, which AutoLeveling below removes.
    let mut pitch = nav_pitch;
    let mut yaw = nav_yaw;
    let mut roll = nav_roll;
    if context.is_world() && buttons.pressed(MouseButton::Right) {
        yaw -= motion.delta.x * AIM_SENSITIVITY;
        pitch -= motion.delta.y * AIM_SENSITIVITY;
    }
    if spin.rate != 0.0 {
        let step = spin.rate * dt;
        match spin.axis {
            SpinAxis::Yaw => yaw += step,
            SpinAxis::Pitch => pitch += step,
            SpinAxis::Roll => roll += step,
        }
    }
    if pitch != 0.0 || yaw != 0.0 || roll != 0.0 {
        let delta = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
        transform.rotation = transform.rotation.mul_quat(delta).normalize();
    }

    // AutoLeveling (reference `AutoLeveling`, on by default for a SpaceNavigator):
    // ease the camera's horizon back to level each frame, which both removes the
    // roll drift that local yaw+pitch composition introduces *and* makes an
    // intentional roll transient (it self-levels), matching the reference.
    //
    // Level by forcing the camera's **right** axis horizontal (as the reference
    // levels its left axis), *not* by deriving up from forward: the right axis
    // stays well-defined looking straight up or down, where a forward-based level
    // is singular — which is what caused the artefacts at those poles.
    if flycam_settings.auto_leveling {
        let forward = transform.forward().as_vec3();
        let right = transform.right().as_vec3();
        if let Some(level_right) = Vec3::new(right.x, 0.0, right.z).try_normalize() {
            let up = level_right.cross(forward).normalize_or_zero();
            if up != Vec3::ZERO {
                // Columns are the rotated frame's axes: X = right, Y = up, Z = back
                // (the camera looks down its local `-Z`).
                let leveled =
                    Quat::from_mat3(&Mat3::from_cols(level_right, up, vscale(forward, -1.0)));
                let ease = (flycam_settings.feathering * dt).min(1.0);
                transform.rotation = transform.rotation.slerp(leveled, ease).normalize();
            }
        }
    }

    // The camera basis after the rotation update.
    let forward = transform.forward().as_vec3();
    let right = transform.right().as_vec3();

    // SpaceNavigator translation in the camera-local frame (as the reference
    // rotates its translation delta by the camera orientation): forward / strafe /
    // up from the feathered functions.
    let nav_move = vadd(
        vadd(vscale(forward, nav_forward), vscale(right, nav_strafe)),
        vscale(Vec3::Y, nav_up),
    );
    if nav_move.length_squared() > 0.0 {
        transform.translation = vadd(transform.translation, nav_move);
    }

    // Keyboard translation along the camera basis (unaffected by the device
    // feathering), accumulated per component so the arithmetic stays in plain `f32`.
    let mut move_vec = Vec3::ZERO;
    if actions.pressed(Action::MoveForward) {
        move_vec = vadd(move_vec, forward);
    }
    if actions.pressed(Action::MoveBackward) {
        move_vec = vsub(move_vec, forward);
    }
    if actions.pressed(Action::MoveRight) {
        move_vec = vadd(move_vec, right);
    }
    if actions.pressed(Action::MoveLeft) {
        move_vec = vsub(move_vec, right);
    }
    if actions.pressed(Action::MoveUp) {
        move_vec = vadd(move_vec, Vec3::Y);
    }
    if actions.pressed(Action::MoveDown) {
        move_vec = vsub(move_vec, Vec3::Y);
    }
    let length_squared = move_vec.length_squared();
    if length_squared > 0.0 {
        let boost = if actions.pressed(Action::Run) {
            FLYCAM_FAST
        } else {
            1.0
        };
        let step = FLYCAM_SPEED * boost * dt / length_squared.sqrt();
        transform.translation = vadd(transform.translation, vscale(move_vec, step));
    }
}

/// Everything [`position_camera`] needs to find the own avatar's world pose.
type AvatarPoseQuery<'world, 'state> = Query<'world, 'state, &'static GlobalTransform>;

/// Compute and apply the final camera pose for the active mode, easing it toward
/// the target so mode transitions glide.
///
/// - **Third person** follows the avatar (or focus point), pulls in on collision,
///   and looks at the focus.
/// - **Mouselook** sits at the avatar's eyes, aimed by the rig.
/// - **Flycam** is already positioned by [`drive_flycam`]; this only seeds the
///   smoothed pose so a switch *into* it glides.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode / focus \
              / aim state, the identity and avatar tables to find the own avatar, the transform \
              query, the ray caster for collision, time for the smoothing, and the camera itself"
)]
pub(crate) fn position_camera(
    mode: Res<CameraMode>,
    focus_target: Res<FocusTarget>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    time: Res<Time>,
    globals: AvatarPoseQuery,
    motions: Query<&AvatarMotion>,
    mut ray_cast: MeshRayCast,
    mut aim_out: ResMut<CameraAim>,
    mut cameras: Query<(&mut Transform, &mut CameraRig), With<ViewerCamera>>,
) {
    let Ok((mut transform, mut rig)) = cameras.single_mut() else {
        return;
    };
    aim_out.mouselook = *mode == CameraMode::Mouselook;

    // The own avatar's world position and stable (heading-derived) facing, if it
    // has arrived.
    let avatar_pose = own_avatar_pose(&identity, &avatars, &globals, &motions);

    match *mode {
        CameraMode::Flycam => {
            // `drive_flycam` owns the transform; just keep the smoothed pose in sync
            // so a later switch out of flycam glides from here.
            rig.smoothed_eye = transform.translation;
            let forward = transform.forward().as_vec3();
            rig.smoothed_focus = vadd(transform.translation, forward);
            rig.seeded = true;
        }
        CameraMode::Mouselook => {
            let dt = time.delta_secs();
            let look = Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, rig.roll);
            let look_forward = look.mul_vec3(Vec3::NEG_Z);
            // The first-person eye: the avatar's **head joint** (accurate eye
            // height), nudged a touch forward along the look so the view is not
            // inside the face. Falls back to the anchor plus a head-height offset for
            // a placeholder-sphere avatar with no skeleton.
            let eye = own_avatar_head(&identity, &avatars, body.as_deref(), &globals)
                .map(|head| vadd(head, vscale(look_forward, MOUSELOOK_EYE_OFFSET.x)))
                .or_else(|| {
                    avatar_pose.map(|(avatar, facing)| {
                        let flat = flatten(facing);
                        vadd(
                            avatar,
                            vadd(
                                vscale(flat, MOUSELOOK_EYE_OFFSET.x),
                                Vec3::new(0.0, MOUSELOOK_EYE_OFFSET.z, 0.0),
                            ),
                        )
                    })
                });
            let Some(desired_eye) = eye else {
                return;
            };
            // Smooth the eye position (the head joint is animated and would otherwise
            // shake the whole view), but set the rotation directly from the
            // mouse-driven look so the aim stays responsive. On entering mouselook the
            // smoothing eases from the previous (third-person) eye, giving the
            // zoom-through glide.
            let eye = if rig.seeded {
                let t = 1.0 - 0.5_f32.powf(dt / MOUSELOOK_EYE_HALF_LIFE);
                vlerp(rig.smoothed_eye, desired_eye, t)
            } else {
                desired_eye
            };
            rig.smoothed_eye = eye;
            rig.smoothed_focus = vadd(eye, look_forward);
            rig.seeded = true;
            // Publish the heading for the avatar body to follow.
            aim_out.sl_yaw = sl_heading_from_bevy_forward(look_forward);
            let mut posed = Transform::from_translation(eye);
            posed.rotation = look;
            *transform = posed;
        }
        CameraMode::ThirdPerson => {
            let (mut eye, focus) = match *focus_target {
                // Focus on a picked point: the camera keeps the world offset it had
                // when the point was picked (and as orbit / zoom has changed it
                // since), so alt-clicking re-pivots around the object *without*
                // moving the camera — the reference's `setFocusGlobal` behaviour.
                FocusTarget::Point(point) => (vadd(point, rig.point_offset), point),
                // Rear-view follow: orbit around the avatar's **head**, so zooming
                // in converges on the back of the head (and into mouselook), not the
                // avatar root.
                FocusTarget::Avatar => {
                    let Some((anchor, facing)) = avatar_pose else {
                        return;
                    };
                    let head = own_avatar_head(&identity, &avatars, body.as_deref(), &globals);
                    let focus = third_person_focus(head, anchor, facing);
                    let eye =
                        third_person_eye(focus, facing, rig.azimuth, rig.elevation, rig.distance);
                    (eye, focus)
                }
            };
            // Camera collision: pull the eye in toward the focus if the line of
            // sight is obstructed, so the camera does not clip through a wall.
            eye = collide_camera(&mut ray_cast, focus, eye);
            apply_pose(&mut transform, &mut rig, eye, focus, &time, false);
        }
    }
}

/// Ease the camera from its smoothed pose toward `(eye, focus)` and write the
/// transform, seeding (snapping) on the first frame so it does not glide in from
/// the origin. `snap` bypasses the smoothing (mouselook, where a lag reads as
/// sluggish aim).
fn apply_pose(
    transform: &mut Transform,
    rig: &mut CameraRig,
    eye: Vec3,
    focus: Vec3,
    time: &Time,
    snap: bool,
) {
    let (final_eye, final_focus) = if !rig.seeded || snap {
        (eye, focus)
    } else {
        let dt = time.delta_secs();
        let t = 1.0 - 0.5_f32.powf(dt / SMOOTH_HALF_LIFE);
        (
            vlerp(rig.smoothed_eye, eye, t),
            vlerp(rig.smoothed_focus, focus, t),
        )
    };
    rig.smoothed_eye = final_eye;
    rig.smoothed_focus = final_focus;
    rig.seeded = true;
    // A degenerate eye==focus (fully zoomed in) would make `looking_at` fail; nudge
    // the focus a hair forward along the previous forward in that case.
    let dir = vsub(final_focus, final_eye);
    let target = if dir.length_squared() > 1.0e-6 {
        final_focus
    } else {
        vadd(final_eye, transform.forward().as_vec3())
    };
    *transform = Transform::from_translation(final_eye).looking_at(target, Vec3::Y);
}

/// Pull the camera `eye` in toward `focus` if a world surface obstructs the line
/// of sight, leaving [`COLLISION_PADDING`] of clearance — the reference's
/// occlusion pushback. Casts from the focus outward (so the near surface, not a far
/// one, is what limits the camera).
fn collide_camera(ray_cast: &mut MeshRayCast, focus: Vec3, eye: Vec3) -> Vec3 {
    let offset = vsub(eye, focus);
    let distance = offset.length();
    let Some(direction) = Dir3::new(offset).ok() else {
        return eye;
    };
    let ray = Ray3d::new(focus, direction);
    let settings = MeshRayCastSettings::default();
    let Some((_entity, hit)) = ray_cast.cast_ray(ray, &settings).first() else {
        return eye;
    };
    if hit.distance < distance {
        let pulled = (hit.distance - COLLISION_PADDING).max(0.0);
        vadd(focus, vscale(direction.as_vec3(), pulled))
    } else {
        eye
    }
}

/// The own avatar's Bevy world position (its body-root anchor) and **stable**
/// facing. `None` until the avatar has spawned.
///
/// The facing comes from the avatar's reported **heading** ([`AvatarMotion::yaw`]),
/// not from a skeleton joint's rotation: the chest / upper-body joints sway with
/// the idle animation, and following that swings the third-person camera
/// left-and-right. The heading is the body yaw, which is what the reference camera
/// tracks. The anchor's own rotation is the fallback when no motion is tracked.
fn own_avatar_pose(
    identity: &SlIdentity,
    avatars: &AvatarState,
    globals: &AvatarPoseQuery,
    motions: &Query<&AvatarMotion>,
) -> Option<(Vec3, Vec3)> {
    let agent = identity.agent_id?;
    let anchor = avatars.body_root_of(agent)?;
    let global = globals.get(anchor).ok()?;
    let facing = motions.get(anchor).map_or_else(
        |_error| global.rotation().mul_vec3(Vec3::X),
        |motion| facing_from_yaw(motion.yaw()),
    );
    Some((global.translation(), facing))
}

/// The own avatar's head-joint (`mHead`) world position, for the third-person
/// focus and the mouselook eye — correct even when the avatar is sitting or
/// otherwise not upright. `None` if no rigged head is available (a
/// placeholder-sphere avatar), where the caller falls back to a head-height offset
/// above the anchor.
fn own_avatar_head(
    identity: &SlIdentity,
    avatars: &AvatarState,
    body: Option<&AvatarBody>,
    globals: &AvatarPoseQuery,
) -> Option<Vec3> {
    let agent = identity.agent_id?;
    let index = body?.joint_index("mHead")?;
    let head = avatars.joint_entities_of(agent)?.get(index)?;
    globals.get(*head).ok().map(GlobalTransform::translation)
}

/// The Second Life heading (yaw about the SL up axis) a Bevy-space forward points
/// along — the inverse of [`facing_from_yaw`], for publishing the mouselook aim to
/// the avatar body.
fn sl_heading_from_bevy_forward(forward: Vec3) -> f32 {
    // Bevy `(x, y, z)` maps back to Second Life `(x, -z, y)`; the heading is the
    // atan2 of the horizontal region components.
    (-forward.z).atan2(forward.x)
}

#[cfg(test)]
mod tests {
    use super::{
        CAMERA_OFFSET, CameraRig, facing_from_yaw, flatten, sl_heading_from_bevy_forward,
        third_person_eye,
    };
    use bevy::math::Vec3;

    /// The default rig reproduces the reference rear-view offset: with the focus at
    /// the origin the camera lands 3 m behind and 0.75 m up, matching
    /// `CameraOffsetRearView`.
    #[test]
    fn default_rig_matches_reference_rear_view() {
        let rig = CameraRig::default();
        assert!((rig.azimuth).abs() < 1.0e-6, "rear view is dead behind");
        assert!((rig.distance - CAMERA_OFFSET.length()).abs() < 1.0e-6);
        // A north-facing avatar with the focus at the origin: the camera is behind
        // (further +Z) and above.
        let facing = Vec3::NEG_Z; // Bevy forward
        let eye = third_person_eye(Vec3::ZERO, facing, rig.azimuth, rig.elevation, rig.distance);
        // Camera 0.75 m above the focus and |CameraOffset.x| = 3 m behind it.
        assert!(
            (eye.y - CAMERA_OFFSET.z).abs() < 1.0e-3,
            "camera 0.75 m above focus: {eye:?}"
        );
        assert!(
            (eye.z - CAMERA_OFFSET.x.abs()).abs() < 1.0e-3,
            "camera 3 m behind focus: {eye:?}"
        );
    }

    /// Orbiting the azimuth by a quarter turn swings the camera to the side without
    /// moving the focus — orbit pivots around the (fixed) focus point.
    #[test]
    fn azimuth_orbits_around_the_focus() {
        let facing = Vec3::NEG_Z;
        let rig = CameraRig::default();
        let rear_eye = third_person_eye(Vec3::ZERO, facing, 0.0, rig.elevation, rig.distance);
        let side_eye = third_person_eye(
            Vec3::ZERO,
            facing,
            core::f32::consts::FRAC_PI_2,
            rig.elevation,
            rig.distance,
        );
        // The camera swung sideways (its X moved off the centre line the rear view
        // sat on); the focus (origin) is unchanged by construction.
        assert!(
            rear_eye.x.abs() < 1.0e-3,
            "rear view is centred: {rear_eye:?}"
        );
        assert!(side_eye.x.abs() > 1.0, "orbited to the side: {side_eye:?}");
    }

    /// The facing round-trips: a Second Life heading to a Bevy forward and back.
    #[test]
    fn facing_round_trips_through_the_axis_map() {
        for yaw in [0.0_f32, 0.5, 1.5, -2.0, 3.0] {
            let forward = facing_from_yaw(yaw);
            // Flattened facing is unit length and horizontal.
            assert!((forward.y).abs() < 1.0e-6, "facing is horizontal");
            let back = sl_heading_from_bevy_forward(forward);
            let (s0, c0) = yaw.sin_cos();
            let (s1, c1) = back.sin_cos();
            assert!(
                (s0 - s1).abs() < 1.0e-4 && (c0 - c1).abs() < 1.0e-4,
                "yaw {yaw} → {back}"
            );
        }
    }

    /// A straight-up facing flattens to a stable default rather than collapsing to
    /// zero, so the camera never loses its heading.
    #[test]
    fn flatten_guards_a_vertical_facing() {
        let flat = flatten(Vec3::Y);
        assert!((flat.length() - 1.0).abs() < 1.0e-6, "still unit length");
        assert!(flat.y.abs() < 1.0e-6, "and horizontal");
    }

    /// `aim_along` sets yaw/pitch so the reconstructed forward matches the input
    /// direction.
    #[test]
    fn aim_along_reconstructs_the_direction() {
        use bevy::math::{EulerRot, Quat};
        let mut rig = CameraRig::default();
        let dir = Vec3::new(1.0, 0.5, -2.0).normalize();
        rig.aim_along(dir);
        let forward =
            Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, 0.0).mul_vec3(Vec3::NEG_Z);
        assert!(
            forward.abs_diff_eq(dir, 1.0e-4),
            "aim {forward:?} vs {dir:?}"
        );
    }

    /// The mode toggles switch as expected: mouselook and flycam each toggle into
    /// their mode from third person and back out again. This is the spine of the
    /// seamless transitions, so it is pinned.
    #[test]
    fn mode_toggles_switch_the_camera_mode() {
        use super::{
            Action, CameraMode, FocusTarget, SpacenavInput, ViewerCamera, switch_camera_mode,
        };
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;

        // Toggle `action` from `start` and assert the mode lands on `want`.
        let run = |start: CameraMode, action: Action, want: CameraMode| {
            let mut app = App::new();
            app.insert_resource(start)
                .init_resource::<FocusTarget>()
                .init_resource::<ButtonInput<Action>>()
                .init_resource::<SpacenavInput>()
                .add_systems(Update, switch_camera_mode);
            app.world_mut()
                .spawn((ViewerCamera, CameraRig::default(), Transform::default()));
            app.world_mut()
                .resource_mut::<ButtonInput<Action>>()
                .press(action);
            app.update();
            assert_eq!(
                *app.world().resource::<CameraMode>(),
                want,
                "{start:?} + {action:?} should give {want:?}"
            );
        };

        run(
            CameraMode::ThirdPerson,
            Action::ToggleMouselook,
            CameraMode::Mouselook,
        );
        run(
            CameraMode::Mouselook,
            Action::ToggleMouselook,
            CameraMode::ThirdPerson,
        );
        run(
            CameraMode::ThirdPerson,
            Action::ToggleFlycam,
            CameraMode::Flycam,
        );
        run(
            CameraMode::Flycam,
            Action::ToggleFlycam,
            CameraMode::ThirdPerson,
        );
    }
}
