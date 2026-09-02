//! The Preferences **camera & movement** tab
//! (`viewer-preferences-camera-move-tab`).
//!
//! The camera / move-and-view tab of the preferences floater
//! ([`crate::preferences`]): the third-person **camera** geometry and feel
//! (field of view, offset scale, smoothing, maximum distance, wheel-zoom
//! gate), the **mouselook** feel (sensitivity, pitch invert, own-avatar
//! visibility) and the **movement** options (tap-tap-hold-to-run, hold-jump
//! automatic fly, turn speed, the in-world double-click action).
//!
//! Every row is a **live** control: the camera / movement systems read the
//! [`CameraTuning`] / [`MovementTuning`] resources this module refreshes from
//! the store each frame (`refresh_camera_tuning` /
//! `refresh_movement_tuning`), the field of view is applied straight onto
//! the world camera's projection (`apply_camera_fov`), and the mouselook
//! avatar visibility onto the own avatar's body-root anchor
//! (`apply_first_person_avatar_visibility`). The defaults reproduce the
//! previously hardcoded constants, so out of the box nothing changes — except
//! tap-tap-hold-to-run, a new gesture shipped enabled like the reference.
//!
//! Reference controls deliberately **not** ported (their backing feature does
//! not exist here — a dead checkbox would violate the live-controls scope):
//! camera constraints / reset-on-TP / edit- & appearance-camera motion /
//! mouse-warp (`panel_preferences_move.xml`'s View tab extras), the mouselook
//! master toggle and crosshair (no crosshair is drawn), scroll-wheel-exits
//! -mouselook, chat-focus key routing (owned by
//! `viewer-chat-input-world-autostart`), walk-backwards turning (we never
//! turn the avatar around). Camera **presets** are `viewer-camera-presets`,
//! click-to-walk / the double-click **Walk** option
//! `viewer-autopilot-click-to-walk`, always-run
//! `viewer-movement-controls-floater`, and the flycam speeds the flycam /
//! SpaceNavigator settings tasks.
//!
//! Reference (Firestorm, read-only): `panel_preferences_move.xml`,
//! `llfloaterpreference.cpp` (the View / Mouselook / Movement sub-tabs),
//! `llagentcamera.cpp` (`CameraOffsetScale`), `llcamera.h` (the field-of-view
//! clamp).

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, SliderRange, SliderStep};
use sl_settings::{Scope, SettingValue};

use crate::camera::CameraTuning;
use crate::movement::MovementTuning;
use crate::preferences::{
    spawn_footer_button, spawn_pref_checkbox, spawn_pref_combo, spawn_pref_section,
    spawn_pref_slider,
};
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;
use crate::world_api::AvatarState;
use crate::world_api::{CameraMode, ViewerCamera};
use sl_client_bevy::SlIdentity;
use sl_viewer_world_scene::viewer_camera::{
    DEFAULT_FIELD_OF_VIEW, MAX_FIELD_OF_VIEW, MIN_FIELD_OF_VIEW, clamp_field_of_view,
};

/// The stable id of this tab in `crate::preferences::PREF_TABS`.
pub(crate) const TAB_ID: &str = "camera-move";

/// The settings section the camera keys live in.
const CAMERA_SECTION: &[&str] = &["camera"];

/// The settings section the movement keys live in.
const MOVEMENT_SECTION: &[&str] = &["movement"];

/// The world camera's vertical field of view, radians (the reference
/// `CameraAngle`, Ctrl+8 / Ctrl+0). Applied by `apply_camera_fov`.
pub(crate) const SETTING_CAMERA_ANGLE: &str = "CameraAngle";

/// Multiplier on the third-person orbit distance (the reference
/// `CameraOffsetScale`).
pub(crate) const SETTING_CAMERA_OFFSET_SCALE: &str = "CameraOffsetScale";

/// The camera-pose smoothing half-life, seconds; `0` snaps. Our own honest
/// unit — the reference splits the same feel across `ZoomTime` /
/// `CameraPositionSmoothing`, neither of which is a half-life.
pub(crate) const SETTING_CAMERA_SMOOTHING: &str = "CameraSmoothingHalfLife";

