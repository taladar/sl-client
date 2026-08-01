//! SpaceNavigator / 6-DOF device input (`viewer-input-spacenav-device`), its
//! mapping onto the flycam (`viewer-input-spacenav-camera-mapping`), and — when
//! flycam is off — onto the **avatar** (`viewer-input-spacenav-avatar-motion`):
//! push forward / back walks, twist turns, and lift / press vertically flies up /
//! down (the same intent PageUp / PageDown express). See [`avatar_nav_drive`] and
//! its consumer [`crate::movement`].
//!
//! A 3Dconnexion SpaceNavigator / SpaceMouse reports six self-centring analogue
//! axes — three translation, three rotation. This module reads them off the Linux
//! evdev device (behind the `spacenav` feature; a stub keeps the resource present
//! on other platforms / builds), **normalises** each to `[-1, 1]`, maps them into
//! the reference viewer's six flycam *functions* (forward / strafe / up / roll /
//! pitch / yaw, in that index order), and publishes them as [`SpacenavInput`].
//! [`crate::camera::drive_flycam`] then applies the reference's per-axis
//! **dead-zone**, **scale** and **feathering** ([`FlycamAxisSettings`], the
//! `Flycam*` settings) exactly as `LLViewerJoystick::moveFlycam` does, so the feel
//! matches Firestorm and a user's own `FlycamAxisScale*` values port straight over.
//!
//! The device's **first button** toggles flycam on and off ([`SpacenavInput`]'s
//! [`toggle_flycam`](SpacenavInput::toggle_flycam)), matching the reference's
//! joystick flycam button.
//!
//! Hot-plug is handled by retrying discovery while disconnected; the read needs
//! access to `/dev/input/event*` (the `input` group). Windows / macOS support is a
//! separate, deferred task (`viewer-input-spacenav-crossplatform`).
//!
//! Reference (Firestorm, read-only): `indra/newview/llviewerjoystick.cpp`
//! (`moveFlycam`), `indra/newview/app_settings/settings.xml` (the `Flycam*` /
//! `JoystickAxis*` defaults).

use bevy::prelude::*;
use sl_settings::SettingValue;

use crate::settings::ViewerSettings;

/// The reference **SpaceNavigator-on-Linux** default per-axis scales, in
/// flycam-function order `[forward, strafe, up, roll, pitch, yaw]`
/// (`FlycamAxisScale0..5`). These are the reference's `setSNDefaults` values with
/// its Linux `platformScale = 20` folded in (e.g. `2.1 * 20 = 42`) — the tuned
/// defaults for a SpaceNavigator, not the generic-joystick ones. Roll is off;
/// forward / strafe / up are brisk; pitch / yaw are gentle.
const DEFAULT_SCALE: [f32; 6] = [42.0, 40.0, 40.0, 0.0, 2.0, 3.0];
/// The reference SpaceNavigator default per-axis dead-zone
/// (`FlycamAxisDeadZone0..5`).
const DEFAULT_DEAD_ZONE: f32 = 0.01;
/// The reference SpaceNavigator default feathering (`FlycamFeathering`) — the
/// input ramp rate; less is softer.
const DEFAULT_FEATHERING: f32 = 5.0;

/// The reference **SpaceNavigator-on-Linux** default per-axis avatar-motion scales,
/// in flycam-function order `[forward, strafe, up, roll, pitch, yaw]`
/// (`AvatarAxisScale0..5`). These are the reference `setSNDefaults` values with its
/// Linux `platformScale = 20` / `platformScaleAvXZ = 1` folded in (so `.1 * 20 = 2`
/// for pitch / yaw). Only forward, up and yaw are consumed here — walking, flying
/// up / down, and turning — matching the requested scope; strafe / roll / pitch keep
/// their reference defaults for a later, fuller mapping.
const DEFAULT_AVATAR_SCALE: [f32; 6] = [1.0, 1.0, 1.0, 0.0, 2.0, 2.0];
/// The reference SpaceNavigator default per-axis avatar dead-zone
/// (`AvatarAxisDeadZone0..5`), flycam-function order — larger than the flycam's, so
/// a resting hand does not creep the avatar.
const DEFAULT_AVATAR_DEAD_ZONE: [f32; 6] = [0.1, 0.1, 0.1, 1.0, 0.02, 0.01];
/// The reference SpaceNavigator default avatar feathering (`AvatarFeathering`) — the
/// turn-rate ramp; less is softer.
const DEFAULT_AVATAR_FEATHERING: f32 = 6.0;
/// The reference default run threshold (`JoystickRunThreshold`): a forward push
/// scaled past this magnitude runs rather than walks.
const DEFAULT_RUN_THRESHOLD: f32 = 0.25;

