//! The environment's editable values, as tables.
//!
//! Every environment editor — the Personal Lighting window over the local
//! layer, the fixed sky and water editors over an inventory asset — puts the
//! same knobs in front of the user. They are enumerated here once: each variant
//! carries its label key, its range, and the pair of accessors that read it out
//! of a settings frame and write it back.
//!
//! # Why a table and not a field per control
//!
//! A window's spawn, its write-back and its re-seed all walk these rows, so a
//! knob cannot exist in one of the three and not the others — the failure mode
//! a hand-written control list has is a slider that shows a value it does not
//! write, or writes a value it never shows. A second window costs a list of
//! which rows it wants, not a second copy of the accessors.
//!
//! The reference's own display scalings live in the tables too: ambient and the
//! sun colour are shown at a third of their stored value, the two blues at a
//! half, glow size as `2 − r/20` and glow focus as `b/−5`. Those conversions
//! belong beside the field they convert, not in the window that happens to draw
//! it — applied on one side only, they brighten or dim a sky every time a
//! picker is opened and dismissed.
//!
//! Reference (Firestorm, read-only): `panel_settings_sky_atmos.xml`,
//! `panel_settings_sky_clouds.xml`, `panel_settings_sky_sunmoon.xml`,
//! `panel_settings_water.xml`, `llfloaterenvironmentadjust.cpp`.

use bevy::prelude::*;
use sl_client_bevy::{
    CloudPosDensity, Color as SlColor, ColorAlpha as SlColorAlpha, DEFAULT_BLOOM_TEXTURE,
    DEFAULT_CLOUD_TEXTURE, DEFAULT_HALO_TEXTURE, DEFAULT_MOON_TEXTURE, DEFAULT_RAINBOW_TEXTURE,
    DEFAULT_SUN_TEXTURE, DEFAULT_WATER_NORMAL_TEXTURE, DensityLayer, Glow, Rotation, Scale,
    SkySettings, TextureKey, WaterSettings, azimuth_altitude_to_rotation,
    rotation_to_azimuth_altitude,
};
use sl_viewer_ui_widgets::ui_trackball::{TrackballAim, TrackballBody};

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

/// The Mie phase anisotropy the reference's own default layer carries
/// (`mieConfigDefault`), and what the slider shows for a layer that omits it —
/// a Mie layer with no `anisotropy` key is not an isotropic one, it is one that
/// left the value to the viewer.
const MIE_ANISOTROPY_DEFAULT: f32 = 0.8;

/// The Fluent key a knob's `slug` labels its row with.
///
/// One key per knob, shared by every window that shows it: "Haze horizon" is
/// the same words in the Personal Lighting window and in the sky editor, and
/// two keys for it would be two translations to keep in step.
#[must_use]
pub fn label_key(slug: &str) -> String {
    format!("env-knob-{slug}")
}

// ---------------------------------------------------------------------------
// Sky.
// ---------------------------------------------------------------------------

/// One scalar of the **sky** a slider edits, in the units the slider shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyKnob {
    /// `haze_horizon`.
    HazeHorizon,
    /// `haze_density`.
    HazeDensity,
    /// `moisture_level`.
    MoistureLevel,
    /// `droplet_radius`.
    DropletRadius,
    /// `ice_level`.
    IceLevel,
    /// `density_multiplier`.
    DensityMultiplier,
    /// `distance_multiplier`.
    DistanceMultiplier,
    /// `max_y` — the reference calls it Maximum Altitude.
    MaxAltitude,
    /// `cloud_shadow` — the reference calls it Cloud Coverage.
    CloudCoverage,
    /// `cloud_scale`.
    CloudScale,
    /// `cloud_variance`.
    CloudVariance,
    /// `cloud_scroll_rate[0]`.
    CloudScrollX,
    /// `cloud_scroll_rate[1]`.
    CloudScrollY,
    /// `cloud_pos_density1.x` — the cloud layer's position and density.
    CloudDensityX,
    /// `cloud_pos_density1.y`.
    CloudDensityY,
    /// `cloud_pos_density1.z`.
    CloudDensityD,
    /// `cloud_pos_density2.x` — the detail layer's.
    CloudDetailX,
    /// `cloud_pos_density2.y`.
    CloudDetailY,
    /// `cloud_pos_density2.z`.
    CloudDetailD,
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
    /// `moon_scale`.
    MoonScale,
    /// `moon_brightness`.
    MoonBrightness,
    /// `sun_arc_radians` — the sun disc's angular radius.
    SunArcRadians,
    /// `planet_radius`, kilometres.
    PlanetRadius,
    /// `sky_top_radius` — the atmosphere's outer shell, kilometres.
    SkyTopRadius,
    /// `sky_bottom_radius` — its inner shell, kilometres.
    SkyBottomRadius,
    /// The Rayleigh profile's exponential term.
    RayleighExpTerm,
    /// The Rayleigh profile's exponential scale.
    RayleighExpScale,
    /// The Rayleigh profile's linear term.
    RayleighLinear,
    /// The Rayleigh profile's constant term.
    RayleighConstant,
    /// The Rayleigh profile's layer width — the reference labels it Maximum
    /// Altitude.
    RayleighWidth,
    /// The Mie profile's exponential term.
    MieExpTerm,
    /// The Mie profile's exponential scale.
    MieExpScale,
    /// The Mie profile's linear term.
    MieLinear,
    /// The Mie profile's constant term.
    MieConstant,
    /// The Mie profile's phase anisotropy — the one term only Mie has.
    MieAnisotropy,
    /// The Mie profile's layer width.
    MieWidth,
    /// The absorption profile's exponential term.
    AbsorptionExpTerm,
    /// The absorption profile's exponential scale.
    AbsorptionExpScale,
    /// The absorption profile's linear term.
    AbsorptionLinear,
    /// The absorption profile's constant term.
    AbsorptionConstant,
    /// The absorption profile's layer width.
    AbsorptionWidth,
}

