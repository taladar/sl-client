//! Underwater fog (P23.1): a fullscreen post-process that reproduces the Second
//! Life / Firestorm water fog (`class1/environment/waterFogF.glsl`,
//! `getWaterFogViewNoClip` / `applyWaterFogViewLinear`) over the whole scene.
//!
//! The reference applies the water fog per fragment in the deferred stage, tinting
//! every underwater surface by the water body colour with a distance-based
//! transmittance and in-scatter, and clipping per fragment against the water plane
//! so a camera straddling the surface splits cleanly along the waterline. A
//! per-material fog would miss objects / avatars, so this runs as one fullscreen
//! pass over the composited image plus the depth buffer — fogging terrain, objects,
//! avatars, and the water underside uniformly, exactly where they are underwater.
//!
//! Scope (R21): the pass fogs only when the **eye is submerged**. The reference
//! fogs the deferred *opaque* geometry before the transparent water surface is
//! composited, so the surface is never fogged by this pass; here the surface is
//! already in the colour buffer, and fogging the underwater seafloor as seen from
//! *above* water painted the sea into a flat dark slab (starkest over the void past
//! a region edge with no neighbour). Above the surface the water-surface shader
//! (`water.wgsl`) already gives the from-above look, so the fog shader passes the
//! scene through untouched when the eye is above water and only fogs when submerged.
//! `SL_VIEWER_DISABLE_UNDERWATER_FOG=1` forces it off entirely (a debug A/B knob).
//!
//! Bevy 0.19 replaced the render graph with a **system-based** renderer, so this is
//! not a render-graph `ViewNode`: the pass is a system in the [`Core3d`] schedule
//! (in [`Core3dSystems::PostProcess`], before the tone mapper), modelled on
//! `bevy_core_pipeline::fullscreen_material` / `bevy_post_process::effect_stack`.
//! The built-in `FullscreenMaterial` trait is not usable here because its bind
//! group is fixed to *(source, sampler, uniform)* with no depth binding, and this
//! effect needs the scene depth; so the pipeline / bind group / pass are
//! hand-written with an extra depth-texture binding. The depth comes from the
//! **main pass** depth texture (made sampleable by setting
//! `Camera3d::depth_texture_usages` to include `TEXTURE_BINDING`) rather than a
//! `DepthPrepass` — the prepass would build depth pipelines for the custom sky /
//! terrain / water materials whose `specialize` pins bespoke vertex layouts, which
//! the prepass vertex shader rejects; the main depth texture already has every
//! material's depth with no extra pipelines.
//!
//! The [`UnderwaterFog`] component on the camera carries the per-frame parameters
//! (`update_underwater_fog` fills them from the region's EEP water settings, the
//! sky sun direction, the camera pose, and the water level).
//!
//! The pass runs after the main pass and **before** the tone mapper
//! ([`tonemap`](crate::tonemap)), so — as in the reference — the fog is mixed into
//! the *linear* scene and the fogged result is what gets tone-mapped. (Until P33.3
//! gave the camera an HDR target and a tone mapper of its own, the viewer's main pass
//! wrote an already-tonemapped, clipped 8-bit image, and this pass fogged that.)

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
use bevy::render::render_resource::binding_types::{texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    FragmentState, MultisampleState, PipelineCache, RenderPassDescriptor, RenderPipelineDescriptor,
    ShaderStages, ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines, TextureFormat,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::{GpuResourceAppExt as _, Render, RenderApp, RenderStartup, RenderSystems};

use crate::coords::sl_to_bevy_object_rotation;
use crate::environment::EnvironmentState;
use crate::sky::day_position;
use crate::water::{WaterLevel, drive_water};
use crate::world_api::{ViewerCamera, WorldPhase};

/// The internal handle the fog shader (`underwater_fog.wgsl`) is loaded under.
const FOG_SHADER_HANDLE: Handle<Shader> = uuid_handle!("3f2a9c17-54e8-4b6d-a90c-2e718d43ff05");

/// The per-frame underwater-fog parameters, extracted to the render world and
/// uploaded as a dynamic uniform. Attached to the camera; also selects the camera
/// the fog pass runs on.
#[derive(Debug, Component, Clone, Copy, PartialEq, ShaderType)]
pub struct UnderwaterFog {
    /// World-from-clip, to reconstruct a fragment's world position from its depth.
    pub(crate) world_from_clip: Mat4,
    /// The camera world position (xyz) + padding.
    pub(crate) camera_pos: Vec4,
    /// The water fog colour (rgb) + padding.
    pub(crate) fog_color: Vec4,
    /// The water surface height, in world metres.
    pub(crate) water_height: f32,
    /// The eye-state-modified water fog density.
    pub(crate) fog_density: f32,
    /// The water fog `KS` term.
    pub(crate) fog_ks: f32,
    /// std140 padding to a 16-byte boundary.
    pub(crate) padding: f32,
}

impl Default for UnderwaterFog {
    fn default() -> Self {
        Self {
            world_from_clip: Mat4::IDENTITY,
            camera_pos: Vec4::ZERO,
            fog_color: Vec4::ZERO,
            // A very low surface with zero density is a harmless no-op until
            // `update_underwater_fog` fills real values.
            water_height: f32::MIN,
            fog_density: 0.0,
            fog_ks: 1.0,
            padding: 0.0,
        }
    }
}

impl SyncComponent for UnderwaterFog {
    type Target = Self;
}

impl ExtractComponent for UnderwaterFog {
    type QueryData = Read<Self>;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// A debug affordance: `SL_VIEWER_DISABLE_UNDERWATER_FOG=1` forces the fog off
/// (zero density is a shader no-op) so a capture can A/B the underwater-fog pass
/// against the plain water-surface shading (used to localise the R21 dark slab).
///
/// Resolved once per process: the environment is fixed at launch, and this gates a
/// per-frame system.
fn fog_disabled() -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("SL_VIEWER_DISABLE_UNDERWATER_FOG")
            .is_ok_and(|value| value != "0" && !value.is_empty())
    })
}

