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
//! The reference's *automatic* exposure (its `exposureMap`, a luminance-driven
//! `exp_scale` multiplying `RenderExposure`) is supplied by [`exposure`](crate::exposure),
//! whose 1×1 map this pass samples; the static `RenderExposure` setting is the scale
//! it multiplies.
//!
//! **Which skies are tone mapped.** Not all of them: the reference exempts a *legacy*
//! (classic-mode) sky from the transfer entirely, and both aditi and the local OpenSim
//! serve legacy skies. It says so twice over — `LLSettingsSky::getTonemapMix` returns
//! `0` when `mCanAutoAdjust && !RenderSkyAutoAdjustLegacy`, and `LLPipeline::tonemap`
//! sets `no_post` for the same sky and binds `gNoPostTonemapProgram`, whose
//! `postDeferredTonemap.glsl` never calls `toneMap` at all and merely clamps to
//! `[0, 1]`. The second is the stronger of the two: it drops the `RenderExposure`
//! multiply as well as the curve. `is_classic_sky` resolves the condition and
//! [`SlTonemap`]'s `no_post` carries it to the shader, which then takes the same
//! clamp-only path.

use std::sync::OnceLock;

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

use sl_settings::SettingValue;

use crate::settings::ViewerSettings;
use crate::underwater_fog::UnderwaterFogPass;

/// The internal handle the tone-map shader (`tonemap.wgsl`) is loaded under.
const TONEMAP_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b1f0c94-3a27-4d58-9c11-70b4e8d5a213");

/// The render-schedule label for the tone-map pass, so later passes (the glow,
/// which the reference runs after `tonemap`) can order themselves after it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SlTonemapPass;

/// The reference `RenderTonemapType` value selecting the Khronos PBR Neutral curve.
pub const TONEMAP_KHRONOS_NEUTRAL: u32 = 0;
/// The reference `RenderTonemapType` value selecting the ACES (Hill) curve — the
/// reference's default, and so this viewer's.
pub const TONEMAP_ACES: u32 = 1;
/// Not a reference value: no tone curve at all (exposure and clamp only, the
/// reference's `NO_POST` path), so a capture can A/B what the curve is doing.
pub const TONEMAP_NONE: u32 = 2;

/// The reference `RenderTonemapMix` default: how far the tone curve is blended in
/// over the merely-exposed linear colour.
const DEFAULT_TONEMAP_MIX: f32 = 0.7;

/// The reference `RenderExposure` default: a plain scale on the linear scene colour
/// ahead of the curve.
const DEFAULT_EXPOSURE: f32 = 1.0;

/// The reference's floor on `RenderExposure` (`llclamp(exposure(), 0.5f, 4.f)` in
/// `LLPipeline::tonemap`): the setting is a user-editable float, and the reference
/// never lets it darken the frame past half.
const EXPOSURE_MIN: f32 = 0.5;
/// The reference's ceiling on `RenderExposure` (the other half of the same
/// `llclamp`).
const EXPOSURE_MAX: f32 = 4.0;

/// The tone-mapper settings, mirroring the reference's three `Render*` settings.
/// Sits on the camera — which both carries them to the GPU as a uniform and *selects*
/// the view the pass runs on, so the reflection probes' capture cameras (which must
/// stay linear, being the source of image-based lighting) are left alone.
#[derive(Debug, Component, Clone, Copy, ShaderType)]
pub struct SlTonemap {
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
    /// The reference's `no_post` gate (`LLPipeline::tonemap`): `1` when the active
    /// sky is a legacy / classic one, which the reference exempts from the tone
    /// mapper altogether — the shader then merely clamps, skipping the
    /// `RenderExposure` multiply, the curve and the mix alike, exactly as
    /// `gNoPostTonemapProgram` does. Driven from the active sky each frame by
    /// [`refresh_tonemap_settings`]; also the u32 that pads the uniform to a 16-byte
    /// boundary.
    pub(crate) no_post: u32,
}

