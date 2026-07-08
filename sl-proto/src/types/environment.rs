//! Extended Environment (EEP): a region's or parcel's sky, water, and day-cycle
//! settings, parsed from the `ExtEnvironment` capability.
//!
//! The environment is a **day cycle**: a set of *tracks* (one for water, up to
//! four for the sky at increasing altitudes) that schedule named *frames* over
//! the course of a day, plus the [`SkySettings`] / [`WaterSettings`] frame
//! definitions the tracks reference.
//!
//! The deep atmospheric-scattering profiles (`rayleigh_config`, `mie_config`,
//! `absorption_config`) that the renderer uses are intentionally not parsed here;
//! every other documented sky/water parameter is.

use std::collections::BTreeMap;

use sl_types::key::TextureKey;
use sl_types::lsl::Rotation;
use uuid::Uuid;

// `Color`, `ColorAlpha`, `Glow`, and `CloudPosDensity` now live in
// `sl_types::environment`, and the 3-axis `Scale` factor in `sl_types::map`;
// they are re-exported here so the existing `sl_proto::…` paths are unchanged.
// The LLSD codec helpers below stay client-local.
pub use sl_types::environment::{CloudPosDensity, Color, ColorAlpha, Glow};
pub use sl_types::map::Scale;

/// A region's or parcel's environment, parsed from the `ExtEnvironment`
/// capability (the reply to
/// [`Command::RequestEnvironment`](crate::Command::RequestEnvironment), delivered
/// as [`Event::Environment`](crate::Event::Environment)).
///
/// (Not `Eq`: it ultimately holds `f32` settings.)
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentSettings {
    /// The parcel these settings apply to, or `-1` for the whole region.
    pub parcel_id: i32,
    /// The region the settings came from (nil if the grid omitted it).
    pub region_id: Uuid,
    /// The length of a full day, in seconds.
    pub day_length: i32,
    /// The day-cycle phase offset, in seconds.
    pub day_offset: i32,
    /// The raw environment behaviour flags (e.g. whether parcels may override the
    /// region environment).
    pub flags: u32,
    /// The environment settings version the grid reported.
    pub env_version: i32,
    /// The three altitude breakpoints, in metres, at which the sky switches from
    /// one [`DayCycle::sky_tracks`] entry to the next.
    pub track_altitudes: [f32; 3],
    /// The day cycle: its schedule of sky/water frames and the frames themselves.
    pub day_cycle: DayCycle,
}

/// A day cycle: the tracks scheduling named frames over a day, plus the frame
/// definitions the tracks reference by name.
///
/// (Not `Eq`: the frames hold `f32` settings.)
#[derive(Debug, Clone, PartialEq)]
pub struct DayCycle {
    /// The cycle's name.
    pub name: String,
    /// The water track (track 0): keyframes naming [`Self::water_frames`] entries.
    pub water_track: Vec<DayCycleFrame>,
    /// The sky tracks (tracks 1+), ground up. Index 0 is the surface track; later
    /// entries take effect above the matching
    /// [`EnvironmentSettings::track_altitudes`] breakpoint. Each keyframe names a
    /// [`Self::sky_frames`] entry.
    pub sky_tracks: Vec<Vec<DayCycleFrame>>,
    /// The named sky frames the sky tracks reference.
    pub sky_frames: BTreeMap<String, SkySettings>,
    /// The named water frames the water track references.
    pub water_frames: BTreeMap<String, WaterSettings>,
}

/// One keyframe within a day-cycle track: a named frame and the time of day it
/// is reached.
#[derive(Debug, Clone, PartialEq)]
pub struct DayCycleFrame {
    /// The time of day this frame is reached, as a fraction of the day in
    /// `0.0..=1.0`.
    pub keyframe: f32,
    /// The name of the [`SkySettings`] / [`WaterSettings`] frame applied at this
    /// keyframe (a key into [`DayCycle::sky_frames`] / [`DayCycle::water_frames`]).
    pub name: String,
}

