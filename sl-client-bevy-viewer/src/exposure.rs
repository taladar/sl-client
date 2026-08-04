//! The Second Life / Firestorm **dynamic exposure** (`generateExposure` /
//! `exposureF.glsl`): the scene-luminance-driven scale the reference multiplies
//! onto `RenderExposure` before the tone curve, and the `sky_hdr_scale`
//! counterweight that keeps an EEP / reflection-probe-ambiance sky from washing
//! out.
//!
//! **What the reference does.** After the deferred scene is composited,
//! `LLPipeline::generateLuminance` renders a per-pixel luminance map (with a mip
//! chain), and `generateExposure` (`exposureF.glsl`) reads its coarsest mip — the
//! whole-frame average luminance `L` — and maps it through
//! `s = mix(exp_max, exp_min, pow(clamp(L / coeff, 0, 1), 2))`. The tone mapper
//! (`tonemapUtilF.glsl` `toneMap`) then applies `RenderExposure * s`. For a
//! **legacy** sky `exp_min == exp_max == 1`, so `s == 1` and the whole thing is a
//! no-op (which is why the grey-disc symptom is *not* fixed by exposure on legacy
//! skies). For an **EEP** sky whose `reflection_probe_ambiance > 0`, the WL sky is
//! scaled up by `sky_hdr_scale = sqrt(gamma) * 2 > 1`
//! ([`SkySettings::sky_hdr_scale`](sl_proto::SkySettings::sky_hdr_scale)); the
//! exposure range becomes `[1 / hdr_scale, hdr_scale]`, so a bright frame pulls the
//! exposure down toward `1 / hdr_scale` and undoes the up-scale — the counterweight
//! that stops EEP / Modern skies from blowing the whole frame to white.
//!
//! **The port.** Bevy 0.19 is system-based (no render-graph `ViewNode`), so this is
//! a fullscreen pass in the [`Core3d`] schedule (modelled on
//! [`tonemap`](crate::tonemap) / [`underwater_fog`](crate::underwater_fog)),
//! ordered after the glow / fog and **before** the tone mapper: it reads the
//! composited linear scene and writes a 1×1 [`ExposureMap`] the tone mapper then
//! samples (mirroring the reference's `exposureMap` texture, sampled in `toneMap`).
//! The average luminance is grid-sampled over the same central crop the reference
//! reduces (`exposure.wgsl` documents that approximation of the mip average).
//!
//! **Temporal adaptation.** History smoothing (`gExposureProgram`'s
//! `USE_LAST_EXPOSURE` fade toward the previous frame's exposure, so a camera turn
//! eases rather than snaps) **is** ported: each frame the pass copies the previous
//! exposure into a second 1×1 [`ExposureMap::last`] texture (the reference's
//! `mLastExposure`) and the shader blends the freshly-computed target toward it by
//! `1 - exp(-speed · dt)`, where `speed = -ln(speed_error) / speed_target` from
//! `RenderDynamicExposureSpeedError` / `RenderDynamicExposureSpeedTarget` and `dt`
//! is the frame interval — so after `speed_target` seconds the error has decayed to
//! `speed_error`. `SL_VIEWER_EXPOSURE_NO_FADE` pins the ease off (the reference's
//! `gExposureProgramNoFade` path), which the screenshot harness needs so a
//! single-frame capture shows the converged exposure instead of one `dt` of ramp.
//!
//! **Sky-settings range.** [`exposure_range`] is a faithful port of the whole
//! `generateExposure` `exp_min` / `exp_max` block, including the
//! `RenderUseExposureSkySettings` branch (the sky's `getHDROffset` / `getHDRMin` /
//! `getHDRMax`) and `RenderSkyAutoAdjustLegacy` (which lifts a legacy sky's probe
//! ambiance so it, too, adapts). The static `RenderExposure` scale stays on
//! [`SlTonemap`](crate::tonemap::SlTonemap); this module supplies only the dynamic
//! factor it is multiplied by.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FragmentState, Operations, Origin3d, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureView, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::ViewTarget;
use bevy::render::{GpuResourceAppExt as _, Render, RenderApp, RenderStartup, RenderSystems};

use sl_settings::SettingValue;

use crate::settings::ViewerSettings;
use crate::underwater_fog::UnderwaterFogPass;

/// The internal handle the exposure-sample shader (`exposure.wgsl`) is loaded under.
const EXPOSURE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2f8d61b4-7c05-4e39-9a12-c4d6e8017b52");

/// The render-schedule label for the exposure pass, so the tone mapper can order
/// itself **after** it (the tone mapper samples the 1×1 exposure map this pass
/// writes).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SlExposurePass;

/// The format of the 1×1 exposure map — floating-point so an `exp_max` above 1 (an
/// EEP sky lifts a dark frame past unity) is stored without clipping.
const EXPOSURE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The reference `RenderDynamicExposureEnabled` default: the dynamic exposure is on.
const DEFAULT_EXPOSURE_ENABLED: bool = true;

/// The persisted-file section the dynamic-exposure settings are grouped under
/// (`[render.exposure]`), matching the reference's `RenderDynamicExposure*` naming.
const EXPOSURE_SECTION: &[&str] = &["render", "exposure"];

