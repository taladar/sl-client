//! The Second Life / Firestorm tone mapper (P33.3): the single transfer that turns
//! the viewer's linear HDR scene into displayable colour, replacing Bevy's built-in
//! tonemapping so content authored for the reference viewer reads the way its
//! authors saw it.
//!
//! **Why the viewer needs its own.** The reference tonemaps *once*, over the whole
//! composited frame (`postDeferredTonemap.glsl` → `tonemapUtilF.glsl`'s `toneMap`):
//! multiply by the exposure (`RenderExposure`), run the chosen curve
//! (`RenderTonemapType` — 0 = Khronos PBR Neutral, 1 = the ACES Hill fit, the
//! default), blend the curve back toward the merely-exposed linear colour by
//! `RenderTonemapMix` (0.7 — the curve is deliberately not applied at full
//! strength), and clamp. Bevy offers a fixed menu of curves (`TonyMcMapface` by
//! default) with no mix and no Khronos Neutral, so a faithful port has to supply the
//! curve itself. [`SlTonemap`] carries the three reference settings; the pass is a
//! fullscreen post-process modelled on [`underwater_fog`](crate::underwater_fog),
//! and Bevy's own tonemapping is switched off on the camera (`Tonemapping::None`).
//!
//! **Why P33.3 (probe calibration) is what brought it in.** Bevy tonemaps *in the
//! mesh shader* when the view target is LDR, which is what the viewer's camera used
//! to be. That left the viewer with two different transfers: `StandardMaterial`
//! prims / meshes / avatars were tonemapped, while the custom sky / terrain / water
//! materials — which write display-space colour and never call Bevy's tonemapper —
//! were merely *clipped* at 1.0 by the 8-bit target. The reflection probes' capture
//! cameras, though, are HDR and un-tonemapped, so a probe's cubemap held the sky at
//! its true radiance (the sky shader ends in the reference's `clamp(color, 0, 5)`)
//! while the eye saw that same sky clipped to 1.0. The probe's image-based lighting
//! was therefore several times brighter than the surroundings it was supposed to
//! reproduce — an over-bright, sky-blue ambient on the terrain that no constant
//! `intensity` could correct, which is exactly why P33.1's hand-tuned intensity felt
//! arbitrary. Giving the camera an HDR target and one honest tone mapper at the end
//! puts every material in the same linear space the probes capture, and the probe
//! intensity then follows from the exposure alone ([`probes`](crate::probes)).
//!
//! Not ported: the reference's *automatic* exposure (its `exposureMap`, a
//! luminance-driven `exp_scale` multiplying `RenderExposure`). The exposure here is
//! the static setting alone.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{GpuResourceAppExt as _, Render, RenderApp, RenderStartup, RenderSystems};

use crate::underwater_fog::UnderwaterFogPass;

/// The internal handle the tone-map shader (`tonemap.wgsl`) is loaded under.
const TONEMAP_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b1f0c94-3a27-4d58-9c11-70b4e8d5a213");

/// The reference `RenderTonemapType` value selecting the Khronos PBR Neutral curve.
const TONEMAP_KHRONOS_NEUTRAL: u32 = 0;
/// The reference `RenderTonemapType` value selecting the ACES (Hill) curve — the
/// reference's default, and so this viewer's.
const TONEMAP_ACES: u32 = 1;
/// Not a reference value: no tone curve at all (exposure and clamp only, the
/// reference's `NO_POST` path), so a capture can A/B what the curve is doing.
const TONEMAP_NONE: u32 = 2;

/// The reference `RenderTonemapMix` default: how far the tone curve is blended in
/// over the merely-exposed linear colour.
const DEFAULT_TONEMAP_MIX: f32 = 0.7;

/// The reference `RenderExposure` default: a plain scale on the linear scene colour
/// ahead of the curve.
const DEFAULT_EXPOSURE: f32 = 1.0;

/// The tone-mapper settings, mirroring the reference's three `Render*` settings.
/// Sits on the camera — which both carries them to the GPU as a uniform and *selects*
/// the view the pass runs on, so the reflection probes' capture cameras (which must
/// stay linear, being the source of image-based lighting) are left alone.
#[derive(Component, Clone, Copy, ShaderType)]
pub(crate) struct SlTonemap {
    /// The reference `RenderExposure`: scales the linear scene colour before the
    /// curve. Overridable by `SL_VIEWER_EXPOSURE`.
    pub(crate) exposure: f32,
    /// The reference `RenderTonemapMix`: blends the exposed linear colour toward the
    /// tone-mapped one. Overridable by `SL_VIEWER_TONEMAP_MIX`.
    pub(crate) tonemap_mix: f32,
    /// The reference `RenderTonemapType`: which curve to run (see
    /// [`TONEMAP_KHRONOS_NEUTRAL`] / [`TONEMAP_ACES`] / [`TONEMAP_NONE`]).
    /// Overridable by `SL_VIEWER_TONEMAP`.
    pub(crate) tonemap_type: u32,
    /// std140 padding to a 16-byte boundary.
    pub(crate) padding: f32,
}

