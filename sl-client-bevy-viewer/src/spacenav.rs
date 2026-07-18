//! SpaceNavigator / 6-DOF device input (`viewer-input-spacenav-device`) and its
//! mapping onto the flycam (`viewer-input-spacenav-camera-mapping`).
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
}

/// The persisted-file section the flycam / SpaceNavigator settings are grouped
/// under (`[spacenav.flycam]`).
const FLYCAM_SECTION: &[&str] = &["spacenav", "flycam"];

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

/// The SpaceNavigator plugin: publishes [`SpacenavInput`] / [`FlycamAxisSettings`]
/// always, and (with the `spacenav` feature on Linux) the device read that fills
/// the input.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SpacenavPlugin;

impl Plugin for SpacenavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpacenavInput>()
            .init_resource::<FlycamAxisSettings>()
            .add_systems(Update, refresh_flycam_settings);
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
    ) {
        input.toggle_flycam = false;
        let Some(mut device) = device else {
            return;
        };
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
