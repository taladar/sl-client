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
//! History smoothing (`gExposureProgram`'s `USE_LAST_EXPOSURE` fade toward the
//! previous frame's exposure) is **not** ported: this is the reference's no-fade
//! path (`gExposureProgramNoFade`), an instantaneous exposure. The static
//! `RenderExposure` scale stays on [`SlTonemap`](crate::tonemap::SlTonemap); this
//! module supplies only the dynamic factor it is multiplied by.

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
    ColorTargetState, ColorWrites, Extent3d, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
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

/// The environment variable force-disabling the dynamic exposure (an A/B knob: pins
/// the scale to `1.0` so a capture can tell the dynamic exposure from the static
/// `RenderExposure`).
const ENV_DISABLE: &str = "SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE";
/// The environment variable overriding the exposure coefficient (`max_L`).
const ENV_COEFFICIENT: &str = "SL_VIEWER_EXPOSURE_COEFFICIENT";

/// The reference `generateExposure` exposure floor (`exp_min`): the scale a bright
/// frame is pulled down to (`1 / hdr_scale` for an EEP sky, `1` legacy).
const DEFAULT_EXPOSURE_COEFFICIENT: f32 = 0.175;

/// The reference `generateExposure` exposure range (`exp_min`, `exp_max`) for one
/// sky frame, on the shipped `RenderUseExposureSkySettings = false` /
/// `RenderDynamicExposureEnabled = true` path (`pipeline.cpp`).
///
/// A legacy / classic-mode sky (`reflection_probe_ambiance == 0`) yields
/// `(1.0, 1.0)` — a flat range, so the exposure curve returns `1.0` for any
/// luminance and the dynamic exposure is inert. An EEP sky with
/// `reflection_probe_ambiance > 0` computes `hdr_scale = sqrt(gamma) * 2`, and when
/// that exceeds `1.0` (as it does for any usable ambiance) the range is
/// `(1 / hdr_scale, hdr_scale)` — the counterweight to the WL sky's `sky_hdr_scale`
/// up-scale. When `hdr_scale <= 1.0` the reference leaves the range at `(1.0, 1.0)`.
///
/// `reflection_probe_ambiance` and `gamma` are the frame's own decoded values (see
/// [`SkySettings::sky_hdr_scale`](sl_proto::SkySettings::sky_hdr_scale), which
/// mirrors the same `sqrt(gamma) * 2` branch).
#[must_use]
pub(crate) fn exposure_range(reflection_probe_ambiance: f32, gamma: f32) -> (f32, f32) {
    if reflection_probe_ambiance <= 0.0 {
        return (1.0, 1.0);
    }
    let hdr_scale = gamma.max(0.0).sqrt() * 2.0;
    if hdr_scale > 1.0 {
        (1.0 / hdr_scale, hdr_scale)
    } else {
        (1.0, 1.0)
    }
}

/// The active sky frame's exposure range, republished each frame by
/// [`drive_sky`](crate::sky::drive_sky) from the rendered [`SkySettings`] so the
/// exposure pass tracks the exact sky the sky dome is drawn from (rather than
/// re-deriving the altitude-blended frame). `(1.0, 1.0)` until a sky is resolved.
#[derive(Resource, Clone, Copy)]
pub(crate) struct ExposureRange {
    /// The `exp_min` floor from [`exposure_range`].
    pub(crate) exp_min: f32,
    /// The `exp_max` ceiling from [`exposure_range`].
    pub(crate) exp_max: f32,
}

impl Default for ExposureRange {
    /// The legacy no-op range until a sky frame publishes one.
    fn default() -> Self {
        Self {
            exp_min: 1.0,
            exp_max: 1.0,
        }
    }
}

/// The per-frame dynamic-exposure inputs, carried on the main camera (which both
/// carries them to the GPU as a uniform and selects the view the pass runs on, so
/// the reflection-probe capture cameras — which must stay linear — are left alone),
/// matching `exposure.wgsl`'s `SlExposure`.
#[derive(Component, Clone, Copy, ShaderType)]
pub(crate) struct SlExposure {
    /// The exposure floor (`exp_min`), from the active sky's [`ExposureRange`].
    pub(crate) exp_min: f32,
    /// The exposure ceiling (`exp_max`).
    pub(crate) exp_max: f32,
    /// The reference `RenderDynamicExposureCoefficient` (`exposureF.glsl`'s
    /// `max_L`). Overridable by `SL_VIEWER_EXPOSURE_COEFFICIENT`.
    pub(crate) coefficient: f32,
    /// `1.0` to run the dynamic exposure, `0.0` to pin the scale to `1.0`.
    pub(crate) enabled: f32,
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
}