impl Default for SlTonemap {
    /// The reference viewer's shipped defaults, each overridable by an environment
    /// variable so a capture can sweep the tone mapper without a rebuild.
    fn default() -> Self {
        Self {
            exposure: env_f32("SL_VIEWER_EXPOSURE", DEFAULT_EXPOSURE),
            tonemap_mix: env_f32("SL_VIEWER_TONEMAP_MIX", DEFAULT_TONEMAP_MIX).clamp(0.0, 1.0),
            tonemap_type: tonemap_type_from_env(),
            padding: 0.0,
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

/// The tone curve to run, from `SL_VIEWER_TONEMAP` (`aces` / `neutral` / `none`),
/// defaulting to the reference's own default (ACES).
fn tonemap_type_from_env() -> u32 {
    match std::env::var("SL_VIEWER_TONEMAP") {
        Ok(value) if value.eq_ignore_ascii_case("neutral") => TONEMAP_KHRONOS_NEUTRAL,
        Ok(value) if value.eq_ignore_ascii_case("none") => TONEMAP_NONE,
        _other => TONEMAP_ACES,
    }
}

impl SyncComponent for SlTonemap {
    type Target = Self;
}

impl ExtractComponent for SlTonemap {
    type QueryData = Read<Self>;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// The plugin: registers extraction / uniform upload, loads the shader, and wires the
/// tone-map pass into the 3D render schedule — after the underwater fog, which the
/// reference likewise applies to the *linear* scene ahead of its tone mapper.
#[derive(Debug, Default)]
pub(crate) struct SlTonemapPlugin;

impl Plugin for SlTonemapPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            TONEMAP_SHADER_HANDLE,
            "tonemap.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            ExtractComponentPlugin::<SlTonemap>::default(),
            UniformComponentPlugin::<SlTonemap>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<SlTonemapPipeline>>()
            .add_systems(RenderStartup, init_tonemap_pipeline)
            .add_systems(
                Render,
                prepare_tonemap_pipelines.in_set(RenderSystems::Prepare),
            )
            .add_systems(
                Core3d,
                sl_tonemap_system
                    .in_set(Core3dSystems::PostProcess)
                    .after(UnderwaterFogPass),
            );
    }
}

/// The tone-map pipeline's global data (bind-group layout descriptor, sampler, and
/// the fullscreen vertex shader, which pipeline specialization needs per view format).
#[derive(Resource)]
struct SlTonemapPipeline {
    /// The bind-group layout descriptor (scene texture, sampler, settings uniform),
    /// resolved to a real layout per frame via the pipeline cache.
    layout: BindGroupLayoutDescriptor,
    /// The sampler used to read the scene colour texture.
    sampler: Sampler,
    /// The shared fullscreen-triangle vertex shader, needed by pipeline
    /// specialization (which has no world access to fetch it).
    fullscreen_shader: FullscreenShader,
}

/// Build the tone-map pipeline's shared data once, in the render world.
fn init_tonemap_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "sl_tonemap_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The (linear, HDR) scene colour texture.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its sampler.
                sampler(SamplerBindingType::Filtering),
                // The tone-mapper settings (dynamic-offset uniform).
                uniform_buffer::<SlTonemap>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    commands.insert_resource(SlTonemapPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
}

impl SpecializedRenderPipeline for SlTonemapPipeline {
    // The post-process source / destination format varies per view, so specialize on
    // it (as the fog pass does).
    type Key = TextureFormat;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("sl_tonemap_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: TONEMAP_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: key,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// The specialized pipeline id for a view.
#[derive(Component)]
struct SlTonemapPipelineId(CachedRenderPipelineId);

/// Specialize the tone-map pipeline for each view's target format.
fn prepare_tonemap_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<SlTonemapPipeline>>,
    pipeline: Res<SlTonemapPipeline>,
    views: Query<(Entity, &ExtractedView), With<ExtractedCamera>>,
) {
    for (entity, view) in &views {
        let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, view.target_format);
        commands
            .entity(entity)
            .insert(SlTonemapPipelineId(pipeline_id));
    }
}

/// The tone-map pass: exposure, curve, mix, clamp — over the whole composited frame.
///
/// Runs only on views carrying an [`SlTonemap`] (the main camera), so the reflection
/// probes' capture cameras keep rendering the linear radiance their cubemaps are
/// supposed to hold.
fn sl_tonemap_system(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<SlTonemap>,
        &SlTonemapPipelineId,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<SlTonemapPipeline>,
    uniforms: Res<ComponentUniforms<SlTonemap>>,
    mut ctx: RenderContext,
) {
    let (view_target, tonemap_index, pipeline_id) = view.into_inner();

    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let Some(uniform_binding) = uniforms.uniforms().binding() else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "sl_tonemap_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline_res.sampler,
            uniform_binding.clone(),
        )),
    );

    let pass_descriptor = RenderPassDescriptor {
        label: Some("sl_tonemap_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
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
    render_pass.set_bind_group(0, &bind_group, &[tonemap_index.index()]);
    render_pass.draw(0..3, 0..1);
}