/// The farthest the third-person camera zooms from the avatar, metres (the
/// reference `MAX_CAMERA_DISTANCE_FROM_AGENT` constant, made tunable).
pub(crate) const SETTING_CAMERA_MAX_DISTANCE: &str = "CameraMaxDistance";

/// Whether the mouse wheel is barred from zooming the third-person camera
/// (the reference `FSDisableMouseWheelCameraZoom`); alt-drag zoom still works.
pub(crate) const SETTING_DISABLE_WHEEL_ZOOM: &str = "FSDisableMouseWheelCameraZoom";

/// Mouselook sensitivity on the reference's 0–15 `MouseSensitivity` scale;
/// consumed as `value ×` [`SENSITIVITY_RAD_PER_PX_PER_UNIT`] radians per
/// pixel, so the reference default `3.0` reproduces the previous hardcoded
/// `0.003`.
pub(crate) const SETTING_MOUSE_SENSITIVITY: &str = "MouseSensitivity";

/// Invert the mouselook pitch axis (the reference `InvertMouse`).
pub(crate) const SETTING_INVERT_MOUSE: &str = "InvertMouse";

/// Show the own avatar's body in mouselook (the reference
/// `FirstPersonAvatarVisible`). This project defaults it **on** — that is
/// today's behaviour — where the reference defaults to hidden.
pub(crate) const SETTING_FIRST_PERSON_AVATAR: &str = "FirstPersonAvatarVisible";

/// Whether double-tapping and holding a walk key runs (the reference
/// `AllowTapTapHoldRun`).
pub(crate) const SETTING_TAP_TAP_HOLD_RUN: &str = "AllowTapTapHoldRun";

/// Whether holding the ascend key while standing auto-engages flight (the
/// reference `AutomaticFly`).
pub(crate) const SETTING_AUTOMATIC_FLY: &str = "AutomaticFly";

/// How fast the ← / → keys turn the avatar, radians per second. Our own
/// honest unit — the reference's `FSAvatarTurnSpeed` is a percent-of-default
/// encoding around a fixed constant, which a real setting has no need for.
pub(crate) const SETTING_AVATAR_TURN_RATE: &str = "AvatarTurnRate";

/// Radians of mouselook aim per pixel per unit of [`SETTING_MOUSE_SENSITIVITY`]
/// — the mapping that makes the reference-scale default of `3.0` equal the
/// previous hardcoded `0.003` rad/px.
const SENSITIVITY_RAD_PER_PX_PER_UNIT: f32 = 0.001;

/// The field-of-view slider's lower bound: the reference viewer's
/// `MIN_FIELD_OF_VIEW`, 5° in radians.
const FOV_MIN: f32 = MIN_FIELD_OF_VIEW;

/// The field-of-view slider's upper bound: the reference viewer's
/// `MAX_FIELD_OF_VIEW`, 175° in radians.
const FOV_MAX: f32 = MAX_FIELD_OF_VIEW;

/// A vertical field of view (radians) pinned for this run, overriding the
/// persisted `CameraAngle` preference without rewriting it.
///
/// Inserted by the binary for `--camera-fov` / `SL_VIEWER_CAPTURE_FOV`. A
/// cross-check does not need it — both viewers default to the reference's 60°
/// — but a comparison whose framing rests on two viewers' defaults agreeing has
/// an unstated premise, and this is how a run states it.
///
/// A resource rather than a store write on purpose: the store is what the
/// operator's preferences live in, and a run must be able to pin a lens without
/// editing them.
#[derive(Debug, Clone, Copy, Resource)]
pub struct CameraFovOverride {
    /// The pinned vertical field of view, in radians.
    pub radians: f32,
}

/// Below this difference (radians) the stored field of view counts as already
/// applied, so `apply_camera_fov` does not re-write (and `Changed`-mark) the
/// projection every frame.
const FOV_EPSILON: f32 = 1.0e-4;

