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

use crate::raycast_index::{DynamicColliders, StaticRaycastIndex};
use bevy::diagnostic::{Diagnostic, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::avatars::{AvatarState, SeatChainQuery, seat_world_transform};
use crate::coords::{bevy_to_sl_vec, sl_to_bevy_vec};
use crate::input_action::{Action, InputMode};
use crate::input_context::InputContext;
use crate::spacenav::{FlycamAxisSettings, SpacenavInput};
use crate::water::{WaterOcean, WaterRegionPlane};
use crate::world_api::{
    AvatarMotion, CameraMode, CameraRig, MAX_DISTANCE, MAX_PITCH, MOUSELOOK_CROSS_DISTANCE,
    ViewerCamera,
};
use sl_client_bevy::{SlIdentity, Vector};

/// The agent-frame focus offset (forward, left, up metres) used only as the
/// **fallback** third-person focus for a placeholder-sphere avatar with no head
/// joint — one metre ahead of and above the anchor. A rigged avatar focuses on its
/// actual head instead ([`third_person_focus`]).
const FOCUS_OFFSET: Vec3 = Vec3::new(1.0, 0.0, 1.0);

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

/// Below this squared positional delta (`(0.5 mm)^2`) the smoothed camera pose
/// is treated as settled and `apply_pose` leaves the transform untouched, so a
/// parked camera stops re-writing (and thus `Changed`-marking) its transform
/// every frame — see the write guard in `apply_pose`.
const CAMERA_SETTLE_POS_EPSILON_SQ: f32 = 0.000_000_25;

/// The rotational companion to [`CAMERA_SETTLE_POS_EPSILON_SQ`] (radians,
/// ~0.017°): below this the settled camera's transform is left untouched.
const CAMERA_SETTLE_ROT_EPSILON: f32 = 0.000_3;

/// The mouselook eye's smoothing half-life (seconds) — short, so the first-person
/// aim stays responsive, but enough to filter out the animated head joint's
/// per-frame vibration (the idle-animation micro-motion that otherwise shakes the
/// whole view).
const MOUSELOOK_EYE_HALF_LIFE: f32 = 0.06;

/// The clearance kept between the third-person camera and an obstruction it would
/// otherwise clip through, so the pulled-in camera sits just short of the wall.
const COLLISION_PADDING: f32 = 0.2;

/// The user-tunable camera parameters, refreshed every frame from the typed
/// settings store by the camera & movement preferences tab
/// (`crate::preferences_camera_move`). The defaults reproduce the module
/// constants exactly, so a run without a settings store (the gallery, headless
/// tests) behaves as it always did.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CameraTuning {
    /// Multiplier on the third-person orbit distance — the reference's
    /// `CameraOffsetScale`, scaling how far the camera sits from the avatar
    /// without changing the orbit angles.
    pub offset_scale: f32,
    /// The camera-pose smoothing half-life (seconds) `apply_pose` eases with;
    /// `0` snaps. Replaces the fixed `SMOOTH_HALF_LIFE`.
    pub smoothing_half_life: f32,
    /// The farthest the third-person camera zooms from the avatar (metres) —
    /// the reference's `MAX_CAMERA_DISTANCE_FROM_AGENT`, replacing the fixed
    /// `MAX_DISTANCE`.
    pub max_distance: f32,
    /// When set, the mouse wheel no longer zooms the third-person camera (the
    /// reference's `FSDisableMouseWheelCameraZoom`); alt-drag zoom still works.
    pub wheel_zoom_disabled: bool,
    /// Radians of mouselook yaw / pitch per pixel of mouse motion — the
    /// reference's `MouseSensitivity` mapped to our units, replacing the fixed
    /// `AIM_SENSITIVITY` for mouselook (the flycam right-drag keeps the
    /// constant).
    pub mouselook_sensitivity_rad_per_px: f32,
    /// Invert the mouselook pitch axis (mouse up looks down) — the reference's
    /// `InvertMouse`. Applies to mouselook only, as the reference does.
    pub invert_mouse_y: bool,
}

impl Default for CameraTuning {
    /// Today's constants — the out-of-the-box behaviour is unchanged.
    fn default() -> Self {
        Self {
            offset_scale: 1.0,
            smoothing_half_life: SMOOTH_HALF_LIFE,
            max_distance: MAX_DISTANCE,
            wheel_zoom_disabled: false,
            mouselook_sensitivity_rad_per_px: AIM_SENSITIVITY,
            invert_mouse_y: false,
        }
    }
}

/// Which of the camera's own axes an [auto-rotation](CameraSpin) spins about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum SpinAxis {
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
#[derive(Debug, Resource, Default)]
pub struct CameraSpin {
    /// Radians per second to auto-rotate; `0.0` disables the spin.
    pub rate: f32,
    /// Which of the camera's axes the spin rotates about.
    pub axis: SpinAxis,
}