/// The reference SpaceNavigator default for `AutoLeveling` — on, so the flycam
/// eases its horizon back to level (removing composed-rotation roll drift, and
/// making an intentional roll transient rather than permanent).
const DEFAULT_AUTO_LEVELING: bool = true;

/// The current 6-DOF device state, published each frame.
///
/// [`axes`](Self::axes) are the normalised (`[-1, 1]`) axis values in the
/// reference's flycam-function order `[forward, strafe, up, roll, pitch, yaw]`,
/// **before** the dead-zone / scale / feathering the camera applies. Zero when no
/// device is connected. Always present, so consumers need no `cfg`.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct SpacenavInput {
    /// The normalised axes in flycam-function order.
    pub(crate) axes: [f32; 6],
    /// Set for the one frame the device's first button is pressed — toggles flycam.
    pub(crate) toggle_flycam: bool,
}

/// The per-axis dead-zone / scale plus feathering the flycam applies to the raw
/// [`SpacenavInput::axes`], refreshed from [`ViewerSettings`] — the reference's
/// `FlycamAxisDeadZone*` / `FlycamAxisScale*` / `FlycamFeathering`.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct FlycamAxisSettings {
    /// Per-axis scale (flycam-function order).
    pub(crate) scale: [f32; 6],
    /// Per-axis dead-zone (flycam-function order).
    pub(crate) dead_zone: [f32; 6],
    /// The feathering (input ramp) rate; less is softer.
    pub(crate) feathering: f32,
    /// Whether the flycam eases its horizon back to level each frame
    /// (`AutoLeveling`).
    pub(crate) auto_leveling: bool,
}

impl Default for FlycamAxisSettings {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            dead_zone: [DEFAULT_DEAD_ZONE; 6],
            feathering: DEFAULT_FEATHERING,
            auto_leveling: DEFAULT_AUTO_LEVELING,
        }
    }
}

/// The per-axis dead-zone / scale plus feathering and the run threshold the avatar
/// motion applies to the raw [`SpacenavInput::axes`] when flycam is off, refreshed
/// from [`ViewerSettings`] — the reference's `AvatarAxisDeadZone*` /
/// `AvatarAxisScale*` / `AvatarFeathering` / `JoystickRunThreshold`.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct AvatarAxisSettings {
    /// Per-axis scale (flycam-function order).
    pub(crate) scale: [f32; 6],
    /// Per-axis dead-zone (flycam-function order).
    pub(crate) dead_zone: [f32; 6],
    /// The feathering (turn-rate ramp) rate; less is softer.
    pub(crate) feathering: f32,
    /// The forward-push magnitude past which walking becomes running.
    pub(crate) run_threshold: f32,
}

impl Default for AvatarAxisSettings {
    fn default() -> Self {
        Self {
            scale: DEFAULT_AVATAR_SCALE,
            dead_zone: DEFAULT_AVATAR_DEAD_ZONE,
            feathering: DEFAULT_AVATAR_FEATHERING,
            run_threshold: DEFAULT_RUN_THRESHOLD,
        }
    }
}

/// The feathering / hysteresis state the avatar-motion mapping carries between
/// frames: the smoothed body-yaw per-frame delta (the reference `sDelta[RY]`) and
/// the run hysteresis ramp (`mJoystickRun`). Translation (forward / up) is a
/// per-frame sign decision and needs no state.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct AvatarNavSmoothing {
    /// The feathered body-yaw per-frame turn (radians).
    yaw_delta: f32,
    /// The run hysteresis ramp: `0` walk, rising to `2` run (the reference's
    /// respond-next-frame debounce).
    run_ramp: i8,
}