/// Register this tab's settings. The numeric defaults are taken from
/// [`CameraTuning::default`] / [`MovementTuning::default`] — the previously
/// hardcoded constants — so registering (and never touching) the settings
/// changes nothing.
pub fn register_settings(settings: &mut ViewerSettings) {
    let camera = CameraTuning::default();
    let movement = MovementTuning::default();
    settings.register_in(
        CAMERA_SECTION,
        SETTING_CAMERA_ANGLE,
        // The reference's own default (`CameraAngle` 1.047197551 = 60°), which
        // is a fidelity number rather than a taste one: how far away things
        // look, how much of a room fits on screen and how a camera offset feels
        // are all read off it, and a resident's eye is calibrated to it by every
        // other viewer. This used to be Bevy's 45°, and the first two-viewer
        // cross-check caught it — both cameras at the same pose, five prims of a
        // fixture row in one frame and three in the other.
        SettingValue::F32(DEFAULT_FIELD_OF_VIEW),
        "World camera vertical field of view in radians",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_CAMERA_OFFSET_SCALE,
        SettingValue::F32(camera.offset_scale),
        "Multiplier on the third-person camera's distance from the avatar",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_CAMERA_SMOOTHING,
        SettingValue::F32(camera.smoothing_half_life),
        "Camera smoothing half-life in seconds; 0 disables the smoothing",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_CAMERA_MAX_DISTANCE,
        SettingValue::F32(camera.max_distance),
        "Farthest the third-person camera zooms from the avatar, in metres",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_DISABLE_WHEEL_ZOOM,
        SettingValue::Bool(camera.wheel_zoom_disabled),
        "Keep the mouse wheel from zooming the camera (drag zoom still works)",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_MOUSE_SENSITIVITY,
        SettingValue::F32(
            camera.mouselook_sensitivity_rad_per_px / SENSITIVITY_RAD_PER_PX_PER_UNIT,
        ),
        "Mouselook mouse sensitivity (the reference viewer's 0-15 scale)",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_INVERT_MOUSE,
        SettingValue::Bool(camera.invert_mouse_y),
        "Invert the vertical mouse look (mouse up looks down)",
    );
    settings.register_in(
        CAMERA_SECTION,
        SETTING_FIRST_PERSON_AVATAR,
        SettingValue::Bool(true),
        "Show my own avatar (and its attachments) while in mouselook",
    );
    settings.register_in(
        MOVEMENT_SECTION,
        SETTING_TAP_TAP_HOLD_RUN,
        SettingValue::Bool(movement.allow_tap_tap_hold_run),
        "Double-tap and hold a walk key to run",
    );
    settings.register_in(
        MOVEMENT_SECTION,
        SETTING_AUTOMATIC_FLY,
        SettingValue::Bool(movement.automatic_fly),
        "Take off by holding the jump key (landing by holding crouch stays on)",
    );
    settings.register_in(
        MOVEMENT_SECTION,
        SETTING_AVATAR_TURN_RATE,
        SettingValue::F32(movement.turn_rate_rad_per_sec),
        "How fast the left / right keys turn the avatar, radians per second",
    );
}