/// One of the three atmospheric-density profiles a sky carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    /// `rayleigh_config` — molecular scattering.
    Rayleigh,
    /// `mie_config` — aerosol scattering, the profile with an anisotropy.
    Mie,
    /// `absorption_config` — ozone.
    Absorption,
}

impl Profile {
    /// The profile's layers in `sky`, or `None` when the frame carries none.
    const fn layers(self, sky: &SkySettings) -> &Vec<DensityLayer> {
        match self {
            Self::Rayleigh => &sky.rayleigh_config,
            Self::Mie => &sky.mie_config,
            Self::Absorption => &sky.absorption_config,
        }
    }

    /// The profile's layers, **materialised** from the reference's default when
    /// the frame carries none.
    ///
    /// The five terms of a layer are a set. Writing one of them into a frame
    /// with no profile at all would store it beside four zeroes — an atmosphere
    /// nobody chose, and not what the slider's neighbours would then be showing.
    /// The reference never meets this case because it merges its own defaults
    /// into every sky it loads; materialising on the first write is the same
    /// outcome reached from a document that says nothing.
    fn layers_mut(self, sky: &mut SkySettings) -> &mut Vec<DensityLayer> {
        let (layers, default): (_, fn() -> Vec<DensityLayer>) = match self {
            Self::Rayleigh => (&mut sky.rayleigh_config, DensityLayer::rayleigh_default),
            Self::Mie => (&mut sky.mie_config, DensityLayer::mie_default),
            Self::Absorption => (&mut sky.absorption_config, DensityLayer::absorption_default),
        };
        if layers.is_empty() {
            *layers = default();
        }
        layers
    }

    /// The layer this profile's knobs read and write when the frame has none —
    /// what a slider shows before anything has been edited.
    fn default_layer(self) -> DensityLayer {
        let default = match self {
            Self::Rayleigh => DensityLayer::rayleigh_default(),
            Self::Mie => DensityLayer::mie_default(),
            Self::Absorption => DensityLayer::absorption_default(),
        };
        default.into_iter().next().unwrap_or(DensityLayer {
            width: 0.0,
            exp_term: 0.0,
            exp_scale: 0.0,
            linear_term: 0.0,
            constant_term: 0.0,
            anisotropy: None,
        })
    }
}

/// Which term of a density layer a knob edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Term {
    /// `exp_term`.
    Exponential,
    /// `exp_scale`.
    ExponentialScale,
    /// `linear_term`.
    Linear,
    /// `constant_term`.
    Constant,
    /// `width` — the reference labels it Maximum Altitude.
    Width,
    /// `anisotropy`, which only a Mie layer carries.
    Anisotropy,
}

impl Term {
    /// Read the term out of `layer`. An absent anisotropy reads as the
    /// reference's own default rather than zero, since zero is the value that
    /// *omits* the key.
    fn read(self, layer: &DensityLayer) -> f32 {
        match self {
            Self::Exponential => layer.exp_term,
            Self::ExponentialScale => layer.exp_scale,
            Self::Linear => layer.linear_term,
            Self::Constant => layer.constant_term,
            Self::Width => layer.width,
            Self::Anisotropy => layer.anisotropy.unwrap_or(MIE_ANISOTROPY_DEFAULT),
        }
    }

    /// Write the term into `layer`.
    const fn write(self, layer: &mut DensityLayer, value: f32) {
        match self {
            Self::Exponential => layer.exp_term = value,
            Self::ExponentialScale => layer.exp_scale = value,
            Self::Linear => layer.linear_term = value,
            Self::Constant => layer.constant_term = value,
            Self::Width => layer.width = value,
            Self::Anisotropy => layer.anisotropy = Some(value),
        }
    }
}