/// The SpaceNavigator's contribution to avatar motion this frame, derived from the
/// raw axes by the reference `moveAvatar` dead-zone / scale / feathering pipeline
/// and consumed by [`crate::movement`], which OR-composes it with the keyboard.
///
/// Only the three requested functions are produced — forward (walk), up (fly up /
/// down, as PageUp / PageDown do) and yaw (turn) — so the mapping stays the
/// keyboard-parallel walk / turn / fly, not the reference's fuller strafe / pitch.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct AvatarNavDrive {
    /// `1` walk forward, `-1` walk back, `0` neither (forward axis, past dead-zone).
    pub(crate) forward: i8,
    /// `1` ascend, `-1` descend, `0` neither (up axis) — the PageUp / PageDown intent.
    pub(crate) vertical: i8,
    /// The body-yaw turn this frame (radians, twist axis, feathered); positive turns
    /// the body left. Zero when centred.
    pub(crate) yaw_delta: f32,
    /// Whether the forward push is past the run threshold this frame.
    pub(crate) run: bool,
}

/// Below this feathered per-frame yaw magnitude (radians) a released twist is
/// treated as fully centred, so it stops turning rather than approaching zero
/// forever.
const YAW_SETTLE_EPSILON: f32 = 1.0e-5;

/// The flycam-function index of the forward (walk), up (fly) and yaw (turn) axes in
/// [`SpacenavInput::axes`] — the three the avatar mapping consumes.
const AVATAR_FORWARD_AXIS: usize = 0;
/// See [`AVATAR_FORWARD_AXIS`].
const AVATAR_UP_AXIS: usize = 2;
/// See [`AVATAR_FORWARD_AXIS`].
const AVATAR_YAW_AXIS: usize = 5;

/// Apply a soft dead-zone to a raw axis value: subtract the dead-zone from the
/// magnitude (ramping from zero at the edge) and keep the sign, clamping to zero
/// inside it — the reference `moveAvatar` per-axis dead-zone.
fn dead_zoned(raw: f32, dead_zone: f32) -> f32 {
    if raw > 0.0 {
        (raw - dead_zone).max(0.0)
    } else {
        (raw + dead_zone).min(0.0)
    }
}

/// Compute the SpaceNavigator's [`AvatarNavDrive`] for this frame from the raw axes,
/// the settings and the carried [`AvatarNavSmoothing`] state.
///
/// Forward / up become a **sign** (walk or fly ±1) once past their dead-zone —
/// avatar motion is the simulator-authoritative discrete intent, so a proportional
/// speed would be discarded. Yaw is **feathered** (ramped) into a per-frame body
/// turn, and the forward push drives a hysteretic run decision, both matching the
/// reference. `dt` is the clamped frame time.
pub(crate) fn avatar_nav_drive(
    input: &SpacenavInput,
    settings: &AvatarAxisSettings,
    smoothing: &mut AvatarNavSmoothing,
    dt: f32,
) -> AvatarNavDrive {
    // Forward / back: sign of the dead-zoned, scaled forward axis. Positive is a
    // forward push (the device normalises push → +forward), so it walks forward.
    let forward_scale = settings
        .scale
        .get(AVATAR_FORWARD_AXIS)
        .copied()
        .unwrap_or(0.0);
    let forward_dz = settings
        .dead_zone
        .get(AVATAR_FORWARD_AXIS)
        .copied()
        .unwrap_or(0.0);
    let forward_val = dead_zoned(
        input.axes.get(AVATAR_FORWARD_AXIS).copied().unwrap_or(0.0),
        forward_dz,
    ) * forward_scale;
    let forward = sign_i8(forward_val);

    // Up / down: sign of the dead-zoned up axis (lift → +up → ascend), the same
    // intent PageUp / PageDown express.
    let up_dz = settings
        .dead_zone
        .get(AVATAR_UP_AXIS)
        .copied()
        .unwrap_or(0.0);
    let vertical = sign_i8(dead_zoned(
        input.axes.get(AVATAR_UP_AXIS).copied().unwrap_or(0.0),
        up_dz,
    ));

    // Yaw: feather the dead-zoned, scaled, frame-timed twist into a body turn (the
    // reference `sDelta[RY]` ramp). Positive twist turns the body left.
    let yaw_scale = settings.scale.get(AVATAR_YAW_AXIS).copied().unwrap_or(0.0);
    let yaw_dz = settings
        .dead_zone
        .get(AVATAR_YAW_AXIS)
        .copied()
        .unwrap_or(0.0);
    let yaw_target = dead_zoned(
        input.axes.get(AVATAR_YAW_AXIS).copied().unwrap_or(0.0),
        yaw_dz,
    ) * yaw_scale
        * dt;
    smoothing.yaw_delta += (yaw_target - smoothing.yaw_delta) * dt * settings.feathering;
    // Snap a decaying-to-centre yaw to exactly zero once negligible, so a released
    // twist stops marking the avatar as turning (the feathering only approaches zero
    // asymptotically) rather than dribbling AgentUpdates forever.
    if yaw_target == 0.0 && smoothing.yaw_delta.abs() < YAW_SETTLE_EPSILON {
        smoothing.yaw_delta = 0.0;
    }

    // Run hysteresis (reference `handleRun`): the forward push magnitude past the
    // threshold ramps up over a frame before running, and back down before walking,
    // so an input spike near the threshold does not flap between walk and run.
    let run_input = forward_val.abs();
    if run_input > settings.run_threshold {
        smoothing.run_ramp = smoothing.run_ramp.saturating_add(1).min(2);
    } else if smoothing.run_ramp > 0 {
        smoothing.run_ramp = smoothing.run_ramp.saturating_sub(1);
    }

    AvatarNavDrive {
        forward,
        vertical,
        yaw_delta: smoothing.yaw_delta,
        run: smoothing.run_ramp >= 2,
    }
}

