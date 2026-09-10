//! Linden's four canonical WindLight sky presets (`A-6AM` / `A-12PM` / `A-6PM`
//! / `A-12AM`), ported as constants, plus the legacy → EEP conversion that turns
//! one into a renderable [`SkySettings`] and the day cycle that schedules all
//! four across a day.
//!
//! These live beside [`SkySettings::legacy_windlight_default`] rather than in a
//! viewer crate because **both halves of the protocol need them**: a client
//! renders them (the World ▸ Environment menu's Sunrise / Midday / Sunset /
//! Midnight entries, and the day-position debug override), and a *server* has to
//! be able to serve a day cycle that actually holds a day in it — a region
//! offering one keyframe gives every client nothing to interpolate, so no viewer
//! can honour a request for a particular time of day against it.

use core::f32::consts::{FRAC_PI_2, PI};
use std::collections::BTreeMap;

use super::environment::{
    Color as SlColor, ColorAlpha, DayCycle, DayCycleFrame, EnvironmentSettings, Glow, SkySettings,
    azimuth_altitude_to_rotation,
};

/// One of Linden's four canonical WindLight sky presets, ported.
///
/// **These are content, and that is the entire point.** The first version of the
/// offline sky scenes moved the sun across one palette — the legacy WindLight
/// default — and produced a midnight nearly as bright as midday, which was filed
/// as a viewer bug and was not one. Second Life's night is dark because the
/// **midnight sky frame's `sunlight_color` is authored dark**: `A-12AM`'s is
/// `(0.35, 0.36, 0.66)` against `A-12PM`'s `(0.73, 0.78, 0.90)`, and the
/// reference's scene light is that colour attenuated by elevation. Nothing
/// computes a night.
///
/// So a sky schedule that does not carry a palette per time of day is not a day
/// cycle at all; it is one sky with the sun in the wrong place, which is an
/// environment that cannot exist in-world. The legacy WindLight default is a
/// **single midday frame** (the reference's `LLSettingsSky::defaults()` is too)
/// — it has no night in it to find.
///
/// The values are Linden's own, from the presets Firestorm ships in
/// `app_settings/windlight/skies/`, converted by the reference's own rules
/// (`LLSettingsSky::translateLegacySettings`): scalars are the `[0]` of their
/// legacy array, `star_brightness` is scaled by 250, and the bodies come from
/// `sun_angle` / `east_angle` — see [`sky_settings_from`]. Ported as constants
/// rather than read from disk because a scene that needs an asset is a scene
/// that skips.
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

/// The four presets and the keyframes they sit at across a day: `A-12AM`
/// (Midnight) at `0.0`, `A-6AM` (Sunrise) at `0.25`, `A-12PM` (Midday) at `0.5`
/// and `A-6PM` (Sunset) at `0.75`.
///
/// A **quarter-day subset** of the reference's own legacy default day cycle
/// (`app_settings/windlight/days/Default.xml`), which names eight frames at
/// eighth-day steps: the four in between (`A-3AM`, `A-9AM`, `A-3PM`, `A-9PM`)
/// are not ported, so this cycle passes through the same four corners on a
/// straight line rather than on Linden's curve. That is a difference in the
/// *shape* of the sweep, not in where the sun stands at each quarter — and both
/// halves of a cross-check read the same schedule, which is what the comparison
/// rests on.
pub const PRESET_DAY_KEYFRAMES: [(f32, &SkyPreset); 4] = [
    (0.0, &MIDNIGHT),
    (0.25, &SUNRISE),
    (0.5, &MIDDAY),
    (0.75, &SUNSET),
];

/// The name [`preset_sky_schedule`] gives the cycle it builds.
pub const PRESET_DAY_CYCLE_NAME: &str = "Legacy WindLight Presets";

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
/// - **`star_brightness` is scaled by 250.** `A-12AM`'s legacy `2.0` becomes
///   `500`, which the star shader's `star_brightness / 500` turns into a fully
///   visible field; `A-12PM`'s `0.0` hides it. So the stars come and go with the
///   time of day for free, from the data, rather than from a flag in the fixture.
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

/// The sky half of a day cycle built from [`PRESET_DAY_KEYFRAMES`]: the surface
/// track and the four frames it names.
///
/// Returned as a pair rather than as a whole [`DayCycle`] because the water
/// schedule is a separate decision — a caller replacing a region's sky keeps
/// whatever water that region already serves.
#[must_use]
pub fn preset_sky_schedule() -> (Vec<DayCycleFrame>, BTreeMap<String, SkySettings>) {
    let mut track = Vec::with_capacity(PRESET_DAY_KEYFRAMES.len());
    let mut frames = BTreeMap::new();
    for (keyframe, preset) in PRESET_DAY_KEYFRAMES {
        let name = preset.label.to_owned();
        track.push(DayCycleFrame {
            keyframe,
            name: name.clone(),
        });
        frames.insert(name, sky_settings_from(preset));
    }
    (track, frames)
}

/// Replace the sky schedule of `settings` with the four-preset day cycle, and
/// name the cycle [`PRESET_DAY_CYCLE_NAME`]. The water schedule is left
/// untouched: this moves the sun, nothing else.
///
/// Two callers, for the same reason from opposite ends. A **grid** dresses a
/// region with this so that asking for a time of day means something there at
/// all; a **client** installs it over a region whose cycle has nothing to
/// interpolate ([`EnvironmentSettings::day_position_moves_the_sky`] is false),
/// so a debug run against such a grid can still see a sunrise. The second is a
/// substitution — the sky rendered is no longer the sky the region sent — so a
/// harness that does it has to say so; see the cross-check runner, which avoids
/// the question by dressing the region instead.
pub fn install_preset_day_cycle(settings: &mut EnvironmentSettings) {
    let (track, frames) = preset_sky_schedule();
    PRESET_DAY_CYCLE_NAME.clone_into(&mut settings.day_cycle.name);
    settings.day_cycle.sky_tracks = vec![track];
    settings.day_cycle.sky_frames = frames;
}