/// Fold the active sky's [`ExposureRange`] and the stored / overridden settings into
/// each camera's live [`SlExposure`] each frame (cheap reads). An environment
/// override (`SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE` / `SL_VIEWER_EXPOSURE_COEFFICIENT`),
/// used by the screenshot harness, **wins** over the stored value so a capture is
/// reproducible.
pub(crate) fn refresh_exposure(
    store: Res<ViewerSettings>,
    range: Res<ExposureRange>,
    mut cameras: Query<&mut SlExposure>,
) {
    let store = store.store();
    let disabled_by_env = std::env::var_os(ENV_DISABLE).is_some();
    for mut exposure in &mut cameras {
        exposure.exp_min = range.exp_min;
        exposure.exp_max = range.exp_max;
        if std::env::var_os(ENV_COEFFICIENT).is_none()
            && let Ok(value) = store.get_f32(SETTING_COEFFICIENT)
        {
            exposure.coefficient = value;
        }
        let enabled = if disabled_by_env {
            false
        } else {
            store
                .get_bool(SETTING_ENABLED)
                .unwrap_or(DEFAULT_EXPOSURE_ENABLED)
        };
        exposure.enabled = f32::from(u8::from(enabled));
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
/// sampler the tone mapper reads it through. A render-world resource so both passes
/// reference the one texture within a frame.
#[derive(Resource)]
pub(crate) struct ExposureMap {
    /// The 1×1 exposure-map texture view (render target for this pass, sampled
    /// source for the tone mapper).
    pub(crate) view: TextureView,
    /// A clamping sampler for reading the 1×1 map.
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

/// Build the exposure pipeline's shared data and the 1×1 [`ExposureMap`] once, in
/// the render world.
fn init_exposure_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
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
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());

    // The shared 1×1 exposure map.
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some("sl_exposure_map"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: EXPOSURE_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let map_sampler = render_device.create_sampler(&SamplerDescriptor::default());

    commands.insert_resource(SlExposurePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
    commands.insert_resource(ExposureMap {
        view,
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

/// The exposure pass: grid-sample the composited scene's average luminance, apply
/// the `exposureF` curve, and write the 1×1 exposure map.
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

    use super::{DEFAULT_EXPOSURE_COEFFICIENT, exposure_range};

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

    /// A legacy sky (`reflection_probe_ambiance == 0`) must give a flat `(1, 1)`
    /// range, so the dynamic exposure is inert on the shipped default sky — the
    /// property the module docs call out (exposure does not touch a legacy frame).
    #[test]
    fn legacy_sky_range_is_flat() {
        assert_eq!(exposure_range(0.0, 1.0), (1.0, 1.0));
        assert_eq!(exposure_range(0.0, 2.5), (1.0, 1.0));
    }

    /// An EEP probe-ambiance sky uses `[1 / hdr_scale, hdr_scale]` with
    /// `hdr_scale = sqrt(gamma) * 2`, matching `SkySettings::sky_hdr_scale`.
    #[test]
    fn eep_sky_range_follows_gamma() {
        let (lo, hi) = exposure_range(0.5, 1.0);
        assert!((lo - 0.5).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 2.0).abs() < 1e-6, "exp_max {hi}");
        let (lo, hi) = exposure_range(0.5, 4.0);
        assert!((lo - 0.25).abs() < 1e-6, "exp_min {lo}");
        assert!((hi - 4.0).abs() < 1e-6, "exp_max {hi}");
    }

    /// A gamma so low that `sqrt(gamma) * 2 <= 1` leaves the range flat — the
    /// reference's `if (hdr_scale > 1.f)` guard, so the exposure never *raises* a
    /// dim EEP frame past unity.
    #[test]
    fn eep_sky_with_tiny_gamma_stays_flat() {
        assert_eq!(exposure_range(0.5, 0.16), (1.0, 1.0));
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
        let (lo, hi) = exposure_range(0.0, 1.0);
        for &l in &[0.0_f32, 0.05, 0.175, 0.5, 4.0] {
            let s = exposure_scale(l, lo, hi, DEFAULT_EXPOSURE_COEFFICIENT);
            assert!((s - 1.0).abs() < 1e-6, "luminance {l} gave {s}");
        }
    }
}
