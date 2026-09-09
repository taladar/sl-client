//! The **Personal Lighting** floater (`viewer-environment-personal-lighting`):
//! change the sky and the water you are standing under, locally.
//!
//! Every control here writes the viewer's **local** environment layer. The
//! region's own settings are not touched, nothing is published to the grid, and
//! nobody else sees the change — it is the reference viewer's `ENV_LOCAL`, the
//! layer a photographer, a machinimist, or anyone who simply wants the sun
//! somewhere else works in.
//!
//! # What opening it does
//!
//! Opening the floater **captures** the environment currently on screen into the
//! local layer and starts editing that copy — the reference's
//! `captureCurrentEnvironment`. Until a control is touched nothing looks
//! different, but the sky in force is now the user's rather than the region's,
//! which is what makes the first slider drag a local edit instead of a request
//! to change the region.
//!
//! **Reset** (with the reference's confirmation) drops the whole local layer,
//! which puts the shared (region) environment back.
//!
//! # Why edits are instant and the menu's are not
//!
//! Picking a preset off the World ▸ Environment menu is an *arrival* at an
//! environment, and may cross-fade
//! (`EnvironmentManualTransitionTime`). Dragging a slider is not an arrival: a
//! fade restarted on every pixel of a drag would run the preview a beat behind
//! the hand moving it. So the editor writes through
//! [`EnvironmentState::set_local_instant`], which is the reference's
//! `TRANSITION_INSTANT` for exactly the same reason.
//!
//! # Deliberate divergences from the reference
//!
//! - **There is a water section.** The reference's Personal Lighting floater
//!   offers exactly one water control — the normal map — and sends anyone who
//!   wants the fog colour or the wave directions to the full settings editor.
//!   The knobs are the same ones, and they are as local and as reversible as the
//!   sky's, so they are here.
//! - **The sun and moon get azimuth / elevation sliders and no trackball.** The
//!   reference offers both, reading one out of the other. The two sliders are
//!   the whole of the state; a trackball is a second way to drive them and can
//!   be added over the same fields whenever there is a trackball widget.
//! - **There are no sun / moon beacon checkboxes.** They belong to the beacons
//!   feature ([[viewer-beacons-control]]), not to this window's state, and the
//!   reference only puts them here for convenience.
//!
//! Reference (Firestorm, read-only): `llfloaterenvironmentadjust.cpp`,
//! `floater_adjust_environment.xml`, `panel_settings_water.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Slider, SliderRange, SliderStep, SliderThumb, SliderValue, ValueChange};
use sl_client_bevy::{
    Color as SlColor, ColorAlpha as SlColorAlpha, DEFAULT_CLOUD_TEXTURE,
    DEFAULT_WATER_NORMAL_TEXTURE, EnvironmentAsset, Glow, Scale, SkySettings, TextureKey,
    WaterSettings, azimuth_altitude_to_rotation, rotation_to_azimuth_altitude,
};
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_pickers::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{
    LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row,
};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use sl_viewer_ui_widgets::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use sl_viewer_world_api::TexturePicked;
use sl_viewer_world_scene::environment::EnvironmentState;
use sl_viewer_world_scene::sky::day_position;

use crate::style::{
    ACTION_BACKGROUND, CONTROL_BORDER, DIM_LABEL_COLOR, FONT_SIZE, HEADING_SIZE, LABEL_COLOR,
    THUMB_FILL, TRACK_FILL,
};

/// The floater's registry id — what the World menu toggles.
pub const PERSONAL_LIGHTING_FLOATER_ID: &str = "personal-lighting";

/// The notification the Reset button raises before it drops the layer.
const RESET_CONFIRM: &str = "PersonalSettingsConfirmReset";

/// A slider track's width, logical px — and, with it, a column's.
///
/// The label sits **above** the slider rather than beside it, as the reference
/// lays this window out. Beside it, a column is label + track + readout wide and
/// four of them do not fit a window anyone would open; above it, four columns
/// are a normal floater wide and the labels have room to say what they mean.
const TRACK_WIDTH: f32 = 140.0;

/// A slider track's height, logical px.
const TRACK_HEIGHT: f32 = 12.0;

/// A slider thumb's width, logical px.
const THUMB_WIDTH: f32 = 9.0;

/// A value readout's width, logical px — right of the label, on its line.
const READOUT_WIDTH: f32 = 44.0;

/// The reference's `SLIDER_SCALE_SUN_AMBIENT`: the sun and ambient colours are
/// shown (and picked) at a third of their stored value, because the stored one
/// is a radiometric colour that routinely exceeds `1.0` and a swatch cannot show
/// that.
const SCALE_SUN_AMBIENT: f32 = 3.0;

/// The reference's `SLIDER_SCALE_BLUE_HORIZON_DENSITY`, for the same reason.
const SCALE_BLUE: f32 = 2.0;

/// The reference's `SLIDER_SCALE_GLOW_R`: the stored glow size runs `40.0..0.2`
/// *backwards*, and the slider shows `0.0..1.99` forwards.
const SCALE_GLOW_SIZE: f32 = 20.0;

/// The reference's `SLIDER_SCALE_GLOW_B`: the stored glow focus is the negated
/// fifth of what the slider shows.
const SCALE_GLOW_FOCUS: f32 = -5.0;

// ---------------------------------------------------------------------------
// The knobs.
// ---------------------------------------------------------------------------

/// One scalar of the **sky** a slider edits, in the units the slider shows.
///
/// A table rather than a field per control: the spawn, the write-back and the
/// refresh all walk the same rows, so a knob cannot exist in one of the three
/// and not the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyKnob {
    /// `haze_horizon`.
    HazeHorizon,
    /// `haze_density`.
    HazeDensity,
    /// `cloud_shadow` — the reference calls it Cloud Coverage.
    CloudCoverage,
    /// `cloud_scale`.
    CloudScale,
    /// `reflection_probe_ambiance`.
    ProbeAmbiance,
    /// `gamma` — the reference labels it Brightness (HDR Scale while the probe
    /// ambiance is non-zero).
    Gamma,
    /// The sun's azimuth, degrees.
    SunAzimuth,
    /// The sun's elevation, degrees.
    SunElevation,
    /// `sun_scale`.
    SunScale,
    /// The glow focus, in the slider's own `-2..2`.
    GlowFocus,
    /// The glow size, in the slider's own `0..1.99`.
    GlowSize,
    /// `star_brightness`.
    StarBrightness,
    /// The moon's azimuth, degrees.
    MoonAzimuth,
    /// The moon's elevation, degrees.
    MoonElevation,
}

