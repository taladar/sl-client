//! Underwater fog (P23.1): a fullscreen post-process that reproduces the Second
//! Life / Firestorm water fog (`class1/environment/waterFogF.glsl`,
//! `getWaterFogViewNoClip` / `applyWaterFogViewLinear`) over the whole scene.
//!
//! The reference applies the water fog per fragment in the deferred stage, tinting
//! every underwater surface by the water body colour with a distance-based
//! transmittance and in-scatter, and clipping per fragment against the water plane
//! so a camera straddling the surface splits cleanly along the waterline. Its
//! deferred stage is the **opaque** scene, so this is one fullscreen pass over the
//! composited image plus the depth buffer — fogging terrain, objects, avatars and
//! the sky dome uniformly, exactly where they are underwater — and it runs at the
//! same point in the frame the reference's does: after the opaque pass, and before
//! the water surface and every translucent draw.
//!
//! **What this pass deliberately does not fog, and who does.** A translucent draw
//! writes no depth, so it is nowhere in the buffer this reads: fogging it here would
//! measure it by the distance of whatever is *behind* it — four kilometres of water
//! where that is the void, which erased it outright
//! (`viewer-underwater-fog-swallows-translucency`). The reference does not fog its
//! alpha pools here either; each alpha shader carries the same fog per fragment
//! (`alphaF.glsl`'s `WATER_FOG` branch), at the fragment's own position. So does
//! this viewer, through the shared [`sl_client_bevy::water_fog`] module: the face
//! material and the water surface's own underside each apply it themselves. (The
//! particle billboards do not yet —
//! `viewer-underwater-alpha-fog-remaining-materials`.) The **sky backdrops** need no
//! shader of their own: they are drawn *before* this pass
//! ([`crate::transparency`]'s backdrop pass, as the reference draws its WL sky pool
//! in the deferred stage), so the empty depth they leave is fogged here — which is
//! what makes the clouds disappear under the sea.
//! `SL_VIEWER_DISABLE_UNDERWATER_FOG=1` forces the whole thing off (a debug A/B
//! knob) — including the material-side fog, since it zeroes the density every
//! consumer reads.
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

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;

use crate::render_overrides::RenderOverrides;
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
use crate::water::drive_water;
use crate::water_fog::WaterFogSettings;
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
    /// The camera's far clip distance, in world metres — how far this frame draws
    /// anything at all. A pixel the depth buffer left empty is one nothing reached
    /// out to here, which is the distance the shader measures such a pixel's water
    /// column to. Bevy's perspective is reverse-Z **infinite**, so the far plane is
    /// not in the projection matrix and cannot be recovered from `world_from_clip`;
    /// it has to be carried.
    pub(crate) far_plane: f32,
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
            // Any positive distance does; the zero density above already makes the
            // pass a no-op until `update_underwater_fog` fills real values.
            far_plane: 1.0,
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