/// The sign of `value` as an `i8` (`1` / `-1` / `0`).
const fn sign_i8(value: f32) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// The `FlycamAxisScale<n>` setting name for flycam function `n`.
fn scale_setting(index: usize) -> String {
    format!("FlycamAxisScale{index}")
}

/// The `FlycamAxisDeadZone<n>` setting name for flycam function `n`.
fn dead_zone_setting(index: usize) -> String {
    format!("FlycamAxisDeadZone{index}")
}

/// The feathering setting name.
const FEATHERING_SETTING: &str = "FlycamFeathering";

/// The auto-leveling setting name (matching the reference).
const AUTO_LEVELING_SETTING: &str = "AutoLeveling";

/// The `AvatarAxisScale<n>` setting name for flycam function `n`.
fn avatar_scale_setting(index: usize) -> String {
    format!("AvatarAxisScale{index}")
}

/// The `AvatarAxisDeadZone<n>` setting name for flycam function `n`.
fn avatar_dead_zone_setting(index: usize) -> String {
    format!("AvatarAxisDeadZone{index}")
}

/// The avatar feathering setting name (matching the reference).
const AVATAR_FEATHERING_SETTING: &str = "AvatarFeathering";

/// The run-threshold setting name (matching the reference).
const RUN_THRESHOLD_SETTING: &str = "JoystickRunThreshold";

/// Register the flycam-axis settings on the store with the reference defaults, so
/// the names exist (and persist) whether or not the read half is compiled in, and
/// a user's Firestorm values port straight over.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    for (index, &scale) in DEFAULT_SCALE.iter().enumerate() {
        settings.register_in(
            FLYCAM_SECTION,
            &scale_setting(index),
            SettingValue::F32(scale),
            "Flycam axis scaler",
        );
        settings.register_in(
            FLYCAM_SECTION,
            &dead_zone_setting(index),
            SettingValue::F32(DEFAULT_DEAD_ZONE),
            "Flycam axis dead zone",
        );
    }
    settings.register_in(
        FLYCAM_SECTION,
        FEATHERING_SETTING,
        SettingValue::F32(DEFAULT_FEATHERING),
        "Flycam feathering (less is softer)",
    );
    settings.register_in(
        FLYCAM_SECTION,
        AUTO_LEVELING_SETTING,
        SettingValue::Bool(DEFAULT_AUTO_LEVELING),
        "Ease the flycam horizon back to level",
    );
    for (index, &scale) in DEFAULT_AVATAR_SCALE.iter().enumerate() {
        settings.register_in(
            AVATAR_SECTION,
            &avatar_scale_setting(index),
            SettingValue::F32(scale),
            "Avatar-motion axis scaler",
        );
        settings.register_in(
            AVATAR_SECTION,
            &avatar_dead_zone_setting(index),
            SettingValue::F32(DEFAULT_AVATAR_DEAD_ZONE.get(index).copied().unwrap_or(0.0)),
            "Avatar-motion axis dead zone",
        );
    }
    settings.register_in(
        AVATAR_SECTION,
        AVATAR_FEATHERING_SETTING,
        SettingValue::F32(DEFAULT_AVATAR_FEATHERING),
        "Avatar-motion feathering (less is softer)",
    );
    settings.register_in(
        AVATAR_SECTION,
        RUN_THRESHOLD_SETTING,
        SettingValue::F32(DEFAULT_RUN_THRESHOLD),
        "Forward-push magnitude past which walking becomes running",
    );
}