/// A single sky frame (`LLSettingsSky`): the atmosphere, sun, moon, and cloud
/// state at one keyframe. The legacy haze colours/scalars (`ambient`,
/// `blue_horizon`, `blue_density`, `haze_*`, the multipliers) are read from the
/// frame's `legacy_haze` block.
///
/// (Not `Eq`: holds `f32` fields.)
#[derive(Debug, Clone, PartialEq)]
pub struct SkySettings {
    /// The frame's name.
    pub name: String,
    /// The sun's orientation.
    pub sun_rotation: Rotation,
    /// The moon's orientation.
    pub moon_rotation: Rotation,
    /// The sunlight colour, RGBA.
    pub sunlight_color: ColorAlpha,
    /// The ambient light colour, RGB (from `legacy_haze`).
    pub ambient: Color,
    /// The horizon blue colour, RGB (from `legacy_haze`).
    pub blue_horizon: Color,
    /// The blue-density colour, RGB (from `legacy_haze`).
    pub blue_density: Color,
    /// The haze horizon factor (from `legacy_haze`).
    pub haze_horizon: f32,
    /// The haze density (from `legacy_haze`).
    pub haze_density: f32,
    /// The atmospheric density multiplier (from `legacy_haze`).
    pub density_multiplier: f32,
    /// The atmospheric distance multiplier (from `legacy_haze`).
    pub distance_multiplier: f32,
    /// The maximum sky dome altitude.
    pub max_y: f32,
    /// The gamma applied to the sky.
    pub gamma: f32,
    /// The cloud colour, RGB.
    pub cloud_color: Color,
    /// The cloud layer 1 position (X, Y) and density (Z).
    pub cloud_pos_density1: CloudPosDensity,
    /// The cloud layer 2 detail position (X, Y) and density (Z).
    pub cloud_pos_density2: CloudPosDensity,
    /// The cloud scale.
    pub cloud_scale: f32,
    /// The cloud scroll rate (X, Y).
    pub cloud_scroll_rate: [f32; 2],
    /// The cloud shadow / coverage.
    pub cloud_shadow: f32,
    /// The cloud variance.
    pub cloud_variance: f32,
    /// The sun/moon glow (size, unused, focus).
    pub glow: Glow,
    /// The starfield brightness.
    pub star_brightness: f32,
    /// The sun size scale.
    pub sun_scale: f32,
    /// The moon size scale.
    pub moon_scale: f32,
    /// The moon brightness multiplier.
    pub moon_brightness: f32,
    /// The sun's angular diameter, in radians.
    pub sun_arc_radians: f32,
    /// The atmospheric droplet radius.
    pub droplet_radius: f32,
    /// The ice level.
    pub ice_level: f32,
    /// The atmospheric moisture level.
    pub moisture_level: f32,
    /// The atmosphere's outer radius.
    pub sky_top_radius: f32,
    /// The atmosphere's inner radius.
    pub sky_bottom_radius: f32,
    /// The planet radius.
    pub planet_radius: f32,
    /// The sun disc texture (`None` for the viewer default).
    pub sun_texture: Option<TextureKey>,
    /// The moon disc texture (`None` for the viewer default).
    pub moon_texture: Option<TextureKey>,
    /// The cloud texture (`None` for the viewer default).
    pub cloud_texture: Option<TextureKey>,
    /// The bloom texture (`None` for the viewer default).
    pub bloom_texture: Option<TextureKey>,
    /// The halo texture (`None` for the viewer default).
    pub halo_texture: Option<TextureKey>,
    /// The rainbow texture (`None` for the viewer default).
    pub rainbow_texture: Option<TextureKey>,
}

/// A single water frame (`LLSettingsWater`): the surface and underwater state at
/// one keyframe.
///
/// (Not `Eq`: holds `f32` fields.)
#[derive(Debug, Clone, PartialEq)]
pub struct WaterSettings {
    /// The frame's name.
    pub name: String,
    /// The reflection blur multiplier.
    pub blur_multiplier: f32,
    /// The Fresnel offset.
    pub fresnel_offset: f32,
    /// The Fresnel scale.
    pub fresnel_scale: f32,
    /// The normal-map (wavelet) scale (X, Y, Z).
    pub normal_scale: Scale,
    /// The normal/wave texture (`None` for the viewer default).
    pub normal_map: Option<TextureKey>,
    /// The refraction scale above the surface.
    pub scale_above: f32,
    /// The refraction scale below the surface.
    pub scale_below: f32,
    /// The transparent-water texture (`None` for the viewer default).
    pub transparent_texture: Option<TextureKey>,
    /// The underwater fog modifier.
    pub underwater_fog_mod: f32,
    /// The water fog colour, RGB.
    pub water_fog_color: Color,
    /// The water fog density exponent.
    pub water_fog_density: f32,
    /// The wave 1 direction (X, Y).
    pub wave1_direction: [f32; 2],
    /// The wave 2 direction (X, Y).
    pub wave2_direction: [f32; 2],
}