/// The water fog density the shader should use, given the water frame's density and
/// underwater fog modifier and whether the eye is submerged — the reference
/// `LLSettingsWater::getModifiedWaterFogDensity` (`llsettingswater.cpp:377`).
///
/// Submerged, the density is raised to the modifier (clamped to the reference's
/// `[0, 10]`); above water it is the frame's density unchanged.
///
/// The guard on a **negative** density is the reference's fix for
/// BUG-233797 / BUG-233798: a negative base raised to a non-integral power is not a
/// real number, so `powf` returns `NaN`, and a `NaN` density reaches the uniform and
/// takes the whole screen with it — the reference's comment calls it an
/// *unrecoverable blackout*. Both are values a region may legitimately send: the
/// density is a free `f32` off the wire, and the modifier is authored per water
/// frame. Of the two remedies the reference weighed, it chose (and this follows)
/// forcing the density to `1.0` in that case, which keeps some notion of fog rather
/// than rounding the modifier and inverting the water's colour.
///
/// Integrality is tested on the *clamped* modifier, as in the reference — a modifier
/// of `10.5` clamps to `10.0`, which is integral, and needs no rescue.
pub(crate) fn modified_water_fog_density(density: f32, fog_mod: f32, submerged: bool) -> f32 {
    if !(submerged && fog_mod > 0.0) {
        return density;
    }
    let fog_mod = fog_mod.clamp(0.0, 10.0);
    let density = if density < 0.0 && fog_mod.fract() > 0.0 {
        1.0
    } else {
        density
    };
    density.powf(fog_mod)
}