/// A debug affordance: place the camera at an absolute flycam pose on startup
/// (from `--camera-position` / `--camera-look-at`) so an unattended capture frames
/// a fixed viewpoint. A `None` [`position`](Self::position) keeps the default
/// third-person follow.
#[derive(Debug, Resource, Default)]
pub struct CameraStart {
    /// The absolute Bevy-space camera position, or `None` to follow the agent.
    pub position: Option<Vec3>,
    /// The Bevy-space look direction (un-normalised is fine), or `None` to keep
    /// the default forward aim.
    pub look: Option<Vec3>,
}

/// What the third-person camera orbits around.
///
/// The alt-zoom focus tool (`lltoolfocus`) moves this off the avatar onto a picked
/// object; a vehicle would move it onto the seat. Defaults to the avatar.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub enum FocusTarget {
    /// The agent's own avatar (rear-view follow).
    #[default]
    Avatar,
    /// A fixed world point (the alt-click focus of `lltoolfocus`).
    Point(Vec3),
}

/// A debug affordance (env `SL_VIEWER_CAMERA_DUMP`): log the current camera pose and the
/// smoothed frame rate once a second. The pose is emitted as a ready-to-paste
/// `--camera-position X,Y,Z --camera-look-at X,Y,Z`, so an operator can fly to a chosen
/// viewpoint (e.g. framing a particle fountain for a repeatable benchmark or screenshot)
/// and then pin that exact pose on later runs — the pose the CLI wants is not otherwise
/// shown anywhere. Coordinates are emitted in Second Life region-local space (Z-up
/// metres), the same convention `--camera-position` parses; the FPS lets an A/B benchmark
/// be read straight from the log rather than off the status bar.
///
/// Off by default, throttled to 1 Hz, and a plain main-world read, so it does not perturb
/// render-world timing.
pub fn dump_camera_pose(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    mut enabled: Local<Option<bool>>,
    mut timer: Local<f32>,
) {
    let on = *enabled.get_or_insert_with(|| std::env::var_os("SL_VIEWER_CAMERA_DUMP").is_some());
    if !on {
        return;
    }
    *timer += time.delta_secs();
    if *timer < 1.0 {
        return;
    }
    *timer = 0.0;
    let Ok(global) = camera.single() else {
        return;
    };
    let pos = global.translation();
    // A point ten metres down the view ray; `--camera-look-at` only needs a point on the
    // ray, so the exact distance is irrelevant.
    let forward = global.forward();
    let look = Vec3::new(
        pos.x + forward.x * 10.0,
        pos.y + forward.y * 10.0,
        pos.z + forward.z * 10.0,
    );
    let sl_pos = bevy_to_sl_vec(pos);
    let sl_look = bevy_to_sl_vec(look);
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(Diagnostic::smoothed)
        .unwrap_or(0.0);
    info!(
        "benchmark: fps={fps:.1} --camera-position {:.2},{:.2},{:.2} --camera-look-at {:.2},{:.2},{:.2}",
        sl_pos.x, sl_pos.y, sl_pos.z, sl_look.x, sl_look.y, sl_look.z,
    );
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
pub struct CameraAim {
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
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    /// Wire the camera systems. `sync_input_mode` runs in `PreUpdate` so the action
    /// map's profile matches this frame's mode; the drivers and [`position_camera`]
    /// run in `Update`, in order, so every `.after(position_camera)` consumer (sky,
    /// water, particles, interest reporting) reads the finished pose.
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .init_resource::<FocusTarget>()
            .init_resource::<CameraAim>()
            .init_resource::<CameraTuning>()
            .init_resource::<FlycamSmoothing>()
            .add_systems(PreUpdate, sync_input_mode)
            .add_systems(
                Update,
                (
                    switch_camera_mode,
                    reset_camera_view,
                    // Each mode driver runs only in its own mode (the internal
                    // early-returns keep the `context.is_world()` half of the
                    // gate). `focus_on_object` stays ungated: its
                    // movement-resets-focus branch applies in every mode.
                    orbit_third_person.run_if(resource_equals(CameraMode::ThirdPerson)),
                    aim_look.run_if(resource_equals(CameraMode::Mouselook)),
                    focus_on_object,
                    drive_flycam.run_if(resource_equals(CameraMode::Flycam)),
                    position_camera,
                )
                    .chain()
                    // Run after the avatar dead-reckoner so `position_camera` reads
                    // the anchor's Transform *after* this frame's motion is applied —
                    // otherwise the follow trails the avatar by a frame (metres, at
                    // fly speed). `drive_avatar_motion` is itself after the avatar
                    // object update, so the anchor is fully current here.
                    .after(crate::physics::drive_avatar_motion),
            )
            .add_systems(Update, update_camera_cursor);
    }
}