/// The reference `RenderDynamicExposureEnabled` setting name.
const SETTING_ENABLED: &str = "RenderDynamicExposureEnabled";
/// The reference `RenderDynamicExposureCoefficient` setting name.
const SETTING_COEFFICIENT: &str = "RenderDynamicExposureCoefficient";
/// The reference `RenderDynamicExposureSpeedError` setting name (the fraction of the
/// error still remaining after `speed_target` seconds).
const SETTING_SPEED_ERROR: &str = "RenderDynamicExposureSpeedError";
/// The reference `RenderDynamicExposureSpeedTarget` setting name (the time constant,
/// in seconds, of the exposure ease).
const SETTING_SPEED_TARGET: &str = "RenderDynamicExposureSpeedTarget";
/// The reference `RenderUseExposureSkySettings` setting name (source the exposure
/// range from the sky's fixed HDR offset / min / max rather than the probe-ambiance
/// `hdr_scale`).
const SETTING_USE_SKY: &str = "RenderUseExposureSkySettings";
/// The reference `RenderSkyAutoAdjustLegacy` setting name (treat a legacy sky as if
/// it carried the auto-adjust probe ambiance, so it adapts too).
const SETTING_AUTO_ADJUST_LEGACY: &str = "RenderSkyAutoAdjustLegacy";

/// The environment variable force-disabling the dynamic exposure (an A/B knob: pins
/// the scale to `1.0` so a capture can tell the dynamic exposure from the static
/// `RenderExposure`).
const ENV_DISABLE: &str = "SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE";
/// The environment variable overriding the exposure coefficient (`max_L`).
const ENV_COEFFICIENT: &str = "SL_VIEWER_EXPOSURE_COEFFICIENT";
/// The environment variable pinning the temporal ease off (the reference's
/// `gExposureProgramNoFade` path): the exposure snaps to the instantaneous target
/// every frame, which the screenshot harness needs so a single-frame capture shows
/// the converged exposure rather than one `dt` of ramp from the initial `1.0`.
const ENV_NO_FADE: &str = "SL_VIEWER_EXPOSURE_NO_FADE";

/// The reference `RenderDynamicExposureCoefficient` default (`exposureF.glsl`'s
/// `max_L`): the average luminance at which the dynamic scale reaches its floor.
const DEFAULT_EXPOSURE_COEFFICIENT: f32 = 0.175;

/// The reference `RenderDynamicExposureSpeedError` default: the fraction of the
/// exposure error still remaining after [`DEFAULT_SPEED_TARGET`] seconds.
const DEFAULT_SPEED_ERROR: f32 = 0.1;
/// The reference `RenderDynamicExposureSpeedTarget` default: the ease time constant,
/// in seconds.
const DEFAULT_SPEED_TARGET: f32 = 2.0;
/// The reference `RenderUseExposureSkySettings` default (off — the shipped path
/// derives the range from the probe-ambiance `hdr_scale`).
const DEFAULT_USE_SKY: bool = false;
/// The reference `RenderSkyAutoAdjustLegacy` default (off — a legacy sky stays
/// inert).
const DEFAULT_AUTO_ADJUST_LEGACY: bool = false;

/// The reference `LLSettingsSky::mHDROffset` (a fixed constant, not a decoded field):
/// the centre of the `RenderUseExposureSkySettings` exposure range.
const HDR_OFFSET: f32 = 1.0;
/// The reference `LLSettingsSky::mHDRMin` (fixed): the downward half-width of the
/// `RenderUseExposureSkySettings` range (`getHDRMin` returns `0` for a legacy sky
/// with auto-adjust off).
const HDR_MIN: f32 = 0.5;
/// The reference `LLSettingsSky::mHDRMax` (fixed): the upward half-width of the
/// `RenderUseExposureSkySettings` range (`getHDRMax` returns `0` for a legacy sky
/// with auto-adjust off).
const HDR_MAX: f32 = 2.0;
/// The reference `LLSettingsSky::sAutoAdjustProbeAmbiance`
/// (`DEFAULT_AUTO_ADJUST_PROBE_AMBIANCE`): the probe ambiance a legacy sky is treated
/// as carrying when `RenderSkyAutoAdjustLegacy` is on.
const AUTO_ADJUST_PROBE_AMBIANCE: f32 = 1.0;

/// The active sky frame's operands for [`exposure_range`] — the `LLSettingsSky`
/// values `generateExposure` (`pipeline.cpp`) reads. Mirrors the [`ExposureRange`]
/// resource `drive_sky` publishes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SkyExposureInputs {
    /// The frame's `reflection_probe_ambiance` (an EEP-only setting, `0.0` on a
    /// legacy sky).
    pub(crate) reflection_probe_ambiance: f32,
    /// The frame's `gamma`.
    pub(crate) gamma: f32,
    /// Whether the sky may auto-adjust (`LLSettingsSky::mCanAutoAdjust`): true for a
    /// legacy / classic-mode sky (no `reflection_probe_ambiance` setting), false for
    /// an EEP sky. Derived from `reflection_probe_ambiance == 0`.
    pub(crate) can_auto_adjust: bool,
}

