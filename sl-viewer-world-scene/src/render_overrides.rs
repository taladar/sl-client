//! The render **overrides**: the debug / harness knobs that pin a render setting
//! regardless of the settings store — glow, dynamic exposure, the tone mapper,
//! the reflection probes, the sky's colour pipeline, the sun's shadows, the
//! underwater fog, the pre-water transparency pass, HUD particles and the day
//! position — as one resource per app.
//!
//! They used to be process-global `OnceLock`s, each read from its `SL_VIEWER_*`
//! environment variable by the system that consumed it. That was fine for a
//! viewer, which runs one app per process, and wrong for a test binary, which
//! runs many: two rigs in one process could never disagree about a knob, so no
//! test could render a scene with an effect on and then off — the A/B shape that
//! localises a rendering defect. A resource is per app, so a test sets what it
//! wants and the viewer reads the environment exactly once
//! ([`RenderOverrides::from_env`]), before its task pools spawn.
//!
//! Every variable keeps its name and its meaning; `from_env` is now the only
//! reader of them.
//!
//! A knob that only changes what is **logged** (`SL_VIEWER_LOG_*`) is not
//! here: the log goes to the process's one subscriber whichever app wrote the
//! line, so there is nothing for two apps to disagree about, and such a switch
//! stays an environment read in the module whose diagnostic it gates.
//!
//! A knob that pins a value a **preferences control is also bound to** wins over
//! the store every frame, which would leave that control moving and doing
//! nothing. [`record_env_pins`] reports those to
//! [`EnvPinnedSettings`], which is what makes the preferences row say so and
//! disable itself.

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use sl_viewer_settings::env_pins::{EnvPinnedSettings, PinKind};

use crate::glow::{DEFAULT_STRENGTH, DEFAULT_WIDTH};
use crate::tonemap::{DEFAULT_TONEMAP_MIX, tonemap_type_from_value};

/// The glow pass overrides (`SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_GLOW_STRENGTH`,
/// `SL_VIEWER_GLOW_WIDTH`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GlowOverrides {
    /// Force the pass off (an A/B knob; the glow is on by default).
    pub disabled: bool,
    /// Pin `RenderGlowStrength`; the store no longer drives it.
    pub strength: Option<f32>,
    /// Pin `RenderGlowWidth`; the store no longer drives it.
    pub width: Option<f32>,
}

/// The dynamic-exposure overrides (`SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE`,
/// `SL_VIEWER_EXPOSURE_COEFFICIENT`, `SL_VIEWER_EXPOSURE_NO_FADE`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExposureOverrides {
    /// Pin the dynamic scale to `1.0`, so a capture can tell the dynamic exposure
    /// from the static `RenderExposure`.
    pub disabled: bool,
    /// Pin the exposure coefficient (`max_L`).
    pub coefficient: Option<f32>,
    /// Pin the temporal ease off (the reference's `gExposureProgramNoFade`): the
    /// exposure snaps to its target every frame, so a single-frame capture shows
    /// the converged exposure rather than one `dt` of ramp.
    pub no_fade: bool,
}

/// The tone-mapper overrides (`SL_VIEWER_TONEMAP`, `SL_VIEWER_TONEMAP_MIX`,
/// `SL_VIEWER_EXPOSURE`, `SL_VIEWER_TONEMAP_FORCE_POST`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TonemapOverrides {
    /// Pin the tone curve (`aces` / `neutral` / `none`).
    pub tonemap_type: Option<u32>,
    /// Pin the tone-curve blend.
    pub tonemap_mix: Option<f32>,
    /// Pin the exposure scale on the finished linear frame.
    pub exposure: Option<f32>,
    /// Run the tone mapper on a legacy sky too, which the reference exempts — so a
    /// capture pair with and without shows what the exemption is worth.
    pub force_post: bool,
}

/// The reflection-probe overrides (`SL_VIEWER_PROBE_AMBIENT_SCALE`,
/// `SL_VIEWER_PROBE_GAIN`, `SL_VIEWER_PROBE_TEST_SPHERE`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProbeOverrides {
    /// The share of the sky's flat ambient kept beside the probes' image-based
    /// lighting ([`crate::probes::probe_ambient_scale`]); `0.0`, the default,
    /// hands the whole ambient to the probes.
    pub ambient_scale: f32,
    /// Pin the probes' gain instead of the calibrated `PROBE_GAIN` (an A/B
    /// knob).
    pub gain: Option<f32>,
    /// Spawn the diagnostic mirror ball, to *see* the captured environment.
    pub test_sphere: bool,
}