/// Derive the action-map `InputMode` from the [`CameraMode`], so a key resolves
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
pub fn update_camera_cursor(
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
/// module docs). The alt-drag focus point is set by `focus_on_object`.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode / focus / \
              context state, the user camera tuning, the Alt / Ctrl modifiers, the mouse button \
              and motion, the wheel, and the camera rig"
)]
pub(crate) fn orbit_third_person(
    mut mode: ResMut<CameraMode>,
    focus: Res<FocusTarget>,
    tuning: Res<CameraTuning>,
    context: Res<InputContext>,
    keyboard: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    hover_map: Res<bevy::picking::hover::HoverMap>,
    pickables: Query<&Pickable>,
    node_sizes: Query<&ComputedNode>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    // A wheel scroll over a blocking UI panel (a floater's scrolling list) scrolls
    // that panel, not the camera — `InputContext` is focus-based, so hovering a
    // list does not leave the world context, and without this the wheel would
    // both scroll the list and zoom the camera. The wheel-zoom preference gates
    // only the wheel: an alt-drag zoom still works with it off.
    let over_ui = crate::hud_pick::pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes);
    let scroll = if over_ui || tuning.wheel_zoom_disabled {
        0.0
    } else {
        scroll_notches(&wheel)
    };
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
                    rig.distance = next.clamp(MOUSELOOK_CROSS_DISTANCE, tuning.max_distance);
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
                    let next =
                        (length * factor).clamp(MOUSELOOK_CROSS_DISTANCE, tuning.max_distance);
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
/// `drive_flycam` (with a local-frame quaternion, so it has no gimbal lock).
pub(crate) fn aim_look(
    mut mode: ResMut<CameraMode>,
    context: Res<InputContext>,
    tuning: Res<CameraTuning>,
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
    let (yaw_delta, pitch_delta) = aim_deltas(
        motion.delta,
        tuning.mouselook_sensitivity_rad_per_px,
        tuning.invert_mouse_y,
    );
    rig.yaw += yaw_delta;
    rig.pitch = (rig.pitch + pitch_delta).clamp(-MAX_PITCH, MAX_PITCH);
    // Scroll out of mouselook back into third person, dropping just outside the head.
    if scroll < 0.0 {
        *mode = CameraMode::ThirdPerson;
    }
}