/// The live `Render*` settings for [`exposure_range`] — the `LLCachedControl`s
/// `generateExposure` reads alongside the sky frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExposureSettings {
    /// `RenderDynamicExposureEnabled`.
    pub(crate) dynamic_enabled: bool,
    /// `RenderUseExposureSkySettings`.
    pub(crate) use_sky_settings: bool,
    /// `RenderSkyAutoAdjustLegacy`.
    pub(crate) sky_auto_adjust_legacy: bool,
}

/// The reference `generateExposure` exposure range (`exp_min`, `exp_max`) for one sky
/// frame — a faithful port of the `exp_min` / `exp_max` block in `pipeline.cpp`,
/// covering all three of the reference's branches.
///
/// **`RenderUseExposureSkySettings`** sources the range from the sky's fixed HDR
/// constants: `exp_min = getHDROffset - getHDRMin`, `exp_max = getHDROffset +
/// getHDRMax` when the dynamic exposure is enabled, else the flat `(offset, offset)`.
/// The getters (`getHDR*`) return the [`HDR_OFFSET`] / [`HDR_MIN`] / [`HDR_MAX`]
/// constants, except that a legacy sky (`can_auto_adjust`) with auto-adjust **off**
/// gets `min == max == 0` (offset stays `1`), so its range collapses to `(1, 1)` —
/// inert. `RenderSkyAutoAdjustLegacy` flips that legacy sky back to the real
/// half-widths, so it adapts like an EEP sky.
///
/// **The shipped path** (`RenderUseExposureSkySettings = false`,
/// `RenderDynamicExposureEnabled = true`) reads the frame's probe ambiance —
/// `getReflectionProbeAmbiance`, which for a legacy sky with
/// `RenderSkyAutoAdjustLegacy` on returns [`AUTO_ADJUST_PROBE_AMBIANCE`] rather than
/// the stored `0`. When that ambiance is positive it computes
/// `hdr_scale = sqrt(gamma) * 2` and, if that exceeds `1`, the range is
/// `(1 / hdr_scale, hdr_scale)` — the counterweight to the WL sky's `sky_hdr_scale`
/// up-scale (see [`SkySettings::sky_hdr_scale`](sl_proto::SkySettings::sky_hdr_scale)).
/// Otherwise the range is the inert `(1, 1)`.
#[must_use]
pub(crate) fn exposure_range(sky: SkyExposureInputs, settings: ExposureSettings) -> (f32, f32) {
    // `getHDR*(should_auto_adjust)`: a legacy sky with auto-adjust off zeroes the
    // half-widths (leaving the offset), which collapses the sky-settings range to
    // `(offset, offset)`. Any other case keeps the real constants.
    let legacy_without_adjust = sky.can_auto_adjust && !settings.sky_auto_adjust_legacy;
    let hdr_min = if legacy_without_adjust { 0.0 } else { HDR_MIN };
    let hdr_max = if legacy_without_adjust { 0.0 } else { HDR_MAX };

    if settings.use_sky_settings {
        if settings.dynamic_enabled {
            return (HDR_OFFSET - hdr_min, HDR_OFFSET + hdr_max);
        }
        return (HDR_OFFSET, HDR_OFFSET);
    }

    if !settings.dynamic_enabled {
        return (1.0, 1.0);
    }

    // `getReflectionProbeAmbiance(should_auto_adjust)`: auto-adjust lifts a legacy
    // sky to the default probe ambiance so it, too, enters the HDR path.
    let probe_ambiance = if settings.sky_auto_adjust_legacy && sky.can_auto_adjust {
        AUTO_ADJUST_PROBE_AMBIANCE
    } else {
        sky.reflection_probe_ambiance
    };
    if probe_ambiance <= 0.0 {
        return (1.0, 1.0);
    }
    let hdr_scale = sky.gamma.max(0.0).sqrt() * 2.0;
    if hdr_scale > 1.0 {
        (1.0 / hdr_scale, hdr_scale)
    } else {
        (1.0, 1.0)
    }
}

/// The active sky frame's exposure inputs, republished each frame by
/// [`drive_sky`](crate::sky::drive_sky) from the rendered [`SkySettings`] so the
/// exposure pass tracks the exact sky the sky dome is drawn from (rather than
/// re-deriving the altitude-blended frame). [`refresh_exposure`] folds these together
/// with the live settings through [`exposure_range`]. A legacy no-op sky until one is
/// resolved.
#[derive(Resource, Clone, Copy)]
pub(crate) struct ExposureRange {
    /// The frame's `reflection_probe_ambiance`.
    pub(crate) reflection_probe_ambiance: f32,
    /// The frame's `gamma`.
    pub(crate) gamma: f32,
    /// Whether the sky may auto-adjust (`mCanAutoAdjust`), i.e. it is a legacy sky.
    pub(crate) can_auto_adjust: bool,
}

impl Default for ExposureRange {
    /// The legacy no-op inputs until a sky frame publishes real ones.
    fn default() -> Self {
        Self {
            reflection_probe_ambiance: 0.0,
            gamma: 1.0,
            can_auto_adjust: true,
        }
    }
}

