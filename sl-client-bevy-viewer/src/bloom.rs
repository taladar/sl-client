//! The Second Life / Firestorm **glow (bloom)** pass.
//!
//! SL's `RenderGlow` pipeline (`LLPipeline::renderBloom`, `glowExtractF.glsl`)
//! extracts bright pixels — luminance above `RenderGlowMinLuminance` — *and*
//! glow-flagged content (the per-face glow scalar, `PASS_GLOW` / `mGlowPool`)
//! into a low-resolution glow buffer, blurs it over `RenderGlowIterations`
//! Gaussian steps, and **additively** composites it back at `RenderGlowStrength`
//! before the final tone map. That single mechanism is what gives the sun its
//! soft halo, glowing prims their bloom, and fullbright / bright surfaces their
//! spread — every object the reference blooms, not just the sun.
//!
//! Ported here as Bevy's screen-space [`Bloom`] on the **main camera** — a
//! mip-chain downsample/upsample bloom that likewise extracts bright pixels above
//! a threshold and adds a blurred copy back. Both are screen-space and
//! luminance-driven (the shipped `RenderGlowMinLuminance` is `1.0`, not the `9999`
//! one-off a single mirror pass uses), so it blooms every bright / glowing surface
//! globally: the sun, glow-flagged faces (P27.4 renders a face's glow as
//! `emissive`, so a glowing face reads above the threshold), fullbright, and
//! bright textures alike. It sits **only** on the main camera, never the
//! reflection-probe capture cameras — those must stay linear, being the source of
//! image-based lighting.
//!
//! The reference `RenderGlow*` settings map onto the [`Bloom`] fields (documented
//! per field below). The algorithms differ (SL blurs a fixed-iteration Gaussian;
//! Bevy blurs a mip chain), so the mapping is a tuned approximation, each knob
//! overridable by an environment variable for a no-rebuild capture sweep and
//! persisted under `[render.glow]` so a user's Firestorm `RenderGlow*` values port
//! across.

use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;

use sl_settings::SettingValue;

use crate::camera::ViewerCamera;
use crate::settings::ViewerSettings;

/// The reference `RenderGlowMinLuminance` default: the luminance a scene pixel must
/// exceed to bloom. Our sky reaches well above `1.0` in HDR near the sun (after the
/// `srgb_to_linear` expansion), so this blooms the sun / near-sun sky but leaves
/// the plain sky below it untouched.
const DEFAULT_MIN_LUMINANCE: f32 = 1.0;

/// The additive strength of the glow — Bevy's [`Bloom`] `intensity`. **Not** the
/// reference `RenderGlowStrength` value (`0.325`): Bevy's mip-chain bloom scales
/// its additive contribution very differently from the reference's fixed-iteration
/// Gaussian glow buffer, so feeding the reference number in over-blooms the whole
/// frame by ~20%. This is a Bevy-tuned value in Bevy's own range (its `OLD_SCHOOL`
/// preset uses `0.05`) that reproduces a Firestorm-like soft halo around bright /
/// glowing content without lifting the whole screen. Overridable live via
/// `SL_VIEWER_BLOOM_STRENGTH`.
const DEFAULT_STRENGTH: f32 = 0.08;

/// The soft edge of the luminance threshold. The reference extraction ramps over
/// `smoothstep(minLuminance, minLuminance + 1.0, …)`, i.e. a softness of `1.0`.
const THRESHOLD_SOFTNESS: f32 = 1.0;

/// The environment variable overriding the glow strength (the [`Bloom`] intensity).
const ENV_STRENGTH: &str = "SL_VIEWER_BLOOM_STRENGTH";
/// The environment variable overriding the glow luminance threshold.
const ENV_MIN_LUMINANCE: &str = "SL_VIEWER_BLOOM_MIN_LUMINANCE";
/// The environment variable that force-disables the glow pass (an A/B knob: sets
/// the intensity to zero so a capture can tell the bloom from the underlying
/// scene).
const ENV_DISABLE: &str = "SL_VIEWER_DISABLE_BLOOM";

/// The persisted-file section the glow settings are grouped under
/// (`[render.glow]`), matching the reference's `RenderGlow*` naming.
const GLOW_SECTION: &[&str] = &["render", "glow"];

/// The reference `RenderGlowStrength` setting name.
const SETTING_STRENGTH: &str = "RenderGlowStrength";
/// The reference `RenderGlowMinLuminance` setting name.
const SETTING_MIN_LUMINANCE: &str = "RenderGlowMinLuminance";

/// Register the glow settings on the store with the reference defaults, so the
/// names exist (and persist) — a user's Firestorm `RenderGlowStrength` /
/// `RenderGlowMinLuminance` port straight over — and the (future) preferences UI
/// has something to bind to. Called from [`ViewerSettings`]'s `FromWorld`.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        GLOW_SECTION,
        SETTING_STRENGTH,
        SettingValue::F32(DEFAULT_STRENGTH),
        "Additive strength of the glow / bloom, Bevy scale (0 disables it)",
    );
    settings.register_in(
        GLOW_SECTION,
        SETTING_MIN_LUMINANCE,
        SettingValue::F32(DEFAULT_MIN_LUMINANCE),
        "Scene luminance a pixel must exceed to bloom",
    );
}

/// Read an `f32` glow knob from the environment, falling back to `default` when it
/// is unset or unparsable.
fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// The strength (intensity) to render at, honouring the disable knob then the
/// strength override, else the passed stored/default value.
fn resolved_strength(stored: f32) -> f32 {
    if std::env::var_os(ENV_DISABLE).is_some() {
        return 0.0;
    }
    env_f32(ENV_STRENGTH, stored)
}

/// The [`Bloom`] component the main camera wears, built from the reference glow
/// defaults (env-overridable). Additive compositing matches the reference's
/// additive glow; the prefilter threshold + softness match its luminance
/// extraction ramp.
pub(crate) fn sl_bloom() -> Bloom {
    Bloom {
        intensity: resolved_strength(DEFAULT_STRENGTH),
        prefilter: BloomPrefilter {
            threshold: env_f32(ENV_MIN_LUMINANCE, DEFAULT_MIN_LUMINANCE),
            threshold_softness: THRESHOLD_SOFTNESS,
        },
        composite_mode: BloomCompositeMode::Additive,
        ..Bloom::OLD_SCHOOL
    }
}

/// Refresh the main camera's live [`Bloom`] from the settings store each frame
/// (cheap reads), so a `RenderGlowStrength` / `RenderGlowMinLuminance` changed in
/// the (future) preferences UI takes effect at once. An environment override
/// (`SL_VIEWER_BLOOM_*` / `SL_VIEWER_DISABLE_BLOOM`), used by the screenshot
/// harness, **wins** over the stored value so a capture is reproducible.
pub(crate) fn refresh_bloom_settings(
    store: Res<ViewerSettings>,
    mut cameras: Query<&mut Bloom, With<ViewerCamera>>,
) {
    let store = store.store();
    for mut bloom in &mut cameras {
        let stored_strength = store.get_f32(SETTING_STRENGTH).unwrap_or(DEFAULT_STRENGTH);
        bloom.intensity = resolved_strength(stored_strength);
        if std::env::var_os(ENV_MIN_LUMINANCE).is_none()
            && let Ok(value) = store.get_f32(SETTING_MIN_LUMINANCE)
        {
            bloom.prefilter.threshold = value;
        }
    }
}
