//! Linden's four canonical WindLight sky presets (`A-6AM` / `A-12PM` / `A-6PM`
//! / `A-12AM`), ported as constants, plus the legacy → EEP conversion that
//! turns one into a renderable [`SkySettings`].
//!
//! Shared by two consumers: the offline sky render scenes
//! (`render_scene`, where the port originated) and the **World ▸
//! Environment** menu (`menu_bar`), whose Sunrise / Midday / Sunset /
//! Midnight entries pin the viewer's environment to one of these frames — the
//! reference viewer's fixed-sky personal lighting
//! (`LLEnvironment::setEnvironment(ENV_LOCAL, …)` on the same four presets).

use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI};

use sl_client_bevy::{
    Color as SlColor, ColorAlpha, DayCycleFrame, EnvironmentSettings, Glow, SkySettings, Uuid,
    azimuth_altitude_to_rotation,
};

/// One of the four fixed times of day the World ▸ Environment menu offers —
/// the reference viewer's Sunrise / Midday / Sunset / Midnight presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedSky {
    /// Linden's `A-6AM` (Sunrise).
    Sunrise,
    /// Linden's `A-12PM` (Midday).
    Midday,
    /// Linden's `A-6PM` (Sunset).
    Sunset,
    /// Linden's `A-12AM` (Midnight).
    Midnight,
}

impl FixedSky {
    /// The ported preset behind this fixed sky.
    const fn preset(self) -> &'static SkyPreset {
        match self {
            Self::Sunrise => &SUNRISE,
            Self::Midday => &MIDDAY,
            Self::Sunset => &SUNSET,
            Self::Midnight => &MIDNIGHT,
        }
    }

    /// This fixed sky as a renderable frame.
    #[must_use]
    pub fn settings(self) -> SkySettings {
        sky_settings_from(self.preset())
    }

    /// The frame name the fixed day cycle files this sky under.
    #[must_use]
    pub const fn frame_name(self) -> &'static str {
        self.preset().label
    }

    /// The normalised day-cycle position (`0.0..=1.0`) at which this time freezes
    /// the region's *own* EEP day cycle — the reference's canonical placement
    /// (`0.25` sunrise, `0.5` midday, `0.75` sunset, `0.0` midnight). Backs the
    /// World ▸ Environment **Day Cycle** presets.
    #[must_use]
    pub const fn day_position(self) -> f32 {
        match self {
            Self::Sunrise => 0.25,
            Self::Midday => 0.5,
            Self::Sunset => 0.75,
            Self::Midnight => 0.0,
        }
    }

    /// The reference viewer's `KNOWN_SKY_*` library sky asset for this time — the
    /// exact modern EEP sky Firestorm's World ▸ Environment preset loads
    /// (`LLEnvironment::KNOWN_SKY_{SUNRISE,MIDDAY,SUNSET,MIDNIGHT}`). Backs the
    /// **Modern** presets: fetching and rendering this asset gives byte-identical
    /// input to Firestorm, so a renderer comparison isolates the renderer.
    #[must_use]
    pub const fn modern_asset(self) -> Uuid {
        match self {
            Self::Sunrise => Uuid::from_u128(0x01e4_1537_ff51_2f1f_8ef7_17e4_df76_0bfb),
            Self::Midday => Uuid::from_u128(0xc462_26b4_0e43_5a56_9708_d27c_a1df_3292),
            Self::Sunset => Uuid::from_u128(0x084e_26cd_a900_28e8_08d0_64a9_de5c_15e2),
            Self::Midnight => Uuid::from_u128(0x8a01_b97a_cb20_c1ea_ac63_f7ea_84ad_0090),
        }
    }
}

/// Replace the sky schedule of `settings` with a synthetic day cycle built from
/// the four ported presets, keyframed `A-12AM` (Midnight) at `0.0`, `A-6AM`
/// (Sunrise) at `0.25`, `A-12PM` (Midday) at `0.5`, and `A-6PM` (Sunset) at
/// `0.75` — the natural placement of the reference's own presets across the day.
///
/// This backs the `SL_VIEWER_SKY_DAY_POSITION` debug override: a grid that
/// supplies only a single-frame environment (the local OpenSim default, whose
/// one midday frame leaves [`blended_sky_settings`](EnvironmentSettings::blended_sky_settings)
/// nothing to interpolate) would otherwise render the same noon sky at every
/// position. With this cycle installed, the override's position drives the sun
/// smoothly through sunrise → midday → sunset → midnight on any grid. The water
/// schedule is left untouched (the override only moves the sky).
pub fn install_preset_day_cycle(settings: &mut EnvironmentSettings) {
    // Placed one keyframe per quarter-day so the position reads intuitively:
    // 0.25 sunrise, 0.5 noon, 0.75 sunset, 0.0/1.0 midnight.
    let placed: [(f32, &SkyPreset); 4] = [
        (0.0, &MIDNIGHT),
        (0.25, &SUNRISE),
        (0.5, &MIDDAY),
        (0.75, &SUNSET),
    ];
    let mut track = Vec::with_capacity(placed.len());
    let mut sky_frames = BTreeMap::new();
    for (keyframe, preset) in placed {
        let name = preset.label.to_owned();
        track.push(DayCycleFrame {
            keyframe,
            name: name.clone(),
        });
        sky_frames.insert(name, sky_settings_from(preset));
    }
    settings.day_cycle.sky_tracks = vec![track];
    settings.day_cycle.sky_frames = sky_frames;
}