/// Fill the camera's [`UnderwaterFog`] from the region's EEP water settings, the
/// sky sun direction, the camera pose, and the current water level — the reference
/// `LLSettingsVOWater` uniform prep (`waterFogKS = 1 / max(lightDir.z, 0.3)`,
/// `getModifiedWaterFogDensity` — `pow(density, fogMod)` when the eye is submerged).
///
/// Reads the camera's **`Transform`**, not its `GlobalTransform`: the fog pass
/// reconstructs a fragment's world position from a depth buffer rendered from
/// *this* frame's pose, and `GlobalTransform` is only recomputed by propagation in
/// `PostUpdate` — `.after(WorldPhase::CameraPositioned)` buys ordering, not
/// freshness. A frame-old `world_from_clip` against a current-frame depth buffer
/// displaces every fogged fragment by exactly the frame's camera motion, which
/// reads as the background swimming behind the fog while walking. The camera is a
/// top-level entity (spawned with no parent), so its `Transform` *is* its world
/// pose.
pub(crate) fn update_underwater_fog(
    environment: Res<EnvironmentState>,
    level: Res<WaterLevel>,
    mut cameras: Query<(&Transform, &Projection, &mut UnderwaterFog), With<ViewerCamera>>,
) {
    let disabled = fog_disabled();
    for (camera_transform, projection, mut fog) in &mut cameras {
        let camera_pos = camera_transform.translation;
        let position = day_position(&environment.settings);
        let water = environment.settings.blended_water_settings(position);
        let sky = environment
            .settings
            .blended_sky_settings(camera_pos.y, position);

        // world_from_clip = inverse(clip_from_view * view_from_world), to
        // reconstruct a fragment's world position from its depth in the shader.
        let clip_from_view = projection.get_clip_from_view();
        let view_from_world = camera_transform.to_matrix().inverse();
        // `mul_mat4` rather than the `*` operator, which trips the workspace
        // `arithmetic_side_effects` lint.
        let world_from_clip = clip_from_view.mul_mat4(&view_from_world).inverse();

        let water_height = level.0;
        let submerged = camera_pos.y < water_height;

        // The active light's up component drives `KS` (the reference clamps it to
        // 0.3); use the sun if up, else the moon, else the floor.
        let light_up = sky.as_ref().map_or(1.0, |sky| {
            let sun = sl_to_bevy_object_rotation(&sky.sun_rotation)
                .mul_vec3(Vec3::X)
                .normalize();
            let moon = sl_to_bevy_object_rotation(&sky.moon_rotation)
                .mul_vec3(Vec3::X)
                .normalize();
            if sun.y >= 0.0 {
                sun.y
            } else if moon.y >= 0.0 {
                moon.y
            } else {
                0.0
            }
        });
        let fog_ks = 1.0 / light_up.max(0.3);

        let (fog_color, fog_density) = match water {
            Some(water) => {
                let density = modified_water_fog_density(
                    water.water_fog_density,
                    water.underwater_fog_mod,
                    submerged,
                );
                let color = Vec3::new(
                    water.water_fog_color.red(),
                    water.water_fog_color.green(),
                    water.water_fog_color.blue(),
                );
                (color, density)
            }
            None => (Vec3::ZERO, 0.0),
        };
        // Debug override: a zero density makes the fog shader a pass-through.
        let fog_density = if disabled { 0.0 } else { fog_density };

        // Write-on-change: with a parked camera and stable water settings the
        // recomputed params are bit-identical, and an unconditional write would
        // mark the component changed every frame.
        fog.set_if_neq(UnderwaterFog {
            world_from_clip,
            camera_pos: camera_pos.extend(0.0),
            fog_color: fog_color.extend(0.0),
            water_height,
            fog_density,
            fog_ks,
            padding: 0.0,
        });
    }
}

/// The system set the fog pass runs in, so a later post-process pass can order itself
/// after it without reaching for the (private) system: the tone mapper
/// ([`tonemap`](crate::tonemap)) must see the *fogged* linear scene, since the
/// reference fogs before it tonemaps.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct UnderwaterFogPass;

/// The plugin: registers extraction / uniform upload, loads the shader, and wires
/// the render-world pipeline prep + the fog pass into the 3D render schedule.
#[derive(Debug, Default)]
pub struct UnderwaterFogPlugin;

