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

/// An RGB colour — three `f32` channels (normally `0.0..=1.0`, but HDR
/// environment colours can exceed `1.0`). A named type so a colour cannot be
/// transposed with a position, direction, or scale. A general SL value type;
/// kept client-local for now (a candidate to migrate into `sl-types` later).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// The red channel.
    red: f32,
    /// The green channel.
    green: f32,
    /// The blue channel.
    blue: f32,
}

impl Color {
    /// Creates a colour from its red/green/blue channels.
    #[must_use]
    pub const fn new(red: f32, green: f32, blue: f32) -> Self {
        Self { red, green, blue }
    }

    /// The red channel.
    #[must_use]
    pub const fn red(&self) -> f32 {
        self.red
    }

    /// The green channel.
    #[must_use]
    pub const fn green(&self) -> f32 {
        self.green
    }

    /// The blue channel.
    #[must_use]
    pub const fn blue(&self) -> f32 {
        self.blue
    }
}

/// An RGBA colour — four `f32` channels (RGB plus an alpha channel). The
/// alpha-carrying sibling of [`Color`]; a distinct type so it can't be
/// transposed with a 3-channel colour, a position, or a rotation quaternion
/// (all of which are also arrays of `f32`). Its one user is the windlight
/// `sunlight_color`. Channels are normally `0.0..=1.0` but HDR values can
/// exceed `1.0`. A general SL value type, kept client-local for now (a
/// candidate to migrate into `sl-types` later).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorAlpha {
    /// The red channel.
    red: f32,
    /// The green channel.
    green: f32,
    /// The blue channel.
    blue: f32,
    /// The alpha channel.
    alpha: f32,
}

impl ColorAlpha {
    /// Creates a colour from its red/green/blue/alpha channels.
    #[must_use]
    pub const fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// The red channel.
    #[must_use]
    pub const fn red(&self) -> f32 {
        self.red
    }

    /// The green channel.
    #[must_use]
    pub const fn green(&self) -> f32 {
        self.green
    }

    /// The blue channel.
    #[must_use]
    pub const fn blue(&self) -> f32 {
        self.blue
    }

    /// The alpha channel.
    #[must_use]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }
}

/// A 3-axis **scale factor** (X/Y/Z) — a dimensionless multiplier per axis,
/// **not** a size in metres. Its one user is the water normal-map "Reflection
/// Wavelet Scale" (the viewer's `normal_scale`: three per-axis multipliers,
/// roughly `0.0..=10.0`, applied to the wavelet normal-map sampling — confirmed
/// against the Firestorm `WATER_NORM_SCALE` shader uniform). A scale is not a
/// position or a direction (it has no origin and need not be a unit vector), so
/// it gets its own type. Candidate to migrate into `sl-types`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// The x-axis scale factor.
    x: f32,
    /// The y-axis scale factor.
    y: f32,
    /// The z-axis scale factor.
    z: f32,
}

impl Scale {
    /// Creates a scale from its per-axis factors.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The x-axis scale factor.
    #[must_use]
    pub const fn x(&self) -> f32 {
        self.x
    }

    /// The y-axis scale factor.
    #[must_use]
    pub const fn y(&self) -> f32 {
        self.y
    }

    /// The z-axis scale factor.
    #[must_use]
    pub const fn z(&self) -> f32 {
        self.z
    }
}

/// A windlight sun/moon **glow** parameter. The wire packs it as a 3-vector
/// `(size, reserved, focus)` whose middle component is unused/reserved (the
/// viewer always sends `0`); it is preserved verbatim so a decode/encode round
/// trip is byte-identical. The meaningful channels are [`size`](Self::size) and
/// [`focus`](Self::focus).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    /// The glow size.
    size: f32,
    /// The unused/reserved middle component (preserved for round-trip).
    reserved: f32,
    /// The glow focus.
    focus: f32,
}

impl Glow {
    /// Creates a glow from its wire `(size, reserved, focus)` components.
    #[must_use]
    pub const fn new(size: f32, reserved: f32, focus: f32) -> Self {
        Self {
            size,
            reserved,
            focus,
        }
    }

    /// The glow size.
    #[must_use]
    pub const fn size(&self) -> f32 {
        self.size
    }

    /// The unused/reserved middle component (normally `0`).
    #[must_use]
    pub const fn reserved(&self) -> f32 {
        self.reserved
    }

    /// The glow focus.
    #[must_use]
    pub const fn focus(&self) -> f32 {
        self.focus
    }
}

/// A windlight cloud layer's scroll **position** (X, Y) packed with its
/// **density** (Z) in one wire 3-vector (the viewer's `cloud_pos_density*`).
/// The three components are semantically distinct — two are a 2-D scroll offset,
/// one is a density — so they get named accessors rather than `x`/`y`/`z`, and
/// this type cannot be confused with a position or direction. Candidate to
/// migrate into `sl-types` later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudPosDensity {
    /// The cloud-scroll x position.
    position_x: f32,
    /// The cloud-scroll y position.
    position_y: f32,
    /// The cloud density.
    density: f32,
}

impl CloudPosDensity {
    /// Creates a value from its wire `(position_x, position_y, density)`
    /// components.
    #[must_use]
    pub const fn new(position_x: f32, position_y: f32, density: f32) -> Self {
        Self {
            position_x,
            position_y,
            density,
        }
    }

    /// The cloud-scroll x position.
    #[must_use]
    pub const fn position_x(&self) -> f32 {
        self.position_x
    }

    /// The cloud-scroll y position.
    #[must_use]
    pub const fn position_y(&self) -> f32 {
        self.position_y
    }

    /// The cloud density.
    #[must_use]
    pub const fn density(&self) -> f32 {
        self.density
    }
}

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

#[cfg(test)]
mod tests {
    use super::{CloudPosDensity, Color, ColorAlpha, Glow, Scale};
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
}