/// Fill the camera's [`UnderwaterFog`] from the scene's [`WaterFogSettings`], the
/// sky sun direction and the camera pose — the reference `LLSettingsVOWater`
/// uniform prep (`waterFogKS = 1 / max(lightDir.z, 0.3)`, and
/// `getModifiedWaterFogDensity`, which [`WaterFogSettings`] has already resolved
/// for both eye states).
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
    settings: Res<WaterFogSettings>,
    mut cameras: Query<(&Transform, &Projection, &mut UnderwaterFog), With<ViewerCamera>>,
) {
    for (camera_transform, projection, mut fog) in &mut cameras {
        let camera_pos = camera_transform.translation;
        let position = day_position(&environment);
        let sky = environment.sky_at(camera_pos.y, position);

        // world_from_clip = inverse(clip_from_view * view_from_world), to
        // reconstruct a fragment's world position from its depth in the shader.
        let clip_from_view = projection.get_clip_from_view();
        let view_from_world = camera_transform.to_matrix().inverse();
        // `mul_mat4` rather than the `*` operator, which trips the workspace
        // `arithmetic_side_effects` lint.
        let world_from_clip = clip_from_view.mul_mat4(&view_from_world).inverse();

        let water_height = settings.level;
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

        // The eye's own side of the surface picks the density, exactly as the
        // reference's `getModifiedWaterFogDensity` does — [`WaterFogSettings`] has
        // resolved both, and zeroed them if `SL_VIEWER_DISABLE_UNDERWATER_FOG` is
        // set (a zero density makes this shader a pass-through).
        let fog_density = if submerged {
            settings.density_submerged
        } else {
            settings.density_above
        };

        // Write-on-change: with a parked camera and stable water settings the
        // recomputed params are bit-identical, and an unconditional write would
        // mark the component changed every frame.
        fog.set_if_neq(UnderwaterFog {
            world_from_clip,
            camera_pos: camera_pos.extend(0.0),
            fog_color: settings.color.extend(0.0),
            water_height,
            fog_density,
            fog_ks,
            far_plane: projection.far(),
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
        app.init_resource::<RenderOverrides>();
        // The other half of the same effect: the fog parameters, and their delivery
        // to the materials that fog themselves. Added here so the two halves are
        // always registered together — a viewer with the haze but no material fog
        // would erase its own translucency again.
        app.add_plugins(crate::water_fog::WaterFogPlugin);
        sl_client_bevy::load_water_fog_shader(app);
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
                .after(drive_water)
                // …and after the fog parameters themselves, so the pass and the
                // materials that fog themselves are working from one resolution of
                // the region's water and not from two frames of it.
                .after(crate::water_fog::update_water_fog_settings),
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
                // Straight after the opaque geometry it fogs, and before everything
                // that is drawn over it: the pre-water translucency, the water
                // surface — whose refraction sample is a copy of what this pass has
                // just fogged, which is where the sea's colour comes from — and the
                // transparent phase. That is where the reference runs it too, over
                // its deferred render and ahead of its water and alpha pools, and it
                // is what leaves each translucent surface to fog itself by its own
                // distance rather than by the distance of whatever stands behind it
                // (`viewer-underwater-fog-swallows-translucency`).
                //
                // One pass for both eye states: which side of the surface the eye is
                // on changes only where the view ray enters the water, which the
                // shader works out per fragment.
                water_haze_system
                    .after(bevy::core_pipeline::core_3d::main_opaque_pass_3d)
                    // …and after the sky backdrops, which are drawn early exactly so
                    // that this pass fogs them: a backdrop writes no depth, so the
                    // pixel it paints still reads empty, which this measures out to
                    // the camera's far clip — how the reference makes clouds
                    // disappear under the sea.
                    .after(crate::transparency::SkyBackdropPass)
                    .before(crate::transparency::PreWaterPass)
                    .before(bevy::pbr::main_transmissive_pass_3d)
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
}

impl SpecializedRenderPipeline for UnderwaterFogPipeline {
    type Key = FogPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("water_haze_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: FOG_SHADER_HANDLE,
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

/// The specialized pipeline id for a view.
#[derive(Component)]
struct UnderwaterFogPipelineId(CachedRenderPipelineId);

/// Specialize the haze pipeline for each view's target format.
fn prepare_fog_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<UnderwaterFogPipeline>>,
    pipeline: Res<UnderwaterFogPipeline>,
    views: Query<(Entity, &ExtractedView, &Msaa), With<ExtractedCamera>>,
) {
    for (entity, view, msaa) in &views {
        let key = FogPipelineKey {
            format: view.target_format,
            samples: msaa.samples(),
        };
        commands
            .entity(entity)
            .insert(UnderwaterFogPipelineId(pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                key,
            )));
    }
}

/// The water-haze pass: fog every pixel of the **opaque** scene that lies under the
/// water surface, straight after that scene is drawn and before anything is drawn
/// over it.
///
/// This is the reference's `class3/deferred/waterHazeF.glsl`, which likewise runs
/// over its deferred render and before its water pool: it is what gives the sea its
/// colour, because the surface shows a *sample of this fogged scene* rather than a
/// tint of its own. It fogs from either side of the surface — an eye above the water
/// sees a fogged sea floor through the surface, an eye under it sees the fogged world
/// around it — the difference being only where the ray enters the water, which the
/// shader works out per fragment.
///
/// What it deliberately does not fog: anything drawn after it, which is everything
/// translucent, plus the water surface. Each of those fogs **itself**, per fragment,
/// through the shared [`sl_client_bevy::water_fog`] module — because none of them
/// writes depth, so none of them is in the buffer this pass reads, and fogging them
/// from it would measure each by the distance of whatever stands behind it. Where
/// that is the void the shader substitutes the camera's far clip, four kilometres of
/// water, and the surface was erased outright
/// (`viewer-underwater-fog-swallows-translucency`).
///
/// Runs inside the main pass rather than as a post-process, because under MSAA the
/// resolved texture a post-process would read and rewrite is discarded by the next
/// resolve; blending into the attachment is what actually reaches the frame.
fn water_haze_system(
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
        pipeline_id.0,
        view_target,
        fog_index,
        view_depth,
        &pipeline_cache,
        &pipeline_res,
        &uniforms,
        &mut ctx,
    );
}

/// Draw the haze pass: bind the fog uniform and the main-pass depth, and blend a
/// fullscreen triangle over the scene.
#[expect(
    clippy::too_many_arguments,
    reason = "the caller is a render system whose params this simply forwards"
)]
fn draw_haze(
    pipeline_id: CachedRenderPipelineId,
    view_target: &ViewTarget,
    fog_index: &DynamicUniformIndex<UnderwaterFog>,
    view_depth: &ViewDepthTexture,
    pipeline_cache: &PipelineCache,
    pipeline_res: &UnderwaterFogPipeline,
    uniforms: &ComponentUniforms<UnderwaterFog>,
    ctx: &mut RenderContext,
) {
    /// The debug label the pass, its bind group and its pipeline share.
    const LABEL: &str = "water_haze";
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id) else {
        return;
    };
    let Some(uniform_binding) = uniforms.uniforms().binding() else {
        return;
    };

    let bind_group = ctx.render_device().create_bind_group(
        Some(LABEL),
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
        label: Some(LABEL),
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

    use super::{UnderwaterFog, update_underwater_fog};
    use crate::environment::EnvironmentState;
    use crate::render_overrides::RenderOverrides;
    use crate::water::WaterLevel;
    use crate::water_fog::{WaterFogSettings, update_water_fog_settings};
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
            .init_resource::<RenderOverrides>()
            .init_resource::<WaterFogSettings>()
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

    /// A pixel the depth buffer left empty is measured out to the camera's **far
    /// clip**, so the uniform has to carry that distance — Bevy's perspective is
    /// reverse-Z *infinite*, so the far plane is not in the projection matrix and
    /// the shader cannot recover it from `world_from_clip`.
    ///
    /// It was a flat 2048 m before, far shorter than the 17 region cells of sea the
    /// viewer draws, which is what drew a hard ring across the open water
    /// (`viewer-sea-distance-band-hard-seam`): every ray that met the surface beyond
    /// 2048 m sampled a point still in the air and came out unfogged. So this pins
    /// that the frame's own far clip is what reaches the shader, whatever it is.
    #[test]
    fn carries_the_camera_far_clip() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = App::new();
        app.init_resource::<EnvironmentState>()
            .init_resource::<WaterLevel>()
            .init_resource::<RenderOverrides>()
            .init_resource::<WaterFogSettings>()
            .add_systems(Update, update_underwater_fog);

        let far = 4096.0;
        let camera = app
            .world_mut()
            .spawn((
                ViewerCamera,
                Transform::from_xyz(0.0, 60.0, 0.0),
                GlobalTransform::default(),
                Projection::Perspective(PerspectiveProjection { far, ..default() }),
                UnderwaterFog::default(),
            ))
            .id();

        app.update();

        let fog = app
            .world()
            .entity(camera)
            .get::<UnderwaterFog>()
            .ok_or("the camera keeps its fog component")?;
        // Carried verbatim, so this is exact equality — spelled as a difference
        // because a float `==` is a lint, not because any rounding is expected.
        assert!(
            (fog.far_plane - far).abs() < f32::EPSILON,
            "the fog measures an empty pixel out to this camera's far clip \
             ({far}), got {}",
            fog.far_plane,
        );
        Ok(())
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
            .init_resource::<RenderOverrides>()
            .init_resource::<WaterFogSettings>()
            // The whole chain, because what this pins is that a hostile water frame
            // cannot reach the shader: the settings resolve the density, and the
            // camera's uniform takes the one its eye state calls for.
            .add_systems(
                Update,
                (update_water_fog_settings, update_underwater_fog).chain(),
            );

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