impl Plugin for UnderwaterFogPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            FOG_SHADER_HANDLE,
            "underwater_fog.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            ExtractComponentPlugin::<UnderwaterFog>::default(),
            UniformComponentPlugin::<UnderwaterFog>::default(),
        ))
        // Refresh the camera's fog parameters (water level, EEP fog
        // colour/density, reconstruction matrix) each frame, after the camera so
        // the matrix matches the current viewpoint and after the ocean so the
        // water level it reads is this frame's.
        .add_systems(
            Update,
            update_underwater_fog
                .after(WorldPhase::CameraPositioned)
                .after(drive_water),
        );

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<UnderwaterFogPipeline>>()
            .add_systems(RenderStartup, init_fog_pipeline)
            .add_systems(Render, prepare_fog_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                (
                    // Above water: after the opaque geometry and the below-water
                    // translucency it fogs, and before the water surface, whose
                    // refraction sample is a copy of what this pass has just fogged.
                    // That is where the sea's colour comes from.
                    water_haze_above_system
                        .after(crate::transparency::PreWaterPass)
                        .before(bevy::pbr::main_transmissive_pass_3d),
                    // Submerged: after everything, so the fog covers the translucent
                    // content drawn after the water as well as the water itself.
                    water_haze_submerged_system
                        .after(bevy::core_pipeline::core_3d::main_transparent_pass_3d),
                )
                    .in_set(Core3dSystems::MainPass)
                    .in_set(UnderwaterFogPass),
            );
    }
}

/// The fog pipeline's global data (bind-group layout descriptor, sampler, and the
/// fullscreen vertex shader, which pipeline specialization needs per view format).
#[derive(Resource)]
struct UnderwaterFogPipeline {
    /// The bind-group layout descriptor (fog uniform, depth texture), resolved to a
    /// real layout per frame via the pipeline cache.
    layout: BindGroupLayoutDescriptor,
    /// The shared fullscreen-triangle vertex shader, needed by pipeline
    /// specialization (which has no world access to fetch it).
    fullscreen_shader: FullscreenShader,
}

/// Build the fog pipeline's shared data once, in the render world.
fn init_fog_pipeline(mut commands: Commands, fullscreen_shader: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "water_haze_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The per-frame fog parameters (dynamic-offset uniform).
                uniform_buffer::<UnderwaterFog>(true),
                // The (multisampled) main-pass depth texture.
                texture_depth_2d_multisampled(),
            ),
        ),
    );
    commands.insert_resource(UnderwaterFogPipeline {
        layout,
        fullscreen_shader: fullscreen_shader.clone(),
    });
}

/// The pipeline key: the view's target format and its MSAA sample count. Both vary
/// per view, and the sample count matters because this pass draws **into the main
/// pass's attachment** — a pipeline whose sample count disagrees with the attachment
/// is a validation error, not a subtle artifact.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct FogPipelineKey {
    /// The colour attachment's format.
    format: TextureFormat,
    /// The colour attachment's MSAA sample count.
    samples: u32,
    /// Which eye state this pipeline fogs for.
    half: HazeHalf,
}

/// Which eye state a haze pipeline is specialized for. One shader with an `#ifdef`,
/// as the reference is one shader with an `above_water` uniform, but the two run at
/// different points in the frame and so need pipelines of their own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum HazeHalf {
    /// The eye is **above** the surface: fog before the water is drawn, so the water
    /// refracts a fogged scene.
    Above,
    /// The eye is **submerged**: fog after everything, so the fog is the medium the
    /// whole picture is seen through.
    Submerged,
}

impl SpecializedRenderPipeline for UnderwaterFogPipeline {
    type Key = FogPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some(
                match key.half {
                    HazeHalf::Above => "water_haze_above_pipeline",
                    HazeHalf::Submerged => "water_haze_submerged_pipeline",
                }
                .into(),
            ),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: FOG_SHADER_HANDLE,
                shader_defs: match key.half {
                    HazeHalf::Above => vec!["WATER_HAZE_ABOVE".into()],
                    HazeHalf::Submerged => vec![],
                },
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    // The reference's own blend for this pass: `(ONE, SOURCE_ALPHA)`,
                    // so the shader's colour is the in-scatter and its alpha is the
                    // transmittance and the blender computes `dst * D + L`.
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::SrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        // Keep the destination alpha: it is the scene's glow mask
                        // (`glow.rs`), not a coverage value.
                        alpha: BlendComponent {
                            src_factor: BlendFactor::Zero,
                            dst_factor: BlendFactor::One,
                            operation: BlendOperation::Add,
                        },
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            multisample: MultisampleState {
                count: key.samples,
                ..default()
            },
            ..default()
        }
    }
}