/// The per-frame dynamic-exposure inputs, carried on the main camera (which both
/// carries them to the GPU as a uniform and selects the view the pass runs on, so
/// the reflection-probe capture cameras — which must stay linear — are left alone),
/// matching `exposure.wgsl`'s `SlExposure`.
#[derive(Component, Clone, Copy, ShaderType)]
pub(crate) struct SlExposure {
    /// The exposure floor (`exp_min`), resolved from the active sky's
    /// [`ExposureRange`] and the settings via [`exposure_range`].
    pub(crate) exp_min: f32,
    /// The exposure ceiling (`exp_max`).
    pub(crate) exp_max: f32,
    /// The reference `RenderDynamicExposureCoefficient` (`exposureF.glsl`'s
    /// `max_L`). Overridable by `SL_VIEWER_EXPOSURE_COEFFICIENT`.
    pub(crate) coefficient: f32,
    /// `1.0` to run the dynamic exposure, `0.0` to pin the scale to `1.0`.
    pub(crate) enabled: f32,
    /// The frame interval, seconds (`gFrameIntervalSeconds`), the temporal ease
    /// integrates over. `0.0` (the first frame) makes the ease a no-op.
    pub(crate) dt: f32,
    /// The reference `RenderDynamicExposureSpeedError` (`dynamic_exposure_params.w`):
    /// the fraction of the exposure error still remaining after `speed_target`
    /// seconds.
    pub(crate) speed_error: f32,
    /// The reference `RenderDynamicExposureSpeedTarget` (`dynamic_exposure_params2.w`):
    /// the ease time constant, seconds.
    pub(crate) speed_target: f32,
    /// `1.0` to ease toward the previous frame's exposure (the reference
    /// `gExposureProgram` path), `0.0` to snap to the instantaneous target (the
    /// `gExposureProgramNoFade` path / `SL_VIEWER_EXPOSURE_NO_FADE`).
    pub(crate) fade: f32,
}

impl Default for SlExposure {
    /// The legacy no-op (flat range) until [`refresh_exposure`] folds in the sky's
    /// range and the settings.
    fn default() -> Self {
        Self {
            exp_min: 1.0,
            exp_max: 1.0,
            coefficient: env_f32(ENV_COEFFICIENT, DEFAULT_EXPOSURE_COEFFICIENT),
            enabled: if std::env::var_os(ENV_DISABLE).is_some() {
                0.0
            } else {
                f32::from(u8::from(DEFAULT_EXPOSURE_ENABLED))
            },
            dt: 0.0,
            speed_error: DEFAULT_SPEED_ERROR,
            speed_target: DEFAULT_SPEED_TARGET,
            fade: if std::env::var_os(ENV_NO_FADE).is_some() {
                0.0
            } else {
                1.0
            },
        }
    }
}

/// Read an `f32` tuning knob from the environment, falling back to `default` when it
/// is unset or unparsable.
fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Register the dynamic-exposure settings on the store with the reference defaults,
/// so a user's Firestorm `RenderDynamicExposureEnabled` /
/// `RenderDynamicExposureCoefficient` port across and the (future) preferences UI
/// has something to bind to. Called from [`ViewerSettings`]'s `FromWorld`.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_ENABLED,
        SettingValue::Bool(DEFAULT_EXPOSURE_ENABLED),
        "Drive the tone-map exposure from the scene's average luminance",
    );
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_COEFFICIENT,
        SettingValue::F32(DEFAULT_EXPOSURE_COEFFICIENT),
        "Average luminance at which the dynamic exposure reaches its floor",
    );
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_SPEED_ERROR,
        SettingValue::F32(DEFAULT_SPEED_ERROR),
        "Fraction of the exposure error still remaining after the target time",
    );
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_SPEED_TARGET,
        SettingValue::F32(DEFAULT_SPEED_TARGET),
        "Seconds over which the exposure eases toward a new target",
    );
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_USE_SKY,
        SettingValue::Bool(DEFAULT_USE_SKY),
        "Source the exposure range from the sky's HDR offset/min/max settings",
    );
    settings.register_in(
        EXPOSURE_SECTION,
        SETTING_AUTO_ADJUST_LEGACY,
        SettingValue::Bool(DEFAULT_AUTO_ADJUST_LEGACY),
        "Let a legacy sky adapt as if it carried the auto-adjust probe ambiance",
    );
}

