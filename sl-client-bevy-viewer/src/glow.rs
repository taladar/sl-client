//! The Second Life / Firestorm **glow** pass (`LLPipeline::generateGlow` +
//! `combineGlow`): the faithful port of SL's `RenderGlow` pipeline, replacing the
//! screen-space Bevy [`Bloom`](bevy::post_process::bloom::Bloom) approximation this
//! viewer used before.
//!
//! **Why a port, not Bevy `Bloom`.** SL's glow is **not** luminance-driven: the one
//! real path (`generateGlow`) runs the extract at `minLuminance = 9999` (off), so
//! the glow is driven by the scene's **alpha channel** — the per-face **glow mask**
//! (the glow scalar a builder sets, plus fullbright / emissive). `glowExtract` draws
//! with `BT_ADD_WITH_ALPHA`, writing `scene_rgb · glow_mask` into a low-res buffer;
//! that buffer is blurred by a fixed separable Gaussian
//! (`RenderGlowIterations · 2` passes, the `[.25,.5,.8,1,1,.8,.5,.25]` kernel at
//! `glowDelta·[-3.5..3.5]`, `× RenderGlowStrength`); and `combineGlow` adds it back
//! (`scene + glow`). Bevy's `Bloom` is a luminance-threshold mip-chain — a different
//! algorithm whose one tuned strength never generalises across sky settings, which
//! is exactly the fidelity gap this replaces.
//!
//! **Ordering.** The reference runs glow in `renderFinalize` **after** `tonemap`
//! (tonemap → `generateGlow` → `combineGlow`), so the glow is built and added in
//! display space over the tone-mapped frame. This pass therefore orders after
//! [`SlTonemapPass`](crate::tonemap::SlTonemapPass), reading the tone-mapped scene ×
//! the alpha mask the materials write (which survives the fog / exposure / tonemap
//! passes, each of which passes alpha through).
//!
//! **Enabled by default.** Every surface now feeds the glow mask (opaque materials
//! write it to alpha; alpha-blended ones preserve it via
//! [`preserve_glow_mask_alpha`](sl_client_bevy::preserve_glow_mask_alpha)), so the
//! glow is on by default and the Bevy `Bloom` it replaced is gone.
//! `SL_VIEWER_DISABLE_GLOW=1` forces it off (an A/B knob); `SL_VIEWER_GLOW_STRENGTH`
//! / `_WIDTH` and the `RenderGlow*` settings tune it.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::ViewTarget;
use bevy::render::{GpuResourceAppExt as _, Render, RenderApp, RenderStartup, RenderSystems};

use sl_settings::SettingValue;

use crate::settings::ViewerSettings;
use crate::tonemap::SlTonemapPass;

/// The internal handle the glow extract shader (`glow_extract.wgsl`) loads under.
const EXTRACT_SHADER_HANDLE: Handle<Shader> = uuid_handle!("1d4a7f60-9b28-4c15-8e33-6a0f2d95c47e");
/// The internal handle the glow blur shader (`glow_blur.wgsl`) loads under.
const BLUR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("3e6c9b21-5f47-4a08-9d62-8b1e0a74f3d5");
/// The internal handle the glow combine shader (`glow_combine.wgsl`) loads under.
const COMBINE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7a2f4d18-6c93-4e57-8b04-1f9d3e620c8a");

/// The low-res glow-buffer format — floating-point so the tone-mapped scene's glow
/// (already ~0..1, but summed by the kernel above 1) is blurred without clipping.
const GLOW_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The glow-buffer edge (`1 << RenderGlowResolutionPow`, the shipped pow `9`). The
/// reference allocates `512 × glow_res`; a square buffer is used here.
const GLOW_RESOLUTION: u32 = 512;

/// The reference `RenderGlowStrength` default, applied every blur pass.
const DEFAULT_STRENGTH: f32 = 0.325;
/// The reference `RenderGlowWidth` default; `delta = width / GLOW_RESOLUTION`.
const DEFAULT_WIDTH: f32 = 1.3;
/// The reference `RenderGlowIterations` default; the blur runs `iterations · 2`
/// passes (alternating horizontal / vertical).
const DEFAULT_ITERATIONS: u32 = 2;

