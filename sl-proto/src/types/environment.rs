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

    /// The 0-based index into [`DayCycle::sky_tracks`] whose altitude band
    /// contains `altitude` (metres above the region), mirroring the reference
    /// `LLEnvironment::calculateSkyTrackForAltitude`
    /// (`indra/newview/llenvironment.cpp`).
    ///
    /// The reference clamps a camera altitude against the four breakpoints
    /// `[0, a1, a2, a3]` and returns a *track number* `1..=4`, where sky track 1
    /// is the surface track. Here the surface track is [`DayCycle::sky_tracks`]
    /// index 0, so the mapping is: `altitude <= a1` → 0, `<= a2` → 1, `<= a3` → 2,
    /// otherwise 3. The result is clamped to the number of tracks the day cycle
    /// actually carries, so a cycle with a single ground track always selects it.
    #[must_use]
    pub fn sky_track_for_altitude(&self, altitude: f32) -> usize {
        let [a1, a2, a3] = self.track_altitudes;
        let raw = if altitude <= a1 {
            0
        } else if altitude <= a2 {
            1
        } else if altitude <= a3 {
            2
        } else {
            3
        };
        let last = self.day_cycle.sky_tracks.len().saturating_sub(1);
        raw.min(last)
    }

    /// The active [`SkySettings`] for a camera at `altitude` and a day-cycle
    /// `position` (the normalised time of day, `0.0..=1.0`): the keyframe in force
    /// at `position` on the altitude-selected
    /// [`sky_track`](Self::sky_track_for_altitude), resolved through
    /// [`DayCycle::sky_frames`].
    ///
    /// This selects the *active* keyframe (the reference
    /// `LLEnvironment::convert_time_to_position` → `get_wrapping_atbefore`)
    /// without blending toward the next one. The smooth day-cycle interpolation
    /// is [`blended_sky_settings`](Self::blended_sky_settings); this unblended
    /// selection is kept for the callers (and tests) that want a borrowed frame.
    /// Falls back to any defined sky frame if the selected track is empty or
    /// names a missing frame, and to `None` only if the cycle defines no sky
    /// frame at all.
    #[must_use]
    pub fn active_sky_settings(&self, altitude: f32, position: f32) -> Option<&SkySettings> {
        let cycle = &self.day_cycle;
        let track_frame = cycle
            .sky_tracks
            .get(self.sky_track_for_altitude(altitude))
            .and_then(|track| active_keyframe(track, position))
            .and_then(|frame| cycle.sky_frames.get(&frame.name));
        track_frame.or_else(|| cycle.sky_frames.values().next())
    }

    /// The **blended** [`SkySettings`] for a camera at `altitude` and a day-cycle
    /// `position` (the normalised time of day, `0.0..=1.0`): the smooth
    /// interpolation between the two keyframes bounding `position` on the
    /// altitude-selected [`sky_track`](Self::sky_track_for_altitude), the
    /// reference `LLEnvironment` day-cycle blender
    /// (`LLSettingsBlender` → `LLSettingsBase::blend`).
    ///
    /// Where [`active_sky_settings`](Self::active_sky_settings) snaps to the
    /// keyframe in force, this finds the bounding pair `(lower, upper)` around
    /// `position` (wrapping across the day boundary) and blends the lower toward
    /// the upper by the fraction of the way `position` has travelled between
    /// their keyframe times (see [`SkySettings::blend`]). A single-keyframe track
    /// (or the built-in default cycle) yields that one frame unchanged.
    ///
    /// Returns an *owned* frame (the blend synthesises new values), unlike the
    /// borrowing `active_sky_settings`. Falls back to any defined sky frame if
    /// the selected track is empty or names missing frames, and to `None` only if
    /// the cycle defines no sky frame at all.
    #[must_use]
    pub fn blended_sky_settings(&self, altitude: f32, position: f32) -> Option<SkySettings> {
        let cycle = &self.day_cycle;
        let track = cycle.sky_tracks.get(self.sky_track_for_altitude(altitude));
        let blended = track.and_then(|track| {
            let (lower, upper, factor) = bounding_keyframes(track, position)?;
            let lower_sky = cycle.sky_frames.get(&lower.name)?;
            // If the upper frame is missing, hold the lower one rather than
            // falling through to an unrelated frame.
            match cycle.sky_frames.get(&upper.name) {
                Some(upper_sky) => Some(lower_sky.blend(upper_sky, factor)),
                None => Some(lower_sky.clone()),
            }
        });
        blended.or_else(|| cycle.sky_frames.values().next().cloned())
    }
}