impl SkyKnob {
    /// Every sky knob, in the order the tables above declare them.
    ///
    /// A window's tab lists are checked against this, so a knob added to the
    /// table and forgotten in the UI is a failing test rather than a control
    /// nobody can reach.
    pub const ALL: &'static [Self] = &[
        Self::HazeHorizon,
        Self::HazeDensity,
        Self::MoistureLevel,
        Self::DropletRadius,
        Self::IceLevel,
        Self::DensityMultiplier,
        Self::DistanceMultiplier,
        Self::MaxAltitude,
        Self::CloudCoverage,
        Self::CloudScale,
        Self::CloudVariance,
        Self::CloudScrollX,
        Self::CloudScrollY,
        Self::CloudDensityX,
        Self::CloudDensityY,
        Self::CloudDensityD,
        Self::CloudDetailX,
        Self::CloudDetailY,
        Self::CloudDetailD,
        Self::ProbeAmbiance,
        Self::Gamma,
        Self::SunAzimuth,
        Self::SunElevation,
        Self::SunScale,
        Self::GlowFocus,
        Self::GlowSize,
        Self::StarBrightness,
        Self::MoonAzimuth,
        Self::MoonElevation,
        Self::MoonScale,
        Self::MoonBrightness,
        Self::SunArcRadians,
        Self::PlanetRadius,
        Self::SkyTopRadius,
        Self::SkyBottomRadius,
        Self::RayleighExpTerm,
        Self::RayleighExpScale,
        Self::RayleighLinear,
        Self::RayleighConstant,
        Self::RayleighWidth,
        Self::MieExpTerm,
        Self::MieExpScale,
        Self::MieLinear,
        Self::MieConstant,
        Self::MieAnisotropy,
        Self::MieWidth,
        Self::AbsorptionExpTerm,
        Self::AbsorptionExpScale,
        Self::AbsorptionLinear,
        Self::AbsorptionConstant,
        Self::AbsorptionWidth,
    ];

    /// The knob's stable short name — its Fluent key's tail, and the tail of
    /// the element id every window names its control by.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::HazeHorizon => "haze-horizon",
            Self::HazeDensity => "haze-density",
            Self::MoistureLevel => "moisture-level",
            Self::DropletRadius => "droplet-radius",
            Self::IceLevel => "ice-level",
            Self::DensityMultiplier => "density-multiplier",
            Self::DistanceMultiplier => "distance-multiplier",
            Self::MaxAltitude => "max-altitude",
            Self::CloudCoverage => "cloud-coverage",
            Self::CloudScale => "cloud-scale",
            Self::CloudVariance => "cloud-variance",
            Self::CloudScrollX => "cloud-scroll-x",
            Self::CloudScrollY => "cloud-scroll-y",
            Self::CloudDensityX => "cloud-density-x",
            Self::CloudDensityY => "cloud-density-y",
            Self::CloudDensityD => "cloud-density-d",
            Self::CloudDetailX => "cloud-detail-x",
            Self::CloudDetailY => "cloud-detail-y",
            Self::CloudDetailD => "cloud-detail-d",
            Self::ProbeAmbiance => "probe-ambiance",
            Self::Gamma => "brightness",
            Self::SunAzimuth => "sun-azimuth",
            Self::SunElevation => "sun-elevation",
            Self::SunScale => "sun-scale",
            Self::GlowFocus => "glow-focus",
            Self::GlowSize => "glow-size",
            Self::StarBrightness => "star-brightness",
            Self::MoonAzimuth => "moon-azimuth",
            Self::MoonElevation => "moon-elevation",
            Self::MoonScale => "moon-scale",
            Self::MoonBrightness => "moon-brightness",
            Self::SunArcRadians => "sun-arc-radians",
            Self::PlanetRadius => "planet-radius",
            Self::SkyTopRadius => "sky-top-radius",
            Self::SkyBottomRadius => "sky-bottom-radius",
            Self::RayleighExpTerm => "rayleigh-exp-term",
            Self::RayleighExpScale => "rayleigh-exp-scale",
            Self::RayleighLinear => "rayleigh-linear",
            Self::RayleighConstant => "rayleigh-constant",
            Self::RayleighWidth => "rayleigh-width",
            Self::MieExpTerm => "mie-exp-term",
            Self::MieExpScale => "mie-exp-scale",
            Self::MieLinear => "mie-linear",
            Self::MieConstant => "mie-constant",
            Self::MieAnisotropy => "mie-anisotropy",
            Self::MieWidth => "mie-width",
            Self::AbsorptionExpTerm => "absorption-exp-term",
            Self::AbsorptionExpScale => "absorption-exp-scale",
            Self::AbsorptionLinear => "absorption-linear",
            Self::AbsorptionConstant => "absorption-constant",
            Self::AbsorptionWidth => "absorption-width",
        }
    }

    /// Which density profile and term this knob edits, for the sixteen that do.
    ///
    /// The three profiles are the same handful of terms over and over, so the
    /// accessors are written once against a `(profile, term)` pair rather than
    /// sixteen times.
    const fn density(self) -> Option<(Profile, Term)> {
        match self {
            Self::RayleighExpTerm => Some((Profile::Rayleigh, Term::Exponential)),
            Self::RayleighExpScale => Some((Profile::Rayleigh, Term::ExponentialScale)),
            Self::RayleighLinear => Some((Profile::Rayleigh, Term::Linear)),
            Self::RayleighConstant => Some((Profile::Rayleigh, Term::Constant)),
            Self::RayleighWidth => Some((Profile::Rayleigh, Term::Width)),
            Self::MieExpTerm => Some((Profile::Mie, Term::Exponential)),
            Self::MieExpScale => Some((Profile::Mie, Term::ExponentialScale)),
            Self::MieLinear => Some((Profile::Mie, Term::Linear)),
            Self::MieConstant => Some((Profile::Mie, Term::Constant)),
            Self::MieAnisotropy => Some((Profile::Mie, Term::Anisotropy)),
            Self::MieWidth => Some((Profile::Mie, Term::Width)),
            Self::AbsorptionExpTerm => Some((Profile::Absorption, Term::Exponential)),
            Self::AbsorptionExpScale => Some((Profile::Absorption, Term::ExponentialScale)),
            Self::AbsorptionLinear => Some((Profile::Absorption, Term::Linear)),
            Self::AbsorptionConstant => Some((Profile::Absorption, Term::Constant)),
            Self::AbsorptionWidth => Some((Profile::Absorption, Term::Width)),
            _other => None,
        }
    }

    /// How many decimals the value readout shows.
    ///
    /// Two for everything a person reads as a quantity, and up to eight for the
    /// scattering terms, whose whole useful range is a millionth wide — a
    /// Rayleigh linear term rounded to two decimals is the string `0.00` at
    /// every position of its slider.
    #[must_use]
    pub const fn decimals(self) -> usize {
        match self {
            Self::RayleighLinear | Self::MieLinear => 8,
            Self::RayleighExpScale | Self::MieExpScale => 6,
            Self::SunArcRadians
            | Self::RayleighExpTerm
            | Self::RayleighConstant
            | Self::MieExpTerm
            | Self::MieConstant
            | Self::AbsorptionExpTerm
            | Self::AbsorptionExpScale
            | Self::AbsorptionLinear
            | Self::AbsorptionConstant => 5,
            _other => 2,
        }
    }

    /// The slider's `(min, max)`, from the reference's own XML.
    #[must_use]
    pub const fn range(self) -> (f32, f32) {
        match self {
            Self::HazeHorizon | Self::HazeDensity => (0.0, 5.0),
            Self::MoistureLevel
            | Self::RayleighExpTerm
            | Self::RayleighConstant
            | Self::MieConstant
            | Self::AbsorptionExpTerm
            | Self::AbsorptionLinear
            | Self::AbsorptionConstant
            | Self::IceLevel
            | Self::CloudCoverage
            | Self::CloudVariance
            | Self::CloudDensityX
            | Self::CloudDensityY
            | Self::CloudDetailX
            | Self::CloudDetailY
            | Self::CloudDetailD
            | Self::MoonBrightness => (0.0, 1.0),
            Self::DropletRadius => (5.0, 1000.0),
            Self::DensityMultiplier => (0.0001, 2.0),
            Self::DistanceMultiplier => (0.05, 1000.0),
            Self::MaxAltitude => (0.0, 10000.0),
            Self::CloudScale | Self::CloudDensityD => (0.01, 3.0),
            Self::CloudScrollX | Self::CloudScrollY => (-30.0, 30.0),
            Self::ProbeAmbiance => (0.0, 10.0),
            Self::Gamma => (0.0, 20.0),
            // Not 360: the reference stops a hair short, because 360° and 0° are
            // the same azimuth and a slider that can reach both has a value it
            // can never be read back as.
            Self::SunAzimuth | Self::MoonAzimuth => (0.0, 359.99),
            Self::SunElevation | Self::MoonElevation => (-90.0, 90.0),
            Self::SunScale | Self::MoonScale => (0.25, 20.0),
            Self::GlowFocus => (-2.0, 2.0),
            Self::GlowSize => (0.0, 1.99),
            Self::StarBrightness => (0.0, 500.0),
            // The reference exposes no slider for these four, but its
            // *validator* declares their bounds, which is a better source than
            // a number chosen here would be (`LLSettingsSky::validationList`).
            Self::SunArcRadians => (0.0, 0.1),
            Self::PlanetRadius | Self::SkyTopRadius | Self::SkyBottomRadius => (1000.0, 32768.0),
            // The density panel's own ranges, which are not the same for the
            // three profiles: ozone absorbs over a far wider band than
            // molecular scattering does.
            Self::RayleighExpScale | Self::MieExpScale => (-0.01, 0.01),
            Self::RayleighLinear => (0.0, 0.000_01),
            Self::RayleighWidth => (1000.0, 40000.0),
            Self::MieExpTerm => (0.0, 3.0),
            Self::MieLinear => (0.0, 0.000_004),
            Self::MieAnisotropy => (0.2, 1.8),
            Self::MieWidth => (1000.0, 30000.0),
            Self::AbsorptionExpScale => (-1.0, 1.0),
            Self::AbsorptionWidth => (1000.0, 25000.0),
        }
    }

    /// Read the knob out of `sky`, in the slider's units.
    #[must_use]
    pub fn read(self, sky: &SkySettings) -> f32 {
        let [scroll_x, scroll_y] = sky.cloud_scroll_rate;
        match self {
            Self::HazeHorizon => sky.haze_horizon,
            Self::HazeDensity => sky.haze_density,
            Self::MoistureLevel => sky.moisture_level,
            Self::DropletRadius => sky.droplet_radius,
            Self::IceLevel => sky.ice_level,
            Self::DensityMultiplier => sky.density_multiplier,
            Self::DistanceMultiplier => sky.distance_multiplier,
            Self::MaxAltitude => sky.max_y,
            Self::CloudCoverage => sky.cloud_shadow,
            Self::CloudScale => sky.cloud_scale,
            Self::CloudVariance => sky.cloud_variance,
            Self::CloudScrollX => scroll_x,
            Self::CloudScrollY => scroll_y,
            Self::CloudDensityX => sky.cloud_pos_density1.position_x(),
            Self::CloudDensityY => sky.cloud_pos_density1.position_y(),
            Self::CloudDensityD => sky.cloud_pos_density1.density(),
            Self::CloudDetailX => sky.cloud_pos_density2.position_x(),
            Self::CloudDetailY => sky.cloud_pos_density2.position_y(),
            Self::CloudDetailD => sky.cloud_pos_density2.density(),
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
            Self::MoonScale => sky.moon_scale,
            Self::MoonBrightness => sky.moon_brightness,
            Self::SunArcRadians => sky.sun_arc_radians,
            Self::PlanetRadius => sky.planet_radius,
            Self::SkyTopRadius => sky.sky_top_radius,
            Self::SkyBottomRadius => sky.sky_bottom_radius,
            _density => {
                let Some((profile, term)) = self.density() else {
                    // Every remaining variant is a density knob; `density`
                    // matching the same set is what makes this arm total.
                    return 0.0;
                };
                // Layer 0, as the reference's own panel reads it
                // (`getRayleighConfig`, which is `*mRayleighConfigs.beginArray()`).
                profile.layers(sky).first().map_or_else(
                    || term.read(&profile.default_layer()),
                    |layer| term.read(layer),
                )
            }
        }
    }

    /// Write `value` (in the slider's units) into `sky`.
    pub fn write(self, sky: &mut SkySettings, value: f32) {
        let density1 = sky.cloud_pos_density1;
        let density2 = sky.cloud_pos_density2;
        let [scroll_x, scroll_y] = sky.cloud_scroll_rate;
        match self {
            Self::HazeHorizon => sky.haze_horizon = value,
            Self::HazeDensity => sky.haze_density = value,
            Self::MoistureLevel => sky.moisture_level = value,
            Self::DropletRadius => sky.droplet_radius = value,
            Self::IceLevel => sky.ice_level = value,
            Self::DensityMultiplier => sky.density_multiplier = value,
            Self::DistanceMultiplier => sky.distance_multiplier = value,
            Self::MaxAltitude => sky.max_y = value,
            Self::CloudCoverage => sky.cloud_shadow = value,
            Self::CloudScale => sky.cloud_scale = value,
            Self::CloudVariance => sky.cloud_variance = value,
            Self::CloudScrollX => sky.cloud_scroll_rate = [value, scroll_y],
            Self::CloudScrollY => sky.cloud_scroll_rate = [scroll_x, value],
            Self::CloudDensityX => {
                sky.cloud_pos_density1 =
                    CloudPosDensity::new(value, density1.position_y(), density1.density());
            }
            Self::CloudDensityY => {
                sky.cloud_pos_density1 =
                    CloudPosDensity::new(density1.position_x(), value, density1.density());
            }
            Self::CloudDensityD => {
                sky.cloud_pos_density1 =
                    CloudPosDensity::new(density1.position_x(), density1.position_y(), value);
            }
            Self::CloudDetailX => {
                sky.cloud_pos_density2 =
                    CloudPosDensity::new(value, density2.position_y(), density2.density());
            }
            Self::CloudDetailY => {
                sky.cloud_pos_density2 =
                    CloudPosDensity::new(density2.position_x(), value, density2.density());
            }
            Self::CloudDetailD => {
                sky.cloud_pos_density2 =
                    CloudPosDensity::new(density2.position_x(), density2.position_y(), value);
            }
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
            Self::MoonScale => sky.moon_scale = value,
            Self::MoonBrightness => sky.moon_brightness = value,
            Self::SunArcRadians => sky.sun_arc_radians = value,
            Self::PlanetRadius => sky.planet_radius = value,
            Self::SkyTopRadius => sky.sky_top_radius = value,
            Self::SkyBottomRadius => sky.sky_bottom_radius = value,
            _density => {
                let Some((profile, term)) = self.density() else {
                    return;
                };
                // Layer 0 **in place**. The reference replaces the whole
                // profile with a single layer on any edit
                // (`createSingleLayerDensityProfile`), which silently throws
                // away the second layer of the ozone ramp its own default
                // ships; editing the layer the sliders are showing keeps the
                // rest of the author's profile.
                if let Some(layer) = profile.layers_mut(sky).first_mut() {
                    term.write(layer, value);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Aim: the knob pair one trackball drives.
// ---------------------------------------------------------------------------

/// The two knobs that place one celestial body — its compass angle and its
/// height — as one thing.
///
/// The sliders drive them separately, because a slider drives one number. A
/// trackball ([`sl_viewer_ui_widgets::ui_trackball`]) drives both at once,
/// because pointing at a hemisphere says where a body is in one gesture; this
/// is the pair it writes through, so the trackball itself never learns what a
/// sky is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AimKnobs {
    /// Which body the pair places.
    pub body: TrackballBody,
    /// The compass-angle knob.
    pub azimuth: SkyKnob,
    /// The height knob.
    pub elevation: SkyKnob,
}

impl AimKnobs {
    /// The sun's pair.
    pub const SUN: Self = Self {
        body: TrackballBody::Sun,
        azimuth: SkyKnob::SunAzimuth,
        elevation: SkyKnob::SunElevation,
    };

    /// The moon's pair.
    pub const MOON: Self = Self {
        body: TrackballBody::Moon,
        azimuth: SkyKnob::MoonAzimuth,
        elevation: SkyKnob::MoonElevation,
    };

    /// Both, in the order the windows draw them.
    pub const ALL: &'static [Self] = &[Self::SUN, Self::MOON];

    /// The pair `knob` belongs to, if it is one of the four aim knobs.
    #[must_use]
    pub fn of(knob: SkyKnob) -> Option<Self> {
        Self::ALL.iter().copied().find(|pair| pair.covers(knob))
    }

    /// Whether `knob` is one of this pair.
    #[must_use]
    pub fn covers(self, knob: SkyKnob) -> bool {
        knob == self.azimuth || knob == self.elevation
    }

    /// The slug the trackball is named and labelled by. Its own, rather than
    /// either knob's: the control is one row showing where a body *is*, and the
    /// two knob labels are on the two sliders under it.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self.body {
            TrackballBody::Sun => "sun-position",
            TrackballBody::Moon => "moon-position",
        }
    }

    /// Where this body is in `sky`, in the degrees the trackball and the two
    /// sliders both show.
    #[must_use]
    pub fn read(self, sky: &SkySettings) -> TrackballAim {
        TrackballAim {
            azimuth: self.azimuth.read(sky),
            elevation: self.elevation.read(sky),
        }
    }

    /// Put this body at `aim`.
    ///
    /// The height goes first, and the order is not arbitrary: a rotation
    /// pointing exactly at a pole has no azimuth to keep, so writing the
    /// azimuth into a body that is *currently* straight up would be dropped and
    /// the height that followed would take it from zero. Writing the height
    /// first leaves the azimuth write last, where it always lands.
    pub fn write(self, sky: &mut SkySettings, aim: TrackballAim) {
        self.elevation.write(sky, aim.elevation);
        self.azimuth.write(sky, aim.azimuth);
    }

    /// `aim` with the one component `knob` names replaced by `value` — how a
    /// slider drag reaches a trackball. A knob outside the pair changes
    /// nothing.
    #[must_use]
    pub fn with(self, aim: TrackballAim, knob: SkyKnob, value: f32) -> TrackballAim {
        if knob == self.azimuth {
            TrackballAim {
                azimuth: value,
                ..aim
            }
        } else if knob == self.elevation {
            TrackballAim {
                elevation: value,
                ..aim
            }
        } else {
            aim
        }
    }

    /// The component of `aim` that `knob` names — how a trackball drag reaches
    /// a slider.
    #[must_use]
    pub fn value_of(self, aim: TrackballAim, knob: SkyKnob) -> Option<f32> {
        if knob == self.azimuth {
            Some(aim.azimuth)
        } else if knob == self.elevation {
            Some(aim.elevation)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Water.
// ---------------------------------------------------------------------------

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
    /// Every water knob — see [`SkyKnob::ALL`].
    pub const ALL: &'static [Self] = &[
        Self::FogDensity,
        Self::UnderwaterModifier,
        Self::FresnelScale,
        Self::FresnelOffset,
        Self::NormalScaleX,
        Self::NormalScaleY,
        Self::NormalScaleZ,
        Self::ScaleAbove,
        Self::ScaleBelow,
        Self::BlurMultiplier,
        Self::LargeWaveX,
        Self::LargeWaveY,
        Self::SmallWaveX,
        Self::SmallWaveY,
    ];

    /// The knob's stable short name — see [`SkyKnob::slug`].
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::FogDensity => "water-fog-density",
            Self::UnderwaterModifier => "water-underwater-mod",
            Self::FresnelScale => "water-fresnel-scale",
            Self::FresnelOffset => "water-fresnel-offset",
            Self::NormalScaleX => "water-normal-scale-x",
            Self::NormalScaleY => "water-normal-scale-y",
            Self::NormalScaleZ => "water-normal-scale-z",
            Self::ScaleAbove => "water-scale-above",
            Self::ScaleBelow => "water-scale-below",
            Self::BlurMultiplier => "water-blur",
            Self::LargeWaveX => "water-large-wave-x",
            Self::LargeWaveY => "water-large-wave-y",
            Self::SmallWaveX => "water-small-wave-x",
            Self::SmallWaveY => "water-small-wave-y",
        }
    }

    /// The slider's `(min, max)`, from the reference's own XML.
    #[must_use]
    pub const fn range(self) -> (f32, f32) {
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

    /// How many decimals the value readout shows — see [`SkyKnob::decimals`].
    /// Every water knob is a quantity a person reads at two.
    #[must_use]
    pub const fn decimals(self) -> usize {
        2
    }

    /// Read the knob out of `water`.
    #[must_use]
    pub const fn read(self, water: &WaterSettings) -> f32 {
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
    pub const fn write(self, water: &mut WaterSettings, value: f32) {
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

// ---------------------------------------------------------------------------
// Colours.
// ---------------------------------------------------------------------------

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
    /// Every colour knob — see [`SkyKnob::ALL`].
    pub const ALL: &'static [Self] = &[
        Self::Ambient,
        Self::BlueHorizon,
        Self::BlueDensity,
        Self::SunColor,
        Self::CloudColor,
        Self::WaterFogColor,
    ];

    /// The knob's stable short name — see [`SkyKnob::slug`].
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Ambient => "ambient",
            Self::BlueHorizon => "blue-horizon",
            Self::BlueDensity => "blue-density",
            Self::SunColor => "sun-color",
            Self::CloudColor => "cloud-color",
            Self::WaterFogColor => "water-fog-color",
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
    #[must_use]
    pub fn read(self, sky: &SkySettings, water: &WaterSettings) -> Color {
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
    pub fn write(self, sky: &mut SkySettings, water: &mut WaterSettings, color: Color) {
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

// ---------------------------------------------------------------------------
// Textures.
// ---------------------------------------------------------------------------

/// One texture of the environment a picker swatch edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKnob {
    /// The sky's `cloud_texture` — the reference's Cloud Image.
    CloudImage,
    /// The sky's `sun_texture`.
    SunImage,
    /// The sky's `moon_texture`.
    MoonImage,
    /// The sky's `bloom_texture` — the sun and moon's bloom.
    BloomImage,
    /// The sky's `halo_texture`.
    HaloImage,
    /// The sky's `rainbow_texture`.
    RainbowImage,
    /// The water's `normal_map` — the reference's Water Image.
    WaterNormalMap,
    /// The water's `transparent_texture` — what the surface shows where it is
    /// see-through.
    WaterTransparentTexture,
}

impl TextureKnob {
    /// Every texture knob — see [`SkyKnob::ALL`].
    pub const ALL: &'static [Self] = &[
        Self::CloudImage,
        Self::SunImage,
        Self::MoonImage,
        Self::BloomImage,
        Self::HaloImage,
        Self::RainbowImage,
        Self::WaterNormalMap,
        Self::WaterTransparentTexture,
    ];

    /// The knob's stable short name — see [`SkyKnob::slug`].
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::CloudImage => "cloud-image",
            Self::SunImage => "sun-image",
            Self::MoonImage => "moon-image",
            Self::BloomImage => "bloom-image",
            Self::HaloImage => "halo-image",
            Self::RainbowImage => "rainbow-image",
            Self::WaterNormalMap => "water-image",
            Self::WaterTransparentTexture => "water-transparent-image",
        }
    }

    /// The built-in texture this field means when it holds nothing — what the
    /// swatch is spawned showing, before there is an environment to read.
    #[must_use]
    pub const fn default_texture(self) -> sl_client_bevy::Uuid {
        match self {
            Self::CloudImage => DEFAULT_CLOUD_TEXTURE,
            Self::SunImage => DEFAULT_SUN_TEXTURE,
            Self::MoonImage => DEFAULT_MOON_TEXTURE,
            Self::BloomImage => DEFAULT_BLOOM_TEXTURE,
            Self::HaloImage => DEFAULT_HALO_TEXTURE,
            Self::RainbowImage => DEFAULT_RAINBOW_TEXTURE,
            Self::WaterNormalMap | Self::WaterTransparentTexture => DEFAULT_WATER_NORMAL_TEXTURE,
        }
    }

    /// The texture in force, falling back to the built-in default the field
    /// means when it holds nothing.
    #[must_use]
    pub fn read(self, sky: &SkySettings, water: &WaterSettings) -> TextureKey {
        let stored = match self {
            Self::CloudImage => sky.cloud_texture,
            Self::SunImage => sky.sun_texture,
            Self::MoonImage => sky.moon_texture,
            Self::BloomImage => sky.bloom_texture,
            Self::HaloImage => sky.halo_texture,
            Self::RainbowImage => sky.rainbow_texture,
            Self::WaterNormalMap => water.normal_map,
            Self::WaterTransparentTexture => water.transparent_texture,
        };
        stored.unwrap_or_else(|| TextureKey::from(self.default_texture()))
    }

    /// Write a picked texture back into the settings.
    pub const fn write(
        self,
        sky: &mut SkySettings,
        water: &mut WaterSettings,
        texture: TextureKey,
    ) {
        match self {
            Self::CloudImage => sky.cloud_texture = Some(texture),
            Self::SunImage => sky.sun_texture = Some(texture),
            Self::MoonImage => sky.moon_texture = Some(texture),
            Self::BloomImage => sky.bloom_texture = Some(texture),
            Self::HaloImage => sky.halo_texture = Some(texture),
            Self::RainbowImage => sky.rainbow_texture = Some(texture),
            Self::WaterNormalMap => water.normal_map = Some(texture),
            Self::WaterTransparentTexture => water.transparent_texture = Some(texture),
        }
    }
}

// ---------------------------------------------------------------------------
// Angles.
// ---------------------------------------------------------------------------

/// The angles helper the sun / moon knobs read through: the body's azimuth in
/// degrees, `0.0..360.0`.
fn azimuth_degrees(rotation: &Rotation) -> f32 {
    let (azimuth, _elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth.to_degrees()
}

/// The body's elevation in degrees, `-90.0..=90.0`.
fn elevation_degrees(rotation: &Rotation) -> f32 {
    let (_azimuth, elevation) = rotation_to_azimuth_altitude(rotation);
    elevation.to_degrees()
}

/// `rotation` re-aimed at a new azimuth, keeping its elevation.
fn with_azimuth(rotation: &Rotation, azimuth_deg: f32) -> Rotation {
    let (_azimuth, elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth_altitude_to_rotation(azimuth_deg.to_radians(), elevation)
}

/// `rotation` re-aimed at a new elevation, keeping its azimuth.
fn with_elevation(rotation: &Rotation, elevation_deg: f32) -> Rotation {
    let (azimuth, _elevation) = rotation_to_azimuth_altitude(rotation);
    azimuth_altitude_to_rotation(azimuth, elevation_deg.to_radians())
}

#[cfg(test)]
mod tests {
    use super::{AimKnobs, ColorKnob, SkyKnob, TextureKnob, TrackballAim, WaterKnob};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        DEFAULT_CLOUD_TEXTURE, DEFAULT_WATER_NORMAL_TEXTURE, SkySettings, TextureKey, WaterSettings,
    };

    /// Every sky knob, for the round-trip sweeps.
    const ALL_SKY: &[SkyKnob] = SkyKnob::ALL;

    /// Every water knob.
    const ALL_WATER: &[WaterKnob] = WaterKnob::ALL;

    /// Every colour knob.
    const ALL_COLOR: &[ColorKnob] = ColorKnob::ALL;

    /// Every texture knob.
    const ALL_TEXTURE: &[TextureKnob] = TextureKnob::ALL;

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

    /// **A sky knob writes its own field and no other.** The packed values are
    /// the hazard here as much as in the water: the two cloud position/density
    /// triples and the scroll pair are rebuilt whole on every write, so a knob
    /// reaching for the wrong sibling would quietly zero it.
    #[test]
    fn a_sky_knob_leaves_its_siblings_alone() {
        for knob in ALL_SKY {
            let mut sky = SkySettings::legacy_windlight_default("probe");
            let before = sky.clone();
            knob.write(&mut sky, probe(knob.range()));
            for other in ALL_SKY {
                if other == knob {
                    continue;
                }
                // The two angles of one body share a rotation, so moving an
                // azimuth is allowed to disturb its own elevation's float
                // rounding; every *other* knob must be untouched.
                let read = other.read(&sky);
                let was = other.read(&before);
                assert!(
                    (read - was).abs() < 0.01,
                    "{knob:?} moved {other:?} from {was} to {read}"
                );
            }
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

    /// **Every knob's short name is its own.** The slug is both the Fluent key's
    /// tail and the element id's, so two knobs sharing one would give a window
    /// two controls with the same name and one label for both.
    #[test]
    fn every_slug_is_unique() {
        let mut slugs: Vec<&str> = ALL_SKY
            .iter()
            .map(|knob| knob.slug())
            .chain(ALL_WATER.iter().map(|knob| knob.slug()))
            .chain(ALL_COLOR.iter().map(|knob| knob.slug()))
            .chain(ALL_TEXTURE.iter().map(|knob| knob.slug()))
            .collect();
        let total = slugs.len();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), total, "two knobs share a slug");
    }

    /// **Editing one scattering profile materialises only that one.** A term
    /// written into a frame that carries no profiles has to bring its four
    /// siblings with it or it would be stored beside four zeroes. What it must
    /// *not* do is invent the other two profiles: those are keys the asset
    /// never had, and writing them would change a document the user only
    /// touched one slider of.
    ///
    /// The empty frame has to be made rather than found:
    /// [`SkySettings::legacy_windlight_default`] carries the reference's own
    /// three profiles, because a sky encoded without them is one the reference
    /// throws away. A decoded asset that held none is still emptied this way.
    #[test]
    fn editing_one_profile_leaves_the_others_absent() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        sky.rayleigh_config.clear();
        sky.mie_config.clear();
        sky.absorption_config.clear();

        SkyKnob::RayleighExpScale.write(&mut sky, -0.005);

        assert_eq!(sky.rayleigh_config.len(), 1, "the Rayleigh default arrived");
        assert!((SkyKnob::RayleighExpScale.read(&sky) + 0.005).abs() < 1e-6);
        assert!(
            (SkyKnob::RayleighExpTerm.read(&sky) - 1.0).abs() < 1e-6,
            "and brought its siblings, not four zeroes"
        );
        assert!(sky.mie_config.is_empty(), "Mie was not invented");
        assert!(
            sky.absorption_config.is_empty(),
            "absorption was not invented"
        );
    }

    /// **Editing a profile keeps the layers it is not showing.** The reference's
    /// own panel replaces the whole profile with a single layer on any edit
    /// (`createSingleLayerDensityProfile`), which throws away the second layer
    /// of the ozone ramp its default ships; the sliders here show layer 0 and
    /// write layer 0.
    #[test]
    fn editing_a_profile_keeps_its_other_layers() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        sky.absorption_config = sl_client_bevy::DensityLayer::absorption_default();
        let layers = sky.absorption_config.len();
        assert_eq!(layers, 2, "the reference's ozone ramp is two layers");
        let second = sky
            .absorption_config
            .get(1)
            .map(|layer| layer.constant_term);

        SkyKnob::AbsorptionConstant.write(&mut sky, 0.5);

        assert!((SkyKnob::AbsorptionConstant.read(&sky) - 0.5).abs() < 1e-6);
        assert_eq!(sky.absorption_config.len(), layers, "no layer was dropped");
        assert_eq!(
            sky.absorption_config
                .get(1)
                .map(|layer| layer.constant_term),
            second,
            "the layer the sliders do not show was left alone"
        );
    }

    /// A Mie layer with no `anisotropy` key is one that left the value to the
    /// viewer, not an isotropic one — so the slider opens on the reference's
    /// own default rather than at zero, which is the value that omits the key.
    #[test]
    fn an_absent_mie_anisotropy_reads_as_the_reference_default() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        sky.mie_config = sl_client_bevy::DensityLayer::rayleigh_default();
        assert!((SkyKnob::MieAnisotropy.read(&sky) - 0.8).abs() < 1e-6);
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

    /// **An aim round-trips through the pair**, and the pair leaves the other
    /// body alone — the same claim as above, for the control that writes both
    /// angles in one gesture.
    #[test]
    fn an_aim_round_trips_through_its_knob_pair() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        let moon = AimKnobs::MOON.read(&sky);
        let wanted = TrackballAim {
            azimuth: 200.0,
            elevation: -35.0,
        };
        AimKnobs::SUN.write(&mut sky, wanted);
        let back = AimKnobs::SUN.read(&sky);
        assert!((back.azimuth - wanted.azimuth).abs() < 0.01, "{back:?}");
        assert!((back.elevation - wanted.elevation).abs() < 0.01, "{back:?}");
        let moon_again = AimKnobs::MOON.read(&sky);
        assert!((moon_again.azimuth - moon.azimuth).abs() < 0.01);
        assert!((moon_again.elevation - moon.elevation).abs() < 0.01);
    }

    /// **A body straight up can still be aimed somewhere else.**
    ///
    /// A rotation pointing at a pole has no azimuth stored in it — every
    /// azimuth is the same direction there. So the pair writes the *height*
    /// first and the compass second: written the other way round, an aim
    /// leaving the zenith would lose its azimuth to a rotation that had nowhere
    /// to keep it and come back pointing due east.
    #[test]
    fn a_body_at_the_zenith_keeps_the_azimuth_it_is_given() {
        let mut sky = SkySettings::legacy_windlight_default("probe");
        AimKnobs::SUN.write(
            &mut sky,
            TrackballAim {
                azimuth: 0.0,
                elevation: 90.0,
            },
        );
        AimKnobs::SUN.write(
            &mut sky,
            TrackballAim {
                azimuth: 137.0,
                elevation: 40.0,
            },
        );
        let back = AimKnobs::SUN.read(&sky);
        assert!((back.azimuth - 137.0).abs() < 0.01, "{back:?}");
        assert!((back.elevation - 40.0).abs() < 0.01, "{back:?}");
    }

    /// **Every aim knob belongs to exactly one pair, and every pair to two
    /// knobs.** The lookup a slider is tagged through: a knob missing from it
    /// is a slider the trackball above it never hears from.
    #[test]
    fn the_aim_pairs_cover_the_four_angle_knobs() {
        let angles = [
            SkyKnob::SunAzimuth,
            SkyKnob::SunElevation,
            SkyKnob::MoonAzimuth,
            SkyKnob::MoonElevation,
        ];
        for knob in angles {
            let found: Vec<AimKnobs> = AimKnobs::ALL
                .iter()
                .copied()
                .filter(|pair| pair.covers(knob))
                .collect();
            assert_eq!(found.len(), 1, "{knob:?} is not in exactly one pair");
        }
        for knob in ALL_SKY {
            if angles.contains(knob) {
                continue;
            }
            assert_eq!(
                AimKnobs::of(*knob),
                None,
                "{knob:?} is not an angle and must not be in a pair"
            );
        }
    }

    /// **A colour swatch round-trips through its display scale.** Ambient, the
    /// sun and the two blues are shown divided down because their stored values
    /// exceed what a swatch can paint; a scale applied on one side only would
    /// brighten or dim the sky every time the picker was merely opened and
    /// dismissed.
    #[test]
    fn every_colour_knob_round_trips_through_its_scale() {
        for knob in ALL_COLOR {
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
        for knob in ALL_TEXTURE {
            let mut sky = SkySettings::legacy_windlight_default("probe");
            let mut water = WaterSettings::legacy_default("probe");
            let picked = TextureKey::from(sl_client_bevy::Uuid::from_u128(0x5EED));

            knob.write(&mut sky, &mut water, picked);
            assert_eq!(knob.read(&sky, &water), picked);
            for other in ALL_TEXTURE {
                if other == knob {
                    continue;
                }
                assert_ne!(
                    other.read(&sky, &water),
                    picked,
                    "{knob:?} also wrote {other:?}"
                );
            }
        }
    }
}