/// The env var that force-**disables** the glow pass (an A/B knob; the glow is on
/// by default now the alpha mask is fed on every surface).
const ENV_DISABLE: &str = "SL_VIEWER_DISABLE_GLOW";
/// The env var overriding the glow strength (`RenderGlowStrength`).
const ENV_STRENGTH: &str = "SL_VIEWER_GLOW_STRENGTH";
/// The env var overriding the glow width (`RenderGlowWidth`).
const ENV_WIDTH: &str = "SL_VIEWER_GLOW_WIDTH";

/// The persisted-file section the glow settings are grouped under (`[render.glow]`),
/// matching the reference's `RenderGlow*` naming.
const GLOW_SECTION: &[&str] = &["render", "glow"];
/// The reference `RenderGlow` setting name (the master enable).
pub(crate) const SETTING_ENABLED: &str = "RenderGlow";
/// The reference `RenderGlowStrength` setting name.
pub(crate) const SETTING_STRENGTH: &str = "RenderGlowStrength";
/// The reference `RenderGlowIterations` setting name.
pub(crate) const SETTING_ITERATIONS: &str = "RenderGlowIterations";
/// The reference `RenderGlowWidth` setting name.
pub(crate) const SETTING_WIDTH: &str = "RenderGlowWidth";

/// Register the glow settings on the store with the reference defaults, so a user's
/// Firestorm `RenderGlow*` port across and the (future) preferences UI has something
/// to bind to. Called from [`ViewerSettings`]'s `FromWorld`. (Replaces the settings
/// the removed Bevy-`Bloom` module registered.)
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        GLOW_SECTION,
        SETTING_ENABLED,
        SettingValue::Bool(true),
        "Render the Second Life glow / bloom pass",
    );
    settings.register_in(
        GLOW_SECTION,
        SETTING_STRENGTH,
        SettingValue::F32(DEFAULT_STRENGTH),
        "Additive strength of the glow, applied each blur pass",
    );
    settings.register_in(
        GLOW_SECTION,
        SETTING_ITERATIONS,
        SettingValue::U32(DEFAULT_ITERATIONS),
        "Number of separable-Gaussian blur iterations (each is two passes)",
    );
    settings.register_in(
        GLOW_SECTION,
        SETTING_WIDTH,
        SettingValue::F32(DEFAULT_WIDTH),
        "Blur width (the per-pass step is this over the glow-buffer resolution)",
    );
}

/// Refresh each camera's live [`SlGlow`] from the settings store each frame (cheap
/// reads), so a `RenderGlow*` changed in the (future) preferences UI takes effect at
/// once. An environment override (`SL_VIEWER_DISABLE_GLOW` / `SL_VIEWER_GLOW_*`),
/// used by the screenshot harness, **wins** over the stored value.
pub(crate) fn refresh_glow(store: Res<ViewerSettings>, mut cameras: Query<&mut SlGlow>) {
    let store = store.store();
    let disabled_by_env = std::env::var_os(ENV_DISABLE).is_some();
    for mut glow in &mut cameras {
        glow.enabled = if disabled_by_env {
            false
        } else {
            store.get_bool(SETTING_ENABLED).unwrap_or(true)
        };
        if std::env::var_os(ENV_STRENGTH).is_none()
            && let Ok(value) = store.get_f32(SETTING_STRENGTH)
        {
            glow.strength = value;
        }
        if std::env::var_os(ENV_WIDTH).is_none()
            && let Ok(value) = store.get_f32(SETTING_WIDTH)
        {
            glow.delta = value / GLOW_RESOLUTION_F32;
        }
        if let Ok(value) = store.get_u32(SETTING_ITERATIONS) {
            glow.iterations = value;
        }
    }
}

/// Read an `f32` knob from the environment, falling back to `default` when unset or
/// unparsable.
fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// The per-frame glow inputs carried on the main camera (which both carries them to
/// the render world and selects the view the pass runs on, so the reflection-probe
/// capture cameras — which stay linear — are left alone).
#[derive(Component, Clone, Copy)]
pub(crate) struct SlGlow {
    /// Whether the glow pass runs at all (off by default; `SL_VIEWER_ENABLE_GLOW`).
    pub(crate) enabled: bool,
    /// The reference `RenderGlowStrength`, applied every blur pass.
    pub(crate) strength: f32,
    /// The blur step magnitude `RenderGlowWidth / GLOW_RESOLUTION`.
    pub(crate) delta: f32,
    /// The reference `RenderGlowIterations`; the blur runs `iterations · 2` passes.
    pub(crate) iterations: u32,
}