impl Default for SlTonemap {
    /// The reference viewer's shipped defaults, each overridable by an environment
    /// variable so a capture can sweep the tone mapper without a rebuild.
    fn default() -> Self {
        Self {
            exposure: env_f32("SL_VIEWER_EXPOSURE", DEFAULT_EXPOSURE)
                .clamp(EXPOSURE_MIN, EXPOSURE_MAX),
            tonemap_mix: env_f32("SL_VIEWER_TONEMAP_MIX", DEFAULT_TONEMAP_MIX).clamp(0.0, 1.0),
            tonemap_type: tonemap_type_from_env(),
            // The legacy exemption, until the first `refresh_tonemap_settings` reads
            // the resolved sky — matching `ExposureRange`'s own legacy default, so the
            // two never disagree about what the unresolved sky is.
            no_post: u32::from(!force_post_from_env()),
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

/// The environment variable overriding the tone curve (see [`tonemap_type_from_env`]).
const ENV_TONEMAP_TYPE: &str = "SL_VIEWER_TONEMAP";
/// The environment variable overriding the tone-curve blend.
const ENV_TONEMAP_MIX: &str = "SL_VIEWER_TONEMAP_MIX";
/// The environment variable overriding the exposure.
const ENV_EXPOSURE: &str = "SL_VIEWER_EXPOSURE";
/// The environment variable forcing the tone mapper to run on a legacy sky too (an
/// A/B knob: the reference exempts one, so a capture pair taken with and without
/// this shows exactly what the exemption is worth on the grid in front of you).
const ENV_FORCE_POST: &str = "SL_VIEWER_TONEMAP_FORCE_POST";

/// Whether the four tone-mapper overrides are set, resolved once per process (the
/// environment is fixed at launch): [`refresh_tonemap_settings`] runs every frame and
/// would otherwise take the process env lock once up front plus three times per
/// camera.
struct TonemapOverrides {
    /// Whether [`ENV_TONEMAP_TYPE`] is set (its value is read by
    /// [`tonemap_type_from_env`]).
    tonemap_type: bool,
    /// [`ENV_TONEMAP_MIX`]'s value when it is set, else `None`. A set-but-unparsable
    /// value still wins over the stored setting, at [`DEFAULT_TONEMAP_MIX`].
    tonemap_mix: Option<f32>,
    /// Whether [`ENV_EXPOSURE`] is set.
    exposure: bool,
    /// Whether [`ENV_FORCE_POST`] is set.
    force_post: bool,
}

/// The process's [`TonemapOverrides`], read from the environment on first use.
fn tonemap_overrides() -> &'static TonemapOverrides {
    static OVERRIDES: OnceLock<TonemapOverrides> = OnceLock::new();
    OVERRIDES.get_or_init(|| TonemapOverrides {
        tonemap_type: std::env::var_os(ENV_TONEMAP_TYPE).is_some(),
        tonemap_mix: std::env::var_os(ENV_TONEMAP_MIX)
            .is_some()
            .then(|| env_f32(ENV_TONEMAP_MIX, DEFAULT_TONEMAP_MIX)),
        exposure: std::env::var_os(ENV_EXPOSURE).is_some(),
        force_post: std::env::var_os(ENV_FORCE_POST).is_some(),
    })
}

/// Whether [`ENV_FORCE_POST`] is set, i.e. the legacy-sky exemption is pinned off.
fn force_post_from_env() -> bool {
    tonemap_overrides().force_post
}

/// The reference's classic-mode test (`LLSettingsVOSky::applySpecial`'s
/// `classic_mode = psky->canAutoAdjust() && !should_auto_adjust()`, and the identical
/// condition behind `LLSettingsSky::getTonemapMix`'s `0` and `LLPipeline::tonemap`'s
/// `no_post`): a legacy sky, unless `RenderSkyAutoAdjustLegacy` promotes it.
///
/// `can_auto_adjust` is the reference's `mCanAutoAdjust`, which our decode collapses
/// to `reflection_probe_ambiance == 0` (see
/// [`ExposureRange`](crate::exposure::ExposureRange)). Under that collapse the
/// reference's two spellings — this one and `getReflectionProbeAmbiance(auto) == 0`,
/// which is what `no_post` actually tests — agree exactly, so one flag serves both.
///
/// `force_post` is the [`ENV_FORCE_POST`] A/B override, which pins the exemption off.
#[must_use]
pub(crate) const fn is_classic_sky(
    can_auto_adjust: bool,
    sky_auto_adjust_legacy: bool,
    force_post: bool,
) -> bool {
    can_auto_adjust && !sky_auto_adjust_legacy && !force_post
}

/// The reference `LLSettingsSky::getTonemapMix`: a classic sky's mix is `0`
/// ("legacy settings do not support tonemaping") whatever `RenderTonemapMix` says —
/// the reference does not even call `setTonemapMix` on one. Any other sky takes the
/// stored setting, clamped to the `[0, 1]` its shader `mix` expects.
#[must_use]
pub(crate) const fn effective_tonemap_mix(stored_mix: f32, classic_sky: bool) -> f32 {
    if classic_sky {
        0.0
    } else {
        stored_mix.clamp(0.0, 1.0)
    }
}

/// The persisted-file section the tone-mapper settings are grouped under
/// (`[render.tonemap]`), matching the reference's `Render*` naming.
const TONEMAP_SECTION: &[&str] = &["render", "tonemap"];

/// The reference `RenderTonemapType` setting name.
pub const SETTING_TONEMAP_TYPE: &str = "RenderTonemapType";
/// The reference `RenderTonemapMix` setting name.
pub const SETTING_TONEMAP_MIX: &str = "RenderTonemapMix";
/// The reference `RenderExposure` setting name.
pub const SETTING_EXPOSURE: &str = "RenderExposure";

/// Register the tone-mapper settings on the store with the reference defaults, so
/// the names exist (and persist) — a user's Firestorm `RenderTonemapType` /
/// `RenderTonemapMix` / `RenderExposure` port straight over — and the (future)
/// preferences UI has something to bind to. Called from
/// [`ViewerSettings`]'s `FromWorld`.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        TONEMAP_SECTION,
        SETTING_TONEMAP_TYPE,
        SettingValue::U32(TONEMAP_ACES),
        "Tone curve: 0 Khronos PBR Neutral, 1 ACES (default), 2 none",
    );
    settings.register_in(
        TONEMAP_SECTION,
        SETTING_TONEMAP_MIX,
        SettingValue::F32(DEFAULT_TONEMAP_MIX),
        "How far the tone curve is blended over the merely-exposed colour (0-1)",
    );
    settings.register_in(
        TONEMAP_SECTION,
        SETTING_EXPOSURE,
        SettingValue::F32(DEFAULT_EXPOSURE),
        "Linear scene-colour scale before the tone curve",
    );
}

