//! Procedural **EEP settings assets** (`AT_SETTINGS`): a night sky, a noon sky,
//! a water frame, and a day cycle that runs from one sky to the other.
//!
//! What the marker colours are to the pixel oracles and the fixture tones are to
//! the audio ones, these are to an *environment* oracle: the only thing a
//! capture can say about a sky is how bright it came out, so the two fixture
//! skies sit at the ends of that scale rather than in the middle. Linden's own
//! four presets are ported in `sl_viewer_kit::sky_presets` and are the right
//! frames for anything that wants to look like Second Life; these are the ones
//! to reach for when a test needs "the environment changed, and here is the
//! direction it changed in".
//!
//! They are *assets*, not typed values pushed over `ExtEnvironment`: the bytes
//! are what a grid serves for a settings asset id, so an inventory item, an
//! offered day cycle, or a parcel pointing at an asset can all be fixtured —
//! none of which the typed capability path can express.
#![expect(
    clippy::module_name_repetitions,
    reason = "the module is the subject: `environment::noon_environment()` is how a \
              caller reads it, and dropping the suffix would leave `environment::noon`"
)]

use sl_proto::{
    Color, ColorAlpha, DayCycle, DayCycleFrame, EnvironmentAsset, EnvironmentSettings, SkySettings,
    WaterSettings, azimuth_altitude_to_rotation, environment_asset_to_bytes,
};

/// The name (and frame name) of the bright fixture sky.
pub const NOON_SKY_NAME: &str = "fixture-noon";

/// The name (and frame name) of the dark fixture sky.
pub const NIGHT_SKY_NAME: &str = "fixture-night";

/// The name (and frame name) of the fixture water frame.
pub const WATER_NAME: &str = "fixture-water";

/// The name of the fixture day cycle.
pub const DAY_CYCLE_NAME: &str = "fixture-day-cycle";

/// The day-cycle position [`night_sky`] is keyframed at — the start of the day,
/// which is where the reference places midnight.
pub const NIGHT_KEYFRAME: f32 = 0.0;

/// The day-cycle position [`noon_sky`] is keyframed at — half a day later, so
/// the two are as far apart on the track as they are in brightness.
pub const NOON_KEYFRAME: f32 = 0.5;

/// A **bright** fixture sky: the legacy WindLight default with the sun straight
/// overhead and a near-white sunlight colour.
///
/// Everything not named here is the reference's own default
/// ([`SkySettings::legacy_windlight_default`]), so the fixture is a sky a grid
/// could really serve rather than a struct full of round numbers. What is named
/// is what a capture can see: the sun's position, the light's colour, and the
/// ambient the unlit faces take.
#[must_use]
pub fn noon_sky() -> SkySettings {
    SkySettings {
        // Straight up, so nothing in the scene is in its own shadow.
        sun_rotation: azimuth_altitude_to_rotation(0.0, core::f32::consts::FRAC_PI_2),
        // The moon opposite it, below the horizon.
        moon_rotation: azimuth_altitude_to_rotation(0.0, -core::f32::consts::FRAC_PI_2),
        sunlight_color: ColorAlpha::new(0.9, 0.9, 0.9, 0.0),
        ambient: Color::new(0.5, 0.5, 0.5),
        // No stars against a daylit sky.
        star_brightness: 0.0,
        ..SkySettings::legacy_windlight_default(NOON_SKY_NAME)
    }
}

/// A **dark** fixture sky: the same frame with the sun below the horizon, the
/// moon up, and the sunlight colour turned down to a tenth.
///
/// Night in Second Life is not computed — it is *authored*, in the midnight
/// frame's `sunlight_color` (see `sl_viewer_kit::sky_presets`, where getting
/// this wrong produced a midnight as bright as midday and a viewer bug that was
/// not one). So the one field that has to differ from [`noon_sky`] for a
/// luminance oracle to see anything is that colour, and it does — by 9x.
#[must_use]
pub fn night_sky() -> SkySettings {
    SkySettings {
        // Below the horizon by the same angle noon is above it.
        sun_rotation: azimuth_altitude_to_rotation(0.0, -core::f32::consts::FRAC_PI_2),
        moon_rotation: azimuth_altitude_to_rotation(0.0, core::f32::consts::FRAC_PI_2),
        sunlight_color: ColorAlpha::new(0.1, 0.1, 0.1, 0.0),
        ambient: Color::new(0.05, 0.05, 0.05),
        // The reference's own night star brightness (its presets carry `1.0`
        // before the 250x legacy conversion).
        star_brightness: 250.0,
        ..SkySettings::legacy_windlight_default(NIGHT_SKY_NAME)
    }
}