impl Default for SlGlow {
    /// The reference `RenderGlow*` defaults, each overridable by an environment
    /// variable so a capture can sweep the glow without a rebuild.
    fn default() -> Self {
        Self {
            enabled: std::env::var_os(ENV_DISABLE).is_none(),
            strength: env_f32(ENV_STRENGTH, DEFAULT_STRENGTH),
            delta: env_f32(ENV_WIDTH, DEFAULT_WIDTH) / GLOW_RESOLUTION_F32,
            iterations: DEFAULT_ITERATIONS,
        }
    }
}

/// [`GLOW_RESOLUTION`] as `f32`, for the `delta` divide (no `as` casts allowed).
const GLOW_RESOLUTION_F32: f32 = 512.0;

impl SyncComponent for SlGlow {
    type Target = Self;
}

impl ExtractComponent for SlGlow {
    type QueryData = Read<Self>;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// The blur uniform (`glow_blur.wgsl`'s `GlowBlur`): the per-pass step and strength.
#[derive(Clone, Copy, Default, ShaderType)]
struct GlowBlur {
    /// `(delta, 0)` horizontal / `(0, delta)` vertical.
    delta: Vec2,
    /// `RenderGlowStrength`.
    strength: f32,
    /// std140 padding.
    padding: f32,
}

/// The plugin: loads the three glow shaders, registers extraction, and wires the
/// pass into the 3D render schedule after the tone mapper.
#[derive(Debug, Default)]
pub(crate) struct SlGlowPlugin;

impl Plugin for SlGlowPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            EXTRACT_SHADER_HANDLE,
            "glow_extract.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(app, BLUR_SHADER_HANDLE, "glow_blur.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            COMBINE_SHADER_HANDLE,
            "glow_combine.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(ExtractComponentPlugin::<SlGlow>::default())
            .add_systems(Update, refresh_glow);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<SlGlowPipeline>>()
            .add_systems(RenderStartup, init_glow_pipeline)
            .add_systems(Render, prepare_glow_pipeline.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                sl_glow_system
                    .in_set(Core3dSystems::PostProcess)
                    // After the tone mapper, whose display-space output the reference
                    // glow (`renderFinalize`) is built and composited over.
                    .after(SlTonemapPass),
            );
    }
}

/// The glow pipelines' global data: the three bind-group layouts (extract / blur /
/// combine), the sampler, and the fullscreen vertex shader.
#[derive(Resource)]
struct SlGlowPipeline {
    /// The extract layout (scene texture, sampler).
    extract_layout: BindGroupLayoutDescriptor,
    /// The blur layout (glow texture, sampler, blur uniform).
    blur_layout: BindGroupLayoutDescriptor,
    /// The combine layout (scene texture, sampler, glow texture, sampler).
    combine_layout: BindGroupLayoutDescriptor,
    /// The sampler used to read the scene / glow textures (clamped, filtering).
    sampler: Sampler,
    /// The shared fullscreen-triangle vertex shader.
    fullscreen_shader: FullscreenShader,
}

/// The two ping-pong glow buffers and the per-direction blur uniforms.
#[derive(Resource)]
struct SlGlowBuffers {
    /// The first glow buffer (`GLOW_RESOLUTION²`).
    view_a: TextureView,
    /// The second glow buffer (ping-pong target).
    view_b: TextureView,
    /// The horizontal blur uniform.
    horizontal: UniformBuffer<GlowBlur>,
    /// The vertical blur uniform.
    vertical: UniformBuffer<GlowBlur>,
}

/// Which glow entry point a pipeline runs — its shader handle and label.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GlowStage {
    /// The extract pass (`glow_extract.wgsl`).
    Extract,
    /// The blur pass (`glow_blur.wgsl`).
    Blur,
    /// The combine pass (`glow_combine.wgsl`).
    Combine,
}