/// A whole [`DayCycle`] carrying the four presets on its surface sky track and
/// `water` as its single water frame, named `water_name`.
///
/// The water frame is passed in rather than defaulted because sky and water
/// frames share one name namespace on the wire (see [`DayCycle`]): only the
/// caller knows what the rest of its environment calls its water.
#[must_use]
pub fn preset_day_cycle(water_name: &str, water: super::environment::WaterSettings) -> DayCycle {
    let (sky_track, sky_frames) = preset_sky_schedule();
    DayCycle {
        name: PRESET_DAY_CYCLE_NAME.to_owned(),
        water_track: vec![DayCycleFrame {
            keyframe: 0.0,
            name: water_name.to_owned(),
        }],
        sky_tracks: vec![sky_track],
        sky_frames,
        water_frames: core::iter::once((water_name.to_owned(), water)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        MIDDAY, MIDNIGHT, PRESET_DAY_CYCLE_NAME, SUNRISE, SUNSET, install_preset_day_cycle,
        preset_sky_schedule, sky_settings_from,
    };
    use crate::types::environment::{DEFAULT_WATER_FRAME, EnvironmentSettings, WaterSettings};

    /// Midnight is dark and midday is not, and the difference is in the data:
    /// the whole reason these are four ported palettes rather than one palette
    /// with the sun moved.
    #[test]
    fn the_night_preset_is_authored_dark() {
        let night = sky_settings_from(&MIDNIGHT);
        let day = sky_settings_from(&MIDDAY);
        assert!(
            night.sunlight_color.red() < day.sunlight_color.red(),
            "A-12AM's sunlight is authored darker than A-12PM's"
        );
        // Legacy 2.0 * 250: the star field is fully visible at midnight and
        // hidden at noon, from the data alone. (Compared with a tolerance
        // because these are `f32`; both values are exactly representable, but
        // asserting that is asserting something about the multiplication rather
        // than about the presets.)
        assert!((night.star_brightness - 500.0).abs() < f32::EPSILON);
        assert!(day.star_brightness.abs() < f32::EPSILON);
    }

    /// Every preset carries the three atmospheric density profiles the reference
    /// marks required-with-no-default, because it is built on top of the legacy
    /// WindLight default which carries them. A preset cycle that lost them would
    /// be rejected wholesale by the reference viewer, sky and water and all.
    #[test]
    fn every_preset_carries_the_required_density_profiles() {
        for preset in [&SUNRISE, &MIDDAY, &SUNSET, &MIDNIGHT] {
            let sky = sky_settings_from(preset);
            assert!(
                !sky.rayleigh_config.is_empty(),
                "{} has a rayleigh profile",
                preset.label
            );
            assert!(
                !sky.mie_config.is_empty(),
                "{} has a mie profile",
                preset.label
            );
            assert!(
                !sky.absorption_config.is_empty(),
                "{} has an absorption profile",
                preset.label
            );
        }
    }

    /// The schedule names four distinct frames at the four quarters, and every
    /// name it schedules resolves — a track naming a frame the cycle does not
    /// define is a cycle that renders a fallback and says nothing about it.
    #[test]
    fn the_schedule_resolves_every_keyframe_it_names() {
        let (track, frames) = preset_sky_schedule();
        assert_eq!(track.len(), 4);
        let positions: Vec<f32> = track.iter().map(|frame| frame.keyframe).collect();
        assert_eq!(positions, [0.0, 0.25, 0.5, 0.75]);
        for frame in &track {
            assert!(
                frames.contains_key(&frame.name),
                "the cycle defines {}",
                frame.name
            );
        }
    }

    /// Installing the cycle replaces the sky schedule and leaves the water
    /// alone: this moves the sun, and a region's water is not the sun's
    /// business.
    #[test]
    fn installing_the_cycle_keeps_the_water() {
        let mut settings = EnvironmentSettings::default_region();
        let water_before = settings.day_cycle.water_frames.clone();
        assert!(!settings.day_position_moves_the_sky(0.0));
        install_preset_day_cycle(&mut settings);
        assert!(settings.day_position_moves_the_sky(0.0));
        assert_eq!(settings.day_cycle.name, PRESET_DAY_CYCLE_NAME);
        assert_eq!(settings.day_cycle.water_frames, water_before);
        assert!(
            settings
                .day_cycle
                .water_frames
                .contains_key(DEFAULT_WATER_FRAME)
        );
    }

    /// A whole cycle built for a grid to serve carries both halves, and the two
    /// halves are named apart: they share one map on the wire.
    #[test]
    fn a_served_cycle_names_its_water_apart_from_its_skies() {
        let water = WaterSettings::legacy_default(DEFAULT_WATER_FRAME);
        let cycle = super::preset_day_cycle(DEFAULT_WATER_FRAME, water);
        assert_eq!(cycle.water_track.len(), 1);
        assert_eq!(cycle.sky_tracks.len(), 1);
        for sky_name in cycle.sky_frames.keys() {
            assert!(
                !cycle.water_frames.contains_key(sky_name),
                "{sky_name} collides with a water frame"
            );
        }
    }
}