/// The specialized pipeline ids for a view — one per eye state.
#[derive(Component)]
struct UnderwaterFogPipelineId {
    /// Runs before the water, and fogs only when the eye is above the surface.
    above: CachedRenderPipelineId,
    /// Runs after everything, and fogs only when the eye is under the surface.
    submerged: CachedRenderPipelineId,
}

/// Specialize the haze pipeline for each view's target format.
fn prepare_fog_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<UnderwaterFogPipeline>>,
    pipeline: Res<UnderwaterFogPipeline>,
    views: Query<(Entity, &ExtractedView, &Msaa), With<ExtractedCamera>>,
) {
    for (entity, view, msaa) in &views {
        let key = |half| FogPipelineKey {
            format: view.target_format,
            samples: msaa.samples(),
            half,
        };
        commands.entity(entity).insert(UnderwaterFogPipelineId {
            above: pipelines.specialize(&pipeline_cache, &pipeline, key(HazeHalf::Above)),
            submerged: pipelines.specialize(&pipeline_cache, &pipeline, key(HazeHalf::Submerged)),
        });
    }
}

/// The water-haze pass: fog every pixel that lies under the water surface, before
/// the water itself is drawn.
///
/// This is the reference's `class3/deferred/waterHazeF.glsl`, which likewise runs
/// before its water pool: it is what gives the sea its colour, because the surface
/// shows a *sample of this fogged scene* rather than a tint of its own. It fogs from
/// either side of the surface — an eye above the water sees a fogged sea floor
/// through the surface, an eye under it sees the fogged world around it — the
/// difference being only where the ray enters the water, which the shader works out
/// per fragment.
///
/// What it deliberately does not fog: anything drawn after it. Above water that is
/// the water surface (which shows the fogged scene through the refraction sample
/// instead) and above-water translucency (correctly unfogged); submerged it is the
/// surface (which fogs itself, as the reference's underwater surface shader does)
/// and the pre-water translucency, which the reference fogs in its alpha shaders and
/// we do not yet.
fn water_haze_above_system(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<UnderwaterFog>,
        &UnderwaterFogPipelineId,
        &ViewDepthTexture,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<UnderwaterFogPipeline>,
    uniforms: Res<ComponentUniforms<UnderwaterFog>>,
    mut ctx: RenderContext,
) {
    let (view_target, fog_index, pipeline_id, view_depth) = view.into_inner();
    draw_haze(
        "water_haze_above",
        pipeline_id.above,
        view_target,
        fog_index,
        view_depth,
        &pipeline_cache,
        &pipeline_res,
        &uniforms,
        &mut ctx,
    );
}

/// The **submerged** haze pass: after every other pass of the main pass, so the fog
/// is applied to the whole picture — the terrain and objects, the translucent content
/// drawn after the water (the cloud dome most visibly), and the water surface itself,
/// whose underside the reference fogs by exactly the distance the depth buffer here
/// gives.
///
/// Still inside the main pass rather than a post-process, because under MSAA the
/// resolved texture a post-process would read and rewrite is discarded by the next
/// resolve; blending into the attachment is what actually reaches the frame.
fn water_haze_submerged_system(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<UnderwaterFog>,
        &UnderwaterFogPipelineId,
        &ViewDepthTexture,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<UnderwaterFogPipeline>,
    uniforms: Res<ComponentUniforms<UnderwaterFog>>,
    mut ctx: RenderContext,
) {
    let (view_target, fog_index, pipeline_id, view_depth) = view.into_inner();
    draw_haze(
        "water_haze_submerged",
        pipeline_id.submerged,
        view_target,
        fog_index,
        view_depth,
        &pipeline_cache,
        &pipeline_res,
        &uniforms,
        &mut ctx,
    );
}