/// One of Linden's four canonical WindLight sky presets, ported.
///
/// **These are content, and that is the entire point.** The first version of these
/// scenes moved the sun across one palette — the legacy WindLight default — and
/// produced a midnight nearly as bright as midday, which was filed as a viewer bug
/// ([[viewer-r27]]) and was not one. Second Life's night is dark because the
/// **midnight sky frame's `sunlight_color` is authored dark**: `A-12AM`'s is
/// `(0.35, 0.36, 0.66)` against `A-12PM`'s `(0.73, 0.78, 0.90)`, and the reference's
/// scene light is that colour attenuated by elevation. Nothing computes a night.
///
/// So a sky scene that does not carry a palette per time of day is not a sky scene
/// at all; it is one sky with the sun in the wrong place, which is an environment
/// that cannot exist in-world. The legacy WindLight default is a **single midday
/// frame** (the reference's `LLSettingsSky::defaults()` is too) — it has no night
/// in it to find.
///
/// The values are Linden's own, from the presets Firestorm ships in
/// `app_settings/windlight/skies/`, converted by the reference's own rules
/// (`LLSettingsSky::translateLegacySettings`): scalars are the `[0]` of their legacy
/// array, `star_brightness` is scaled by 250, and the bodies come from `sun_angle` /
/// `east_angle` — see [`sky_settings_from`]. Ported as constants rather than read
/// from disk because a scene that needs an asset is a scene that skips.
#[derive(Debug, Clone, Copy)]
pub struct SkyPreset {
    /// How this time names its scene and its entities.
    pub label: &'static str,
    /// The legacy `sunlight_color` — the one field that makes a night a night.
    sunlight: [f32; 3],
    /// The legacy `ambient`.
    ambient: [f32; 3],
    /// The legacy `blue_horizon`.
    blue_horizon: [f32; 3],
    /// The legacy `blue_density`.
    blue_density: [f32; 3],
    /// The legacy `cloud_color`.
    cloud_color: [f32; 3],
    /// The legacy `haze_horizon`.
    haze_horizon: f32,
    /// The legacy `haze_density`.
    haze_density: f32,
    /// The legacy `density_multiplier`.
    density_multiplier: f32,
    /// The legacy `distance_multiplier`.
    distance_multiplier: f32,
    /// The legacy `max_y`.
    max_y: f32,
    /// The legacy `gamma`.
    gamma: f32,
    /// The legacy `cloud_shadow`.
    cloud_shadow: f32,
    /// The legacy `cloud_scale`.
    cloud_scale: f32,
    /// The legacy `glow`.
    glow: [f32; 3],
    /// The legacy `star_brightness`, **before** the reference's 250x conversion.
    star_brightness: f32,
    /// The legacy `sun_angle`, in radians — the sun's altitude.
    sun_angle: f32,
    /// The legacy `east_angle`, in radians. Negated to an azimuth.
    east_angle: f32,
}

/// Linden's `A-6AM` preset, ported from `app_settings/windlight/skies/A-6AM.xml`.
pub const SUNRISE: SkyPreset = SkyPreset {
    label: "sky-sunrise",
    sunlight: [2.37, 2.37, 2.37],
    ambient: [0.81, 0.4629, 0.63],
    blue_horizon: [0.2067, 0.4099, 0.48],
    blue_density: [0.1579, 0.435, 0.87],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.16,
    haze_density: 0.54,
    density_multiplier: 0.000_620,
    distance_multiplier: 2.6999,
    max_y: 563.0,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.001, 0.001, -0.48],
    star_brightness: 0.0,
    sun_angle: 0.0942,
    east_angle: 0.0,
};