/// The mouselook yaw / pitch deltas (radians) a mouse motion `delta` produces at
/// `sensitivity` radians per pixel. Mouse right always yaws right (negative yaw);
/// `invert_y` flips only the pitch axis, so mouse up looks down — the reference's
/// `InvertMouse`. Pure, so the sensitivity scaling and the invert are
/// unit-testable.
fn aim_deltas(delta: Vec2, sensitivity: f32, invert_y: bool) -> (f32, f32) {
    let pitch_sign = if invert_y { 1.0 } else { -1.0 };
    (-delta.x * sensitivity, pitch_sign * delta.y * sensitivity)
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

/// The own avatar anchor's **current-frame** local `Transform`. The body-root
/// anchor is a top-level entity (spawned with no parent), so its `Transform` is its
/// world pose — and, crucially, it is the value written *this* frame by the
/// avatar-motion systems, whereas its `GlobalTransform` is only recomputed in
/// `PostUpdate` and so lags a frame. Reading the `Transform` (with
/// [`position_camera`] ordered after `drive_avatar_motion`) lets the follow track
/// the avatar's live position with **no frame lag** — at fast fly speeds a single
/// frame of staleness is metres, enough to throw the avatar off screen. Filtered
/// `Without<ViewerCamera>` so it never overlaps the camera's own `&mut Transform`.
type AvatarTransformQuery<'world, 'state> =
    Query<'world, 'state, &'static Transform, Without<ViewerCamera>>;

/// Compute and apply the final camera pose for the active mode, easing it toward
/// the target so mode transitions glide.
///
/// - **Third person** follows the avatar (or focus point), pulls in on collision,
///   and looks at the focus.
/// - **Mouselook** sits at the avatar's eyes, aimed by the rig.
/// - **Flycam** is already positioned by `drive_flycam`; this only seeds the
///   smoothed pose so a switch *into* it glides.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mode / focus \
              / aim state, the user camera tuning, the identity and avatar tables to find the own \
              avatar, the transform query, the ray caster for collision, time for the smoothing, \
              and the camera itself"
)]
pub fn position_camera(
    // Bundled into one tuple param: a Bevy system tops out at 16 parameters and
    // this one is full — a tuple of `SystemParam`s is itself a `SystemParam`.
    camera_state: (Res<CameraMode>, Res<FocusTarget>, Res<CameraTuning>),
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    objects: Res<crate::objects::ObjectState>,
    sit_camera: Res<crate::sit_camera::SitCamera>,
    time: Res<Time>,
    globals: AvatarPoseQuery,
    transforms: AvatarTransformQuery,
    // The seat and its linkset ancestors, to compose a scripted sit camera's seat
    // pose from current-frame local transforms (see [`sit_camera_pose`]).
    seat_chain: SeatChainQuery,
    motions: Query<&AvatarMotion>,
    // The own avatar's mesh sub-hierarchy, so the collision ray can ignore the
    // agent's own body (see [`collide_camera`]).
    children: Query<&Children>,
    // Tracks whether the scripted-sit-camera path was active last frame, so the
    // diagnostic below logs only on the transition.
    mut sit_camera_active: Local<bool>,
    // The custom static raycast index for camera collision: a parry BVH broad
    // phase over the prim colliders (maintained off-thread), so [`collide_camera`]
    // tests only colliders near the short head→eye segment instead of every mesh
    // in the scene. Replaces avian's `SpatialQuery`
    // (viewer-perf-custom-static-raycast-index).
    index: Res<StaticRaycastIndex>,
    // The moving (physical-prim) colliders, cast alongside the static index so the
    // camera also occludes on a physical mover.
    dynamic: Res<DynamicColliders>,
    mut aim_out: ResMut<CameraAim>,
    mut cameras: Query<(&mut Transform, &mut CameraRig), With<ViewerCamera>>,
) {
    let (mode, focus_target, tuning) = camera_state;
    let Ok((mut transform, mut rig)) = cameras.single_mut() else {
        return;
    };
    aim_out.mouselook = *mode == CameraMode::Mouselook;

    // The own avatar's live world position and stable (heading-derived) facing, if
    // it has arrived. Read from the current-frame anchor `Transform`, not the
    // frame-late `GlobalTransform`, so the follow does not trail by a frame.
    let avatar_pose = own_avatar_pose(&identity, &avatars, &transforms, &motions);

    // The own avatar's mesh entities (its anchor and the whole rigged-body
    // sub-hierarchy), so [`collide_camera`] does not treat the agent's own body as
    // an occluder. Without this the ray cast from the head focus exits through the
    // skull / hair a few centimetres out and yanks the camera into the head — worst
    // while the walk animation tilts the head into the rearward ray. The reference
    // viewer excludes the agent's own avatar from the same occlusion test.
    let own_avatar_entities: std::collections::HashSet<Entity> = identity
        .agent_id
        .and_then(|agent| avatars.body_root_of(agent))
        .map(|anchor| {
            let mut set: std::collections::HashSet<Entity> =
                children.iter_descendants(anchor).collect();
            set.insert(anchor);
            set
        })
        .unwrap_or_default();

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
            let eye = own_avatar_head(&identity, &avatars, &globals, &transforms)
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
            // A scripted sit camera (the seat set `llSetCamera*Offset`) overrides the
            // ordinary follow while seated and focused on the avatar: the eye and
            // focus ride the seat at the script's offsets. It is not collided (the
            // script's placement is authoritative — e.g. a camera inside a vehicle).
            let sit_pose = matches!(*focus_target, FocusTarget::Avatar)
                .then(|| sit_camera_pose(&sit_camera, &objects, &seat_chain))
                .flatten();
            // Log only when the scripted-sit-camera path engages / disengages (not
            // every frame), so a live run reveals whether the seat a driver rides uses
            // a scripted sit camera (this rigid-seat path) or the ordinary rear-view
            // follow — the two track the vehicle differently.
            let sit_now = sit_pose.is_some();
            if sit_now != *sit_camera_active {
                *sit_camera_active = sit_now;
                debug!(
                    "camera: scripted sit-camera path {}",
                    if sit_now {
                        "engaged (rigid seat follow)"
                    } else {
                        "disengaged"
                    }
                );
            }
            let (mut eye, focus, follow_avatar, collide) = match (sit_pose, *focus_target) {
                // Scripted sit camera: fixed offsets from the (moving) seat, tracked
                // rigidly like a follow, never collided.
                (Some((eye, focus)), _) => (eye, focus, true, false),
                // Focus on a picked point: the camera keeps the world offset it had
                // when the point was picked (and as orbit / zoom has changed it
                // since), so alt-clicking re-pivots around the object *without*
                // moving the camera — the reference's `setFocusGlobal` behaviour. A
                // fixed world point is smoothed in world space (there is nothing to
                // trail), so it is not an avatar follow.
                (None, FocusTarget::Point(point)) => {
                    (vadd(point, rig.point_offset), point, false, true)
                }
                // Rear-view follow: orbit around the avatar's **head**, so zooming
                // in converges on the back of the head (and into mouselook), not the
                // avatar root. The focus is recomputed from the live avatar every
                // frame and followed **rigidly** (only the orbit offset is smoothed),
                // so the camera and avatar stay a locked pair — no trailing on a
                // sustained vertical flight.
                (None, FocusTarget::Avatar) => {
                    let Some((anchor, facing)) = avatar_pose else {
                        return;
                    };
                    let head = own_avatar_head(&identity, &avatars, &globals, &transforms);
                    let focus = third_person_focus(head, anchor, facing);
                    // The user's `CameraOffsetScale` multiplies the orbit distance
                    // (not the angles), pushing the whole rear view in or out.
                    let eye = third_person_eye(
                        focus,
                        facing,
                        rig.azimuth,
                        rig.elevation,
                        rig.distance * tuning.offset_scale,
                    );
                    (eye, focus, true, true)
                }
            };
            // Camera collision: pull the eye in toward the focus if the line of
            // sight is obstructed, so the camera does not clip through a wall.
            if collide {
                eye = collide_camera(&index, &dynamic, focus, eye, &own_avatar_entities);
            }
            apply_pose(
                &mut transform,
                &mut rig,
                eye,
                focus,
                follow_avatar,
                &time,
                tuning.smoothing_half_life,
                false,
            );
        }
    }
}