/// The fixture water frame: the reference's own default water, renamed.
///
/// Water is not what an environment oracle measures — the sky is — so this
/// exists to give the day cycle's water track something to name and to give the
/// water *asset* kind a fixture at all.
#[must_use]
pub fn water() -> WaterSettings {
    WaterSettings::legacy_default(WATER_NAME)
}

/// A day cycle that runs from [`night_sky`] at the start of the day to
/// [`noon_sky`] halfway through it, over one water track holding [`water`].
///
/// Two keyframes rather than one, because a single-frame cycle is what the
/// local OpenSim default serves and it leaves the frame interpolation nothing
/// to do: a test that moves the day position through this one gets a *measurably*
/// different sky at each end.
#[must_use]
pub fn day_cycle() -> DayCycle {
    DayCycle {
        name: DAY_CYCLE_NAME.to_owned(),
        water_track: vec![DayCycleFrame {
            keyframe: 0.0,
            name: WATER_NAME.to_owned(),
        }],
        sky_tracks: vec![vec![
            DayCycleFrame {
                keyframe: NIGHT_KEYFRAME,
                name: NIGHT_SKY_NAME.to_owned(),
            },
            DayCycleFrame {
                keyframe: NOON_KEYFRAME,
                name: NOON_SKY_NAME.to_owned(),
            },
        ]],
        sky_frames: [
            (NIGHT_SKY_NAME.to_owned(), night_sky()),
            (NOON_SKY_NAME.to_owned(), noon_sky()),
        ]
        .into_iter()
        .collect(),
        water_frames: core::iter::once((WATER_NAME.to_owned(), water())).collect(),
    }
}

/// The `AT_SETTINGS` asset bytes for [`noon_sky`] — what a grid serves for a
/// sky settings asset id.
#[must_use]
pub fn noon_sky_asset() -> Vec<u8> {
    environment_asset_to_bytes(&EnvironmentAsset::Sky(Box::new(noon_sky())))
}

/// The `AT_SETTINGS` asset bytes for [`night_sky`].
#[must_use]
pub fn night_sky_asset() -> Vec<u8> {
    environment_asset_to_bytes(&EnvironmentAsset::Sky(Box::new(night_sky())))
}

/// The `AT_SETTINGS` asset bytes for [`water`].
#[must_use]
pub fn water_asset() -> Vec<u8> {
    environment_asset_to_bytes(&EnvironmentAsset::Water(water()))
}

/// The `AT_SETTINGS` asset bytes for [`day_cycle`] — the kind an environment
/// *inventory* item usually holds.
#[must_use]
pub fn day_cycle_asset() -> Vec<u8> {
    environment_asset_to_bytes(&EnvironmentAsset::DayCycle(Box::new(day_cycle())))
}

/// A whole-region [`EnvironmentSettings`] whose day cycle holds `sky` and
/// nothing else — the **typed** value the `ExtEnvironment` capability serves,
/// as against the asset bytes above.
///
/// One sky keyframe rather than two, deliberately: a single-frame cycle renders
/// the same sky whatever the clock says, so a capture of it is a capture of
/// *that sky* and not of the moment it was taken. That is what makes a
/// before/after pair of captures a comparison of two environments.
#[must_use]
pub fn single_sky_environment(sky: SkySettings) -> EnvironmentSettings {
    let name = sky.name.clone();
    EnvironmentSettings {
        day_cycle: DayCycle {
            name: name.clone(),
            water_track: vec![DayCycleFrame {
                keyframe: 0.0,
                name: WATER_NAME.to_owned(),
            }],
            sky_tracks: vec![vec![DayCycleFrame {
                keyframe: 0.0,
                name: name.clone(),
            }]],
            sky_frames: core::iter::once((name, sky)).collect(),
            water_frames: core::iter::once((WATER_NAME.to_owned(), water())).collect(),
        },
        ..EnvironmentSettings::legacy_windlight_default()
    }
}