/// The persisted-file section the flycam / SpaceNavigator settings are grouped
/// under (`[spacenav.flycam]`).
const FLYCAM_SECTION: &[&str] = &["spacenav", "flycam"];

/// The persisted-file section the avatar-motion SpaceNavigator settings are grouped
/// under (`[spacenav.avatar]`).
const AVATAR_SECTION: &[&str] = &["spacenav", "avatar"];

/// Refresh [`FlycamAxisSettings`] from the store each frame (cheap reads), so a
/// value changed in the (future) settings UI takes effect live.
pub(crate) fn refresh_flycam_settings(
    store: Res<ViewerSettings>,
    mut settings: ResMut<FlycamAxisSettings>,
) {
    let store = store.store();
    for index in 0..6 {
        if let Ok(value) = store.get_f32(&scale_setting(index))
            && let Some(slot) = settings.scale.get_mut(index)
        {
            *slot = value;
        }
        if let Ok(value) = store.get_f32(&dead_zone_setting(index))
            && let Some(slot) = settings.dead_zone.get_mut(index)
        {
            *slot = value;
        }
    }
    if let Ok(value) = store.get_f32(FEATHERING_SETTING) {
        settings.feathering = value;
    }
    if let Ok(value) = store.get_bool(AUTO_LEVELING_SETTING) {
        settings.auto_leveling = value;
    }
}

/// Refresh [`AvatarAxisSettings`] from the store each frame (cheap reads), so a
/// value changed in the (future) settings UI takes effect live.
pub(crate) fn refresh_avatar_settings(
    store: Res<ViewerSettings>,
    mut settings: ResMut<AvatarAxisSettings>,
) {
    let store = store.store();
    for index in 0..6 {
        if let Ok(value) = store.get_f32(&avatar_scale_setting(index))
            && let Some(slot) = settings.scale.get_mut(index)
        {
            *slot = value;
        }
        if let Ok(value) = store.get_f32(&avatar_dead_zone_setting(index))
            && let Some(slot) = settings.dead_zone.get_mut(index)
        {
            *slot = value;
        }
    }
    if let Ok(value) = store.get_f32(AVATAR_FEATHERING_SETTING) {
        settings.feathering = value;
    }
    if let Ok(value) = store.get_f32(RUN_THRESHOLD_SETTING) {
        settings.run_threshold = value;
    }
}

/// The SpaceNavigator plugin: publishes [`SpacenavInput`] / [`FlycamAxisSettings`]
/// always, and (with the `spacenav` feature on Linux) the device read that fills
/// the input.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SpacenavPlugin;

impl Plugin for SpacenavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpacenavInput>()
            .init_resource::<FlycamAxisSettings>()
            .init_resource::<AvatarAxisSettings>()
            .init_resource::<AvatarNavSmoothing>()
            .add_systems(Update, (refresh_flycam_settings, refresh_avatar_settings));
        #[cfg(all(feature = "spacenav", target_os = "linux"))]
        {
            app.add_systems(Startup, device::open_device)
                .add_systems(Update, device::poll_device);
        }
    }
}

#[cfg(all(feature = "spacenav", target_os = "linux"))]
mod device {
    //! The Linux evdev read half of the SpaceNavigator support, compiled only
    //! with the `spacenav` feature.