/// Ease the camera from its smoothed pose toward `(eye, focus)` and write the
/// transform, seeding (snapping) on the first frame so it does not glide in from
/// the origin. `half_life` is the exponential easing's half-life in seconds
/// ([`CameraTuning::smoothing_half_life`]); zero (or less) snaps every frame.
/// `snap` bypasses the smoothing (mouselook, where a lag reads as sluggish aim).
///
/// `follow_avatar` selects **rigid follow**: the focus is taken from the live
/// avatar every frame with no world-space easing, and only the eye's **offset from
/// the focus** — the orbit / zoom / collision geometry — is smoothed. So the camera
/// and avatar move as a **locked pair**: they can jump together against the world
/// (whatever the avatar's rendered position does, the camera does too), but never
/// drift relative to each other — a sustained vertical flight has zero follow lag,
/// as the reference viewer does. `false` (a fixed focus point) smooths the whole
/// pose in world space as before — a static point has nothing to trail.
#[expect(
    clippy::too_many_arguments,
    reason = "the camera pose write needs the transform and rig it writes, the desired eye / \
              focus pair, the follow mode, the frame time, and the two smoothing controls — \
              bundling them into a struct for one internal call site would only obscure it"
)]
fn apply_pose(
    transform: &mut Transform,
    rig: &mut CameraRig,
    eye: Vec3,
    focus: Vec3,
    follow_avatar: bool,
    time: &Time,
    half_life: f32,
    snap: bool,
) {
    let (final_eye, final_focus) = if !rig.seeded || snap {
        (eye, focus)
    } else {
        let dt = time.delta_secs();
        // A non-positive half-life means "no smoothing": snap the full way. (The
        // formula would otherwise hit 0/0 = NaN on a zero-dt frame.)
        let t = if half_life > 0.0 {
            1.0 - 0.5_f32.powf(dt / half_life)
        } else {
            1.0
        };
        if follow_avatar {
            // Rigid follow: the focus tracks the avatar this frame (no world-space
            // easing, so the camera never trails the body's translation), and only
            // the eye's offset from the focus is eased — the orbit / zoom / collision
            // geometry — so those changes still glide while the follow stays locked.
            let previous_offset = vsub(rig.smoothed_eye, rig.smoothed_focus);
            let offset = vlerp(previous_offset, vsub(eye, focus), t);
            (vadd(focus, offset), focus)
        } else {
            (
                vlerp(rig.smoothed_eye, eye, t),
                vlerp(rig.smoothed_focus, focus, t),
            )
        }
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
    let new_transform = Transform::from_translation(final_eye).looking_at(target, Vec3::Y);
    // Only write when the pose actually moved beyond a sub-perceptible epsilon.
    // The exponential smoothing above approaches its target asymptotically and
    // never settles *exactly*, so an unguarded write would mark the camera
    // `Changed` every single frame even when parked — which defeats every
    // change-driven consumer that gates on camera movement (e.g. the async
    // shadow-cull dispatch). A `snap` always writes.
    let settled = transform
        .translation
        .distance_squared(new_transform.translation)
        <= CAMERA_SETTLE_POS_EPSILON_SQ
        && transform.rotation.angle_between(new_transform.rotation) <= CAMERA_SETTLE_ROT_EPSILON;
    if snap || !settled {
        *transform = new_transform;
    }
}

/// Pull the camera `eye` in toward `focus` if a world surface obstructs the line
/// of sight, leaving [`COLLISION_PADDING`] of clearance — the reference's
/// occlusion pushback. Casts from the focus outward (so the near surface, not a
/// far one, is what limits the camera).
///
/// Uses the custom [`StaticRaycastIndex`] BVH (plus the moving-collider set)
/// rather than a whole-scene `MeshRayCast`: the ray is bounded to the short
/// head→eye segment (`max_distance = distance`), so the broad phase only tests the
/// few colliders near it, not every mesh in the scene (which at crowd scale cost
/// tens of milliseconds — the third-person `position_camera` spike,
/// [[viewer-perf-custom-static-raycast-index]]). The index holds a collider for
/// *every* prim (`build_static_colliders`), so the
/// camera occludes on all of them — phantom and physics-shape-`None` prims
/// included (they are visually solid); the query uses all collision layers. The
/// one thing with no collider is an **avatar**, so the camera never pulls in for
/// another avatar walking behind you (the reference behaviour).
fn collide_camera(
    index: &StaticRaycastIndex,
    dynamic: &DynamicColliders,
    focus: Vec3,
    eye: Vec3,
    ignore: &std::collections::HashSet<Entity>,
) -> Vec3 {
    let offset = vsub(eye, focus);
    let distance = offset.length();
    let Some(direction) = Dir3::new(offset).ok() else {
        return eye;
    };
    // Camera collision is *visual* occlusion: the camera pulls in at any opaque
    // surface, so the ray tests **all** layers of the shared index — including
    // phantom / physics-shape-`None` prims (in the `NonSolid` layer), which are
    // still visually solid (the whole-scene `MeshRayCast` this replaced hit them
    // too). Only the own avatar is excluded (its worn rigid attachments can carry
    // colliders; the focus sits at the head, so a nearby own-attachment collider
    // would otherwise pull the camera in). Other avatars carry no collider, so they
    // are excluded for free — the reference does not pull in for them.
    //
    // `solid = false` (treat colliders as **hollow**): the prim colliders are solid
    // volumes (cuboids, mesh convex hulls, a prim's transient bounding-box
    // placeholder), and when the head focus sits *inside* one — the avatar standing
    // in a building, or a large prim's placeholder cuboid enveloping it — a solid
    // cast returns the ray origin itself (`distance == 0`) and slams the eye into
    // the head. Hollow casting reports the collider's boundary instead: from
    // *outside* a wall it still hits the near surface (so wall pushback is
    // unchanged — the whole point), and from *inside* a volume it reports the far
    // exit rather than the origin, so the camera is not yanked in. This matches the
    // old visual-mesh cast, whose surfaces had no solid interior. Bounded to
    // `distance` so only the head→eye segment is tested.
    // `solid_only = false`: the camera occludes on every indexed collider,
    // including phantom / physics-shape-`None` prims (still visually solid). The
    // own avatar's entities are excluded so a nearby own-attachment collider does
    // not pull the camera into the head; other avatars carry no collider.
    let ray = direction.as_vec3();
    let static_hit = index.cast_ray(focus, ray, distance, false, false, ignore);
    let dynamic_hit = dynamic.cast_ray(focus, ray, distance, false, false, ignore);
    // Nearest of the static-index and moving-collider hits.
    let hit_distance = match (static_hit, dynamic_hit) {
        (Some(a), Some(b)) => a.min(b),
        (hit, None) | (None, hit) => match hit {
            Some(distance) => distance,
            None => return eye,
        },
    };
    let pulled = (hit_distance - COLLISION_PADDING).max(0.0);
    vadd(focus, vscale(ray, pulled))
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
    transforms: &AvatarTransformQuery,
    motions: &Query<&AvatarMotion>,
) -> Option<(Vec3, Vec3)> {
    let agent = identity.agent_id?;
    let anchor = avatars.body_root_of(agent)?;
    // Seated on an object, the anchor still holds its **world** pose (a top-level
    // entity), written this frame by `place_seated_avatars` from the seat's
    // current-frame transform — so read its local `Transform` (this frame's value),
    // not the frame-late `GlobalTransform`, and take the facing from that world
    // orientation. Reading the stale global here was the seated-vehicle rubber-band
    // (`viewer-seated-avatar-vehicle-rubberband`).
    if avatars.is_seated(agent) {
        let transform = transforms.get(anchor).ok()?;
        return Some((transform.translation, transform.rotation.mul_vec3(Vec3::X)));
    }
    // Standing, the anchor is a root entity, so its local `Transform` is its world
    // pose — and it is this frame's value (unlike the frame-late `GlobalTransform`),
    // so the camera follows the avatar's live position without a frame of drift.
    let transform = transforms.get(anchor).ok()?;
    let facing = motions.get(anchor).map_or_else(
        |_error| transform.rotation.mul_vec3(Vec3::X),
        |motion| facing_from_yaw(motion.yaw()),
    );
    Some((transform.translation, facing))
}

/// The scripted sit camera's world `(eye, focus)`, or `None` when no sit camera is
/// set or its seat is not currently in the scene. The seat set eye / at offsets in
/// its own Second Life frame; the seat entity's world transform carries the single
/// SL→Bevy basis change, so [`Transform::transform_point`] composes each offset onto
/// the seat's live world pose — the reference's `object_pos + mSitCameraPos *
/// object_rot` / `object_pos + mSitCameraFocus * object_rot`.
///
/// The seat's world pose is composed **this frame** from the chain of local
/// [`Transform`]s up its `ChildOf` parents ([`seat_world_transform`]), *not* the
/// seat's [`GlobalTransform`] (which Bevy only recomputes in `PostUpdate`, so it is a
/// frame stale). Reading the stale global made the scripted sit camera trail the
/// vehicle by a frame, so the vehicle **wobbled in the driver's view** on each of the
/// object's dead-reckon / snap corrections — the sit-camera counterpart of the
/// seated-rider fix ([[viewer-seated-avatar-vehicle-rubberband]]), which already
/// composes the rider from the current-frame seat locals. Composing the camera the
/// same way locks the driver's viewpoint rigidly to the seat, so the vehicle holds
/// its place on screen and only the world jitters past it
/// ([[viewer-physical-object-motion-not-smooth]]).
fn sit_camera_pose(
    sit_camera: &crate::sit_camera::SitCamera,
    objects: &crate::objects::ObjectState,
    chain: &SeatChainQuery,
) -> Option<(Vec3, Vec3)> {
    let (seat, eye_offset, at_offset) = sit_camera.offsets()?;
    let seat_entity = objects.entity_of(seat)?;
    let seat_world = seat_world_transform(seat_entity, chain)?;
    Some((
        seat_world.transform_point(eye_offset),
        seat_world.transform_point(at_offset),
    ))
}

/// The own avatar's head-joint (`mHead`) world position, for the third-person
/// focus and the mouselook eye — correct even when the avatar is sitting or
/// otherwise not upright. `None` if no rigged head is available (a
/// placeholder-sphere avatar), where the caller falls back to a head-height offset
/// above the anchor.
fn own_avatar_head(
    identity: &SlIdentity,
    avatars: &AvatarState,
    globals: &AvatarPoseQuery,
    transforms: &AvatarTransformQuery,
) -> Option<Vec3> {
    let agent = identity.agent_id?;
    let anchor = avatars.body_root_of(agent)?;
    // The head-focus socket (§5.4): an avatar-root child the pose driver places
    // at the posed `mHead` joint, so the camera holds the animated head without a
    // head joint entity.
    let head = avatars.head_socket_of(agent)?;
    // The socket's world pose is available through its `GlobalTransform` — which
    // lags a frame (its local is written in `PostUpdate`, propagated next frame).
    // Correct it by the anchor's own motion this frame (current `Transform` minus
    // frame-late `GlobalTransform`, both taken from the same root anchor) so the
    // head focus tracks the avatar's live position — the per-frame head sway
    // relative to the anchor is negligible (measured `d_head ≈ d_root`).
    let head_global = globals.get(head).ok()?.translation();
    let anchor_global = globals.get(anchor).ok()?.translation();
    let anchor_now = transforms.get(anchor).ok()?.translation;
    Some(vadd(head_global, vsub(anchor_now, anchor_global)))
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
    use super::{facing_from_yaw, flatten, sl_heading_from_bevy_forward, third_person_eye};
    use crate::world_api::{CAMERA_OFFSET, CameraRig};
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

    /// A third-person camera following the avatar tracks a **sustained** vertical
    /// flight with **zero** steady-state lag: rigid follow re-derives the focus from
    /// the avatar every frame and smooths only the eye's offset from it, so a
    /// constant climb leaves the smoothed eye exactly on the desired eye — the
    /// reference's "camera moves up and down with the avatar" — and the eye stays a
    /// fixed offset from the focus (a locked pair). World-space smoothing (a fixed
    /// focus point) instead trails the climb, so the assertion pins the fix rather
    /// than the framework.
    #[test]
    fn follow_has_no_steady_state_vertical_lag() {
        use super::{CameraRig, SMOOTH_HALF_LIFE, apply_pose, vadd, vsub};
        use bevy::math::Vec3;
        use bevy::prelude::{Time, Transform};
        use std::time::Duration;

        let dt = 1.0 / 60.0;
        let mut time = Time::default();
        time.advance_by(Duration::from_secs_f32(dt));

        // The fixed rear-view geometry (eye behind + above the anchor, focus above
        // it) and a steady climb of ~6 m/s (0.1 m per 60 Hz frame).
        let eye_off = Vec3::new(0.0, 0.75, 3.0);
        let focus_off = Vec3::new(0.0, 0.5, 0.0);
        let climb = Vec3::new(0.0, 0.1, 0.0);

        // Run a constant-velocity climb through the smoother and return the final
        // smoothed eye and focus once it has settled.
        let settle = |follow_avatar: bool| -> (Vec3, Vec3, Vec3) {
            let mut rig = CameraRig::default();
            let mut transform = Transform::default();
            let mut anchor = Vec3::new(100.0, 20.0, 50.0);
            // Seed on the first frame (snaps), then climb for long enough to reach
            // steady state.
            apply_pose(
                &mut transform,
                &mut rig,
                vadd(anchor, eye_off),
                vadd(anchor, focus_off),
                follow_avatar,
                &time,
                SMOOTH_HALF_LIFE,
                false,
            );
            for _frame in 0..60 {
                anchor = vadd(anchor, climb);
                apply_pose(
                    &mut transform,
                    &mut rig,
                    vadd(anchor, eye_off),
                    vadd(anchor, focus_off),
                    follow_avatar,
                    &time,
                    SMOOTH_HALF_LIFE,
                    false,
                );
            }
            (rig.smoothed_eye, rig.smoothed_focus, vadd(anchor, eye_off))
        };

        // Following the avatar: the smoothed eye sits exactly on the desired eye (no
        // vertical trailing) and stays the fixed rear-view offset from the focus (a
        // locked pair).
        let (eye, focus, desired) = settle(true);
        assert!(
            vsub(eye, desired).length() < 1.0e-4,
            "following the avatar should have zero steady-state lag"
        );
        assert!(
            vsub(vsub(eye, focus), vsub(eye_off, focus_off)).length() < 1.0e-4,
            "the eye stays a fixed offset from the focus (camera + avatar locked)"
        );
        // World-space smoothing (a fixed focus point) trails the climb by a visible
        // margin (a fraction of a metre), which is the reported bug.
        let (lagging_eye, _focus, desired) = settle(false);
        assert!(
            vsub(lagging_eye, desired).length() > 0.1,
            "world-space smoothing lags a sustained climb (the bug)"
        );
    }

    /// The mouselook aim deltas scale linearly with the sensitivity, and the
    /// invert flag flips **only** the pitch axis — yaw is identical either way, so
    /// inverting can never mirror the horizontal look.
    #[test]
    fn aim_deltas_scale_and_invert_pitch_only() {
        use super::aim_deltas;
        use bevy::math::Vec2;

        let delta = Vec2::new(10.0, -4.0);
        let (yaw, pitch) = aim_deltas(delta, 0.003, false);
        assert!((yaw - (-0.03)).abs() < 1.0e-6, "yaw at 0.003 rad/px: {yaw}");
        assert!((pitch - 0.012).abs() < 1.0e-6, "pitch mouse-up looks up");

        // Double the sensitivity, double both deltas.
        let (yaw2, pitch2) = aim_deltas(delta, 0.006, false);
        assert!((yaw2 - 2.0 * yaw).abs() < 1.0e-6);
        assert!((pitch2 - 2.0 * pitch).abs() < 1.0e-6);

        // Inverting flips the pitch sign and leaves the yaw untouched.
        let (yaw_inv, pitch_inv) = aim_deltas(delta, 0.003, true);
        assert!((yaw_inv - yaw).abs() < 1.0e-6, "invert never touches yaw");
        assert!((pitch_inv + pitch).abs() < 1.0e-6, "invert flips pitch");
    }

    /// The offset scale multiplies the third-person eye's distance from the focus
    /// without changing its direction — the whole rear view slides in / out along
    /// the same ray.
    #[test]
    fn offset_scale_scales_distance_not_direction() {
        let facing = Vec3::NEG_Z;
        let rig = CameraRig::default();
        let near = third_person_eye(Vec3::ZERO, facing, 0.3, rig.elevation, rig.distance);
        let far = third_person_eye(Vec3::ZERO, facing, 0.3, rig.elevation, rig.distance * 2.0);
        assert!(
            (far.length() - 2.0 * near.length()).abs() < 1.0e-4,
            "doubled distance doubles the offset: {near:?} vs {far:?}"
        );
        assert!(
            near.normalize().abs_diff_eq(far.normalize(), 1.0e-5),
            "same direction from the focus"
        );
    }

    /// A zero smoothing half-life snaps the eased pose straight onto the desired
    /// pose (no residual glide), which is what the preferences slider's `0` means.
    #[test]
    fn zero_half_life_snaps_the_pose() {
        use super::{CameraRig, apply_pose, vadd, vsub};
        use bevy::prelude::{Time, Transform};
        use std::time::Duration;

        let mut time = Time::default();
        time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
        let mut rig = CameraRig::default();
        let mut transform = Transform::default();
        // Seed far away, then ask for a distant pose with half-life 0: it must
        // land exactly on it in a single frame.
        apply_pose(
            &mut transform,
            &mut rig,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
            false,
            &time,
            0.0,
            false,
        );
        let eye = Vec3::new(50.0, 10.0, -20.0);
        let focus = vadd(eye, Vec3::new(0.0, 0.0, -1.0));
        apply_pose(
            &mut transform,
            &mut rig,
            eye,
            focus,
            false,
            &time,
            0.0,
            false,
        );
        assert!(
            vsub(rig.smoothed_eye, eye).length() < 1.0e-5,
            "half-life 0 snaps: {:?}",
            rig.smoothed_eye
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