impl SkyKnob {
    /// The Fluent key for this knob's row label.
    const fn key(self) -> &'static str {
        match self {
            Self::HazeHorizon => "personal-lighting-haze-horizon",
            Self::HazeDensity => "personal-lighting-haze-density",
            Self::CloudCoverage => "personal-lighting-cloud-coverage",
            Self::CloudScale => "personal-lighting-cloud-scale",
            Self::ProbeAmbiance => "personal-lighting-probe-ambiance",
            Self::Gamma => "personal-lighting-brightness",
            Self::SunAzimuth => "personal-lighting-sun-azimuth",
            Self::SunElevation => "personal-lighting-sun-elevation",
            Self::SunScale => "personal-lighting-sun-scale",
            Self::GlowFocus => "personal-lighting-glow-focus",
            Self::GlowSize => "personal-lighting-glow-size",
            Self::StarBrightness => "personal-lighting-star-brightness",
            Self::MoonAzimuth => "personal-lighting-moon-azimuth",
            Self::MoonElevation => "personal-lighting-moon-elevation",
        }
    }

    /// The slider's `(min, max)`, from the reference's own XML.
    const fn range(self) -> (f32, f32) {
        match self {
            Self::HazeHorizon | Self::HazeDensity => (0.0, 5.0),
            Self::CloudCoverage => (0.0, 1.0),
            Self::CloudScale => (0.01, 3.0),
            Self::ProbeAmbiance => (0.0, 10.0),
            Self::Gamma => (0.0, 20.0),
            // Not 360: the reference stops a hair short, because 360° and 0° are
            // the same azimuth and a slider that can reach both has a value it
            // can never be read back as.
            Self::SunAzimuth | Self::MoonAzimuth => (0.0, 359.99),
            Self::SunElevation | Self::MoonElevation => (-90.0, 90.0),
            Self::SunScale => (0.25, 20.0),
            Self::GlowFocus => (-2.0, 2.0),
            Self::GlowSize => (0.0, 1.99),
            Self::StarBrightness => (0.0, 500.0),
        }
    }

    /// Read the knob out of `sky`, in the slider's units.
    fn read(self, sky: &SkySettings) -> f32 {
        match self {
            Self::HazeHorizon => sky.haze_horizon,
            Self::HazeDensity => sky.haze_density,
            Self::CloudCoverage => sky.cloud_shadow,
            Self::CloudScale => sky.cloud_scale,
            Self::ProbeAmbiance => sky.reflection_probe_ambiance,
            Self::Gamma => sky.gamma,
            Self::SunAzimuth => azimuth_degrees(&sky.sun_rotation),
            Self::SunElevation => elevation_degrees(&sky.sun_rotation),
            Self::SunScale => sky.sun_scale,
            Self::GlowFocus => sky.glow.focus() / SCALE_GLOW_FOCUS,
            Self::GlowSize => 2.0 - sky.glow.size() / SCALE_GLOW_SIZE,
            Self::StarBrightness => sky.star_brightness,
            Self::MoonAzimuth => azimuth_degrees(&sky.moon_rotation),
            Self::MoonElevation => elevation_degrees(&sky.moon_rotation),
        }
    }

    /// Write `value` (in the slider's units) into `sky`.
    fn write(self, sky: &mut SkySettings, value: f32) {
        match self {
            Self::HazeHorizon => sky.haze_horizon = value,
            Self::HazeDensity => sky.haze_density = value,
            Self::CloudCoverage => sky.cloud_shadow = value,
            Self::CloudScale => sky.cloud_scale = value,
            Self::ProbeAmbiance => sky.reflection_probe_ambiance = value,
            Self::Gamma => sky.gamma = value,
            Self::SunAzimuth => {
                sky.sun_rotation = with_azimuth(&sky.sun_rotation, value);
            }
            Self::SunElevation => {
                sky.sun_rotation = with_elevation(&sky.sun_rotation, value);
            }
            Self::SunScale => sky.sun_scale = value,
            Self::GlowFocus => {
                sky.glow = Glow::new(
                    sky.glow.size(),
                    sky.glow.reserved(),
                    value * SCALE_GLOW_FOCUS,
                );
            }
            Self::GlowSize => {
                sky.glow = Glow::new(
                    (2.0 - value) * SCALE_GLOW_SIZE,
                    sky.glow.reserved(),
                    sky.glow.focus(),
                );
            }
            Self::StarBrightness => sky.star_brightness = value,
            Self::MoonAzimuth => {
                sky.moon_rotation = with_azimuth(&sky.moon_rotation, value);
            }
            Self::MoonElevation => {
                sky.moon_rotation = with_elevation(&sky.moon_rotation, value);
            }
        }
    }
}

/// One scalar of the **water** a slider edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterKnob {
    /// `water_fog_density` — the reference's Fog Density Exponent.
    FogDensity,
    /// `underwater_fog_mod`.
    UnderwaterModifier,
    /// `fresnel_scale`.
    FresnelScale,
    /// `fresnel_offset`.
    FresnelOffset,
    /// `normal_scale.x` — the reference's Reflection Wavelet Scale X.
    NormalScaleX,
    /// `normal_scale.y`.
    NormalScaleY,
    /// `normal_scale.z`.
    NormalScaleZ,
    /// `scale_above` — refraction above the surface.
    ScaleAbove,
    /// `scale_below` — refraction below it.
    ScaleBelow,
    /// `blur_multiplier`.
    BlurMultiplier,
    /// `wave1_direction.x` — the large-wave speed.
    LargeWaveX,
    /// `wave1_direction.y`.
    LargeWaveY,
    /// `wave2_direction.x` — the small-wave speed.
    SmallWaveX,
    /// `wave2_direction.y`.
    SmallWaveY,
}