    use super::SpacenavInput;
    use bevy::prelude::*;
    use bevy::window::PrimaryWindow;
    use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};
    use tracing::{info, warn};

    /// The raw axis magnitude a SpaceNavigator reports at full deflection, used to
    /// normalise the evdev value to `[-1, 1]` when the device does not report a
    /// range.
    const FALLBACK_AXIS_RANGE: f32 = 350.0;

    /// One evdev axis' mapping into a flycam function: the evdev code, the flycam
    /// index it drives, and a sign so the motion goes the natural way.
    struct AxisMap {
        /// The evdev absolute-axis code.
        code: AbsoluteAxisCode,
        /// The flycam-function index (`0..6`: forward / strafe / up / roll / pitch
        /// / yaw).
        index: usize,
        /// The sign applied so the physical push moves the camera the expected way.
        sign: f32,
    }

    /// The evdev → flycam-function map for a 3Dconnexion SpaceNavigator: slide →
    /// strafe, push → forward, lift → up, and the three tilts / twist → roll /
    /// pitch / yaw. Signs are the natural directions (invert on the device end if a
    /// unit is wired the other way).
    const AXIS_MAP: [AxisMap; 6] = [
        AxisMap {
            code: AbsoluteAxisCode::ABS_Y,
            index: 0,
            sign: -1.0,
        }, // push → forward
        AxisMap {
            code: AbsoluteAxisCode::ABS_X,
            index: 1,
            sign: 1.0,
        }, // slide → strafe
        AxisMap {
            code: AbsoluteAxisCode::ABS_Z,
            index: 2,
            sign: -1.0,
        }, // lift → up
        AxisMap {
            code: AbsoluteAxisCode::ABS_RY,
            index: 3,
            sign: 1.0,
        }, // tilt L/R → roll
        AxisMap {
            code: AbsoluteAxisCode::ABS_RX,
            index: 4,
            sign: 1.0,
        }, // tilt F/B → pitch
        AxisMap {
            code: AbsoluteAxisCode::ABS_RZ,
            index: 5,
            sign: -1.0,
        }, // twist → yaw
    ];

    /// The opened device plus the per-axis normalisation range and the last button
    /// state (for the toggle edge).
    #[derive(Resource)]
    pub(super) struct SpacenavDevice {
        /// The evdev device.
        device: Device,
        /// The `[-1, 1]` normalisation divisor per evdev axis code index (`0..6`).
        range: [f32; 6],
        /// The raw axis values, in evdev order `[X, Y, Z, RX, RY, RZ]`.
        raw: [f32; 6],
        /// Whether the first button was down last frame, for the press edge.
        button_down: bool,
    }

    /// Discover and open a 3Dconnexion device at startup, non-blocking, learning
    /// each axis' range for normalisation. A missing device is not an error.
    pub(super) fn open_device(mut commands: Commands) {
        for (path, device) in evdev::enumerate() {
            let is_spacenav = device.name().is_some_and(|name| {
                name.contains("3Dconnexion") || name.contains("SpaceNavigator")
            });
            if !is_spacenav {
                continue;
            }
            if let Err(error) = device.set_nonblocking(true) {
                warn!(
                    "spacenav: {} could not be set non-blocking: {error}",
                    path.display()
                );
                continue;
            }
            let range = axis_ranges(&device);
            info!("spacenav: using device at {}", path.display());
            commands.insert_resource(SpacenavDevice {
                device,
                range,
                raw: [0.0; 6],
                button_down: false,
            });
            return;
        }
        warn!(
            "spacenav: no 3Dconnexion device found (needs read access to \
             /dev/input/event*, e.g. membership of the `input` group)"
        );
    }

    /// Learn the `[-1, 1]` normalisation divisor for each evdev axis from the
    /// device's absinfo (the larger of |min| / max), falling back to a constant.
    fn axis_ranges(device: &Device) -> [f32; 6] {
        let mut range = [FALLBACK_AXIS_RANGE; 6];
        for (code, info) in device.get_absinfo().into_iter().flatten() {
            let index = match code {
                AbsoluteAxisCode::ABS_X => 0,
                AbsoluteAxisCode::ABS_Y => 1,
                AbsoluteAxisCode::ABS_Z => 2,
                AbsoluteAxisCode::ABS_RX => 3,
                AbsoluteAxisCode::ABS_RY => 4,
                AbsoluteAxisCode::ABS_RZ => 5,
                _other => continue,
            };
            let extent = f32::from(i16::try_from(info.maximum().abs()).unwrap_or(0))
                .max(f32::from(i16::try_from(info.minimum().abs()).unwrap_or(0)));
            if let Some(slot) = range.get_mut(index)
                && extent > 1.0
            {
                *slot = extent;
            }
        }
        range
    }

    /// The evdev axis-code index (`0..6`) for `code`, or `None` for a non-axis.
    const fn code_index(code: AbsoluteAxisCode) -> Option<usize> {
        match code {
            AbsoluteAxisCode::ABS_X => Some(0),
            AbsoluteAxisCode::ABS_Y => Some(1),
            AbsoluteAxisCode::ABS_Z => Some(2),
            AbsoluteAxisCode::ABS_RX => Some(3),
            AbsoluteAxisCode::ABS_RY => Some(4),
            AbsoluteAxisCode::ABS_RZ => Some(5),
            _other => None,
        }
    }

    /// Poll the device each frame, fold events into the raw axis / button state,
    /// and publish the normalised, flycam-ordered [`SpacenavInput`].
    pub(super) fn poll_device(
        device: Option<ResMut<SpacenavDevice>>,
        mut input: ResMut<SpacenavInput>,
        windows: Query<&Window, With<PrimaryWindow>>,
    ) {
        input.toggle_flycam = false;
        let Some(mut device) = device else {
            return;
        };
        // Keyboard / mouse input only reaches the focused window, but the
        // SpaceNavigator is read straight off evdev — a global device — so without
        // this guard it keeps driving the camera / avatar while the viewer is in
        // the background. When the window is not focused, drain the evdev backlog
        // (so it does not replay as a burst on refocus) and publish a zeroed input;
        // the self-centring axes then rest at 0 until focus returns. Done here at
        // the read so every `SpacenavInput` consumer inherits the gate.
        if !windows.single().is_ok_and(|window| window.focused) {
            match device.device.fetch_events() {
                Ok(events) => {
                    let _drained = events.count();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => warn!("spacenav: read error while unfocused: {error}"),
            }
            device.raw = [0.0; 6];
            device.button_down = false;
            input.axes = [0.0; 6];
            return;
        }
        let mut button_now = device.button_down;
        // Collect the pending events first (ending the `fetch_events` borrow) so the
        // axis / button state on `device` can be updated below without a second
        // mutable borrow. A would-block is the normal "nothing new" case.
        let events: Vec<_> = match device.device.fetch_events() {
            Ok(events) => events.collect(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Vec::new(),
            Err(error) => {
                warn!("spacenav: read error: {error}");
                Vec::new()
            }
        };
        for event in events {
            match event.destructure() {
                EventSummary::AbsoluteAxis(_, code, value) => {
                    if let Some(index) = code_index(code) {
                        let raw = f32::from(i16::try_from(value).unwrap_or(0));
                        if let Some(slot) = device.raw.get_mut(index) {
                            *slot = raw;
                        }
                    }
                }
                // The first button (BTN_0) toggles flycam on its press edge.
                EventSummary::Key(_, KeyCode::BTN_0, value) => button_now = value != 0,
                _other => {}
            }
        }

        // Map each evdev axis into the flycam function order, normalised and signed.
        let mut axes = [0.0_f32; 6];
        for map in &AXIS_MAP {
            if let Some(raw_index) = code_index(map.code) {
                let raw = device.raw.get(raw_index).copied().unwrap_or(0.0);
                let range = device
                    .range
                    .get(raw_index)
                    .copied()
                    .unwrap_or(FALLBACK_AXIS_RANGE);
                if let Some(slot) = axes.get_mut(map.index) {
                    *slot = (raw / range).clamp(-1.0, 1.0) * map.sign;
                }
            }
        }
        input.axes = axes;
        // Toggle on the press edge (down now, up last frame).
        input.toggle_flycam = button_now && !device.button_down;
        device.button_down = button_now;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AVATAR_FORWARD_AXIS, AVATAR_UP_AXIS, AVATAR_YAW_AXIS, AvatarAxisSettings, AvatarNavDrive,
        AvatarNavSmoothing, SpacenavInput, avatar_nav_drive,
    };
    use pretty_assertions::assert_eq;

    /// A [`SpacenavInput`] with a single flycam-function axis deflected.
    fn axis_input(index: usize, value: f32) -> SpacenavInput {
        let mut input = SpacenavInput::default();
        if let Some(slot) = input.axes.get_mut(index) {
            *slot = value;
        }
        input
    }

    /// A representative frame time (~60 fps).
    const DT: f32 = 1.0 / 60.0;

    /// A centred device drives nothing.
    #[test]
    fn centred_input_is_inert() {
        let mut smoothing = AvatarNavSmoothing::default();
        let drive = avatar_nav_drive(
            &SpacenavInput::default(),
            &AvatarAxisSettings::default(),
            &mut smoothing,
            DT,
        );
        assert_eq!(drive, AvatarNavDrive::default());
    }

    /// A forward push past the dead-zone walks forward; a pull back walks back; an
    /// input inside the dead-zone does neither.
    #[test]
    fn forward_axis_walks_by_sign() {
        let settings = AvatarAxisSettings::default();

        let mut smoothing = AvatarNavSmoothing::default();
        assert_eq!(
            avatar_nav_drive(
                &axis_input(AVATAR_FORWARD_AXIS, 0.5),
                &settings,
                &mut smoothing,
                DT
            )
            .forward,
            1
        );

        let mut smoothing = AvatarNavSmoothing::default();
        assert_eq!(
            avatar_nav_drive(
                &axis_input(AVATAR_FORWARD_AXIS, -0.5),
                &settings,
                &mut smoothing,
                DT
            )
            .forward,
            -1
        );

        // Inside the (0.1) dead-zone: no walk.
        let mut smoothing = AvatarNavSmoothing::default();
        assert_eq!(
            avatar_nav_drive(
                &axis_input(AVATAR_FORWARD_AXIS, 0.05),
                &settings,
                &mut smoothing,
                DT
            )
            .forward,
            0
        );
    }

    /// A lift ascends, a press descends — the PageUp / PageDown intent.
    #[test]
    fn up_axis_flies_by_sign() {
        let settings = AvatarAxisSettings::default();

        let mut smoothing = AvatarNavSmoothing::default();
        assert_eq!(
            avatar_nav_drive(
                &axis_input(AVATAR_UP_AXIS, 0.5),
                &settings,
                &mut smoothing,
                DT
            )
            .vertical,
            1
        );

        let mut smoothing = AvatarNavSmoothing::default();
        assert_eq!(
            avatar_nav_drive(
                &axis_input(AVATAR_UP_AXIS, -0.5),
                &settings,
                &mut smoothing,
                DT
            )
            .vertical,
            -1
        );
    }

    /// A held twist ramps the body-yaw turn up over frames (feathering) in the
    /// direction of the twist, and a released twist settles it back to exactly zero.
    #[test]
    fn yaw_axis_ramps_and_settles() {
        let settings = AvatarAxisSettings::default();
        let mut smoothing = AvatarNavSmoothing::default();
        let held = axis_input(AVATAR_YAW_AXIS, 1.0);

        let first = avatar_nav_drive(&held, &settings, &mut smoothing, DT).yaw_delta;
        let second = avatar_nav_drive(&held, &settings, &mut smoothing, DT).yaw_delta;
        // Positive twist turns the body left (positive yaw), and the feathering ramps
        // it up rather than snapping.
        assert!(first > 0.0);
        assert!(second > first);

        // Release: after enough frames the feathered turn settles back to exactly
        // zero, so the avatar stops being marked as turning.
        let centred = SpacenavInput::default();
        let mut settled = false;
        for _ in 0..10_000 {
            if avatar_nav_drive(&centred, &settings, &mut smoothing, DT).yaw_delta == 0.0 {
                settled = true;
                break;
            }
        }
        assert!(settled);
    }

    /// A gentle forward push walks; a hard push past the run threshold runs, but only
    /// after the one-frame hysteresis debounce.
    #[test]
    fn run_threshold_needs_a_hard_sustained_push() {
        let settings = AvatarAxisSettings::default();

        // A small push (scale 1.0, so the scaled magnitude ~0.4) stays under the 0.25
        // threshold? 0.4 > 0.25 — use a genuinely gentle push instead.
        let mut smoothing = AvatarNavSmoothing::default();
        let gentle = axis_input(AVATAR_FORWARD_AXIS, 0.2); // dead-zoned to 0.1, scaled 0.1
        assert!(!avatar_nav_drive(&gentle, &settings, &mut smoothing, DT).run);
        assert!(!avatar_nav_drive(&gentle, &settings, &mut smoothing, DT).run);

        // A hard push (dead-zoned to 0.9, scaled 0.9 > 0.25): the ramp debounces one
        // frame, then runs.
        let mut smoothing = AvatarNavSmoothing::default();
        let hard = axis_input(AVATAR_FORWARD_AXIS, 1.0);
        assert!(!avatar_nav_drive(&hard, &settings, &mut smoothing, DT).run);
        assert!(avatar_nav_drive(&hard, &settings, &mut smoothing, DT).run);
    }
}