/// The sky's colour-pipeline overrides (`SL_VIEWER_SKY_LINEARIZE`,
/// `SL_VIEWER_SKY_HDR_SCALE`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkyOverrides {
    /// Skip the linearisation of the sky / cloud colour before the tone
    /// mapper (the reference linearises), to isolate what it does.
    pub linearize_disabled: bool,
    /// Force the sky's "fake HDR" scale (clamped to `>= 0`) instead of the
    /// frame's own, so the EEP blow-out path can be exercised on a grid that
    /// serves only legacy skies.
    pub hdr_scale: Option<f32>,
}

/// The sun-shadow overrides (`SL_VIEWER_SUN_SHADOWS`,
/// `SL_VIEWER_SHADOW_CASCADES`), each winning over its stored preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShadowOverrides {
    /// Force the sun's shadow maps off (`SL_VIEWER_SUN_SHADOWS=0`), to measure
    /// the directional-shadow subsystem's whole per-frame cost.
    pub sun_disabled: bool,
    /// Pin the number of sun shadow cascades (clamped to `1..=4`).
    pub cascades: Option<usize>,
}

/// Every render override, per app. `Default` overrides nothing.
#[derive(Resource, ExtractResource, Debug, Clone, PartialEq, Default)]
pub struct RenderOverrides {
    /// The glow pass.
    pub glow: GlowOverrides,
    /// The dynamic exposure.
    pub exposure: ExposureOverrides,
    /// The tone mapper.
    pub tonemap: TonemapOverrides,
    /// `SL_VIEWER_DISABLE_UNDERWATER_FOG`: force the fog off (zero density is a
    /// shader no-op), to A/B the fog pass against the plain water-surface shading.
    pub underwater_fog_disabled: bool,
    /// `SL_VIEWER_DISABLE_PRE_WATER_PASS`: record no pre-water split, so Bevy's
    /// own transparent pass draws the whole phase — a diagnostic for telling an
    /// artifact of the split from one in the drawn item, not a mode.
    pub pre_water_pass_disabled: bool,
    /// `SL_VIEWER_DISABLE_HUD_PARTICLES`: the reference's `RenderHUDParticles`
    /// flag, mirrored — suppress HUD emitters entirely.
    pub hud_particles_disabled: bool,
    /// `SL_VIEWER_SKY_DAY_POSITION`: pin the day position (`0.0..=1.0`) instead of
    /// following the clock, so a capture can show any point of the day.
    pub day_position: Option<f32>,
    /// The reflection probes.
    pub probes: ProbeOverrides,
    /// The sky's colour pipeline.
    pub sky: SkyOverrides,
    /// The sun's shadows.
    pub shadows: ShadowOverrides,
}

impl RenderOverrides {
    /// Read every knob from the environment — once, by the viewer, before its
    /// task pools spawn (reading the environment is only sound while the process
    /// is single-threaded).
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            glow: GlowOverrides {
                disabled: is_set("SL_VIEWER_DISABLE_GLOW"),
                // A set-but-unparsable value still pins the field, at its default:
                // the store must not win over a knob the user set.
                strength: pinned_f32("SL_VIEWER_GLOW_STRENGTH", DEFAULT_STRENGTH),
                width: pinned_f32("SL_VIEWER_GLOW_WIDTH", DEFAULT_WIDTH),
            },
            exposure: ExposureOverrides {
                disabled: is_set("SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE"),
                coefficient: pinned_f32(
                    "SL_VIEWER_EXPOSURE_COEFFICIENT",
                    crate::exposure::DEFAULT_EXPOSURE_COEFFICIENT,
                ),
                no_fade: is_set("SL_VIEWER_EXPOSURE_NO_FADE"),
            },
            tonemap: TonemapOverrides {
                tonemap_type: std::env::var("SL_VIEWER_TONEMAP")
                    .ok()
                    .map(|value| tonemap_type_from_value(&value)),
                tonemap_mix: pinned_f32("SL_VIEWER_TONEMAP_MIX", DEFAULT_TONEMAP_MIX),
                exposure: pinned_f32("SL_VIEWER_EXPOSURE", crate::tonemap::DEFAULT_EXPOSURE),
                force_post: is_set("SL_VIEWER_TONEMAP_FORCE_POST"),
            },
            // Unlike the others this one is a value: `0` and the empty string
            // leave the fog on.
            underwater_fog_disabled: std::env::var("SL_VIEWER_DISABLE_UNDERWATER_FOG")
                .is_ok_and(|value| value != "0" && !value.is_empty()),
            pre_water_pass_disabled: is_set("SL_VIEWER_DISABLE_PRE_WATER_PASS"),
            hud_particles_disabled: is_set("SL_VIEWER_DISABLE_HUD_PARTICLES"),
            // Unset or unparsable falls back to the clock; a value is clamped.
            day_position: std::env::var("SL_VIEWER_SKY_DAY_POSITION")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .map(|position| position.clamp(0.0, 1.0)),
            probes: ProbeOverrides {
                ambient_scale: parsed_f32("SL_VIEWER_PROBE_AMBIENT_SCALE").unwrap_or(0.0),
                gain: parsed_f32("SL_VIEWER_PROBE_GAIN"),
                test_sphere: std::env::var("SL_VIEWER_PROBE_TEST_SPHERE")
                    .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true")),
            },
            sky: SkyOverrides {
                // Only `0` turns it off; anything else keeps the reference's.
                linearize_disabled: std::env::var("SL_VIEWER_SKY_LINEARIZE").as_deref() == Ok("0"),
                hdr_scale: parsed_f32("SL_VIEWER_SKY_HDR_SCALE").map(|scale| scale.max(0.0)),
            },
            shadows: ShadowOverrides {
                // Only `0` turns them off: the variable is an off switch.
                sun_disabled: std::env::var("SL_VIEWER_SUN_SHADOWS").as_deref() == Ok("0"),
                cascades: std::env::var("SL_VIEWER_SHADOW_CASCADES")
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .map(|count| count.clamp(1, 4)),
            },
        }
    }
}