impl WaterKnob {
    /// The Fluent key for this knob's row label.
    const fn key(self) -> &'static str {
        match self {
            Self::FogDensity => "personal-lighting-water-fog-density",
            Self::UnderwaterModifier => "personal-lighting-water-underwater-mod",
            Self::FresnelScale => "personal-lighting-water-fresnel-scale",
            Self::FresnelOffset => "personal-lighting-water-fresnel-offset",
            Self::NormalScaleX => "personal-lighting-water-normal-scale-x",
            Self::NormalScaleY => "personal-lighting-water-normal-scale-y",
            Self::NormalScaleZ => "personal-lighting-water-normal-scale-z",
            Self::ScaleAbove => "personal-lighting-water-scale-above",
            Self::ScaleBelow => "personal-lighting-water-scale-below",
            Self::BlurMultiplier => "personal-lighting-water-blur",
            Self::LargeWaveX => "personal-lighting-water-large-wave-x",
            Self::LargeWaveY => "personal-lighting-water-large-wave-y",
            Self::SmallWaveX => "personal-lighting-water-small-wave-x",
            Self::SmallWaveY => "personal-lighting-water-small-wave-y",
        }
    }

    /// The slider's `(min, max)`, from the reference's own XML.
    const fn range(self) -> (f32, f32) {
        match self {
            Self::FogDensity => (0.001, 100.0),
            Self::UnderwaterModifier => (0.0, 20.0),
            Self::FresnelScale | Self::FresnelOffset => (0.0, 1.0),
            Self::NormalScaleX | Self::NormalScaleY | Self::NormalScaleZ => (0.0, 10.0),
            Self::ScaleAbove | Self::ScaleBelow => (0.0, 3.0),
            Self::BlurMultiplier => (0.0, 0.5),
            Self::LargeWaveX | Self::LargeWaveY | Self::SmallWaveX | Self::SmallWaveY => {
                (-20.0, 20.0)
            }
        }
    }

    /// Read the knob out of `water`.
    const fn read(self, water: &WaterSettings) -> f32 {
        let [wave1_x, wave1_y] = water.wave1_direction;
        let [wave2_x, wave2_y] = water.wave2_direction;
        match self {
            Self::FogDensity => water.water_fog_density,
            Self::UnderwaterModifier => water.underwater_fog_mod,
            Self::FresnelScale => water.fresnel_scale,
            Self::FresnelOffset => water.fresnel_offset,
            Self::NormalScaleX => water.normal_scale.x(),
            Self::NormalScaleY => water.normal_scale.y(),
            Self::NormalScaleZ => water.normal_scale.z(),
            Self::ScaleAbove => water.scale_above,
            Self::ScaleBelow => water.scale_below,
            Self::BlurMultiplier => water.blur_multiplier,
            Self::LargeWaveX => wave1_x,
            Self::LargeWaveY => wave1_y,
            Self::SmallWaveX => wave2_x,
            Self::SmallWaveY => wave2_y,
        }
    }

    /// Write `value` into `water`.
    const fn write(self, water: &mut WaterSettings, value: f32) {
        let scale = water.normal_scale;
        let [wave1_x, wave1_y] = water.wave1_direction;
        let [wave2_x, wave2_y] = water.wave2_direction;
        match self {
            Self::FogDensity => water.water_fog_density = value,
            Self::UnderwaterModifier => water.underwater_fog_mod = value,
            Self::FresnelScale => water.fresnel_scale = value,
            Self::FresnelOffset => water.fresnel_offset = value,
            Self::NormalScaleX => water.normal_scale = Scale::new(value, scale.y(), scale.z()),
            Self::NormalScaleY => water.normal_scale = Scale::new(scale.x(), value, scale.z()),
            Self::NormalScaleZ => water.normal_scale = Scale::new(scale.x(), scale.y(), value),
            Self::ScaleAbove => water.scale_above = value,
            Self::ScaleBelow => water.scale_below = value,
            Self::BlurMultiplier => water.blur_multiplier = value,
            Self::LargeWaveX => water.wave1_direction = [value, wave1_y],
            Self::LargeWaveY => water.wave1_direction = [wave1_x, value],
            Self::SmallWaveX => water.wave2_direction = [value, wave2_y],
            Self::SmallWaveY => water.wave2_direction = [wave2_x, value],
        }
    }
}

/// One colour of the environment a swatch edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKnob {
    /// `ambient`, shown at a third of its stored value.
    Ambient,
    /// `blue_horizon`, shown at half.
    BlueHorizon,
    /// `blue_density`, shown at half.
    BlueDensity,
    /// `sunlight_color`, shown at a third. Its alpha is not a channel the
    /// picker has and is kept as it was.
    SunColor,
    /// `cloud_color`, shown as stored.
    CloudColor,
    /// The water's `water_fog_color`, shown as stored.
    WaterFogColor,
}

impl ColorKnob {
    /// The Fluent key for this swatch's label.
    const fn key(self) -> &'static str {
        match self {
            Self::Ambient => "personal-lighting-ambient",
            Self::BlueHorizon => "personal-lighting-blue-horizon",
            Self::BlueDensity => "personal-lighting-blue-density",
            Self::SunColor => "personal-lighting-sun-color",
            Self::CloudColor => "personal-lighting-cloud-color",
            Self::WaterFogColor => "personal-lighting-water-fog-color",
        }
    }

    /// The element id the swatch is named by (its `Name`, for the UI sweeps).
    const fn element(self) -> &'static str {
        match self {
            Self::Ambient => "personal-lighting-ambient",
            Self::BlueHorizon => "personal-lighting-blue-horizon",
            Self::BlueDensity => "personal-lighting-blue-density",
            Self::SunColor => "personal-lighting-sun-color",
            Self::CloudColor => "personal-lighting-cloud-color",
            Self::WaterFogColor => "personal-lighting-water-fog-color",
        }
    }

    /// How far the stored colour is divided down for display.
    const fn scale(self) -> f32 {
        match self {
            Self::Ambient | Self::SunColor => SCALE_SUN_AMBIENT,
            Self::BlueHorizon | Self::BlueDensity => SCALE_BLUE,
            Self::CloudColor | Self::WaterFogColor => 1.0,
        }
    }

    /// The swatch colour for the current settings.
    fn read(self, sky: &SkySettings, water: &WaterSettings) -> Color {
        let scale = self.scale();
        let stored = match self {
            Self::Ambient => sky.ambient,
            Self::BlueHorizon => sky.blue_horizon,
            Self::BlueDensity => sky.blue_density,
            Self::SunColor => SlColor::new(
                sky.sunlight_color.red(),
                sky.sunlight_color.green(),
                sky.sunlight_color.blue(),
            ),
            Self::CloudColor => sky.cloud_color,
            Self::WaterFogColor => water.water_fog_color,
        };
        Color::linear_rgb(
            stored.red() / scale,
            stored.green() / scale,
            stored.blue() / scale,
        )
    }

    /// Write a picked swatch colour back into the settings.
    fn write(self, sky: &mut SkySettings, water: &mut WaterSettings, color: Color) {
        let scale = self.scale();
        let linear = LinearRgba::from(color);
        let scaled = SlColor::new(
            linear.red * scale,
            linear.green * scale,
            linear.blue * scale,
        );
        match self {
            Self::Ambient => sky.ambient = scaled,
            Self::BlueHorizon => sky.blue_horizon = scaled,
            Self::BlueDensity => sky.blue_density = scaled,
            Self::SunColor => {
                sky.sunlight_color = SlColorAlpha::new(
                    scaled.red(),
                    scaled.green(),
                    scaled.blue(),
                    sky.sunlight_color.alpha(),
                );
            }
            Self::CloudColor => sky.cloud_color = scaled,
            Self::WaterFogColor => water.water_fog_color = scaled,
        }
    }
}