/// Build the glow pipelines' shared data and the two glow buffers once, in the
/// render world.
fn init_glow_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let extract_layout = BindGroupLayoutDescriptor::new(
        "sl_glow_extract_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let blur_layout = BindGroupLayoutDescriptor::new(
        "sl_glow_blur_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<GlowBlur>(false),
            ),
        ),
    );
    let combine_layout = BindGroupLayoutDescriptor::new(
        "sl_glow_combine_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    // **Linear** filtering (the reference samples the glow buffer trilinearly): the
    // glow buffer is a low-res 512² downsample of the scene, so the extract
    // downsample, the sub-texel blur taps, and the upsample back to full screen must
    // interpolate — a `Nearest` sampler (wgpu's `SamplerDescriptor::default`) point-
    // samples it and the bloom shows the buffer's texels as blocky pixels. Clamp at
    // the edges so the blur does not wrap.
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("sl_glow_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        // The glow buffers have no mip chain, so `mipmap_filter` is left at default.
        ..default()
    });

    let view_a = glow_buffer_view(&render_device, "sl_glow_buffer_a");
    let view_b = glow_buffer_view(&render_device, "sl_glow_buffer_b");

    commands.insert_resource(SlGlowPipeline {
        extract_layout,
        blur_layout,
        combine_layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
    commands.insert_resource(SlGlowBuffers {
        view_a,
        view_b,
        horizontal: UniformBuffer::default(),
        vertical: UniformBuffer::default(),
    });
}

/// Create one `GLOW_RESOLUTION²` glow buffer and return its default view.
fn glow_buffer_view(render_device: &RenderDevice, label: &'static str) -> TextureView {
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: GLOW_RESOLUTION,
            height: GLOW_RESOLUTION,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: GLOW_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&TextureViewDescriptor::default())
}