/// Build the tab's content: the camera, mouselook and movement sections per
/// the module docs, each control bound to the store (global scope — the
/// reference keeps all of these machine-wide).
pub(crate) fn build_camera_move_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-camera-view");
    let fov_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-camera-angle",
        SettingBinding::global(SETTING_CAMERA_ANGLE),
        SliderRange::new(FOV_MIN, FOV_MAX),
        SliderStep(0.01),
    );
    spawn_reset_button(commands, fov_row, Scope::Global, SETTING_CAMERA_ANGLE);
    let offset_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-camera-offset-scale",
        SettingBinding::global(SETTING_CAMERA_OFFSET_SCALE),
        SliderRange::new(0.5, 3.0),
        SliderStep(0.05),
    );
    spawn_reset_button(
        commands,
        offset_row,
        Scope::Global,
        SETTING_CAMERA_OFFSET_SCALE,
    );
    let smoothing_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-camera-smoothing",
        SettingBinding::global(SETTING_CAMERA_SMOOTHING),
        SliderRange::new(0.0, 1.0),
        SliderStep(0.05),
    );
    spawn_reset_button(
        commands,
        smoothing_row,
        Scope::Global,
        SETTING_CAMERA_SMOOTHING,
    );
    let distance_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-camera-max-distance",
        SettingBinding::global(SETTING_CAMERA_MAX_DISTANCE),
        SliderRange::new(5.0, 256.0),
        SliderStep(1.0),
    );
    spawn_reset_button(
        commands,
        distance_row,
        Scope::Global,
        SETTING_CAMERA_MAX_DISTANCE,
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-camera-no-wheel-zoom",
        SettingBinding::global(SETTING_DISABLE_WHEEL_ZOOM),
    );

    spawn_pref_section(commands, panel, "preferences-section-mouselook");
    let sensitivity_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-mouse-sensitivity",
        SettingBinding::global(SETTING_MOUSE_SENSITIVITY),
        SliderRange::new(0.1, 15.0),
        SliderStep(0.1),
    );
    spawn_reset_button(
        commands,
        sensitivity_row,
        Scope::Global,
        SETTING_MOUSE_SENSITIVITY,
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-invert-mouse",
        SettingBinding::global(SETTING_INVERT_MOUSE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-first-person-avatar",
        SettingBinding::global(SETTING_FIRST_PERSON_AVATAR),
    );

    spawn_pref_section(commands, panel, "preferences-section-movement");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-tap-tap-hold-run",
        SettingBinding::global(SETTING_TAP_TAP_HOLD_RUN),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-automatic-fly",
        SettingBinding::global(SETTING_AUTOMATIC_FLY),
    );
    let turn_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-avatar-turn-rate",
        SettingBinding::global(SETTING_AVATAR_TURN_RATE),
        SliderRange::new(0.5, 8.0),
        SliderStep(0.1),
    );
    spawn_reset_button(commands, turn_row, Scope::Global, SETTING_AVATAR_TURN_RATE);
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-double-click-action",
        SettingBinding::global(crate::world_api::SETTING_DOUBLE_CLICK_ACTION),
        &[
            ("preferences-double-click-none", SettingValue::I32(0)),
            ("preferences-double-click-teleport", SettingValue::I32(1)),
            // Walk (2) arrives with viewer-autopilot-click-to-walk.
        ],
    );
}

/// Add a reset-to-default button into a control `row` — the reference View
/// tab's per-slider "D" buttons. Resetting clears the `scope`'s override (the
/// scope the row's binding writes); the two-way binding then moves the control
/// back to the declared default, and the shell's snapshot keeps Cancel
/// semantics intact. Shared with the other tabs that want per-row resets (the
/// network & cache and colors & skins tabs).
pub(crate) fn spawn_reset_button(
    commands: &mut Commands,
    row: Entity,
    scope: Scope,
    setting: &'static str,
) {
    let button = spawn_footer_button(commands, row, "preferences-reset-default", 0);
    commands.entity(button).observe(
        move |_activate: On<Activate>, settings: Option<ResMut<ViewerSettings>>| {
            if let Some(mut settings) = settings {
                settings.reset(scope, setting);
            }
        },
    );
}

/// Refresh [`CameraTuning`] from the store (the SpaceNavigator settings
/// idiom): read every camera knob each frame and write the resource only when
/// something changed. Tolerates a missing store (the gallery) by leaving the
/// defaults alone.
fn refresh_camera_tuning(settings: Option<Res<ViewerSettings>>, mut tuning: ResMut<CameraTuning>) {
    let Some(settings) = settings else {
        return;
    };
    let store = settings.store();
    let defaults = CameraTuning::default();
    let next = CameraTuning {
        offset_scale: store
            .get_f32(SETTING_CAMERA_OFFSET_SCALE)
            .unwrap_or(defaults.offset_scale),
        smoothing_half_life: store
            .get_f32(SETTING_CAMERA_SMOOTHING)
            .unwrap_or(defaults.smoothing_half_life),
        max_distance: store
            .get_f32(SETTING_CAMERA_MAX_DISTANCE)
            .unwrap_or(defaults.max_distance),
        wheel_zoom_disabled: store
            .get_bool(SETTING_DISABLE_WHEEL_ZOOM)
            .unwrap_or(defaults.wheel_zoom_disabled),
        mouselook_sensitivity_rad_per_px: store
            .get_f32(SETTING_MOUSE_SENSITIVITY)
            .map_or(defaults.mouselook_sensitivity_rad_per_px, |scale| {
                scale * SENSITIVITY_RAD_PER_PX_PER_UNIT
            }),
        invert_mouse_y: store
            .get_bool(SETTING_INVERT_MOUSE)
            .unwrap_or(defaults.invert_mouse_y),
    };
    if *tuning != next {
        *tuning = next;
    }
}