impl EnvironmentSettings {
    /// The built-in **legacy WindLight default** environment: the sky and water
    /// the reference viewer falls back to when a region advertises no Extended
    /// Environment (EEP) capability. Mirrors Firestorm's `LLSettingsSky::defaults`
    /// / `LLSettingsWater::defaults` (`indra/llinventory/llsettings{sky,water}.cpp`)
    /// — one midday sky frame and one water frame on a trivial single-keyframe day
    /// cycle. Used as the viewer's starting environment until a real
    /// [`Event::Environment`](crate::Event::Environment) arrives.
    #[must_use]
    pub fn legacy_windlight_default() -> Self {
        let sky = SkySettings::legacy_windlight_default("Default");
        let water = WaterSettings::legacy_default("Default");
        let mut sky_frames = BTreeMap::new();
        drop(sky_frames.insert(sky.name.clone(), sky));
        let mut water_frames = BTreeMap::new();
        drop(water_frames.insert(water.name.clone(), water));
        let frame = |name: &str| DayCycleFrame {
            keyframe: 0.0,
            name: name.to_owned(),
        };
        Self {
            parcel_id: -1,
            region_id: Uuid::nil(),
            // The reference default day length is four hours.
            day_length: 4 * 60 * 60,
            day_offset: 0,
            flags: 0,
            env_version: -1,
            track_altitudes: [1000.0, 2000.0, 3000.0],
            day_cycle: DayCycle {
                name: "Default".to_owned(),
                water_track: vec![frame("Default")],
                sky_tracks: vec![vec![frame("Default")]],
                sky_frames,
                water_frames,
            },
        }
    }
}

/// Reproduces the reference `convert_azimuth_and_altitude_to_quat`
/// (`indra/llinventory/llsettingssky.cpp`): the rotation taking the local `+X`
/// axis to the sky direction at the given spherical angles, in radians.
fn azimuth_altitude_to_rotation(azimuth: f32, altitude: f32) -> Rotation {
    // The unit direction the angles point at (SL's `+x` right, `+y` at, `+z` up).
    let dir_x = azimuth.cos() * altitude.cos();
    let dir_y = azimuth.sin() * altitude.cos();
    let dir_z = altitude.sin();
    // `axis = x_axis × dir`; `dir` is a unit vector, so `x_axis · dir` is `dir_x`.
    let axis_x = 0.0_f32;
    let axis_y = -dir_z;
    let axis_z = dir_y;
    let axis_len = axis_y.hypot(axis_z);
    let angle = dir_x.clamp(-1.0, 1.0).acos();
    // `dir` parallel to `+x`: no rotation.
    if axis_len <= f32::EPSILON {
        return Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
    }
    let half = angle * 0.5;
    let sin_half = half.sin();
    let scale = sin_half / axis_len;
    Rotation {
        x: axis_x * scale,
        y: axis_y * scale,
        z: axis_z * scale,
        s: half.cos(),
    }
}

impl SkySettings {
    /// The reference viewer's built-in default sky (`LLSettingsSky::defaults`,
    /// `indra/llinventory/llsettingssky.cpp`), including the legacy-haze fallbacks
    /// (`LLColor3`/`F32` defaults from `LLSettingsSky::loadValuesFromLLSD`). The
    /// deep atmospheric-scattering profiles are not modelled (see the module
    /// docs); every documented scalar/colour is set to its reference default.
    #[must_use]
    pub fn legacy_windlight_default(name: &str) -> Self {
        // Sun and moon tracks at the default day's start (track position 0): the
        // reference offsets the two so they do not sit at opposite poles.
        let eighty_deg = 80.0_f32.to_radians();
        let eighth_pi = std::f32::consts::FRAC_PI_8;
        let sun_rotation = azimuth_altitude_to_rotation(0.0, eighty_deg);
        let moon_rotation = azimuth_altitude_to_rotation(eighth_pi, eighty_deg + eighth_pi);
        Self {
            name: name.to_owned(),
            sun_rotation,
            moon_rotation,
            sunlight_color: ColorAlpha::new(0.7342, 0.7815, 0.8999, 0.0),
            // Legacy-haze defaults.
            ambient: Color::new(0.25, 0.25, 0.25),
            blue_horizon: Color::new(0.4954, 0.4954, 0.6399),
            blue_density: Color::new(0.2447, 0.4487, 0.7599),
            haze_horizon: 0.19,
            haze_density: 0.7,
            density_multiplier: 0.0001,
            distance_multiplier: 0.8,
            max_y: 1605.0,
            gamma: 1.0,
            cloud_color: Color::new(0.4099, 0.4099, 0.4099),
            cloud_pos_density1: CloudPosDensity::new(1.0, 0.526, 1.0),
            cloud_pos_density2: CloudPosDensity::new(1.0, 0.526, 1.0),
            cloud_scale: 0.4199,
            cloud_scroll_rate: [0.2, 0.01],
            cloud_shadow: 0.2699,
            cloud_variance: 0.0,
            glow: Glow::new(5.0, 0.001, -0.4799),
            star_brightness: 250.0,
            sun_scale: 1.0,
            moon_scale: 1.0,
            moon_brightness: 0.5,
            sun_arc_radians: 0.00045,
            droplet_radius: 800.0,
            ice_level: 0.0,
            moisture_level: 0.0,
            sky_top_radius: 6420.0,
            sky_bottom_radius: 6360.0,
            planet_radius: 6360.0,
            // `None` selects the viewer's built-in sun/moon/cloud/etc. textures.
            sun_texture: None,
            moon_texture: None,
            cloud_texture: None,
            bloom_texture: None,
            halo_texture: None,
            rainbow_texture: None,
        }
    }
}