/// One texture of the environment a picker swatch edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKnob {
    /// The sky's `cloud_texture` — the reference's Cloud Image.
    CloudImage,
    /// The water's `normal_map` — the reference's Water Image.
    WaterNormalMap,
}

impl TextureKnob {
    /// The Fluent key for this swatch's label.
    const fn key(self) -> &'static str {
        match self {
            Self::CloudImage => "personal-lighting-cloud-image",
            Self::WaterNormalMap => "personal-lighting-water-image",
        }
    }

    /// The element id the swatch is named by.
    const fn element(self) -> &'static str {
        match self {
            Self::CloudImage => "personal-lighting-cloud-image",
            Self::WaterNormalMap => "personal-lighting-water-image",
        }
    }

    /// The built-in texture this field means when it holds nothing — what the
    /// swatch is spawned showing, before there is an environment to read.
    const fn default_texture(self) -> sl_client_bevy::Uuid {
        match self {
            Self::CloudImage => DEFAULT_CLOUD_TEXTURE,
            Self::WaterNormalMap => DEFAULT_WATER_NORMAL_TEXTURE,
        }
    }

    /// The texture in force, falling back to the built-in default the field
    /// means when it holds nothing.
    fn read(self, sky: &SkySettings, water: &WaterSettings) -> TextureKey {
        let stored = match self {
            Self::CloudImage => sky.cloud_texture,
            Self::WaterNormalMap => water.normal_map,
        };
        stored.unwrap_or_else(|| TextureKey::from(self.default_texture()))
    }

    /// Write a picked texture back into the settings.
    const fn write(self, sky: &mut SkySettings, water: &mut WaterSettings, texture: TextureKey) {
        match self {
            Self::CloudImage => sky.cloud_texture = Some(texture),
            Self::WaterNormalMap => water.normal_map = Some(texture),
        }
    }
}