/// Draw one haze pass: bind the fog uniform and the main-pass depth, and blend a
/// fullscreen triangle over the scene. Shared by both eye states, which differ only
/// in the pipeline (and so the shader branch) and in where in the frame they run.
#[expect(
    clippy::too_many_arguments,
    reason = "the two callers are render systems whose params this simply forwards"
)]
fn draw_haze(
    label: &str,
    pipeline_id: CachedRenderPipelineId,
    view_target: &ViewTarget,
    fog_index: &DynamicUniformIndex<UnderwaterFog>,
    view_depth: &ViewDepthTexture,
    pipeline_cache: &PipelineCache,
    pipeline_res: &UnderwaterFogPipeline,
    uniforms: &ComponentUniforms<UnderwaterFog>,
    ctx: &mut RenderContext,
) {
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id) else {
        return;
    };
    let Some(uniform_binding) = uniforms.uniforms().binding() else {
        return;
    };

    let bind_group = ctx.render_device().create_bind_group(
        Some(label),
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            uniform_binding.clone(),
            // The main-pass depth texture (made sampleable via
            // `Camera3d::depth_texture_usages`), from which the shader reconstructs
            // each fragment's world position. Sampled, not attached — this pass does
            // no depth testing, so the buffer it reads is not bound against it.
            view_depth.view(),
        )),
    );

    let pass_descriptor = RenderPassDescriptor {
        label: Some(label),
        // The main pass's own colour attachment, loaded: this blends into the scene
        // being drawn (and, under MSAA, into the multisampled texture that *is* the
        // scene until it resolves), rather than reading and rewriting a resolved copy
        // that the next resolve would throw away.
        color_attachments: &[Some(view_target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut render_pass = ctx.begin_tracked_render_pass(pass_descriptor);
    render_pass.set_render_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[fog_index.index()]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{UnderwaterFog, modified_water_fog_density, update_underwater_fog};
    use crate::environment::EnvironmentState;
    use crate::water::WaterLevel;
    use crate::world_api::ViewerCamera;

    /// A fog density is the expected one, to within a relative tolerance that leaves
    /// room for the last bit of a `powf` — and, since every comparison against a
    /// `NaN` is false, an assertion that the value is a real number at all.
    #[track_caller]
    fn assert_density(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-6,
            "expected a fog density of {expected}, got {actual}",
        );
    }

    /// The fog reconstructs a fragment's world position from a depth buffer
    /// rendered from *this* frame's camera pose, so it must read the camera's
    /// current-frame `Transform` — not the `GlobalTransform` propagation only
    /// refreshes in `PostUpdate`, which is a frame behind whenever the camera is
    /// moving.
    ///
    /// Stage a camera whose two poses disagree (the shape of every `Update` frame
    /// after the camera has moved) and check the uniform followed the `Transform`.
    #[test]
    fn reads_the_current_frame_camera_pose() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = App::new();
        app.init_resource::<EnvironmentState>()
            .init_resource::<WaterLevel>()
            .add_systems(Update, update_underwater_fog);

        // This frame's pose, as `position_camera` just wrote it.
        let current = Transform::from_xyz(10.0, 20.0, 30.0);
        // Last frame's pose, as propagation left the `GlobalTransform`.
        let stale = Transform::from_xyz(-1.0, -2.0, -3.0);
        let camera = app
            .world_mut()
            .spawn((
                ViewerCamera,
                current,
                GlobalTransform::from(stale),
                Projection::default(),
                UnderwaterFog::default(),
            ))
            .id();

        app.update();

        let fog = app
            .world()
            .entity(camera)
            .get::<UnderwaterFog>()
            .ok_or("the camera keeps its fog component")?;
        assert_eq!(
            fog.camera_pos.truncate(),
            current.translation,
            "the fog eye position is this frame's camera pose",
        );
        // The reconstruction matrix must agree with that eye. Bevy's perspective is
        // reverse-Z infinite, so the *near* plane is clip `z = 1`: unprojecting its
        // centre gives a point just in front of this frame's eye, not the stale one.
        let near = fog.world_from_clip.project_point3(Vec3::new(0.0, 0.0, 1.0));
        assert!(
            near.distance(current.translation) < 1.0,
            "world_from_clip unprojects the near-plane centre next to this frame's \
             eye, got {near:?} against {:?}",
            current.translation,
        );
        assert!(
            near.distance(stale.translation) > 1.0,
            "…and nowhere near the frame-old GlobalTransform pose {:?}",
            stale.translation,
        );
        Ok(())
    }

    /// Above water the frame's density is the density, whatever the modifier says —
    /// the modifier only applies to a submerged eye.
    #[test]
    fn above_water_the_density_is_untouched() {
        assert_density(modified_water_fog_density(2.0, 0.25, false), 2.0);
        // Including the value that would otherwise need the negative-base rescue:
        // out of the water there is no `powf` to go non-real.
        assert_density(modified_water_fog_density(-2.0, 0.25, false), -2.0);
    }

    /// A non-positive modifier is the reference's own "no modification" case
    /// (`underwater && underwater_fog_mod > 0.0f`), submerged or not.
    #[test]
    fn a_non_positive_modifier_is_untouched() {
        assert_density(modified_water_fog_density(2.0, 0.0, true), 2.0);
        assert_density(modified_water_fog_density(2.0, -1.0, true), 2.0);
    }

    /// The ordinary submerged case: the density raised to the modifier, with the
    /// modifier clamped to the reference's `[0, 10]`.
    #[test]
    fn submerged_the_density_is_raised_to_the_modifier() {
        assert_density(modified_water_fog_density(4.0, 0.5, true), 2.0);
        // 16 clamps to 10, not 16 — `2^10`, not `2^16`.
        assert_density(modified_water_fog_density(2.0, 16.0, true), 1024.0);
    }

    /// BUG-233797 / BUG-233798: a negative density raised to a non-integral power is
    /// not a real number, and the `NaN` `powf` returns would reach the uniform and
    /// black out the whole screen. The reference forces the density to `1.0` in that
    /// case; so does this.
    #[test]
    fn a_negative_density_never_yields_a_non_real_result() {
        assert_density(modified_water_fog_density(-2.0, 0.25, true), 1.0);
        // An *integral* modifier needs no rescue — the power is real, and the
        // reference lets it through.
        assert_density(modified_water_fog_density(-2.0, 2.0, true), 4.0);
        // Integrality is tested after the clamp, as in the reference: 10.5 clamps to
        // 10, which is integral, so this is a plain (real) power, not a rescue.
        assert_density(modified_water_fog_density(-2.0, 10.5, true), 1024.0);
    }

    /// The end-to-end shape of that bug: a region whose water frame carries a
    /// negative density and a fractional modifier, with the eye under the surface.
    /// The uniform the fog shader reads must be a real number.
    #[test]
    fn a_hostile_water_frame_cannot_nan_the_uniform() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = App::new();
        let mut environment = EnvironmentState::default();
        // The values the reference's bug report describes, on every frame of the
        // cycle so the day position in force cannot pick a benign one.
        for water in environment.settings.day_cycle.water_frames.values_mut() {
            water.water_fog_density = -2.0;
            water.underwater_fog_mod = 0.25;
        }
        assert!(
            !environment.settings.day_cycle.water_frames.is_empty(),
            "the default environment defines a water frame to poison",
        );
        app.insert_resource(environment)
            .init_resource::<WaterLevel>()
            .add_systems(Update, update_underwater_fog);

        // Below the default water level: the eye is submerged, so the modifier
        // applies.
        let camera = app
            .world_mut()
            .spawn((
                ViewerCamera,
                Transform::from_xyz(0.0, -5.0, 0.0),
                GlobalTransform::default(),
                Projection::default(),
                UnderwaterFog::default(),
            ))
            .id();

        app.update();

        let fog = app
            .world()
            .entity(camera)
            .get::<UnderwaterFog>()
            .ok_or("the camera keeps its fog component")?;
        assert!(
            fog.fog_density.is_finite(),
            "the fog density stays a real number, got {}",
            fog.fog_density,
        );
        // …and is the reference's rescued density.
        assert_density(fog.fog_density, 1.0);
        Ok(())
    }
}