/// Refresh [`MovementTuning`] from the store — see `refresh_camera_tuning`.
fn refresh_movement_tuning(
    settings: Option<Res<ViewerSettings>>,
    mut tuning: ResMut<MovementTuning>,
) {
    let Some(settings) = settings else {
        return;
    };
    let store = settings.store();
    let defaults = MovementTuning::default();
    let next = MovementTuning {
        turn_rate_rad_per_sec: store
            .get_f32(SETTING_AVATAR_TURN_RATE)
            .unwrap_or(defaults.turn_rate_rad_per_sec),
        allow_tap_tap_hold_run: store
            .get_bool(SETTING_TAP_TAP_HOLD_RUN)
            .unwrap_or(defaults.allow_tap_tap_hold_run),
        automatic_fly: store
            .get_bool(SETTING_AUTOMATIC_FLY)
            .unwrap_or(defaults.automatic_fly),
    };
    if *tuning != next {
        *tuning = next;
    }
}

/// Apply the stored field of view onto the world camera's perspective
/// projection — clamped to the slider range, written only when it actually
/// differs (an unguarded `Mut` deref would `Changed`-mark the projection every
/// frame). Every FOV consumer (`render_priority`, the minimap compass, the
/// session look-at) reads the live projection, so the change propagates by
/// itself.
fn apply_camera_fov(
    settings: Option<Res<ViewerSettings>>,
    pinned: Option<Res<CameraFovOverride>>,
    mut cameras: Query<&mut Projection, With<ViewerCamera>>,
) {
    let stored = match pinned {
        // A run that pinned a lens keeps it whatever the preference says.
        Some(pinned) => pinned.radians,
        None => {
            let Some(settings) = settings else {
                return;
            };
            let Ok(stored) = settings.store().get_f32(SETTING_CAMERA_ANGLE) else {
                return;
            };
            stored
        }
    };
    for mut projection in &mut cameras {
        // Read through the immutable reborrow first: `projection.as_mut()`
        // would dirty the projection unconditionally.
        let Projection::Perspective(perspective) = &*projection else {
            continue;
        };
        // Clamped the way the reference clamps it, against *this* view's aspect
        // ratio: the 5°–175° bounds limit the horizontal extent, so a wide view
        // admits a narrower vertical field than a square one
        // (`LLCamera::getMinView` / `getMaxView`).
        let want = clamp_field_of_view(stored, perspective.aspect_ratio);
        if (perspective.fov - want).abs() <= FOV_EPSILON {
            continue;
        }
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = want;
        }
    }
}