/// Record, into `pins`, every knob above that holds a **registered,
/// GUI-editable** setting — the ones whose preferences control would otherwise
/// move and do nothing, because `refresh_glow` / `refresh_exposure` /
/// `refresh_tonemap_settings` re-apply the environment value every frame.
///
/// The knobs with no preference behind them (`SL_VIEWER_GLOW_WIDTH`'s
/// companions in the harness: the underwater fog, the pre-water split, the day
/// position, the exposure coefficient, HUD particles, the force-post exemption)
/// are deliberately absent: nothing in the UI claims to own those, so there is
/// no second authority to report. They are still listed by
/// [`log_active_env_knobs`](sl_viewer_settings::env_pins::log_active_env_knobs).
///
/// Reads the environment; call it from the same start-up pass as
/// [`RenderOverrides::from_env`].
pub fn record_env_pins(pins: &mut EnvPinnedSettings) {
    pins.pin_if_set(
        crate::glow::SETTING_ENABLED,
        "SL_VIEWER_DISABLE_GLOW",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::glow::SETTING_STRENGTH,
        "SL_VIEWER_GLOW_STRENGTH",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::glow::SETTING_WIDTH,
        "SL_VIEWER_GLOW_WIDTH",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::exposure::SETTING_ENABLED,
        "SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::tonemap::SETTING_TONEMAP_TYPE,
        "SL_VIEWER_TONEMAP",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::tonemap::SETTING_TONEMAP_MIX,
        "SL_VIEWER_TONEMAP_MIX",
        PinKind::Live,
    );
    pins.pin_if_set(
        crate::tonemap::SETTING_EXPOSURE,
        "SL_VIEWER_EXPOSURE",
        PinKind::Live,
    );
}

/// Whether `key` is set at all (to anything).
fn is_set(key: &str) -> bool {
    std::env::var_os(key).is_some()
}

/// `key`'s value as a number, or `None` when it is unset or does not parse
/// (such a knob falls back to its default rather than pinning it).
fn parsed_f32(key: &str) -> Option<f32> {
    std::env::var(key).ok()?.parse().ok()
}

/// `Some(value)` when `key` is set: its parsed value, or `default` when it does
/// not parse — a set variable always pins its field.
fn pinned_f32(key: &str, default: f32) -> Option<f32> {
    let value = std::env::var(key).ok()?;
    Some(value.parse().unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::RenderOverrides;
    use pretty_assertions::assert_eq;

    /// Two apps in one process can disagree about a knob — the property the
    /// process-global locks could not offer.
    #[test]
    fn two_apps_in_one_process_can_disagree() {
        use bevy::prelude::*;

        let mut on = App::new();
        on.init_resource::<RenderOverrides>();
        let mut off = App::new();
        off.insert_resource(RenderOverrides {
            underwater_fog_disabled: true,
            ..RenderOverrides::default()
        });
        assert!(
            !on.world()
                .resource::<RenderOverrides>()
                .underwater_fog_disabled
        );
        assert!(
            off.world()
                .resource::<RenderOverrides>()
                .underwater_fog_disabled
        );
    }

    #[test]
    fn nothing_is_overridden_by_default() {
        let overrides = RenderOverrides::default();
        assert_eq!(overrides, RenderOverrides::default());
        assert!(overrides.glow.strength.is_none());
        assert!(overrides.day_position.is_none());
        assert!(!overrides.tonemap.force_post);
    }
}