impl SpecializedRenderPipeline for SlGlowPipeline {
    // The three glow stages, each a fixed-format output; specialize on which stage.
    type Key = GlowStage;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let (label, shader, layout) = match key {
            GlowStage::Extract => (
                "sl_glow_extract_pipeline",
                EXTRACT_SHADER_HANDLE,
                self.extract_layout.clone(),
            ),
            GlowStage::Blur => (
                "sl_glow_blur_pipeline",
                BLUR_SHADER_HANDLE,
                self.blur_layout.clone(),
            ),
            GlowStage::Combine => (
                "sl_glow_combine_pipeline",
                COMBINE_SHADER_HANDLE,
                self.combine_layout.clone(),
            ),
        };
        RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader,
                targets: vec![Some(ColorTargetState {
                    format: GLOW_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// The three specialized glow pipeline ids.
#[derive(Resource)]
struct SlGlowPipelineIds {
    /// The extract pipeline.
    extract: CachedRenderPipelineId,
    /// The blur pipeline.
    blur: CachedRenderPipelineId,
    /// The combine pipeline (writes the composited scene, so its target format is
    /// the view format, not `GLOW_FORMAT`).
    combine: CachedRenderPipelineId,
}

/// Specialize the three glow pipelines once (their formats are fixed).
fn prepare_glow_pipeline(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<SlGlowPipeline>>,
    pipeline: Res<SlGlowPipeline>,
) {
    let extract = pipelines.specialize(&pipeline_cache, &pipeline, GlowStage::Extract);
    let blur = pipelines.specialize(&pipeline_cache, &pipeline, GlowStage::Blur);
    let combine = pipelines.specialize(&pipeline_cache, &pipeline, GlowStage::Combine);
    commands.insert_resource(SlGlowPipelineIds {
        extract,
        blur,
        combine,
    });
}

/// The glow pass: extract the glow mask, blur it with the reference separable
/// Gaussian, and additively combine it back over the tone-mapped scene.
///
/// Runs only on the view carrying an enabled [`SlGlow`] (the main camera); disabled
/// by default while the materials are migrated to write the alpha glow mask.
fn sl_glow_system(
    view: ViewQuery<(&ViewTarget, &SlGlow)>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<SlGlowPipeline>,
    pipeline_ids: Res<SlGlowPipelineIds>,
    mut buffers: ResMut<SlGlowBuffers>,
    render_queue: Res<RenderQueue>,
    mut ctx: RenderContext,
) {
    let (view_target, glow) = view.into_inner();
    if !glow.enabled {
        return;
    }

    let (Some(extract_pipeline), Some(blur_pipeline), Some(combine_pipeline)) = (
        pipeline_cache.get_render_pipeline(pipeline_ids.extract),
        pipeline_cache.get_render_pipeline(pipeline_ids.blur),
        pipeline_cache.get_render_pipeline(pipeline_ids.combine),
    ) else {
        return;
    };

    // Upload the per-direction blur uniforms for this frame's strength / width.
    buffers.horizontal.set(GlowBlur {
        delta: Vec2::new(glow.delta, 0.0),
        strength: glow.strength,
        padding: 0.0,
    });
    buffers.vertical.set(GlowBlur {
        delta: Vec2::new(0.0, glow.delta),
        strength: glow.strength,
        padding: 0.0,
    });
    buffers
        .horizontal
        .write_buffer(ctx.render_device(), &render_queue);
    buffers
        .vertical
        .write_buffer(ctx.render_device(), &render_queue);
    let (Some(h_binding), Some(v_binding)) =
        (buffers.horizontal.binding(), buffers.vertical.binding())
    else {
        return;
    };

    let extract_layout = pipeline_cache.get_bind_group_layout(&pipeline_res.extract_layout);
    let blur_layout = pipeline_cache.get_bind_group_layout(&pipeline_res.blur_layout);
    let combine_layout = pipeline_cache.get_bind_group_layout(&pipeline_res.combine_layout);

    // 1) Extract: scene (tone-mapped) × glow mask → buffer A.
    {
        let bind_group = ctx.render_device().create_bind_group(
            "sl_glow_extract_bind_group",
            &extract_layout,
            &BindGroupEntries::sequential((view_target.main_texture_view(), &pipeline_res.sampler)),
        );
        run_fullscreen_to(
            &mut ctx,
            "sl_glow_extract",
            &buffers.view_a,
            extract_pipeline,
            &bind_group,
        );
    }

    // 2) Blur: `iterations · 2` separable passes, ping-ponging A↔B, alternating
    //    horizontal then vertical. After an even number of passes the result is
    //    back in buffer A.
    let passes = glow.iterations.saturating_mul(2);
    for pass in 0..passes {
        let horizontal = pass % 2 == 0;
        let (source, target) = if horizontal {
            (&buffers.view_a, &buffers.view_b)
        } else {
            (&buffers.view_b, &buffers.view_a)
        };
        let uniform = if horizontal {
            h_binding.clone()
        } else {
            v_binding.clone()
        };
        let bind_group = ctx.render_device().create_bind_group(
            "sl_glow_blur_bind_group",
            &blur_layout,
            &BindGroupEntries::sequential((source, &pipeline_res.sampler, uniform)),
        );
        run_fullscreen_to(&mut ctx, "sl_glow_blur", target, blur_pipeline, &bind_group);
    }
    // Buffer holding the final blur: A if `passes` is even (always, since it is
    // `iterations · 2`), else B.
    let blurred = if passes % 2 == 0 {
        &buffers.view_a
    } else {
        &buffers.view_b
    };

    // 3) Combine: scene + blurred glow → the view target (ping-pong write).
    {
        let post_process = view_target.post_process_write();
        let bind_group = ctx.render_device().create_bind_group(
            "sl_glow_combine_bind_group",
            &combine_layout,
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline_res.sampler,
                blurred,
                &pipeline_res.sampler,
            )),
        );
        run_fullscreen_to(
            &mut ctx,
            "sl_glow_combine",
            post_process.destination,
            combine_pipeline,
            &bind_group,
        );
    }
}

/// Run a fullscreen-triangle pass writing `target`. Each glow pass writes every
/// pixel of its target (a fullscreen triangle), so the default `Load` op needs no
/// prior clear.
fn run_fullscreen_to(
    ctx: &mut RenderContext,
    label: &'static str,
    target: &TextureView,
    pipeline: &bevy::render::render_resource::RenderPipeline,
    bind_group: &bevy::render::render_resource::BindGroup,
) {
    let pass_descriptor = RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
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
    render_pass.set_bind_group(0, bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}