/// Fold the active sky's [`ExposureRange`] and the stored / overridden settings into
/// each camera's live [`SlExposure`] each frame (cheap reads), resolving the exposure
/// range through [`exposure_range`] and carrying this frame's `dt` and ease constants
/// to the GPU. An environment override (`SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE` /
/// `SL_VIEWER_EXPOSURE_COEFFICIENT` / `SL_VIEWER_EXPOSURE_NO_FADE`), used by the
/// screenshot harness, **wins** over the stored value so a capture is reproducible.
pub(crate) fn refresh_exposure(
    store: Res<ViewerSettings>,
    range: Res<ExposureRange>,
    time: Res<Time>,
    mut cameras: Query<&mut SlExposure>,
) {
    let store = store.store();
    let disabled_by_env = std::env::var_os(ENV_DISABLE).is_some();
    let no_fade_by_env = std::env::var_os(ENV_NO_FADE).is_some();
    let dynamic_enabled = store
        .get_bool(SETTING_ENABLED)
        .unwrap_or(DEFAULT_EXPOSURE_ENABLED);
    let use_sky_settings = store.get_bool(SETTING_USE_SKY).unwrap_or(DEFAULT_USE_SKY);
    let sky_auto_adjust_legacy = store
        .get_bool(SETTING_AUTO_ADJUST_LEGACY)
        .unwrap_or(DEFAULT_AUTO_ADJUST_LEGACY);
    // The reference reads these with `LLCachedControl` and never guards them; clamp
    // defensively so a user-edited setting cannot flip `speed = -ln(error)/target`
    // negative (a non-decaying or diverging ease).
    let speed_error = store
        .get_f32(SETTING_SPEED_ERROR)
        .unwrap_or(DEFAULT_SPEED_ERROR)
        .clamp(1.0e-4, 0.999);
    let speed_target = store
        .get_f32(SETTING_SPEED_TARGET)
        .unwrap_or(DEFAULT_SPEED_TARGET)
        .max(1.0e-4);

    let (exp_min, exp_max) = exposure_range(
        SkyExposureInputs {
            reflection_probe_ambiance: range.reflection_probe_ambiance,
            gamma: range.gamma,
            can_auto_adjust: range.can_auto_adjust,
        },
        ExposureSettings {
            dynamic_enabled,
            use_sky_settings,
            sky_auto_adjust_legacy,
        },
    );
    let dt = time.delta_secs();

    for mut exposure in &mut cameras {
        exposure.exp_min = exp_min;
        exposure.exp_max = exp_max;
        if std::env::var_os(ENV_COEFFICIENT).is_none()
            && let Ok(value) = store.get_f32(SETTING_COEFFICIENT)
        {
            exposure.coefficient = value;
        }
        exposure.enabled = f32::from(u8::from(!disabled_by_env && dynamic_enabled));
        exposure.dt = dt;
        exposure.speed_error = speed_error;
        exposure.speed_target = speed_target;
        exposure.fade = if no_fade_by_env { 0.0 } else { 1.0 };
    }
}

impl SyncComponent for SlExposure {
    type Target = Self;
}

impl ExtractComponent for SlExposure {
    type QueryData = Read<Self>;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// The plugin: registers extraction / uniform upload, loads the shader, seeds the
/// [`ExposureRange`] resource, and wires the exposure pass into the 3D render
/// schedule — after the glow / fog, before the tone mapper.
#[derive(Debug, Default)]
pub(crate) struct SlExposurePlugin;

impl Plugin for SlExposurePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            EXPOSURE_SHADER_HANDLE,
            "exposure.wgsl",
            Shader::from_wgsl
        );
        app.init_resource::<ExposureRange>()
            .add_plugins((
                ExtractComponentPlugin::<SlExposure>::default(),
                UniformComponentPlugin::<SlExposure>::default(),
            ))
            .add_systems(Update, refresh_exposure);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<SlExposurePipeline>>()
            .add_systems(RenderStartup, init_exposure_pipeline)
            .add_systems(
                Render,
                prepare_exposure_pipeline.in_set(RenderSystems::Prepare),
            )
            .add_systems(
                Core3d,
                sl_exposure_system
                    .in_set(Core3dSystems::PostProcess)
                    .in_set(SlExposurePass)
                    // Read the composited linear scene *after* the glow / fog have
                    // added into it; the tone mapper orders itself after
                    // `SlExposurePass`, so it consumes the 1×1 exposure map this
                    // pass writes.
                    .after(UnderwaterFogPass)
                    .after(bevy::post_process::bloom::bloom),
            );
    }
}

/// The 1×1 exposure map the pass writes and the tone mapper samples, plus the
/// previous frame's copy the temporal ease reads and the sampler both are read
/// through. A render-world resource so every pass references the one pair of textures
/// across frames.
#[derive(Resource)]
pub(crate) struct ExposureMap {
    /// The 1×1 exposure-map texture (the reference's `mExposureMap`): the exposure
    /// pass's render target, the tone mapper's sampled source, and the source the
    /// per-frame copy into [`ExposureMap::last`] reads.
    pub(crate) current: Texture,
    /// The 1×1 exposure-map texture view (render target for this pass, sampled
    /// source for the tone mapper).
    pub(crate) view: TextureView,
    /// The previous frame's exposure (the reference's `mLastExposure`): the exposure
    /// pass copies [`ExposureMap::current`] into this at the start of the frame, then
    /// the shader eases the new target toward it.
    pub(crate) last: Texture,
    /// The 1×1 view of [`ExposureMap::last`], sampled by the ease.
    pub(crate) last_view: TextureView,
    /// A clamping sampler for reading the 1×1 maps.
    pub(crate) sampler: Sampler,
}

/// The exposure pipeline's global data (bind-group layout descriptor, sampler, and
/// the fullscreen vertex shader).
#[derive(Resource)]
struct SlExposurePipeline {
    /// The bind-group layout descriptor (scene texture, sampler, settings uniform).
    layout: BindGroupLayoutDescriptor,
    /// The sampler used to read the scene colour texture.
    sampler: Sampler,
    /// The shared fullscreen-triangle vertex shader, needed by pipeline
    /// specialization (which has no world access to fetch it).
    fullscreen_shader: FullscreenShader,
}

/// The 1×1 `Rgba16Float` texel `(1, 1, 1, 1)`, little-endian (`0x3C00` is half-float
/// `1.0`): the initial exposure the maps are seeded to, so the first frame's copy and
/// ease start from unity (the reference clears `mExposureMap` to `1` for the same
/// reason) instead of a black ramp-in.
const ONE_TEXEL: [u8; 8] = [0x00, 0x3C, 0x00, 0x3C, 0x00, 0x3C, 0x00, 0x3C];