/// Linden's `A-12PM` preset, ported from `app_settings/windlight/skies/A-12PM.xml`.
pub const MIDDAY: SkyPreset = SkyPreset {
    label: "sky-midday",
    sunlight: [0.7342, 0.7816, 0.9],
    ambient: [1.05, 1.05, 1.05],
    blue_horizon: [0.4955, 0.4955, 0.64],
    blue_density: [0.2448, 0.4487, 0.76],
    cloud_color: [0.41, 0.41, 0.41],
    haze_horizon: 0.19,
    haze_density: 0.7,
    density_multiplier: 0.000_180,
    distance_multiplier: 0.8,
    max_y: 1605.0,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 0.0,
    // The preset's literal 1.5708 is pi/2 — the sun at the zenith.
    sun_angle: FRAC_PI_2,
    east_angle: 0.0,
};

/// Linden's `A-6PM` preset, ported from `app_settings/windlight/skies/A-6PM.xml`.
pub const SUNSET: SkyPreset = SkyPreset {
    label: "sky-sunset",
    sunlight: [2.8386, 2.8386, 2.8386],
    ambient: [1.02, 0.81, 0.81],
    blue_horizon: [0.1077, 0.2135, 0.25],
    blue_density: [0.1452, 0.4, 0.8],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.16,
    haze_density: 0.7,
    density_multiplier: 0.000_460,
    distance_multiplier: 1.0,
    max_y: 562.5,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 0.0,
    sun_angle: 3.0662,
    east_angle: 0.0,
};

/// Linden's `A-12AM` preset, ported from `app_settings/windlight/skies/A-12AM.xml`.
pub const MIDNIGHT: SkyPreset = SkyPreset {
    label: "sky-midnight",
    sunlight: [0.3488, 0.3557, 0.66],
    ambient: [0.2041, 0.2425, 0.33],
    blue_horizon: [0.24, 0.24, 0.24],
    blue_density: [0.45, 0.45, 0.45],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.0,
    haze_density: 4.0,
    density_multiplier: 0.000_300,
    distance_multiplier: 0.0,
    max_y: 906.2,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 2.0,
    sun_angle: 4.7124,
    east_angle: 0.0,
};

/// Build a [`SkySettings`] from a ported preset, by the reference's own legacy →
/// EEP conversion (`LLSettingsSky::translateLegacySettings`).
///
/// The two rules worth stating, because both are easy to get subtly wrong:
///
/// - **The bodies come from the angles.** `azimuth = -east_angle` ("get
///   counter-clockwise radian angle from clockwise legacy WL east angle") and
///   `altitude = sun_angle`; the moon is **diametrically opposed**, at
///   `(azimuth + PI, -altitude)`. That is what makes `A-12AM` a night with no
///   special casing: its `sun_angle` of 4.7124 rad (270°) puts the sun straight
///   down, so the moon is straight up and *it* is the light — and since the
///   reference shares one colour between the two bodies, the moon's light is the
///   frame's own dark blue.
/// - **`star_brightness` is scaled by 250.** `A-12AM`'s legacy `2.0` becomes `500`,
///   which the star shader's `star_brightness / 500` turns into a fully visible
///   field; `A-12PM`'s `0.0` hides it. So the stars come and go with the time of day
///   for free, from the data, rather than from a flag in the fixture.
#[must_use]
pub fn sky_settings_from(preset: &SkyPreset) -> SkySettings {
    let azimuth = -preset.east_angle;
    let altitude = preset.sun_angle;
    let [sun_r, sun_g, sun_b] = preset.sunlight;
    let [amb_r, amb_g, amb_b] = preset.ambient;
    let [bh_r, bh_g, bh_b] = preset.blue_horizon;
    let [bd_r, bd_g, bd_b] = preset.blue_density;
    let [cc_r, cc_g, cc_b] = preset.cloud_color;
    let [glow_x, glow_y, glow_z] = preset.glow;
    SkySettings {
        sun_rotation: azimuth_altitude_to_rotation(azimuth, altitude),
        moon_rotation: azimuth_altitude_to_rotation(azimuth + PI, -altitude),
        // The alpha is unused by the shader (`sky_params` reads rgb), and the
        // reference's own EEP defaults carry a zero there.
        sunlight_color: ColorAlpha::new(sun_r, sun_g, sun_b, 0.0),
        ambient: SlColor::new(amb_r, amb_g, amb_b),
        blue_horizon: SlColor::new(bh_r, bh_g, bh_b),
        blue_density: SlColor::new(bd_r, bd_g, bd_b),
        cloud_color: SlColor::new(cc_r, cc_g, cc_b),
        haze_horizon: preset.haze_horizon,
        haze_density: preset.haze_density,
        density_multiplier: preset.density_multiplier,
        distance_multiplier: preset.distance_multiplier,
        max_y: preset.max_y,
        gamma: preset.gamma,
        cloud_shadow: preset.cloud_shadow,
        cloud_scale: preset.cloud_scale,
        glow: Glow::new(glow_x, glow_y, glow_z),
        star_brightness: preset.star_brightness * 250.0,
        ..SkySettings::legacy_windlight_default(preset.label)
    }
}