/// The bright region environment: [`noon_sky`] held over [`water`].
#[must_use]
pub fn noon_environment() -> EnvironmentSettings {
    single_sky_environment(noon_sky())
}

/// The dark region environment: [`night_sky`] held over [`water`]. Paired with
/// [`noon_environment`] — the two differ by a luminance and by nothing else a
/// capture can see.
#[must_use]
pub fn night_environment() -> EnvironmentSettings {
    single_sky_environment(night_sky())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::{EnvironmentAsset, environment_asset_from_bytes};

    use super::{
        DAY_CYCLE_NAME, NIGHT_SKY_NAME, NOON_SKY_NAME, WATER_NAME, day_cycle, day_cycle_asset,
        night_environment, night_sky, night_sky_asset, noon_environment, noon_sky, noon_sky_asset,
        water, water_asset,
    };

    type TestError = Box<dyn core::error::Error>;

    /// Each fixture's bytes decode back through the viewer's own settings-asset
    /// decoder into the value they were written from.
    #[test]
    fn every_fixture_asset_decodes_back() -> Result<(), TestError> {
        let decode = |name: &str, bytes: Vec<u8>| {
            environment_asset_from_bytes(name, &bytes).ok_or("not a settings asset")
        };
        assert_eq!(
            decode(NOON_SKY_NAME, noon_sky_asset())?,
            EnvironmentAsset::Sky(Box::new(noon_sky()))
        );
        assert_eq!(
            decode(NIGHT_SKY_NAME, night_sky_asset())?,
            EnvironmentAsset::Sky(Box::new(night_sky()))
        );
        assert_eq!(
            decode(WATER_NAME, water_asset())?,
            EnvironmentAsset::Water(water())
        );
        assert_eq!(
            decode(DAY_CYCLE_NAME, day_cycle_asset())?,
            EnvironmentAsset::DayCycle(Box::new(day_cycle()))
        );
        Ok(())
    }

    /// The two skies are what an environment oracle can tell apart: the noon
    /// frame's sunlight is brighter than the night frame's in every channel, by
    /// enough that no capture noise closes the gap.
    #[test]
    fn the_two_skies_are_a_luminance_apart() {
        let (day, night) = (noon_sky().sunlight_color, night_sky().sunlight_color);
        for (bright, dark) in [
            (day.red(), night.red()),
            (day.green(), night.green()),
            (day.blue(), night.blue()),
        ] {
            assert!(
                bright > dark * 4.0,
                "the fixture skies are not far enough apart ({bright} vs {dark})"
            );
        }
    }

    /// The cycle's tracks name frames the cycle actually carries — a keyframe
    /// naming a missing frame renders as the previous one, which is a fixture
    /// that silently stops changing.
    #[test]
    fn the_day_cycle_resolves_every_keyframe() {
        let cycle = day_cycle();
        for track in &cycle.sky_tracks {
            for frame in track {
                assert!(
                    cycle.sky_frames.contains_key(&frame.name),
                    "sky keyframe {} names no frame",
                    frame.name
                );
            }
        }
        for frame in &cycle.water_track {
            assert!(
                cycle.water_frames.contains_key(&frame.name),
                "water keyframe {} names no frame",
                frame.name
            );
        }
    }

    /// The typed region environments are whole-region, single-frame and named
    /// for the sky they hold — the three properties that make a capture of one
    /// a capture of that sky rather than of the clock.
    #[test]
    fn a_single_sky_environment_holds_one_named_frame() {
        for (environment, name) in [
            (noon_environment(), NOON_SKY_NAME),
            (night_environment(), NIGHT_SKY_NAME),
        ] {
            assert_eq!(environment.parcel_id, -1, "the whole region, not a parcel");
            assert_eq!(
                environment.day_cycle.sky_tracks,
                vec![vec![sl_proto::DayCycleFrame {
                    keyframe: 0.0,
                    name: name.to_owned(),
                }]]
            );
            assert_eq!(
                environment.day_cycle.sky_frames.keys().collect::<Vec<_>>(),
                vec![&name.to_owned()]
            );
            assert!(environment.day_cycle.water_frames.contains_key(WATER_NAME));
        }
    }
}