/// Build the exposure pipeline's shared data and the two 1×1 [`ExposureMap`] textures
/// once, in the render world, seeding both to `1.0`.
fn init_exposure_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "sl_exposure_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The (linear, HDR) composited scene colour texture.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its sampler.
                sampler(SamplerBindingType::Filtering),
                // The dynamic-exposure settings (dynamic-offset uniform).
                uniform_buffer::<SlExposure>(true),
                // The previous frame's exposure (`mLastExposure`), for the ease.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its sampler.
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());

    let extent = Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    // The current exposure map: rendered into, sampled by the tone mapper, and the
    // copy source for next frame's `last` — so it needs the attachment, sampling, and
    // both copy usages. It is also seeded once via `write_texture` (COPY_DST).
    let current = render_device.create_texture(&TextureDescriptor {
        label: Some("sl_exposure_map"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: EXPOSURE_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // The previous frame's exposure: sampled by the ease and written by the per-frame
    // copy from `current`, so it needs sampling and COPY_DST.
    let last = render_device.create_texture(&TextureDescriptor {
        label: Some("sl_last_exposure_map"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: EXPOSURE_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let layout_1x1 = TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(8),
        rows_per_image: Some(1),
    };
    for texture in [&current, &last] {
        render_queue.write_texture(
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &ONE_TEXEL,
            layout_1x1,
            extent,
        );
    }
    let view = current.create_view(&TextureViewDescriptor::default());
    let last_view = last.create_view(&TextureViewDescriptor::default());
    let map_sampler = render_device.create_sampler(&SamplerDescriptor::default());

    commands.insert_resource(SlExposurePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
    commands.insert_resource(ExposureMap {
        current,
        view,
        last,
        last_view,
        sampler: map_sampler,
    });
}

impl SpecializedRenderPipeline for SlExposurePipeline {
    // The output is always the fixed 1×1 exposure-map format, so there is nothing
    // to specialize on.
    type Key = ();

    fn specialize(&self, (): Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("sl_exposure_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: EXPOSURE_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: EXPOSURE_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// The specialized pipeline id (constant key, so one id).
#[derive(Resource)]
struct SlExposurePipelineId(CachedRenderPipelineId);

/// Specialize the exposure pipeline once (its output format is fixed).
fn prepare_exposure_pipeline(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<SlExposurePipeline>>,
    pipeline: Res<SlExposurePipeline>,
) {
    let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, ());
    commands.insert_resource(SlExposurePipelineId(pipeline_id));
}

/// The exposure pass: copy last frame's exposure into [`ExposureMap::last`],
/// grid-sample the composited scene's average luminance, apply the `exposureF` curve,
/// ease toward the previous exposure, and write the 1×1 exposure map.
///
/// Runs only on views carrying an [`SlExposure`] (the main camera), so the
/// reflection-probe capture cameras keep rendering the linear radiance their
/// cubemaps hold, untouched by exposure.
fn sl_exposure_system(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<SlExposure>)>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<SlExposurePipeline>,
    pipeline_id: Res<SlExposurePipelineId>,
    exposure_map: Res<ExposureMap>,
    uniforms: Res<ComponentUniforms<SlExposure>>,
    mut ctx: RenderContext,
) {
    let (view_target, exposure_index) = view.into_inner();

    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let Some(uniform_binding) = uniforms.uniforms().binding() else {
        return;
    };

    // Copy last frame's result (`current` still holds it — the render pass below
    // overwrites it) into `last`, which the ease then reads (the reference's
    // `mExposureMap` → `mLastExposure` copy). Encoded before the render pass on the
    // same command encoder, so it runs first on the GPU.
    ctx.command_encoder().copy_texture_to_texture(
        TexelCopyTextureInfo {
            texture: &exposure_map.current,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyTextureInfo {
            texture: &exposure_map.last,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );

    let bind_group = ctx.render_device().create_bind_group(
        "sl_exposure_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            // Read the current composited scene (not a ping-pong write — this pass
            // writes the separate 1×1 map, leaving the scene untouched for the tone
            // mapper).
            view_target.main_texture_view(),
            &pipeline_res.sampler,
            uniform_binding.clone(),
            // The previous frame's exposure, for the ease.
            &exposure_map.last_view,
            &exposure_map.sampler,
        )),
    );

    let pass_descriptor = RenderPassDescriptor {
        label: Some("sl_exposure_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &exposure_map.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut render_pass = ctx.begin_tracked_render_pass(pass_descriptor);
    render_pass.set_render_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[exposure_index.index()]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        DEFAULT_EXPOSURE_COEFFICIENT, DEFAULT_SPEED_ERROR, DEFAULT_SPEED_TARGET, ExposureSettings,
        SkyExposureInputs, exposure_range,
    };

    /// A sky frame with `can_auto_adjust` derived the way `drive_sky` derives it
    /// (`reflection_probe_ambiance == 0`).
    fn sky(reflection_probe_ambiance: f32, gamma: f32) -> SkyExposureInputs {
        SkyExposureInputs {
            reflection_probe_ambiance,
            gamma,
            can_auto_adjust: reflection_probe_ambiance == 0.0,
        }
    }

    /// The shipped default settings: dynamic exposure on, sky-settings and
    /// auto-adjust-legacy off.
    fn default_settings() -> ExposureSettings {
        ExposureSettings {
            dynamic_enabled: true,
            use_sky_settings: false,
            sky_auto_adjust_legacy: false,
        }
    }

    /// The reference `exposureF.glsl` curve
    /// `s = mix(exp_max, exp_min, pow(clamp(L / coeff, 0, 1), 2))`, the CPU mirror of
    /// the expression `exposure.wgsl` evaluates on the reduced average luminance.
    /// Test-only: at runtime the GPU pass computes it, so it lives here to pin the
    /// shader's arithmetic rather than as a compiled-but-unused function.
    fn exposure_scale(average_luminance: f32, exp_min: f32, exp_max: f32, coeff: f32) -> f32 {
        let clamped = average_luminance.clamp(0.0, coeff);
        let normalised = if coeff > 0.0 { clamped / coeff } else { 1.0 };
        let shaped = normalised * normalised;
        exp_max * (1.0 - shaped) + exp_min * shaped
    }

    /// The reference `gExposureProgram` temporal ease
    /// `s = mix(prev, target, 1 - exp(-speed * dt))` with
    /// `speed = -ln(speed_error) / speed_target`, the CPU mirror of the ease
    /// `exposure.wgsl` runs. Test-only, for the same reason as [`exposure_scale`].
    fn eased(prev: f32, target: f32, dt: f32, speed_error: f32, speed_target: f32) -> f32 {
        let speed = -speed_error.ln() / speed_target;
        let alpha = 1.0 - (-speed * dt).exp();
        prev * (1.0 - alpha) + target * alpha
    }

    /// A legacy sky (`reflection_probe_ambiance == 0`) must give a flat `(1, 1)`
    /// range, so the dynamic exposure is inert on the shipped default sky — the
    /// property the module docs call out (exposure does not touch a legacy frame).
    #[test]
    fn legacy_sky_range_is_flat() {
        assert_eq!(
            exposure_range(sky(0.0, 1.0), default_settings()),
            (1.0, 1.0)
        );
        assert_eq!(
            exposure_range(sky(0.0, 2.5), default_settings()),
            (1.0, 1.0)
        );
    }

    /// An EEP probe-ambiance sky uses `[1 / hdr_scale, hdr_scale]` with
    /// `hdr_scale = sqrt(gamma) * 2`, matching `SkySettings::sky_hdr_scale`.
    #[test]
    fn eep_sky_range_follows_gamma() {
        let (lo, hi) = exposure_range(sky(0.5, 1.0), default_settings());
        assert!((lo - 0.5).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 2.0).abs() < 1e-6, "exp_max {hi}");
        let (lo, hi) = exposure_range(sky(0.5, 4.0), default_settings());
        assert!((lo - 0.25).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 4.0).abs() < 1e-6, "exp_max {hi}");
    }

    /// A gamma so low that `sqrt(gamma) * 2 <= 1` leaves the range flat — the
    /// reference's `if (hdr_scale > 1.f)` guard, so the exposure never *raises* a
    /// dim EEP frame past unity.
    #[test]
    fn eep_sky_with_tiny_gamma_stays_flat() {
        assert_eq!(
            exposure_range(sky(0.5, 0.16), default_settings()),
            (1.0, 1.0)
        );
    }

    /// `RenderDynamicExposureEnabled = false` collapses the shipped path to `(1, 1)`
    /// even for an HDR EEP sky — the reference's `else if (dynamic_exposure_enabled)`
    /// guard.
    #[test]
    fn disabled_dynamic_exposure_is_flat() {
        let settings = ExposureSettings {
            dynamic_enabled: false,
            ..default_settings()
        };
        assert_eq!(exposure_range(sky(0.5, 4.0), settings), (1.0, 1.0));
    }

    /// `RenderSkyAutoAdjustLegacy` lifts a legacy sky to the auto-adjust probe
    /// ambiance, so it enters the HDR path and adapts like an EEP sky — the reference
    /// `getReflectionProbeAmbiance(should_auto_adjust)` branch.
    #[test]
    fn auto_adjust_legacy_makes_a_legacy_sky_adapt() {
        // Off: a legacy sky is inert.
        assert_eq!(
            exposure_range(sky(0.0, 4.0), default_settings()),
            (1.0, 1.0)
        );
        // On: the same legacy sky picks up `hdr_scale = sqrt(gamma) * 2 = 4`.
        let settings = ExposureSettings {
            sky_auto_adjust_legacy: true,
            ..default_settings()
        };
        let (lo, hi) = exposure_range(sky(0.0, 4.0), settings);
        assert!((lo - 0.25).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 4.0).abs() < 1e-6, "exp_max {hi}");
    }

    /// `RenderUseExposureSkySettings` sources the range from the fixed HDR constants:
    /// an EEP sky (or any sky that can't legacy-auto-adjust away its half-widths)
    /// gets `(offset - min, offset + max) = (0.5, 3.0)`.
    #[test]
    fn use_sky_settings_uses_the_hdr_constants() {
        let settings = ExposureSettings {
            dynamic_enabled: true,
            use_sky_settings: true,
            sky_auto_adjust_legacy: false,
        };
        let (lo, hi) = exposure_range(sky(0.5, 1.0), settings);
        assert!((lo - 0.5).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 3.0).abs() < 1e-6, "exp_max {hi}");
    }

    /// Under `RenderUseExposureSkySettings`, a legacy sky with auto-adjust off zeroes
    /// its HDR half-widths (`getHDRMin`/`getHDRMax` return `0`), so the range is the
    /// inert `(offset, offset) = (1, 1)`; turning auto-adjust on restores `(0.5, 3)`.
    #[test]
    fn use_sky_settings_legacy_needs_auto_adjust() {
        let base = ExposureSettings {
            dynamic_enabled: true,
            use_sky_settings: true,
            sky_auto_adjust_legacy: false,
        };
        assert_eq!(exposure_range(sky(0.0, 1.0), base), (1.0, 1.0));
        let adjusted = ExposureSettings {
            sky_auto_adjust_legacy: true,
            ..base
        };
        let (lo, hi) = exposure_range(sky(0.0, 1.0), adjusted);
        assert!((lo - 0.5).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 3.0).abs() < 1e-6, "exp_max {hi}");
    }

    /// `RenderUseExposureSkySettings` with the dynamic exposure disabled gives the
    /// flat `(offset, offset)` — the reference's `else` inside the sky-settings branch.
    #[test]
    fn use_sky_settings_disabled_is_flat_at_offset() {
        let settings = ExposureSettings {
            dynamic_enabled: false,
            use_sky_settings: true,
            sky_auto_adjust_legacy: false,
        };
        assert_eq!(exposure_range(sky(0.5, 1.0), settings), (1.0, 1.0));
    }

    /// The curve returns `exp_max` for a black frame and `exp_min` at/above the
    /// coefficient, ramping quadratically between — the `exposureF.glsl` mapping.
    #[test]
    fn curve_maps_dark_to_max_and_bright_to_min() {
        let coeff = DEFAULT_EXPOSURE_COEFFICIENT;
        assert!((exposure_scale(0.0, 0.5, 2.0, coeff) - 2.0).abs() < 1e-6);
        assert!((exposure_scale(coeff, 0.5, 2.0, coeff) - 0.5).abs() < 1e-6);
        assert!((exposure_scale(coeff * 4.0, 0.5, 2.0, coeff) - 0.5).abs() < 1e-6);
        let mid = exposure_scale(coeff * 0.5, 0.5, 2.0, coeff);
        let expected = 2.0 * 0.75 + 0.5 * 0.25;
        assert!(
            (mid - expected).abs() < 1e-6,
            "mid {mid} expected {expected}"
        );
    }

    /// A flat `(1, 1)` range makes the curve return `1.0` for every luminance — the
    /// legacy no-op, verified end-to-end through both pure functions.
    #[test]
    fn flat_range_is_a_no_op_at_any_luminance() {
        let (lo, hi) = exposure_range(sky(0.0, 1.0), default_settings());
        for &l in &[0.0_f32, 0.05, 0.175, 0.5, 4.0] {
            let s = exposure_scale(l, lo, hi, DEFAULT_EXPOSURE_COEFFICIENT);
            assert!((s - 1.0).abs() < 1e-6, "luminance {l} gave {s}");
        }
    }

    /// A zero `dt` (the first frame) leaves the eased exposure at the previous value,
    /// and the ease always moves toward — never past — the target.
    #[test]
    fn ease_is_a_no_op_at_zero_dt() {
        let s = eased(1.0, 3.0, 0.0, DEFAULT_SPEED_ERROR, DEFAULT_SPEED_TARGET);
        assert!((s - 1.0).abs() < 1e-6, "zero dt gave {s}");
    }

    /// After exactly `speed_target` seconds the remaining error is `speed_error` of
    /// the original — the property the `speed = -ln(error)/target` formula encodes,
    /// so `RenderDynamicExposureSpeedTarget` really is the time constant.
    #[test]
    fn ease_reaches_speed_error_after_the_target_time() {
        let (prev, target) = (1.0_f32, 3.0_f32);
        let s = eased(
            prev,
            target,
            DEFAULT_SPEED_TARGET,
            DEFAULT_SPEED_ERROR,
            DEFAULT_SPEED_TARGET,
        );
        let remaining_error = (target - s) / (target - prev);
        assert!(
            (remaining_error - DEFAULT_SPEED_ERROR).abs() < 1e-6,
            "remaining error {remaining_error}"
        );
    }

    /// The ease converges monotonically toward the target over successive frames and
    /// never overshoots — the reason a camera turn glides rather than flashing.
    #[test]
    fn ease_converges_monotonically_toward_the_target() {
        let target = 0.25_f32;
        let mut exposure = 1.0_f32;
        let mut previous_gap = (target - exposure).abs();
        for _ in 0..600 {
            exposure = eased(
                exposure,
                target,
                1.0 / 60.0,
                DEFAULT_SPEED_ERROR,
                DEFAULT_SPEED_TARGET,
            );
            let gap = (target - exposure).abs();
            assert!(
                gap <= previous_gap + 1e-7,
                "gap grew: {gap} > {previous_gap}"
            );
            previous_gap = gap;
        }
        assert!(
            (exposure - target).abs() < 1e-3,
            "did not converge: {exposure}"
        );
    }
}