/// The angles helper the sun / moon knobs read through: the body's azimuth in
/// degrees, `0.0..360.0`.
fn azimuth_degrees(rotation: &sl_client_bevy::Rotation) -> f32 {
    let (azimuth, _elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth.to_degrees()
}

/// The body's elevation in degrees, `-90.0..=90.0`.
fn elevation_degrees(rotation: &sl_client_bevy::Rotation) -> f32 {
    let (_azimuth, elevation) = rotation_to_azimuth_altitude(rotation);
    elevation.to_degrees()
}

/// `rotation` re-aimed at a new azimuth, keeping its elevation.
fn with_azimuth(rotation: &sl_client_bevy::Rotation, azimuth_deg: f32) -> sl_client_bevy::Rotation {
    let (_azimuth, elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth_altitude_to_rotation(azimuth_deg.to_radians(), elevation)
}

/// `rotation` re-aimed at a new elevation, keeping its azimuth.
fn with_elevation(
    rotation: &sl_client_bevy::Rotation,
    elevation_deg: f32,
) -> sl_client_bevy::Rotation {
    let (azimuth, _elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth_altitude_to_rotation(azimuth, elevation_deg.to_radians())
}

// ---------------------------------------------------------------------------
// Components and state.
// ---------------------------------------------------------------------------

/// A sky slider: the knob it drives and the readout beside it.
#[derive(Component, Debug, Clone, Copy)]
struct SkySliderRow {
    /// The knob this slider edits.
    knob: SkyKnob,
    /// The `Text` entity showing the value.
    readout: Entity,
}

/// A water slider: the knob it drives and the readout beside it.
#[derive(Component, Debug, Clone, Copy)]
struct WaterSliderRow {
    /// The knob this slider edits.
    knob: WaterKnob,
    /// The `Text` entity showing the value.
    readout: Entity,
}

/// A colour swatch's knob.
#[derive(Component, Debug, Clone, Copy)]
struct ColorSwatchKnob(ColorKnob);

/// A texture swatch's knob.
#[derive(Component, Debug, Clone, Copy)]
struct TextureSwatchKnob(TextureKnob);

/// The Reset button.
#[derive(Component, Debug, Clone, Copy)]
struct ResetButton;

/// The floater's chrome handles.
#[derive(Resource, Debug)]
struct PersonalLightingUi {
    /// The floater root, which carries the `UiPanelShown` the window is closed
    /// by and re-opened from.
    panel: Entity,
}

/// The environment the floater is editing.
///
/// Two buffers rather than reading the scene back each frame: the sky in force
/// is the *pinned* copy of what these hold, so re-reading it would round-trip
/// every edit through the day-cycle machinery and hand back a frame that has
/// already been blended and named. Editing the buffers and pinning the result is
/// the reference's `mLiveSky` / `mLiveWater`, for the same reason.
#[derive(Resource, Debug, Default)]
struct PersonalLightingEdit {
    /// The sky being edited, or `None` while the floater has never been opened.
    sky: Option<Box<SkySettings>>,
    /// The water being edited.
    water: Option<WaterSettings>,
    /// A control changed a buffer: push both to the environment.
    dirty: bool,
    /// The buffers were replaced (the floater opened): re-seed every widget from
    /// them.
    reseed: bool,
}

impl PersonalLightingEdit {
    /// The two buffers, mutably, or `None` before the first capture.
    fn pair(&mut self) -> Option<(&mut SkySettings, &mut WaterSettings)> {
        match (self.sky.as_deref_mut(), self.water.as_mut()) {
            (Some(sky), Some(water)) => Some((sky, water)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the Personal Lighting floater into a host.
#[derive(Debug, Clone, Copy, Default)]
pub struct PersonalLightingPlugin;

impl Plugin for PersonalLightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PersonalLightingEdit>()
            .add_systems(
                Startup,
                spawn_personal_lighting.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                // Ordered: a capture on open must reach the widgets (reseed)
                // and the environment (push) in the same frame it happened, or
                // the window's first frame shows the previous sky's values.
                (
                    capture_on_open,
                    apply_color_picks,
                    apply_texture_picks,
                    reseed_personal_widgets,
                    push_personal_edits,
                    sync_personal_sliders,
                    handle_reset_confirmation,
                )
                    .chain(),
            );
    }
}

/// The floater's [`FloaterSpec`] — shared with the `FLOATERS` registry, so the
/// swept window is the one the viewer spawns.
#[must_use]
pub fn personal_lighting_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PERSONAL_LIGHTING_FLOATER_ID,
        title: "Personal Lighting".to_owned(),
        position: Vec2::new(120.0, 120.0),
        // Four columns side by side, as the reference lays it out — a tall
        // single column would need a scroll view for a window whose whole point
        // is that every knob is under the hand at once. The water column is the
        // long one at fourteen knobs, and sets the height.
        default_size: Some(Vec2::new(660.0, 540.0)),
        min_size: Some(Vec2::new(320.0, 240.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the (hidden) floater chrome, its content deferred to first
/// open.
fn spawn_personal_lighting(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, personal_lighting_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("personal-lighting-title"));
    let builder = commands.register_system(build_personal_lighting_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
    commands.insert_resource(PersonalLightingUi { panel: handle.root });
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// The sky sliders of the second column — the atmosphere and cloud knobs.
const ATMOSPHERE_KNOBS: &[SkyKnob] = &[
    SkyKnob::HazeHorizon,
    SkyKnob::HazeDensity,
    SkyKnob::CloudCoverage,
    SkyKnob::CloudScale,
    SkyKnob::ProbeAmbiance,
    SkyKnob::Gamma,
];

/// The third column: where the sun and the moon are, and how they glow.
const SUN_MOON_KNOBS: &[SkyKnob] = &[
    SkyKnob::SunAzimuth,
    SkyKnob::SunElevation,
    SkyKnob::SunScale,
    SkyKnob::GlowFocus,
    SkyKnob::GlowSize,
    SkyKnob::StarBrightness,
    SkyKnob::MoonAzimuth,
    SkyKnob::MoonElevation,
];

/// The fourth column: the water.
const WATER_KNOBS: &[WaterKnob] = &[
    WaterKnob::FogDensity,
    WaterKnob::UnderwaterModifier,
    WaterKnob::FresnelScale,
    WaterKnob::FresnelOffset,
    WaterKnob::NormalScaleX,
    WaterKnob::NormalScaleY,
    WaterKnob::NormalScaleZ,
    WaterKnob::ScaleAbove,
    WaterKnob::ScaleBelow,
    WaterKnob::BlurMultiplier,
    WaterKnob::LargeWaveX,
    WaterKnob::LargeWaveY,
    WaterKnob::SmallWaveX,
    WaterKnob::SmallWaveY,
];

/// The colour swatches of the first column.
const COLOR_KNOBS: &[ColorKnob] = &[
    ColorKnob::Ambient,
    ColorKnob::BlueHorizon,
    ColorKnob::BlueDensity,
    ColorKnob::SunColor,
    ColorKnob::CloudColor,
    ColorKnob::WaterFogColor,
];

/// The texture swatches of the first column.
const TEXTURE_KNOBS: &[TextureKnob] = &[TextureKnob::CloudImage, TextureKnob::WaterNormalMap];

/// First-open content build: four columns — colours and images, the atmosphere,
/// the sun and moon, and the water.
fn build_personal_lighting_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..row(Val::Px(10.0))
            },
            Name::new("personal-lighting:content"),
            ChildOf(handle.content),
        ))
        .id();
    let mut tab = 0_i32;

    let swatches = spawn_column(&mut commands, content, "personal-lighting-colours");
    for knob in COLOR_KNOBS {
        spawn_color_row(&mut commands, swatches, *knob, &mut tab);
    }
    for knob in TEXTURE_KNOBS {
        spawn_texture_row(&mut commands, swatches, *knob, &mut tab);
    }
    spawn_reset_button(&mut commands, swatches, &mut tab);

    let atmosphere = spawn_column(&mut commands, content, "personal-lighting-atmosphere");
    for knob in ATMOSPHERE_KNOBS {
        spawn_sky_slider(&mut commands, atmosphere, *knob, &mut tab);
    }

    let bodies = spawn_column(&mut commands, content, "personal-lighting-sun-moon");
    for knob in SUN_MOON_KNOBS {
        spawn_sky_slider(&mut commands, bodies, *knob, &mut tab);
    }

    let water = spawn_column(&mut commands, content, "personal-lighting-water");
    for knob in WATER_KNOBS {
        spawn_water_slider(&mut commands, water, *knob, &mut tab);
    }
}

/// One titled column of the floater; returns the node rows are parented into.
fn spawn_column(commands: &mut Commands, parent: Entity, heading_key: &'static str) -> Entity {
    let column_entity = commands
        .spawn((
            Node {
                min_width: Val::Px(0.0),
                ..column(Val::Px(3.0))
            },
            Name::new(format!("{heading_key}:column")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(HEADING_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Translated::new(heading_key),
        ChildOf(column_entity),
    ));
    column_entity
}

/// One labelled control: a caption line, then the control under it. Returns the
/// node the control is parented into, and the caption row a readout can join.
fn spawn_labelled_block(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
) -> (Entity, Entity) {
    let block = commands
        .spawn((
            Node {
                width: Val::Px(TRACK_WIDTH),
                ..column(Val::Px(1.0))
            },
            ChildOf(parent),
        ))
        .id();
    let caption = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(4.0))
            },
            ChildOf(block),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..Default::default()
        },
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Translated::new(label_key),
        ChildOf(caption),
    ));
    (block, caption)
}

/// The slider track, thumb and readout shared by both slider kinds; returns the
/// track entity and its readout so the caller can tag it with its own knob.
fn spawn_slider_control(
    commands: &mut Commands,
    block: Entity,
    caption: Entity,
    name: String,
    range: (f32, f32),
    tab: &mut i32,
) -> (Entity, Entity) {
    let (min, max) = range;
    let readout = commands
        .spawn((
            Text::new(String::new()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..Default::default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                min_width: Val::Px(READOUT_WIDTH),
                justify_content: JustifyContent::End,
                ..Default::default()
            },
            ChildOf(caption),
        ))
        .id();
    let track = commands
        .spawn((
            Slider::default(),
            SliderValue(min),
            SliderRange::new(min, max),
            SliderStep((max - min) / 100.0),
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(*tab),
            Name::new(name),
            ChildOf(block),
        ))
        .with_child((
            SliderThumb,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(THUMB_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                ..Default::default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ))
        .id();
    *tab = tab.saturating_add(1);
    (track, readout)
}

/// One sky-knob slider row.
fn spawn_sky_slider(commands: &mut Commands, parent: Entity, knob: SkyKnob, tab: &mut i32) {
    let (block, caption) = spawn_labelled_block(commands, parent, knob.key());
    let (track, readout) = spawn_slider_control(
        commands,
        block,
        caption,
        format!("{}:slider", knob.key()),
        knob.range(),
        tab,
    );
    commands
        .entity(track)
        .insert(SkySliderRow { knob, readout })
        .observe(on_sky_slider_change);
}

/// One water-knob slider row.
fn spawn_water_slider(commands: &mut Commands, parent: Entity, knob: WaterKnob, tab: &mut i32) {
    let (block, caption) = spawn_labelled_block(commands, parent, knob.key());
    let (track, readout) = spawn_slider_control(
        commands,
        block,
        caption,
        format!("{}:slider", knob.key()),
        knob.range(),
        tab,
    );
    commands
        .entity(track)
        .insert(WaterSliderRow { knob, readout })
        .observe(on_water_slider_change);
}

/// One colour-swatch row.
fn spawn_color_row(commands: &mut Commands, parent: Entity, knob: ColorKnob, tab: &mut i32) {
    let (block, _caption) = spawn_labelled_block(commands, parent, knob.key());
    let swatch = spawn_color_swatch(commands, block, knob.element(), *tab, Color::BLACK);
    commands.entity(swatch).insert(ColorSwatchKnob(knob));
    *tab = tab.saturating_add(1);
}

/// One texture-swatch row.
fn spawn_texture_row(commands: &mut Commands, parent: Entity, knob: TextureKnob, tab: &mut i32) {
    let (block, _caption) = spawn_labelled_block(commands, parent, knob.key());
    let swatch = spawn_texture_swatch(
        commands,
        block,
        knob.element(),
        *tab,
        TextureKey::from(knob.default_texture()),
    );
    commands.entity(swatch).insert(TextureSwatchKnob(knob));
    *tab = tab.saturating_add(1);
}

/// The Reset button, which drops the whole local layer after a confirmation.
fn spawn_reset_button(commands: &mut Commands, parent: Entity, tab: &mut i32) {
    commands
        .spawn((
            Button,
            ResetButton,
            TabIndex(*tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                margin: UiRect::top(Val::Px(6.0)),
                align_self: AlignSelf::FlexStart,
                ..Default::default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            Name::new("personal-lighting-reset:button"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new("personal-lighting-reset"),
            Pickable::IGNORE,
        ))
        .observe(on_reset_pressed);
    *tab = tab.saturating_add(1);
}

// ---------------------------------------------------------------------------
// Capture, edit, apply.
// ---------------------------------------------------------------------------

/// Capture the environment on screen into the edit buffers when the floater
/// opens — the reference's `captureCurrentEnvironment`, which is what makes the
/// first slider drag a *local* edit rather than a change to the region.
fn capture_on_open(
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    ui: Option<Res<PersonalLightingUi>>,
    environment: Option<Res<EnvironmentState>>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    let (Some(ui), Some(environment)) = (ui, environment) else {
        return;
    };
    let opened = panels
        .iter()
        .any(|(entity, shown)| entity == ui.panel && shown.0);
    if !opened {
        return;
    }
    let position = day_position(&environment);
    let (Some(sky), Some(water)) = (
        environment.sky_at(0.0, position),
        environment.water_at(position),
    ) else {
        // An environment with no sky or no water frame at all: there is nothing
        // to clone, and inventing one would be a sky nobody asked for.
        warn!("personal lighting: no environment to capture; the window opens empty");
        return;
    };
    edit.sky = Some(Box::new(sky));
    edit.water = Some(water);
    edit.reseed = true;
    // Install the capture at once, so the layer being edited is the local one
    // from the moment the window is up.
    edit.dirty = true;
}

/// A sky slider moved: clamp it, write the knob, and mark the buffers dirty.
fn on_sky_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(&SkySliderRow, &SliderRange)>,
    mut edit: ResMut<PersonalLightingEdit>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(sky) = edit.sky.as_deref_mut() {
        row_info.knob.write(sky, clamped);
        edit.dirty = true;
    }
}

/// A water slider moved.
fn on_water_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(&WaterSliderRow, &SliderRange)>,
    mut edit: ResMut<PersonalLightingEdit>,
    mut commands: Commands,
) {
    let Ok((row_info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(water) = edit.water.as_mut() {
        row_info.knob.write(water, clamped);
        edit.dirty = true;
    }
}

/// A colour came back from the picker: write it into the buffers.
///
/// Both the live-preview replies and the committed one are applied, so the
/// preview in the picker and the sky behind it move together — and Cancel's
/// revert reply puts the original back for free.
fn apply_color_picks(
    mut picks: MessageReader<ColorPicked>,
    mut swatches: Query<(&ColorSwatchKnob, &mut ColorSwatchValue)>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    for pick in picks.read() {
        let Ok((knob, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.color;
        let knob = knob.0;
        let Some((sky, water)) = edit.pair() else {
            continue;
        };
        knob.write(sky, water, pick.color);
        edit.dirty = true;
    }
}

/// A texture came back from the picker.
fn apply_texture_picks(
    mut picks: MessageReader<TexturePicked>,
    mut swatches: Query<(&TextureSwatchKnob, &mut TextureSwatchValue)>,
    mut edit: ResMut<PersonalLightingEdit>,
) {
    for pick in picks.read() {
        let Ok((knob, mut value)) = swatches.get_mut(pick.requester) else {
            continue;
        };
        value.0 = pick.texture;
        let knob = knob.0;
        let Some((sky, water)) = edit.pair() else {
            continue;
        };
        knob.write(sky, water, pick.texture);
        edit.dirty = true;
    }
}

/// Seed every widget from the buffers after a capture, so the window opens
/// showing the sky it is about to edit rather than the last one it edited.
fn reseed_personal_widgets(
    mut commands: Commands,
    mut edit: ResMut<PersonalLightingEdit>,
    sky_sliders: Query<(Entity, &SkySliderRow, &SliderRange, &SliderValue)>,
    water_sliders: Query<(Entity, &WaterSliderRow, &SliderRange, &SliderValue)>,
    mut colors: Query<(&ColorSwatchKnob, &mut ColorSwatchValue)>,
    mut textures: Query<(&TextureSwatchKnob, &mut TextureSwatchValue)>,
) {
    if !edit.reseed {
        return;
    }
    // The content is built on the window's *first* open, by a deferred builder
    // whose commands land a frame later — so the capture that asked for this
    // reseed can arrive before there is a single widget to seed. Hold the
    // request rather than spend it on an empty query, or every slider would
    // spend that first open showing the bottom of its range instead of the sky
    // it is editing.
    if sky_sliders.is_empty() {
        return;
    }
    edit.reseed = false;
    let (Some(sky), Some(water)) = (edit.sky.as_deref(), edit.water.as_ref()) else {
        return;
    };
    // `SliderValue` is an immutable component, so a new value is *inserted*
    // rather than assigned — and only when it differs, since an insert marks the
    // component changed whether or not it holds anything new.
    // Compared by bits, not within a tolerance: this asks "would the insert
    // change anything", and an insert marks the component changed whether or not
    // it carries a new number. Two values that differ in their last bit are two
    // different slider positions to everything downstream.
    for (entity, row_info, range, value) in &sky_sliders {
        let wanted = range.clamp(row_info.knob.read(sky));
        if value.0.to_bits() != wanted.to_bits() {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
    for (entity, row_info, range, value) in &water_sliders {
        let wanted = range.clamp(row_info.knob.read(water));
        if value.0.to_bits() != wanted.to_bits() {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
    for (knob, mut value) in &mut colors {
        value.0 = knob.0.read(sky, water);
    }
    for (knob, mut value) in &mut textures {
        value.0 = knob.0.read(sky, water);
    }
}

/// Push the edit buffers into the local environment layer.
///
/// Both tracks every time: the sky and the water are separate tracks and this
/// window owns both, so writing only the one that changed would leave the other
/// following the region while the user is plainly editing it.
fn push_personal_edits(
    mut edit: ResMut<PersonalLightingEdit>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    if !edit.dirty {
        return;
    }
    let Some(mut environment) = environment else {
        return;
    };
    edit.dirty = false;
    if let Some(sky) = edit.sky.clone() {
        // No asset id: this sky is the user's own edit, and no inventory row
        // names it — a preset list must not claim one is in force.
        environment.set_local_instant(EnvironmentAsset::Sky(sky), None);
    }
    if let Some(water) = edit.water.clone() {
        environment.set_local_instant(EnvironmentAsset::Water(water), None);
    }
}

/// Keep each slider's thumb and readout in step with its value.
fn sync_personal_sliders(
    sky_sliders: Query<(&SkySliderRow, &SliderValue, &SliderRange, &Children)>,
    water_sliders: Query<(&WaterSliderRow, &SliderValue, &SliderRange, &Children)>,
    mut insets: Query<&mut LogicalInset, With<SliderThumb>>,
    mut texts: Query<&mut Text>,
) {
    for (row_info, value, range, children) in &sky_sliders {
        place_thumb(value, range, children, &mut insets);
        write_readout(row_info.readout, value.0, &mut texts);
    }
    for (row_info, value, range, children) in &water_sliders {
        place_thumb(value, range, children, &mut insets);
        write_readout(row_info.readout, value.0, &mut texts);
    }
}

/// Move a slider's thumb to where its value sits in its range.
fn place_thumb(
    value: &SliderValue,
    range: &SliderRange,
    children: &Children,
    insets: &mut Query<&mut LogicalInset, With<SliderThumb>>,
) {
    let span = range.span();
    let fraction = if span > f32::EPSILON {
        ((value.0 - range.start()) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
    for child in children.iter() {
        if let Ok(mut inset) = insets.get_mut(child) {
            inset.0.inline_start = Val::Px(offset);
        }
    }
}

/// Write a slider's value into its readout, only when the text would change.
fn write_readout(readout: Entity, value: f32, texts: &mut Query<&mut Text>) {
    if let Ok(mut text) = texts.get_mut(readout) {
        let wanted = format!("{value:.2}");
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// Reset pressed: ask first. Dropping a personal environment somebody has been
/// building is not undoable, which is why the reference asks too.
fn on_reset_pressed(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<ResetButton>>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut show: MessageWriter<ShowNotification>,
) {
    if press.button != PointerButton::Primary || disabled.contains(press.entity) {
        return;
    }
    if buttons.contains(press.entity) {
        show.write(ShowNotification::new(RESET_CONFIRM));
    }
}

/// The confirmation came back: on Yes, drop the whole local layer (which is what
/// puts the shared environment back), forget the buffers and close the window —
/// the reference closes it too, since what it was editing no longer exists.
fn handle_reset_confirmation(
    mut responses: MessageReader<NotificationResponse>,
    mut edit: ResMut<PersonalLightingEdit>,
    environment: Option<ResMut<EnvironmentState>>,
    ui: Option<Res<PersonalLightingUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let confirmed = responses
        .read()
        .any(|response| response.template == RESET_CONFIRM && response.button == Some("OK"));
    if !confirmed {
        return;
    }
    if let Some(mut environment) = environment {
        environment.set_fixed(None);
    }
    edit.sky = None;
    edit.water = None;
    edit.dirty = false;
    edit.reseed = false;
    if let Some(ui) = ui
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorKnob, SkyKnob, TextureKnob, WaterKnob};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        DEFAULT_CLOUD_TEXTURE, DEFAULT_WATER_NORMAL_TEXTURE, SkySettings, TextureKey, WaterSettings,
    };

    /// Every sky knob in the floater, for the round-trip sweeps.
    const ALL_SKY: &[SkyKnob] = &[
        SkyKnob::HazeHorizon,
        SkyKnob::HazeDensity,
        SkyKnob::CloudCoverage,
        SkyKnob::CloudScale,
        SkyKnob::ProbeAmbiance,
        SkyKnob::Gamma,
        SkyKnob::SunAzimuth,
        SkyKnob::SunElevation,
        SkyKnob::SunScale,
        SkyKnob::GlowFocus,
        SkyKnob::GlowSize,
        SkyKnob::StarBrightness,
        SkyKnob::MoonAzimuth,
        SkyKnob::MoonElevation,
    ];

    /// Every water knob.
    const ALL_WATER: &[WaterKnob] = &[
        WaterKnob::FogDensity,
        WaterKnob::UnderwaterModifier,
        WaterKnob::FresnelScale,
        WaterKnob::FresnelOffset,
        WaterKnob::NormalScaleX,
        WaterKnob::NormalScaleY,
        WaterKnob::NormalScaleZ,
        WaterKnob::ScaleAbove,
        WaterKnob::ScaleBelow,
        WaterKnob::BlurMultiplier,
        WaterKnob::LargeWaveX,
        WaterKnob::LargeWaveY,
        WaterKnob::SmallWaveX,
        WaterKnob::SmallWaveY,
    ];

    /// A value a third of the way along a knob's range — off every default, so a
    /// knob that wrote the wrong field shows up as another one not moving.
    fn probe(range: (f32, f32)) -> f32 {
        let (min, max) = range;
        min + (max - min) / 3.0
    }

    /// **Every sky knob reads back what it wrote.** The scaled ones (the glow
    /// pair) and the derived ones (the four sun / moon angles) are the reason
    /// this is a sweep and not a spot check: each converts on the way in and out,
    /// and a sign or a factor dropped on one side only shows up as a slider that
    /// walks away from itself.
    #[test]
    fn every_sky_knob_round_trips() {
        for knob in ALL_SKY {
            let mut sky = SkySettings::legacy_windlight_default("probe");
            let wanted = probe(knob.range());
            knob.write(&mut sky, wanted);
            let read = knob.read(&sky);
            assert!(
                (read - wanted).abs() < 0.01,
                "{knob:?}: wrote {wanted}, read back {read}"
            );
        }
    }

    /// Every water knob reads back what it wrote — the three normal-scale
    /// channels and the four wave components most of all, since each rebuilds a
    /// packed value from its siblings.
    #[test]
    fn every_water_knob_round_trips() {
        for knob in ALL_WATER {
            let mut water = WaterSettings::legacy_default("probe");
            let wanted = probe(knob.range());
            knob.write(&mut water, wanted);
            let read = knob.read(&water);
            assert!(
                (read - wanted).abs() < 0.01,
                "{knob:?}: wrote {wanted}, read back {read}"
            );
        }
    }

    /// **A knob writes its own field and no other.** The packed values are the
    /// hazard: the wave directions and the normal scale are rebuilt whole on
    /// every write, so a knob that reached for the wrong sibling would quietly
    /// zero it.
    #[test]
    fn a_water_knob_leaves_its_siblings_alone() {
        for knob in ALL_WATER {
            let mut water = WaterSettings::legacy_default("probe");
            let before = water.clone();
            knob.write(&mut water, probe(knob.range()));
            for other in ALL_WATER {
                if other == knob {
                    continue;
                }
                let read = other.read(&water);
                let was = other.read(&before);
                assert!(
                    (read - was).abs() < f32::EPSILON,
                    "{knob:?} moved {other:?} from {was} to {read}"
                );
            }
        }
    }

    /// The sun and the moon are separate bodies: moving one leaves the other.
    #[test]
    fn the_sun_and_the_moon_are_placed_separately() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        let moon_azimuth = SkyKnob::MoonAzimuth.read(&sky);
        let moon_elevation = SkyKnob::MoonElevation.read(&sky);

        SkyKnob::SunAzimuth.write(&mut sky, 123.0);
        SkyKnob::SunElevation.write(&mut sky, -12.0);

        assert!((SkyKnob::MoonAzimuth.read(&sky) - moon_azimuth).abs() < 0.01);
        assert!((SkyKnob::MoonElevation.read(&sky) - moon_elevation).abs() < 0.01);
        assert!((SkyKnob::SunAzimuth.read(&sky) - 123.0).abs() < 0.01);
        assert!((SkyKnob::SunElevation.read(&sky) + 12.0).abs() < 0.01);
    }

    /// **A colour swatch round-trips through its display scale.** Ambient, the
    /// sun and the two blues are shown divided down because their stored values
    /// exceed what a swatch can paint; a scale applied on one side only would
    /// brighten or dim the sky every time the picker was merely opened and
    /// dismissed.
    #[test]
    fn every_colour_knob_round_trips_through_its_scale() {
        for knob in [
            ColorKnob::Ambient,
            ColorKnob::BlueHorizon,
            ColorKnob::BlueDensity,
            ColorKnob::SunColor,
            ColorKnob::CloudColor,
            ColorKnob::WaterFogColor,
        ] {
            let mut sky = SkySettings::legacy_windlight_default("probe");
            let mut water = WaterSettings::legacy_default("probe");
            let shown = bevy::prelude::LinearRgba::from(knob.read(&sky, &water));
            knob.write(&mut sky, &mut water, shown.into());
            let again = bevy::prelude::LinearRgba::from(knob.read(&sky, &water));
            for (channel, was, is) in [
                ("red", shown.red, again.red),
                ("green", shown.green, again.green),
                ("blue", shown.blue, again.blue),
            ] {
                assert!(
                    (was - is).abs() < 1e-5,
                    "{knob:?} {channel} went from {was} to {is} on a read/write round trip"
                );
            }
        }
    }

    /// The sun's alpha is not a channel the picker has, so picking a sun colour
    /// must leave it as it was rather than reset it.
    #[test]
    fn picking_a_sun_colour_keeps_its_alpha() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        let mut water = WaterSettings::legacy_default("probe");
        let alpha = sky.sunlight_color.alpha();
        ColorKnob::SunColor.write(&mut sky, &mut water, bevy::prelude::Color::WHITE);
        assert_eq!(sky.sunlight_color.alpha().to_bits(), alpha.to_bits());
    }

    /// A settings frame that names no texture means "the viewer's own default",
    /// and that is what the swatch has to open on — a null id would paint an
    /// empty swatch over a sky that plainly has clouds.
    #[test]
    fn an_unset_texture_shows_the_built_in_default() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        let mut water = WaterSettings::legacy_default("probe");
        sky.cloud_texture = None;
        water.normal_map = None;

        assert_eq!(
            TextureKnob::CloudImage.read(&sky, &water),
            TextureKey::from(DEFAULT_CLOUD_TEXTURE)
        );
        assert_eq!(
            TextureKnob::WaterNormalMap.read(&sky, &water),
            TextureKey::from(DEFAULT_WATER_NORMAL_TEXTURE)
        );
    }

    /// Each texture knob writes its own field.
    #[test]
    fn a_texture_knob_writes_only_its_own_field() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        let mut water = WaterSettings::legacy_default("probe");
        let picked = TextureKey::from(sl_client_bevy::Uuid::from_u128(0x5EED));

        TextureKnob::CloudImage.write(&mut sky, &mut water, picked);
        assert_eq!(sky.cloud_texture, Some(picked));
        assert_ne!(water.normal_map, Some(picked));

        TextureKnob::WaterNormalMap.write(&mut sky, &mut water, picked);
        assert_eq!(water.normal_map, Some(picked));
    }
}