/// Hide the own avatar's body-root anchor while in mouselook with
/// [`SETTING_FIRST_PERSON_AVATAR`] off, and restore it otherwise. The anchor
/// subtree carries the body parts, skeleton and world attachments; HUD
/// attachments hang off the screen-space [`crate::hud`] subtree and name tags
/// are top-level entities, so neither is affected. Poll-and-restore, so
/// leaving mouselook (or flipping the setting live) always un-hides.
fn apply_first_person_avatar_visibility(
    settings: Option<Res<ViewerSettings>>,
    mode: Option<Res<CameraMode>>,
    identity: Option<Res<SlIdentity>>,
    avatars: Option<Res<AvatarState>>,
    mut visibilities: Query<&mut Visibility>,
) {
    let (Some(settings), Some(mode), Some(identity), Some(avatars)) =
        (settings, mode, identity, avatars)
    else {
        return;
    };
    let show_in_mouselook = settings
        .store()
        .get_bool(SETTING_FIRST_PERSON_AVATAR)
        .unwrap_or(true);
    let want = if *mode == CameraMode::Mouselook && !show_in_mouselook {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    let Some(anchor) = identity
        .agent_id
        .and_then(|agent| avatars.body_root_of(agent))
    else {
        return;
    };
    let Ok(mut visibility) = visibilities.get_mut(anchor) else {
        return;
    };
    if *visibility != want {
        *visibility = want;
    }
}

/// Owns the camera & movement tab's runtime side: the per-frame tuning
/// refreshes and the two direct appliers (field of view, mouselook avatar
/// visibility). The tab *content* is built by the preferences shell through
/// `crate::preferences::PREF_TABS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferencesCameraMovePlugin;

impl Plugin for PreferencesCameraMovePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                refresh_camera_tuning,
                refresh_movement_tuning,
                apply_camera_fov,
                apply_first_person_avatar_visibility,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        FOV_MAX, FOV_MIN, SETTING_AUTOMATIC_FLY, SETTING_AVATAR_TURN_RATE, SETTING_CAMERA_ANGLE,
        SETTING_CAMERA_MAX_DISTANCE, SETTING_CAMERA_OFFSET_SCALE, SETTING_CAMERA_SMOOTHING,
        SETTING_DISABLE_WHEEL_ZOOM, SETTING_INVERT_MOUSE, SETTING_MOUSE_SENSITIVITY,
        SETTING_TAP_TAP_HOLD_RUN, apply_camera_fov, refresh_camera_tuning, refresh_movement_tuning,
        register_settings,
    };
    use crate::camera::CameraTuning;
    use crate::movement::MovementTuning;
    use crate::settings::ViewerSettings;
    use crate::world_api::ViewerCamera;

    /// The boxed-error type the tests bubble failures with.
    type TestError = Box<dyn core::error::Error>;

    /// A minimal app with the registered settings and both tuning resources.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        app.insert_resource(settings)
            .init_resource::<CameraTuning>()
            .init_resource::<MovementTuning>()
            .add_systems(
                Update,
                (
                    refresh_camera_tuning,
                    refresh_movement_tuning,
                    apply_camera_fov,
                ),
            );
        app
    }

    /// On a freshly registered (untouched) store the refreshed tunings are
    /// exactly the resources' defaults — pinning that the registered setting
    /// defaults equal the previously hardcoded constants.
    #[test]
    fn default_store_reproduces_the_tuning_defaults() {
        let mut app = test_app();
        // Perturb the resources so the assertion proves the refresh ran.
        app.world_mut().resource_mut::<CameraTuning>().max_distance = 1.0;
        app.world_mut()
            .resource_mut::<MovementTuning>()
            .turn_rate_rad_per_sec = 0.1;
        app.update();
        assert_eq!(
            *app.world().resource::<CameraTuning>(),
            CameraTuning::default()
        );
        assert_eq!(
            *app.world().resource::<MovementTuning>(),
            MovementTuning::default()
        );
    }

    /// Edited store values land in the tuning resources, with the mouselook
    /// sensitivity mapped from the reference 0-15 scale to radians per pixel.
    #[test]
    fn edited_settings_reach_the_tunings() {
        let mut app = test_app();
        {
            let mut settings = app.world_mut().resource_mut::<ViewerSettings>();
            settings.set(
                Scope::Global,
                SETTING_CAMERA_OFFSET_SCALE,
                SettingValue::F32(2.0),
            );
            settings.set(
                Scope::Global,
                SETTING_CAMERA_SMOOTHING,
                SettingValue::F32(0.5),
            );
            settings.set(
                Scope::Global,
                SETTING_CAMERA_MAX_DISTANCE,
                SettingValue::F32(120.0),
            );
            settings.set(
                Scope::Global,
                SETTING_DISABLE_WHEEL_ZOOM,
                SettingValue::Bool(true),
            );
            settings.set(
                Scope::Global,
                SETTING_INVERT_MOUSE,
                SettingValue::Bool(true),
            );
            settings.set(
                Scope::Global,
                SETTING_MOUSE_SENSITIVITY,
                SettingValue::F32(6.0),
            );
            settings.set(
                Scope::Global,
                SETTING_TAP_TAP_HOLD_RUN,
                SettingValue::Bool(false),
            );
            settings.set(
                Scope::Global,
                SETTING_AUTOMATIC_FLY,
                SettingValue::Bool(false),
            );
            settings.set(
                Scope::Global,
                SETTING_AVATAR_TURN_RATE,
                SettingValue::F32(6.4),
            );
        }
        app.update();
        let camera = app.world().resource::<CameraTuning>();
        assert!((camera.offset_scale - 2.0).abs() < 1.0e-6);
        assert!((camera.smoothing_half_life - 0.5).abs() < 1.0e-6);
        assert!((camera.max_distance - 120.0).abs() < 1.0e-6);
        assert!(camera.wheel_zoom_disabled);
        assert!((camera.mouselook_sensitivity_rad_per_px - 0.006).abs() < 1.0e-6);
        assert!(camera.invert_mouse_y);
        let movement = app.world().resource::<MovementTuning>();
        assert!(!movement.allow_tap_tap_hold_run);
        assert!(!movement.automatic_fly);
        assert!((movement.turn_rate_rad_per_sec - 6.4).abs() < 1.0e-6);
    }

    /// The FOV applier clamps an out-of-range stored angle onto the camera
    /// projection and, once applied, stops touching (and `Changed`-marking)
    /// the projection on later frames.
    #[test]
    fn fov_applies_clamped_and_only_on_change() -> Result<(), TestError> {
        let mut app = test_app();
        let camera = app
            .world_mut()
            .spawn((
                ViewerCamera,
                Projection::Perspective(PerspectiveProjection::default()),
            ))
            .id();
        // Way out of range: clamps to the maximum.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_CAMERA_ANGLE,
            SettingValue::F32(10.0),
        );
        app.update();
        let projection = app
            .world()
            .entity(camera)
            .get::<Projection>()
            .ok_or("projection missing")?;
        let Projection::Perspective(perspective) = projection else {
            return Err("not a perspective projection".into());
        };
        assert!((perspective.fov - FOV_MAX).abs() < 1.0e-6, "clamped to max");
        // Below range clamps to the minimum.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_CAMERA_ANGLE,
            SettingValue::F32(0.0),
        );
        app.update();
        // A second frame with no change must not `Changed`-mark the projection.
        app.update();
        let mut changed = app
            .world_mut()
            .query_filtered::<Entity, Changed<Projection>>();
        assert_eq!(
            changed.iter(app.world()).count(),
            0,
            "an unchanged FOV must not re-write the projection"
        );
        let projection = app
            .world()
            .entity(camera)
            .get::<Projection>()
            .ok_or("projection missing")?;
        let Projection::Perspective(perspective) = projection else {
            return Err("not a perspective projection".into());
        };
        assert!((perspective.fov - FOV_MIN).abs() < 1.0e-6, "clamped to min");
        Ok(())
    }

    /// Building the tab into an empty panel spawns every searchable row — five
    /// camera, three mouselook, four movement — without panicking. The shell
    /// defers the build to the floater's first open, so a broken build fn
    /// would otherwise only surface live.
    #[test]
    fn build_spawns_every_row() {
        let mut app = App::new();
        let panel = app.world_mut().spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        super::build_camera_move_tab(&mut commands, panel);
        queue.apply(app.world_mut());
        let mut rows = app
            .world_mut()
            .query::<&crate::preferences::PrefSearchRow>();
        assert_eq!(rows.iter(app.world()).count(), 12, "12 searchable rows");
    }

    /// Every row / section / option Fluent key this tab spawns is distinct, so
    /// the search filter and the translations can never collide.
    #[test]
    fn tab_label_keys_are_distinct() {
        let keys = [
            "preferences-tab-camera-move",
            "preferences-section-camera-view",
            "preferences-section-mouselook",
            "preferences-section-movement",
            "preferences-row-camera-angle",
            "preferences-row-camera-offset-scale",
            "preferences-row-camera-smoothing",
            "preferences-row-camera-max-distance",
            "preferences-row-camera-no-wheel-zoom",
            "preferences-row-mouse-sensitivity",
            "preferences-row-invert-mouse",
            "preferences-row-first-person-avatar",
            "preferences-row-tap-tap-hold-run",
            "preferences-row-automatic-fly",
            "preferences-row-avatar-turn-rate",
            "preferences-row-double-click-action",
            "preferences-double-click-none",
            "preferences-double-click-teleport",
            "preferences-reset-default",
        ];
        let distinct: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(distinct.len(), keys.len(), "duplicate Fluent key");
    }
}