/// Refresh each camera's live [`SlTonemap`] from the settings store each frame
/// (cheap reads), so a `RenderTonemapType` / `RenderTonemapMix` / `RenderExposure`
/// changed in the (future) preferences UI takes effect at once — the preferences
/// counterpart to the environment overrides.
///
/// An environment override (`SL_VIEWER_TONEMAP*` / `SL_VIEWER_EXPOSURE`), used by
/// the screenshot harness to sweep the tone mapper without a config, **wins** over
/// the stored value: a set variable pins its field and the store no longer drives
/// it, so a capture is reproducible regardless of the user's saved preferences.
///
/// The *sky*, though, wins over both: a legacy / classic sky is exempt from the tone
/// mapper (see the module docs), so its `no_post` is set and its mix forced to `0`
/// whatever the store or `SL_VIEWER_TONEMAP_MIX` asked for — `SL_VIEWER_TONEMAP_FORCE_POST`
/// is the one knob that overrides *that*.
pub(crate) fn refresh_tonemap_settings(
    store: Res<ViewerSettings>,
    range: Res<crate::exposure::ExposureRange>,
    mut cameras: Query<&mut SlTonemap>,
) {
    let store = store.store();
    let sky_auto_adjust_legacy = store
        .get_bool(crate::exposure::SETTING_AUTO_ADJUST_LEGACY)
        .unwrap_or(crate::exposure::DEFAULT_AUTO_ADJUST_LEGACY);
    let overrides = tonemap_overrides();
    let classic_sky = is_classic_sky(
        range.can_auto_adjust,
        sky_auto_adjust_legacy,
        overrides.force_post,
    );
    for mut tonemap in &mut cameras {
        if !overrides.tonemap_type
            && let Ok(value) = store.get_u32(SETTING_TONEMAP_TYPE)
        {
            tonemap.tonemap_type = value;
        }
        // Re-derived from source every frame rather than read back off the component:
        // a classic sky zeroes the live field, so carrying that zero forward would
        // make the exemption stick when the sky stops being a legacy one.
        let stored_mix = overrides.tonemap_mix.unwrap_or_else(|| {
            store
                .get_f32(SETTING_TONEMAP_MIX)
                .unwrap_or(DEFAULT_TONEMAP_MIX)
        });
        tonemap.tonemap_mix = effective_tonemap_mix(stored_mix, classic_sky);
        tonemap.no_post = u32::from(classic_sky);
        if !overrides.exposure
            && let Ok(value) = store.get_f32(SETTING_EXPOSURE)
        {
            tonemap.exposure = value.clamp(EXPOSURE_MIN, EXPOSURE_MAX);
        }
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
pub struct SlTonemapPlugin;

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
        ))
        // Drive the live camera settings from the preferences store (env overrides
        // still win, for the screenshot harness).
        .add_systems(Update, refresh_tonemap_settings);

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
                    .in_set(SlTonemapPass)
                    // After the exposure pass, whose 1×1 map this samples.
                    .after(crate::exposure::SlExposurePass)
                    .after(UnderwaterFogPass)
                    // Bloom (the SL glow pass) also runs in `PostProcess`, but only
                    // orders itself before Bevy's *own* tonemapping (which we disable),
                    // so pin ours after it: bloom must add its glow to the linear HDR
                    // scene before this tone map compresses it.
                    .after(bevy::post_process::bloom::bloom),
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
                // The 1×1 dynamic-exposure map (the reference's `exposureMap`).
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its sampler.
                sampler(SamplerBindingType::Filtering),
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
    exposure_map: Res<crate::exposure::ExposureMap>,
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
            &exposure_map.view,
            &exposure_map.sampler,
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        DEFAULT_EXPOSURE, DEFAULT_TONEMAP_MIX, EXPOSURE_MAX, EXPOSURE_MIN, SETTING_EXPOSURE,
        SETTING_TONEMAP_MIX, SETTING_TONEMAP_TYPE, TONEMAP_ACES, effective_tonemap_mix,
        is_classic_sky, register_settings,
    };
    use crate::settings::ViewerSettings;

    /// The store defaults `register_settings` declares must match the tone
    /// mapper's own reference defaults, so a fresh install (no override, no env)
    /// renders exactly as the shipped [`super::SlTonemap`] default — the two
    /// default sources must never drift apart.
    #[test]
    fn registered_defaults_match_reference() {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        let store = settings.store();
        assert_eq!(store.get_u32(SETTING_TONEMAP_TYPE).ok(), Some(TONEMAP_ACES));
        assert_eq!(
            store.get_f32(SETTING_TONEMAP_MIX).ok(),
            Some(DEFAULT_TONEMAP_MIX)
        );
        assert_eq!(store.get_f32(SETTING_EXPOSURE).ok(), Some(DEFAULT_EXPOSURE));
    }

    /// A user override round-trips through the getters `refresh_tonemap_settings`
    /// reads — the path a preferences edit reaches the live camera by.
    #[test]
    fn overrides_round_trip_through_the_store() {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        settings.set(Scope::Global, SETTING_TONEMAP_TYPE, SettingValue::U32(0));
        settings.set(Scope::Global, SETTING_TONEMAP_MIX, SettingValue::F32(0.25));
        settings.set(Scope::Global, SETTING_EXPOSURE, SettingValue::F32(1.5));
        let store = settings.store();
        assert_eq!(store.get_u32(SETTING_TONEMAP_TYPE).ok(), Some(0));
        assert_eq!(store.get_f32(SETTING_TONEMAP_MIX).ok(), Some(0.25));
        assert_eq!(store.get_f32(SETTING_EXPOSURE).ok(), Some(1.5));
    }

    /// The reference's classic-mode test, all four ways round. A legacy sky
    /// (`mCanAutoAdjust`) is classic unless `RenderSkyAutoAdjustLegacy` promotes it;
    /// an EEP sky never is. This is what both the reference's `getTonemapMix` zero
    /// and its `gNoPostTonemapProgram` selection hang off, so getting it wrong
    /// tone-maps a sky the reference leaves alone — which is every sky aditi and the
    /// local OpenSim serve.
    #[test]
    fn a_legacy_sky_is_classic_unless_auto_adjust_promotes_it() {
        // A legacy sky with the shipped `RenderSkyAutoAdjustLegacy = false`.
        assert!(is_classic_sky(true, false, false));
        // …and with it turned on, which lifts the sky into the EEP/HDR path.
        assert!(!is_classic_sky(true, true, false));
        // An EEP sky is never classic, either way.
        assert!(!is_classic_sky(false, false, false));
        assert!(!is_classic_sky(false, true, false));
    }

    /// `SL_VIEWER_TONEMAP_FORCE_POST` is the one override of the exemption — the A/B
    /// knob that runs the curve on a legacy sky so a capture pair can show what the
    /// exemption is worth.
    #[test]
    fn forcing_post_pins_the_exemption_off() {
        assert!(!is_classic_sky(true, false, true));
    }

    /// `LLSettingsSky::getTonemapMix`: a classic sky mixes in `0` of the curve no
    /// matter what `RenderTonemapMix` holds, and any other sky takes the setting,
    /// clamped to the `[0, 1]` the shader's `mix` expects.
    #[test]
    fn a_classic_sky_zeroes_the_tonemap_mix() {
        for stored in [DEFAULT_TONEMAP_MIX, 1.0, 0.0] {
            let mix = effective_tonemap_mix(stored, true);
            assert!(mix.abs() < 1e-6, "classic mix from {stored} is {mix}");
        }
        let mix = effective_tonemap_mix(DEFAULT_TONEMAP_MIX, false);
        assert!((mix - DEFAULT_TONEMAP_MIX).abs() < 1e-6, "eep mix {mix}");
        // Out-of-range stored values are held to what the shader's `mix` expects.
        let high = effective_tonemap_mix(1.5, false);
        assert!((high - 1.0).abs() < 1e-6, "clamped high mix {high}");
        let low = effective_tonemap_mix(-0.5, false);
        assert!(low.abs() < 1e-6, "clamped low mix {low}");
    }

    /// The reference's `llclamp(exposure(), 0.5f, 4.f)` on `RenderExposure`: the
    /// bounds the store's value and the `SL_VIEWER_EXPOSURE` override are both held
    /// to, so a hand-edited setting cannot black out or blow out the frame.
    #[test]
    fn the_exposure_clamp_matches_the_reference_bounds() {
        assert!((EXPOSURE_MIN - 0.5).abs() < 1e-6, "min {EXPOSURE_MIN}");
        assert!((EXPOSURE_MAX - 4.0).abs() < 1e-6, "max {EXPOSURE_MAX}");
        // The shipped default sits inside them, so a fresh install is unclamped.
        let clamped = DEFAULT_EXPOSURE.clamp(EXPOSURE_MIN, EXPOSURE_MAX);
        assert!(
            (clamped - DEFAULT_EXPOSURE).abs() < 1e-6,
            "default exposure {clamped}"
        );
    }
}
