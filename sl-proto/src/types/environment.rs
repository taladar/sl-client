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
    pub sunlight_color: [f32; 4],
    /// The ambient light colour, RGB (from `legacy_haze`).
    pub ambient: [f32; 3],
    /// The horizon blue colour, RGB (from `legacy_haze`).
    pub blue_horizon: [f32; 3],
    /// The blue-density colour, RGB (from `legacy_haze`).
    pub blue_density: [f32; 3],
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
    pub cloud_color: [f32; 3],
    /// The cloud layer 1 position (X, Y) and density (Z).
    pub cloud_pos_density1: [f32; 3],
    /// The cloud layer 2 detail position (X, Y) and density (Z).
    pub cloud_pos_density2: [f32; 3],
    /// The cloud scale.
    pub cloud_scale: f32,
    /// The cloud scroll rate (X, Y).
    pub cloud_scroll_rate: [f32; 2],
    /// The cloud shadow / coverage.
    pub cloud_shadow: f32,
    /// The cloud variance.
    pub cloud_variance: f32,
    /// The sun/moon glow (size, unused, focus).
    pub glow: [f32; 3],
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
    /// The sun disc texture (nil for the default).
    pub sun_texture: TextureKey,
    /// The moon disc texture (nil for the default).
    pub moon_texture: TextureKey,
    /// The cloud texture (nil for the default).
    pub cloud_texture: TextureKey,
    /// The bloom texture (nil for the default).
    pub bloom_texture: TextureKey,
    /// The halo texture (nil for the default).
    pub halo_texture: TextureKey,
    /// The rainbow texture (nil for the default).
    pub rainbow_texture: TextureKey,
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
    pub normal_scale: [f32; 3],
    /// The normal/wave texture.
    pub normal_map: TextureKey,
    /// The refraction scale above the surface.
    pub scale_above: f32,
    /// The refraction scale below the surface.
    pub scale_below: f32,
    /// The transparent-water texture.
    pub transparent_texture: TextureKey,
    /// The underwater fog modifier.
    pub underwater_fog_mod: f32,
    /// The water fog colour, RGB.
    pub water_fog_color: [f32; 3],
    /// The water fog density exponent.
    pub water_fog_density: f32,
    /// The wave 1 direction (X, Y).
    pub wave1_direction: [f32; 2],
    /// The wave 2 direction (X, Y).
    pub wave2_direction: [f32; 2],
}
