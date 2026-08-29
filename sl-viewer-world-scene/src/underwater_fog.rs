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
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d_multisampled, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
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
///
/// The water *surface* shader needs the same value ([`crate::water::water_params`]):
/// the reference binds this one density to both (`lldrawpoolwater.cpp:242`,
/// `llsettingsvo.cpp:1130`).
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
                underwater_fog_system
                    .in_set(Core3dSystems::PostProcess)
                    .in_set(UnderwaterFogPass)
                    .before(tonemapping)
                    // Pin the post-process order fog → bloom → `SlTonemap` fully:
                    // all three run in `PostProcess`, and without this edge fog and
                    // bloom are unordered, so Bevy's parallel executor runs them in a
                    // different order on random frames — flipping the view-target
                    // ping-pong parity and flickering the whole screen.
                    .before(bevy::post_process::bloom::bloom),
            );
    }
}

/// The fog pipeline's global data (bind-group layout descriptor, sampler, and the
/// fullscreen vertex shader, which pipeline specialization needs per view format).
#[derive(Resource)]
struct UnderwaterFogPipeline {
    /// The bind-group layout descriptor (source texture, sampler, fog uniform,
    /// depth texture), resolved to a real layout per frame via the pipeline cache.
    layout: BindGroupLayoutDescriptor,
    /// The sampler used to read the scene colour texture.
    sampler: Sampler,
    /// The shared fullscreen-triangle vertex shader, needed by pipeline
    /// specialization (which has no world access to fetch it).
    fullscreen_shader: FullscreenShader,
}

/// Build the fog pipeline's shared data once, in the render world.
fn init_fog_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "underwater_fog_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The scene colour texture.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its sampler.
                sampler(SamplerBindingType::Filtering),
                // The per-frame fog parameters (dynamic-offset uniform).
                uniform_buffer::<UnderwaterFog>(true),
                // The (multisampled) depth prepass texture.
                texture_depth_2d_multisampled(),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    commands.insert_resource(UnderwaterFogPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
}

impl SpecializedRenderPipeline for UnderwaterFogPipeline {
    // The post-process source / destination format varies per view (HDR vs the
    // swapchain format), so specialize on it.
    type Key = TextureFormat;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("underwater_fog_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: FOG_SHADER_HANDLE,
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
struct UnderwaterFogPipelineId(CachedRenderPipelineId);

/// Specialize the fog pipeline for each view's target format.
fn prepare_fog_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<UnderwaterFogPipeline>>,
    pipeline: Res<UnderwaterFogPipeline>,
    views: Query<(Entity, &ExtractedView), With<ExtractedCamera>>,
) {
    for (entity, view) in &views {
        let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, view.target_format);
        commands
            .entity(entity)
            .insert(UnderwaterFogPipelineId(pipeline_id));
    }
}

/// The fog pass: reconstruct world position from depth and apply the water fog to
/// the scene colour for every pixel that is underwater.
fn underwater_fog_system(
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

    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let Some(uniform_binding) = uniforms.uniforms().binding() else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "underwater_fog_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline_res.sampler,
            uniform_binding.clone(),
            // The main-pass depth texture (made sampleable via
            // `Camera3d::depth_texture_usages`), from which the shader reconstructs
            // each fragment's world position.
            view_depth.view(),
        )),
    );

    let pass_descriptor = RenderPassDescriptor {
        label: Some("underwater_fog_pass"),
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