impl WaterSettings {
    /// The reference viewer's built-in default water (`LLSettingsWater::defaults`,
    /// `indra/llinventory/llsettingswater.cpp`).
    #[must_use]
    pub fn legacy_default(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            blur_multiplier: 0.04,
            fresnel_offset: 0.5,
            fresnel_scale: 0.3999,
            normal_scale: Scale::new(2.0, 2.0, 2.0),
            normal_map: None,
            scale_above: 0.0299,
            scale_below: 0.2,
            transparent_texture: None,
            underwater_fog_mod: 0.25,
            water_fog_color: Color::new(0.0156, 0.149, 0.2509),
            water_fog_density: 2.0,
            wave1_direction: [1.04999, -0.42],
            wave2_direction: [1.10999, -1.16],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloudPosDensity, Color, ColorAlpha, EnvironmentSettings, Glow, Scale, SkySettings,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn color_channels_round_trip() {
        let color = Color::new(0.25, 0.5, 0.75);
        // Compare bit patterns: `float_cmp` forbids an exact `==` on the floats.
        assert_eq!(color.red().to_bits(), 0.25_f32.to_bits());
        assert_eq!(color.green().to_bits(), 0.5_f32.to_bits());
        assert_eq!(color.blue().to_bits(), 0.75_f32.to_bits());
    }

    #[test]
    fn color_alpha_channels_round_trip() {
        let color = ColorAlpha::new(0.25, 0.5, 0.75, 0.875);
        assert_eq!(color.red().to_bits(), 0.25_f32.to_bits());
        assert_eq!(color.green().to_bits(), 0.5_f32.to_bits());
        assert_eq!(color.blue().to_bits(), 0.75_f32.to_bits());
        assert_eq!(color.alpha().to_bits(), 0.875_f32.to_bits());
    }

    #[test]
    fn scale_axes_round_trip() {
        let scale = Scale::new(2.0, 3.0, 4.0);
        assert_eq!(scale.x().to_bits(), 2.0_f32.to_bits());
        assert_eq!(scale.y().to_bits(), 3.0_f32.to_bits());
        assert_eq!(scale.z().to_bits(), 4.0_f32.to_bits());
    }

    #[test]
    fn glow_preserves_the_reserved_middle_component() {
        // The middle component is unused but must round-trip verbatim.
        let glow = Glow::new(5.0, -1.5, -2.5);
        assert_eq!(glow.size().to_bits(), 5.0_f32.to_bits());
        assert_eq!(glow.reserved().to_bits(), (-1.5_f32).to_bits());
        assert_eq!(glow.focus().to_bits(), (-2.5_f32).to_bits());
    }

    #[test]
    fn cloud_pos_density_names_its_components() {
        let value = CloudPosDensity::new(1.0, 0.5, 0.25);
        assert_eq!(value.position_x().to_bits(), 1.0_f32.to_bits());
        assert_eq!(value.position_y().to_bits(), 0.5_f32.to_bits());
        assert_eq!(value.density().to_bits(), 0.25_f32.to_bits());
    }

    #[test]
    fn legacy_windlight_default_has_one_referenced_sky_and_water_frame() {
        let env = EnvironmentSettings::legacy_windlight_default();
        let cycle = &env.day_cycle;
        // The single sky/water track keyframe must resolve to a defined frame.
        let sky_ok = cycle
            .sky_tracks
            .first()
            .and_then(|track| track.first())
            .is_some_and(|frame| cycle.sky_frames.contains_key(&frame.name));
        let water_ok = cycle
            .water_track
            .first()
            .is_some_and(|frame| cycle.water_frames.contains_key(&frame.name));
        assert!(sky_ok);
        assert!(water_ok);
    }

    #[test]
    fn default_sky_sun_rotation_is_a_unit_quaternion() {
        let sky = SkySettings::legacy_windlight_default("Default");
        let rotation = sky.sun_rotation;
        // A rotation must be a unit quaternion; the fallback sun should point up
        // and away from straight ahead (a non-identity daytime track).
        let length_squared = rotation.x * rotation.x
            + rotation.y * rotation.y
            + rotation.z * rotation.z
            + rotation.s * rotation.s;
        assert!((length_squared - 1.0).abs() < 1.0e-4);
        assert!(rotation.s.to_bits() != 1.0_f32.to_bits());
    }
}