/// The day-cycle keyframe in force at normalised time `position` (`0.0..=1.0`) on
/// `track`: the frame with the greatest keyframe time `<= position`, wrapping to
/// the last keyframe of the cycle when `position` precedes the first (the
/// reference `get_wrapping_atbefore`). `None` only for an empty track.
fn active_keyframe(track: &[DayCycleFrame], position: f32) -> Option<&DayCycleFrame> {
    let at_before = track
        .iter()
        .filter(|frame| frame.keyframe <= position)
        .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe));
    // Before the first keyframe the cycle wraps: the latest keyframe (end of the
    // previous day) is still in force.
    at_before.or_else(|| {
        track
            .iter()
            .max_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
    })
}

/// The two keyframes bounding normalised time `position` (`0.0..=1.0`) on
/// `track`, plus the blend factor `0.0..=1.0` measuring how far `position` has
/// travelled from the lower keyframe toward the upper one (the reference
/// `LLSettingsDay` bounding-keyframe lookup that feeds `LLSettingsBlender`).
///
/// The *lower* keyframe is the one in force at `position` ([`active_keyframe`]);
/// the *upper* is the next keyframe after it. The day cycle wraps, so when
/// `position` sits after the last keyframe the upper wraps to the first
/// (its keyframe time treated as `+ 1.0`), and when `position` precedes the
/// first keyframe the lower wraps to the last (treated as `- 1.0`). A
/// single-keyframe track returns that frame as both bounds with factor `0.0`.
/// `None` only for an empty track.
fn bounding_keyframes(
    track: &[DayCycleFrame],
    position: f32,
) -> Option<(&DayCycleFrame, &DayCycleFrame, f32)> {
    let lower = active_keyframe(track, position)?;
    // A single keyframe is in force all day: it is both bounds, blended with
    // itself (factor is immaterial, so report the natural `0.0`).
    if let [only] = track {
        return Some((only, only, 0.0));
    }
    // The upper bound is the earliest keyframe strictly after `position`; if none
    // exists the cycle wraps to the earliest keyframe of the day.
    let upper = track
        .iter()
        .filter(|frame| frame.keyframe > position)
        .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
        .or_else(|| {
            track
                .iter()
                .min_by(|a, b| a.keyframe.total_cmp(&b.keyframe))
        })?;
    // Unwrap the two keyframe times onto a monotonic line around `position` so the
    // span is positive even across the day boundary.
    let lower_time = if lower.keyframe <= position {
        lower.keyframe
    } else {
        lower.keyframe - 1.0
    };
    let upper_time = if upper.keyframe > position {
        upper.keyframe
    } else {
        upper.keyframe + 1.0
    };
    let span = upper_time - lower_time;
    let factor = if span > f32::EPSILON {
        ((position - lower_time) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some((lower, upper, factor))
}

/// Linear interpolation between `a` and `b` by `factor` (`0.0` → `a`, `1.0` →
/// `b`), the scalar primitive every [`SkySettings::blend`] channel builds on.
fn lerp_f32(a: f32, b: f32, factor: f32) -> f32 {
    a + (b - a) * factor
}

/// Per-channel lerp of two [`Color`]s.
fn lerp_color(a: Color, b: Color, factor: f32) -> Color {
    Color::new(
        lerp_f32(a.red(), b.red(), factor),
        lerp_f32(a.green(), b.green(), factor),
        lerp_f32(a.blue(), b.blue(), factor),
    )
}

/// Per-channel lerp of two [`ColorAlpha`]s.
fn lerp_color_alpha(a: ColorAlpha, b: ColorAlpha, factor: f32) -> ColorAlpha {
    ColorAlpha::new(
        lerp_f32(a.red(), b.red(), factor),
        lerp_f32(a.green(), b.green(), factor),
        lerp_f32(a.blue(), b.blue(), factor),
        lerp_f32(a.alpha(), b.alpha(), factor),
    )
}

/// Per-component lerp of two [`Glow`]s (the reserved middle component is
/// interpolated too, so a round trip stays well-defined).
fn lerp_glow(a: Glow, b: Glow, factor: f32) -> Glow {
    Glow::new(
        lerp_f32(a.size(), b.size(), factor),
        lerp_f32(a.reserved(), b.reserved(), factor),
        lerp_f32(a.focus(), b.focus(), factor),
    )
}

/// Per-component lerp of two [`CloudPosDensity`]s.
fn lerp_cloud_pos_density(a: CloudPosDensity, b: CloudPosDensity, factor: f32) -> CloudPosDensity {
    CloudPosDensity::new(
        lerp_f32(a.position_x(), b.position_x(), factor),
        lerp_f32(a.position_y(), b.position_y(), factor),
        lerp_f32(a.density(), b.density(), factor),
    )
}

/// Per-component lerp of two 2-vectors (e.g. `cloud_scroll_rate`).
fn lerp_array2(a: [f32; 2], b: [f32; 2], factor: f32) -> [f32; 2] {
    let [ax, ay] = a;
    let [bx, by] = b;
    [lerp_f32(ax, bx, factor), lerp_f32(ay, by, factor)]
}

/// Normalise a quaternion, falling back to identity for a degenerate (zero)
/// input so a blend never produces a non-rotation.
fn normalize_rotation(r: Rotation) -> Rotation {
    let length = (r.x * r.x + r.y * r.y + r.z * r.z + r.s * r.s).sqrt();
    if length > f32::EPSILON {
        let inv = 1.0 / length;
        Rotation {
            x: r.x * inv,
            y: r.y * inv,
            z: r.z * inv,
            s: r.s * inv,
        }
    } else {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    }
}

/// Spherical linear interpolation between two rotations (the reference slerps the
/// sun/moon `sun_rotation` / `moon_rotation` keys rather than lerping their
/// components — `LLSettingsBase::getSlerps`). Takes the shortest arc (negating
/// the far quaternion when the dot product is negative) and degrades to a
/// normalised lerp for nearly-parallel inputs to stay numerically stable.
fn slerp_rotation(a: &Rotation, b: &Rotation, factor: f32) -> Rotation {
    // Nearly parallel: the arc is tiny, so a normalised lerp is both stable and
    // visually identical to a slerp.
    const SLERP_LERP_THRESHOLD: f32 = 0.9995;
    let raw_dot = a.x * b.x + a.y * b.y + a.z * b.z + a.s * b.s;
    // Take the shortest arc: a quaternion and its negation are the same rotation.
    let (bx, by, bz, bs, dot) = if raw_dot < 0.0 {
        (-b.x, -b.y, -b.z, -b.s, -raw_dot)
    } else {
        (b.x, b.y, b.z, b.s, raw_dot)
    };
    if dot > SLERP_LERP_THRESHOLD {
        return normalize_rotation(Rotation {
            x: lerp_f32(a.x, bx, factor),
            y: lerp_f32(a.y, by, factor),
            z: lerp_f32(a.z, bz, factor),
            s: lerp_f32(a.s, bs, factor),
        });
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let sin_theta_0 = theta_0.sin();
    let theta = theta_0 * factor;
    let scale_from = (theta_0 - theta).sin() / sin_theta_0;
    let scale_to = theta.sin() / sin_theta_0;
    Rotation {
        x: a.x * scale_from + bx * scale_to,
        y: a.y * scale_from + by * scale_to,
        z: a.z * scale_from + bz * scale_to,
        s: a.s * scale_from + bs * scale_to,
    }
}

/// Selects `lower` below the halfway point and `upper` at or beyond it — the
/// reference `LLSettingsBase::interpolateSDValue` fallback for the non-numeric
/// settings (textures, names): a discrete `mix > 0.5 ? other : this`.
fn pick_at_half<T: Clone>(lower: &T, upper: &T, factor: f32) -> T {
    if factor > 0.5 {
        upper.clone()
    } else {
        lower.clone()
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
    /// Blend this sky frame toward `other` by `factor` (`0.0` → `self`, `1.0` →
    /// `other`), the reference day-cycle frame interpolation
    /// (`LLSettingsBase::blend` over the sky settings map).
    ///
    /// Numeric channels (the haze scalars, colours, cloud/glow parameters,
    /// radii, …) are linearly interpolated; the sun and moon rotations are
    /// **slerped** (the reference marks them as slerp keys); and the discrete,
    /// non-blendable settings — the frame name and the sun / moon / cloud / bloom
    /// / halo / rainbow textures — snap to whichever frame is nearer
    /// (`factor > 0.5` picks `other`), matching the reference's
    /// `mix > 0.5 ? other : this` for its non-numeric settings.
    #[must_use]
    pub fn blend(&self, other: &Self, factor: f32) -> Self {
        Self {
            name: pick_at_half(&self.name, &other.name, factor),
            sun_rotation: slerp_rotation(&self.sun_rotation, &other.sun_rotation, factor),
            moon_rotation: slerp_rotation(&self.moon_rotation, &other.moon_rotation, factor),
            sunlight_color: lerp_color_alpha(self.sunlight_color, other.sunlight_color, factor),
            ambient: lerp_color(self.ambient, other.ambient, factor),
            blue_horizon: lerp_color(self.blue_horizon, other.blue_horizon, factor),
            blue_density: lerp_color(self.blue_density, other.blue_density, factor),
            haze_horizon: lerp_f32(self.haze_horizon, other.haze_horizon, factor),
            haze_density: lerp_f32(self.haze_density, other.haze_density, factor),
            density_multiplier: lerp_f32(self.density_multiplier, other.density_multiplier, factor),
            distance_multiplier: lerp_f32(
                self.distance_multiplier,
                other.distance_multiplier,
                factor,
            ),
            max_y: lerp_f32(self.max_y, other.max_y, factor),
            gamma: lerp_f32(self.gamma, other.gamma, factor),
            cloud_color: lerp_color(self.cloud_color, other.cloud_color, factor),
            cloud_pos_density1: lerp_cloud_pos_density(
                self.cloud_pos_density1,
                other.cloud_pos_density1,
                factor,
            ),
            cloud_pos_density2: lerp_cloud_pos_density(
                self.cloud_pos_density2,
                other.cloud_pos_density2,
                factor,
            ),
            cloud_scale: lerp_f32(self.cloud_scale, other.cloud_scale, factor),
            cloud_scroll_rate: lerp_array2(self.cloud_scroll_rate, other.cloud_scroll_rate, factor),
            cloud_shadow: lerp_f32(self.cloud_shadow, other.cloud_shadow, factor),
            cloud_variance: lerp_f32(self.cloud_variance, other.cloud_variance, factor),
            glow: lerp_glow(self.glow, other.glow, factor),
            star_brightness: lerp_f32(self.star_brightness, other.star_brightness, factor),
            sun_scale: lerp_f32(self.sun_scale, other.sun_scale, factor),
            moon_scale: lerp_f32(self.moon_scale, other.moon_scale, factor),
            moon_brightness: lerp_f32(self.moon_brightness, other.moon_brightness, factor),
            sun_arc_radians: lerp_f32(self.sun_arc_radians, other.sun_arc_radians, factor),
            droplet_radius: lerp_f32(self.droplet_radius, other.droplet_radius, factor),
            ice_level: lerp_f32(self.ice_level, other.ice_level, factor),
            moisture_level: lerp_f32(self.moisture_level, other.moisture_level, factor),
            sky_top_radius: lerp_f32(self.sky_top_radius, other.sky_top_radius, factor),
            sky_bottom_radius: lerp_f32(self.sky_bottom_radius, other.sky_bottom_radius, factor),
            planet_radius: lerp_f32(self.planet_radius, other.planet_radius, factor),
            sun_texture: pick_at_half(&self.sun_texture, &other.sun_texture, factor),
            moon_texture: pick_at_half(&self.moon_texture, &other.moon_texture, factor),
            cloud_texture: pick_at_half(&self.cloud_texture, &other.cloud_texture, factor),
            bloom_texture: pick_at_half(&self.bloom_texture, &other.bloom_texture, factor),
            halo_texture: pick_at_half(&self.halo_texture, &other.halo_texture, factor),
            rainbow_texture: pick_at_half(&self.rainbow_texture, &other.rainbow_texture, factor),
        }
    }

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
    fn sky_track_selection_maps_altitude_bands_to_the_reference_track_numbers() {
        let mut env = EnvironmentSettings::legacy_windlight_default();
        env.track_altitudes = [1000.0, 2000.0, 3000.0];
        // Four tracks so every band is distinct (the default cycle has one).
        let frame = |name: &str| super::DayCycleFrame {
            keyframe: 0.0,
            name: name.to_owned(),
        };
        env.day_cycle.sky_tracks = vec![
            vec![frame("ground")],
            vec![frame("mid")],
            vec![frame("high")],
            vec![frame("space")],
        ];
        assert_eq!(env.sky_track_for_altitude(0.0), 0);
        assert_eq!(env.sky_track_for_altitude(1000.0), 0);
        assert_eq!(env.sky_track_for_altitude(1000.1), 1);
        assert_eq!(env.sky_track_for_altitude(2000.0), 1);
        assert_eq!(env.sky_track_for_altitude(2500.0), 2);
        assert_eq!(env.sky_track_for_altitude(3000.0), 2);
        assert_eq!(env.sky_track_for_altitude(9000.0), 3);
    }

    #[test]
    fn sky_track_selection_clamps_to_available_tracks() {
        // The default cycle carries only the surface track, so every altitude
        // must resolve to it rather than an out-of-range index.
        let env = EnvironmentSettings::legacy_windlight_default();
        assert_eq!(env.day_cycle.sky_tracks.len(), 1);
        assert_eq!(env.sky_track_for_altitude(0.0), 0);
        assert_eq!(env.sky_track_for_altitude(50_000.0), 0);
        // And the surface frame resolves to a defined sky frame at any day time.
        assert!(env.active_sky_settings(50_000.0, 0.0).is_some());
        assert!(env.active_sky_settings(50_000.0, 0.5).is_some());
    }

    #[test]
    fn active_keyframe_picks_the_frame_in_force_and_wraps_before_the_first() {
        use super::{DayCycle, active_keyframe};
        let track = vec![
            super::DayCycleFrame {
                keyframe: 0.25,
                name: "morning".to_owned(),
            },
            super::DayCycleFrame {
                keyframe: 0.75,
                name: "evening".to_owned(),
            },
        ];
        // At/after a keyframe, that frame is in force.
        assert_eq!(
            active_keyframe(&track, 0.25).map(|f| f.name.as_str()),
            Some("morning")
        );
        assert_eq!(
            active_keyframe(&track, 0.5).map(|f| f.name.as_str()),
            Some("morning")
        );
        assert_eq!(
            active_keyframe(&track, 0.9).map(|f| f.name.as_str()),
            Some("evening")
        );
        // Before the first keyframe the cycle wraps to the last frame.
        assert_eq!(
            active_keyframe(&track, 0.1).map(|f| f.name.as_str()),
            Some("evening")
        );
        // An empty track has no active frame.
        let empty = DayCycle {
            name: String::new(),
            water_track: Vec::new(),
            sky_tracks: Vec::new(),
            sky_frames: std::collections::BTreeMap::new(),
            water_frames: std::collections::BTreeMap::new(),
        };
        assert!(active_keyframe(&empty.water_track, 0.5).is_none());
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

    #[test]
    fn bounding_keyframes_brackets_the_position_and_wraps_across_the_day() {
        use super::{DayCycleFrame, bounding_keyframes};
        let track = vec![
            DayCycleFrame {
                keyframe: 0.25,
                name: "morning".to_owned(),
            },
            DayCycleFrame {
                keyframe: 0.75,
                name: "evening".to_owned(),
            },
        ];
        // The bounds' names plus the blend factor, for a lint-clean assertion
        // (no `expect`, and the factor is compared approximately).
        let bracket = |position: f32| {
            bounding_keyframes(&track, position)
                .map(|(lower, upper, factor)| (lower.name.clone(), upper.name.clone(), factor))
        };
        // Mid-morning: bracketed by morning→evening, half-way between them.
        assert!(bracket(0.5).is_some_and(|(lower, upper, factor)| {
            lower == "morning" && upper == "evening" && (factor - 0.5).abs() < 1.0e-6
        }));
        // After the last keyframe the upper wraps to the first (next day):
        // span 0.75→1.25, position 0.9 → (0.9 - 0.75) / 0.5 = 0.3.
        assert!(bracket(0.9).is_some_and(|(lower, upper, factor)| {
            lower == "evening" && upper == "morning" && (factor - 0.3).abs() < 1.0e-6
        }));
        // Before the first keyframe the lower wraps to the last (previous day):
        // span -0.25→0.25, position 0.1 → (0.1 + 0.25) / 0.5 = 0.7.
        assert!(bracket(0.1).is_some_and(|(lower, upper, factor)| {
            lower == "evening" && upper == "morning" && (factor - 0.7).abs() < 1.0e-6
        }));
    }

    #[test]
    fn bounding_keyframes_of_a_single_frame_track_returns_it_as_both_bounds() {
        use super::{DayCycleFrame, bounding_keyframes};
        let track = vec![DayCycleFrame {
            keyframe: 0.4,
            name: "only".to_owned(),
        }];
        assert!(
            bounding_keyframes(&track, 0.9).is_some_and(|(lower, upper, factor)| {
                lower.name == "only"
                    && upper.name == "only"
                    && factor.to_bits() == 0.0_f32.to_bits()
            })
        );
        // An empty track has no bounds.
        assert!(bounding_keyframes(&[], 0.5).is_none());
    }

    #[test]
    fn sky_blend_interpolates_scalars_and_snaps_at_the_endpoints() {
        let mut a = SkySettings::legacy_windlight_default("A");
        let mut b = SkySettings::legacy_windlight_default("B");
        a.cloud_shadow = 0.2;
        b.cloud_shadow = 0.8;
        a.gamma = 1.0;
        b.gamma = 2.0;
        // Endpoints reproduce the source frames exactly.
        let at_zero = a.blend(&b, 0.0);
        assert_eq!(at_zero.cloud_shadow.to_bits(), 0.2_f32.to_bits());
        assert_eq!(at_zero.gamma.to_bits(), 1.0_f32.to_bits());
        let at_one = a.blend(&b, 1.0);
        assert!((at_one.cloud_shadow - 0.8).abs() < 1.0e-6);
        assert!((at_one.gamma - 2.0).abs() < 1.0e-6);
        // Midpoint is the arithmetic mean of each scalar.
        let mid = a.blend(&b, 0.5);
        assert!((mid.cloud_shadow - 0.5).abs() < 1.0e-6);
        assert!((mid.gamma - 1.5).abs() < 1.0e-6);
    }

    #[test]
    fn sky_blend_slerps_rotations_to_a_unit_quaternion() {
        let a = SkySettings::legacy_windlight_default("A");
        let b = SkySettings::legacy_windlight_default("B");
        // Give the two frames genuinely different sun orientations to slerp.
        let mut b = b;
        b.sun_rotation = a.moon_rotation.clone();
        let mid = a.blend(&b, 0.5);
        let r = &mid.sun_rotation;
        let length_squared = r.x * r.x + r.y * r.y + r.z * r.z + r.s * r.s;
        assert!((length_squared - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn sky_blend_snaps_textures_and_name_at_the_halfway_point() {
        use sl_types::key::TextureKey;
        use uuid::Uuid;
        let mut a = SkySettings::legacy_windlight_default("A");
        let mut b = SkySettings::legacy_windlight_default("B");
        a.sun_texture = Some(TextureKey::from(Uuid::from_u128(1)));
        b.sun_texture = Some(TextureKey::from(Uuid::from_u128(2)));
        // Below halfway the lower frame's discrete settings win; at/above, the upper.
        assert_eq!(a.blend(&b, 0.25).sun_texture, a.sun_texture);
        assert_eq!(a.blend(&b, 0.25).name, "A");
        assert_eq!(a.blend(&b, 0.75).sun_texture, b.sun_texture);
        assert_eq!(a.blend(&b, 0.75).name, "B");
    }

    #[test]
    fn blended_sky_settings_interpolates_between_the_bounding_keyframes() {
        use super::{DayCycle, DayCycleFrame};
        use std::collections::BTreeMap;
        let mut dawn = SkySettings::legacy_windlight_default("dawn");
        let mut dusk = SkySettings::legacy_windlight_default("dusk");
        dawn.cloud_shadow = 0.0;
        dusk.cloud_shadow = 1.0;
        let mut sky_frames = BTreeMap::new();
        drop(sky_frames.insert("dawn".to_owned(), dawn));
        drop(sky_frames.insert("dusk".to_owned(), dusk));
        let track = vec![
            DayCycleFrame {
                keyframe: 0.0,
                name: "dawn".to_owned(),
            },
            DayCycleFrame {
                keyframe: 0.5,
                name: "dusk".to_owned(),
            },
        ];
        let mut env = EnvironmentSettings::legacy_windlight_default();
        env.day_cycle = DayCycle {
            name: "test".to_owned(),
            water_track: Vec::new(),
            sky_tracks: vec![track],
            sky_frames,
            water_frames: BTreeMap::new(),
        };
        // A quarter of the way from dawn (0.0) to dusk (0.5): factor 0.5.
        assert!(
            env.blended_sky_settings(0.0, 0.25)
                .is_some_and(|quarter| (quarter.cloud_shadow - 0.5).abs() < 1.0e-6)
        );
        // Exactly on the dawn keyframe: the dawn frame, unblended.
        assert!(
            env.blended_sky_settings(0.0, 0.0)
                .is_some_and(|at_dawn| at_dawn.cloud_shadow.abs() < 1.0e-6)
        );
    }

    #[test]
    fn blended_sky_settings_of_the_default_cycle_returns_its_single_frame() {
        // The built-in default cycle has one keyframe, so every day position and
        // altitude blends the same frame with itself — its values unchanged.
        let env = EnvironmentSettings::legacy_windlight_default();
        let reference = SkySettings::legacy_windlight_default("Default");
        assert!(env.blended_sky_settings(0.0, 0.5).is_some_and(|noon| {
            noon.cloud_shadow.to_bits() == reference.cloud_shadow.to_bits()
                && noon.gamma.to_bits() == reference.gamma.to_bits()
                && noon.name == reference.name
        }));
    }
}
