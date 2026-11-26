//! GPU-instanced particle rendering (`viewer-perf-gpu-particles`): draw every live
//! particle across a source's cloud as one instanced draw of a single shared
//! unit-quad mesh, expanding each billboard camera-facing in the vertex shader and
//! shading it (textured, per-instance tint, optional PBR lighting) in the fragment.
//!
//! **Why.** The Phase 30 render (`particles`) was fully CPU-side: every
//! frame, per source, it rebuilt a five-attribute billboard mesh (`build_cloud_mesh`)
//! and `meshes.insert`ed it — a full vertex-buffer re-upload per source per frame, plus
//! the camera-facing quad math on the CPU. This module replaces that with GPU
//! instancing: one quad mesh uploaded once at startup ([`ParticleQuad`]), and per source
//! a compact per-particle **instance buffer** ([`ParticleInstance`], 52 bytes/particle)
//! that is the *only* per-frame upload — reused in place ([`RawBufferVec`], reallocated
//! only when a cloud grows) rather than a fresh mesh. Textures resolve once per source
//! and stay put. The billboard expansion and PBR lighting move into
//! [`particle.wesl`](../particle.wesl).
//!
//! **How it fits Bevy 0.19.** The pipeline is derived from Bevy's [`MeshPipeline`]
//! (mirroring the `custom_shader_instancing` example) so it inherits the mesh-view bind
//! group at `@group(0)` — the lights / view uniforms the fragment's `apply_pbr_lighting`
//! needs — and the mesh bind group at `@group(2)`. A per-cloud material bind group at
//! `@group(3)` carries the source's diffuse texture + sampler. The custom shader never
//! reads the mesh transform (`get_world_from_local`): each particle carries its
//! **absolute world position** in the instance buffer, so a plain `draw_indexed` is
//! correct and the view keeps Bevy's GPU indirect drawing / culling for the rest of the
//! scene (no scene-wide `NoIndirectDrawing`, which the example needs only because its
//! shader reads the mesh transform). The one concession: every cloud instances the *same*
//! quad through the same pipeline, so the GPU-preprocessing batcher would merge
//! sort-adjacent clouds into a single draw and our per-item instance-buffer draw would
//! then render only the first — so each cloud entity carries
//! [`NoAutomaticBatching`](bevy::render::batching::NoAutomaticBatching) (see
//! `particles`) to keep its own draw.
//!
//! Per-view render-layer scoping (a HUD cloud draws only under the HUD camera, a world
//! cloud only under the fly camera — P35.4) falls out for free: `queue_particles`
//! honours each view's [`RenderVisibleEntities`], which `check_visibility` has already
//! filtered by [`RenderLayers`](bevy::camera::visibility::RenderLayers) (a
//! [`NoFrustumCulling`](bevy::camera::visibility::NoFrustumCulling) cloud stays in that list — only the frustum test is skipped).
//!
//! **Water fog.** A particle is alpha content blended at its own depth, which writes
//! none, so the fullscreen haze pass (`sl_viewer_world_scene::underwater_fog`) cannot
//! fog it: measured from the depth buffer it would be fogged by whatever stands behind
//! it. The reference fogs its particles per fragment, in the alpha pool
//! (`applySkyAndWaterFog` in `fullbrightF.glsl` / `alphaF.glsl`), and so does this
//! shader, from the scene-wide [`ParticleWaterFog`] that `sl-viewer-world-scene` fills.
//! The values reach the GPU in **one** uniform buffer every cloud's `@group(3)` binds
//! (`ParticleFogUniformBuffer`), rewritten in place when the fog changes, so a new
//! region or an environment edit reaches every live cloud without rebuilding a single
//! bind group.
//!
//! Reference (read-only): `LLVOPartGroup::getGeometry` — the camera-facing billboard the
//! vertex shader ports; particles are drawn `FULLBRIGHT` only when `EMISSIVE`
//! (`llvopartgroup.cpp:359`), so a non-emissive cloud is lit and an emissive / additive
//! / HUD cloud is unlit (the [`ParticleDrawParams::unlit`] pipeline specialization).

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexBufferLayout};
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, MeshPipelineSystems, RenderMeshInstances, SetMeshBindGroup,
    SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup, ViewKeyCache,
};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand as _, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, BlendComponent, BlendFactor, BlendOperation, BlendState, BufferId,
    BufferUsages, PipelineCache, RawBufferVec, RenderPipelineDescriptor, SamplerBindingType,
    ShaderStages, ShaderType, SpecializedMeshPipeline, SpecializedMeshPipelineError,
    SpecializedMeshPipelines, TextureSampleType, UniformBuffer, VertexAttribute, VertexFormat,
    VertexStepMode,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::sync_component::SyncComponent;
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, RenderVisibleEntities};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bytemuck::{Pod, Zeroable};

/// The internal handle the particle shader (`particle.wesl`) is loaded under.
const PARTICLE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2d7f6a10-9c4b-4e33-8a51-6f0b2c9d7e84");

/// One per-particle GPU instance record — the whole of what the vertex shader needs to
/// expand a camera-facing billboard. Packed `repr(C)` with no padding gaps so it is
/// [`Pod`] and maps straight to the instance-rate vertex buffer (`@location(3..=7)` in
/// [`particle.wesl`](../particle.wesl)).
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct ParticleInstance {
    /// The particle centre in **Bevy world space** (Y-up), absolute (the cloud entity is
    /// at the origin). Attribute `@location(3)`.
    pub position: [f32; 3],
    /// The billboard `(width, height)` in metres. Attribute `@location(4)`.
    pub scale: [f32; 2],
    /// The per-instance RGBA tint, `0.0..=1.0`. Attribute `@location(5)`.
    pub color: [f32; 4],
    /// The world-space velocity, for the `FOLLOW_VELOCITY` billboard re-orientation.
    /// Attribute `@location(6)`.
    pub velocity: [f32; 3],
    /// The per-particle flags (`part_flags::*`), read for `FOLLOW_VELOCITY`. Attribute
    /// `@location(7)`.
    pub flags: u32,
    /// The per-particle glow (`PSYS_PART_*_GLOW`, `0.0..=1.0`), interpolated over the
    /// particle's life. Fed into the scene alpha (the glow mask) by the **additive**
    /// path so a glowing particle blooms under the glow pass. Attribute `@location(8)`.
    pub glow: f32,
}

/// The per-cloud list of live particle instances, rebuilt each frame by
/// `drive_particles` and extracted to the render
/// world where `prepare_instance_buffers` uploads it. A component on the cloud entity.
#[derive(Component, Clone, Debug, Default)]
pub struct ParticleInstances {
    /// The live instances (one per live particle).
    pub instances: Vec<ParticleInstance>,
}

impl ExtractComponent<RenderApp> for ParticleInstances {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;

    /// Clone the cloud's instance list into the render world each frame.
    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(item.clone())
    }
}

impl SyncComponent<RenderApp> for ParticleInstances {
    type Target = Self;
}

/// How a cloud's particles blend into the frame — the alpha mode its blend function
/// implies. A whole system shares one blend function (it lives on the particle
/// template), so one choice per cloud drives the pipeline specialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleBlend {
    /// Ordinary source-over alpha blending (`SrcAlpha` / `OneMinusSrcAlpha`).
    Alpha,
    /// Additive blending (destination factor `ONE`) — the glow / fire look.
    Additive,
}

/// The per-cloud render parameters, extracted to the render world: the diffuse texture,
/// the blend mode, whether the cloud is drawn unlit, and its sort centre. A component on
/// the cloud entity, refreshed by `drive_particles`.
#[derive(Component, Clone, Debug)]
pub struct ParticleDrawParams {
    /// The resolved diffuse texture (the source's sprite, or the default soft blob).
    pub texture: Handle<Image>,
    /// The blend mode the source's blend function implies.
    pub blend: ParticleBlend,
    /// Whether the cloud renders unlit (emissive / additive / HUD) rather than lit.
    pub unlit: bool,
    /// The world-space centroid of the live particles, used as the transparency sort key
    /// (the cloud entity itself sits at the origin, so its transform is no help).
    pub sort_center: Vec3,
}

impl ExtractComponent<RenderApp> for ParticleDrawParams {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;

    /// Clone the cloud's render parameters into the render world each frame.
    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(item.clone())
    }
}

impl SyncComponent<RenderApp> for ParticleDrawParams {
    type Target = Self;
}

/// The scene's **water fog**, as the particle shader applies it to a billboard under
/// the surface: the authored fog colour, the density on each side of the surface, and
/// the surface height.
///
/// The same four values `sl_viewer_world_scene::water_fog::WaterFogSettings` resolves
/// for the face materials and the haze pass, mirrored here because the dependency runs
/// from the scene layer to this crate and not the other way. The default is a zero
/// density, which fogs nothing — the state of a scene with no water settings yet, of
/// the gallery, and of `SL_VIEWER_DISABLE_UNDERWATER_FOG=1`.
///
/// The `KS` term is not carried: the shader derives it from the view's directional
/// light, exactly as `face_material.wesl` does, so the two fog the same point alike.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleWaterFog {
    /// The **authored** (sRGB) water fog colour; the shader decodes it.
    pub color: Vec3,
    /// The fog density while the eye is above the surface.
    pub density_above: f32,
    /// The fog density while the eye is submerged.
    pub density_submerged: f32,
    /// The water surface height, in Bevy world metres (`y`).
    pub level: f32,
}

impl ExtractResource<RenderApp> for ParticleWaterFog {
    type Source = Self;

    /// Copy the fog into the render world; run only when it changed.
    fn extract_resource(source: &Self::Source) -> Self {
        *source
    }
}

/// [`ParticleWaterFog`] as `particle.wesl` declares it (`ParticleWaterFog` there):
/// the colour with the above-water density in `w`, the submerged density and the
/// level — the packing `face_material.wesl` uses for the same values.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
struct ParticleFogUniform {
    /// The authored fog colour (`xyz`) and the above-water density (`w`).
    color: Vec4,
    /// The density while the eye is submerged.
    submerged_density: f32,
    /// The water surface height.
    level: f32,
}

impl From<ParticleWaterFog> for ParticleFogUniform {
    /// Pack the fog the way the shader reads it.
    fn from(fog: ParticleWaterFog) -> Self {
        Self {
            color: fog.color.extend(fog.density_above),
            submerged_density: fog.density_submerged,
            level: fog.level,
        }
    }
}

/// The render world's one water-fog uniform buffer, bound by every cloud's
/// `@group(3)`. Rewritten in place by [`prepare_particle_fog`] when the fog changes;
/// the buffer itself is created once, so the bind groups built over it stay valid.
#[derive(Resource, Default)]
struct ParticleFogUniformBuffer {
    /// The GPU-side copy of the fog.
    buffer: UniformBuffer<ParticleFogUniform>,
}

/// The one shared unit-quad mesh every particle cloud instances — uploaded once at
/// startup ([`setup_particle_quad`]) and never rebuilt, replacing the per-source
/// per-frame dynamic mesh of Phase 30.
#[derive(Debug, Resource)]
pub struct ParticleQuad {
    /// The shared quad mesh handle.
    pub mesh: Handle<Mesh>,
}

/// Build the shared unit quad: a 1×1 square in the local XY plane centred on the origin,
/// its corner positions in `[-0.5, 0.5]`. The vertex shader reads a corner's `xy` as the
/// billboard offset (scaled by the instance size and the camera-facing basis), so the
/// quad only supplies corner offsets, UVs, a nominal `+Z` normal, and the two triangles.
fn particle_quad_mesh() -> Mesh {
    let positions: Vec<[f32; 3]> = vec![
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [-0.5, 0.5, 0.0],
        [0.5, 0.5, 0.0],
    ];
    // The reference's billboard texcoords, matching `LLVOPartGroup::getGeometry`'s corner
    // assignment for the four corners above (V runs top-down).
    let uvs: Vec<[f32; 2]> = vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]];
    let normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 1.0]; 4];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3]));
    mesh
}

/// Startup: build and upload the shared quad, storing its handle in [`ParticleQuad`].
pub fn setup_particle_quad(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let mesh = meshes.add(particle_quad_mesh());
    commands.insert_resource(ParticleQuad { mesh });
}

// ---------------------------------------------------------------------------
// Render world.
// ---------------------------------------------------------------------------

/// A cloud's uploaded instance buffer, kept on its render-world entity and reused in
/// place across frames ([`RawBufferVec`] reallocates only when the cloud grows).
#[derive(Component)]
struct InstanceBuffer {
    /// The GPU instance buffer (instance-rate vertex data).
    buffer: RawBufferVec<ParticleInstance>,
}

/// A cloud's material bind group (`@group(3)`: diffuse texture + sampler + the shared
/// water-fog uniform), cached on its render-world entity and rebuilt only when the
/// source's texture changes (or, defensively, if the fog buffer were ever recreated).
#[derive(Component)]
struct ParticleBindGroup {
    /// The bound texture + sampler + fog bind group.
    bind_group: BindGroup,
    /// The texture the bind group was built for; a change triggers a rebuild.
    texture: AssetId<Image>,
    /// The fog buffer the bind group was built over; a different one triggers a
    /// rebuild, so a cloud can never keep reading a buffer that is no longer written.
    fog_buffer: BufferId,
}

/// The custom particle render pipeline: Bevy's [`MeshPipeline`] (for the view / mesh bind
/// groups and the base descriptor), the particle shader, and the `@group(3)` material
/// layout.
#[derive(Resource)]
struct ParticlePipeline {
    /// The base mesh pipeline this specializes from (supplies the `@group(0..=2)`
    /// layouts and the view-key-dependent mesh-view bindings).
    mesh_pipeline: MeshPipeline,
    /// The particle shader (both vertex and fragment stages).
    shader: Handle<Shader>,
    /// The compiled `@group(3)` material bind-group layout (diffuse texture + sampler +
    /// water-fog uniform),
    /// used to build each cloud's bind group in [`prepare_particle_bind_groups`].
    material_layout: BindGroupLayout,
    /// The same layout as a descriptor, pushed onto each specialized pipeline's
    /// `@group(3)` (Bevy 0.19 stores pipeline layouts as descriptors the cache compiles).
    /// Built from the same entries, so the two are layout-compatible.
    material_layout_desc: BindGroupLayoutDescriptor,
}

/// The specialization key: the base mesh-pipeline key (view + topology bits) plus the two
/// per-cloud choices — unlit vs lit, and the blend mode.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ParticleKey {
    /// The inherited mesh-pipeline key (MSAA / tonemap / view-projection / topology).
    mesh_key: MeshPipelineKey,
    /// Whether this variant is drawn unlit (fullbright) rather than PBR-lit.
    unlit: bool,
    /// The blend mode this variant writes with.
    blend: ParticleBlend,
}

/// `RenderStartup`: build the [`ParticlePipeline`] once the [`MeshPipeline`] exists.
fn init_particle_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    mesh_pipeline: Res<MeshPipeline>,
) {
    let entries = BindGroupLayoutEntries::sequential(
        ShaderStages::FRAGMENT,
        (
            texture_2d(TextureSampleType::Float { filterable: true }),
            sampler(SamplerBindingType::Filtering),
            uniform_buffer::<ParticleFogUniform>(false),
        ),
    );
    let material_layout =
        render_device.create_bind_group_layout("particle_material_layout", &entries);
    let material_layout_desc = BindGroupLayoutDescriptor::new("particle_material_layout", &entries);
    commands.insert_resource(ParticlePipeline {
        mesh_pipeline: mesh_pipeline.clone(),
        shader: PARTICLE_SHADER_HANDLE,
        material_layout,
        material_layout_desc,
    });
}

/// The instance-rate vertex buffer layout, describing [`ParticleInstance`] to the
/// pipeline (attributes `@location(3..=7)`, one step per instance). The offsets are the
/// packed field offsets of the `repr(C)` struct.
fn instance_buffer_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: u64::try_from(size_of::<ParticleInstance>()).unwrap_or(0),
        step_mode: VertexStepMode::Instance,
        attributes: vec![
            // position: [f32; 3] @ offset 0
            VertexAttribute {
                format: VertexFormat::Float32x3,
                offset: 0,
                shader_location: 3,
            },
            // scale: [f32; 2] @ offset 12
            VertexAttribute {
                format: VertexFormat::Float32x2,
                offset: 12,
                shader_location: 4,
            },
            // color: [f32; 4] @ offset 20
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 20,
                shader_location: 5,
            },
            // velocity: [f32; 3] @ offset 36
            VertexAttribute {
                format: VertexFormat::Float32x3,
                offset: 36,
                shader_location: 6,
            },
            // flags: u32 @ offset 48
            VertexAttribute {
                format: VertexFormat::Uint32,
                offset: 48,
                shader_location: 7,
            },
            // glow: f32 @ offset 52
            VertexAttribute {
                format: VertexFormat::Float32,
                offset: 52,
                shader_location: 8,
            },
        ],
    }
}

impl SpecializedMeshPipeline for ParticlePipeline {
    type Key = ParticleKey;

    /// Specialize the base mesh pipeline for a particle cloud: swap in the particle shader
    /// for both stages, append the instance-rate vertex buffer and the `@group(3)`
    /// material layout, select the blend + depth state, and (when unlit) push the
    /// `PARTICLE_UNLIT` shader def. The inherited `shader_defs` from the mesh pipeline are
    /// kept untouched (they must match the view bind-group layout `@group(0)`).
    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        // Fold the alpha bits into the mesh key so `MeshPipeline` sets a sane transparent
        // target + disables depth writes; additive overrides the target blend below.
        let mesh_key = key.mesh_key | MeshPipelineKey::BLEND_ALPHA;
        let mut descriptor = self.mesh_pipeline.specialize(mesh_key, layout)?;

        descriptor.vertex.shader = self.shader.clone();
        descriptor.vertex.buffers.push(instance_buffer_layout());
        descriptor.layout.push(self.material_layout_desc.clone());

        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = self.shader.clone();
            if key.unlit {
                fragment.shader_defs.push("PARTICLE_UNLIT".into());
            }
            if key.blend == ParticleBlend::Additive {
                // The additive path carries the per-particle glow in alpha:
                // `PARTICLE_ADDITIVE` makes the fragment write `glow` (not coverage) to
                // alpha, and the additive alpha blend `(One, One)` accumulates it into the
                // scene alpha — the glow mask — so a glowing particle blooms under the
                // glow pass (`glow`), weighted by particle density.
                fragment.shader_defs.push("PARTICLE_ADDITIVE".into());
                if let Some(Some(target)) = fragment.targets.first_mut() {
                    // Additive (destination `ONE`): the glow / fire look.
                    target.blend = Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::One,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::One,
                            operation: BlendOperation::Add,
                        },
                    });
                }
            }
        }
        // Particles never write depth (they are unsorted transparent billboards); the
        // `BLEND_ALPHA` branch already set this, but make it explicit for the additive
        // override path too.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        // A non-additive (alpha-blend) particle keeps its coverage in alpha for the
        // colour blend, so it must not corrupt the glow mask: override its alpha blend
        // to `(Zero, One)`. Such a particle therefore does not glow (a documented
        // limitation — glowing particles are the additive / emissive kind, handled
        // above). The additive path deliberately keeps `(One, One)` to feed glow to the
        // mask, so it is excluded here.
        if key.blend != ParticleBlend::Additive {
            sl_client_bevy::preserve_glow_mask_alpha(&mut descriptor);
        }
        Ok(descriptor)
    }
}

/// `PrepareResources`: upload each cloud's instance list into its (reused) GPU buffer.
/// The [`RawBufferVec`] reallocates only when a cloud grows; an unchanged or smaller
/// cloud reuses the existing buffer and just re-copies, so no new allocation happens
/// frame to frame.
fn prepare_instance_buffers(
    mut commands: Commands,
    mut clouds: Query<(Entity, &ParticleInstances, Option<&mut InstanceBuffer>)>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    for (entity, instances, buffer) in &mut clouds {
        match buffer {
            Some(mut existing) => {
                existing.buffer.clear();
                existing.buffer.extend(instances.instances.iter().copied());
                existing.buffer.write_buffer(&render_device, &render_queue);
            }
            None => {
                let mut buffer = RawBufferVec::new(BufferUsages::VERTEX | BufferUsages::COPY_DST);
                buffer.extend(instances.instances.iter().copied());
                buffer.write_buffer(&render_device, &render_queue);
                commands.entity(entity).insert(InstanceBuffer { buffer });
            }
        }
    }
}

/// `PrepareResources`: write the scene's water fog into the shared uniform buffer, on
/// the frame it changed (and on the first, when the buffer is created). A settled scene
/// writes nothing.
fn prepare_particle_fog(
    fog: Res<ParticleWaterFog>,
    mut uniform: ResMut<ParticleFogUniformBuffer>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    if !fog.is_changed() && uniform.buffer.buffer().is_some() {
        return;
    }
    uniform.buffer.set(ParticleFogUniform::from(*fog));
    uniform.buffer.write_buffer(&render_device, &render_queue);
}

/// `PrepareBindGroups`: build each cloud's `@group(3)` material bind group from its
/// diffuse texture and the shared fog buffer, rebuilding only when either changes (or on
/// first sight, once the [`GpuImage`] has uploaded). A cloud whose texture has not
/// finished uploading keeps no bind group and is skipped by the draw command until it is
/// ready.
fn prepare_particle_bind_groups(
    mut commands: Commands,
    clouds: Query<(Entity, &ParticleDrawParams, Option<&ParticleBindGroup>)>,
    pipeline: Res<ParticlePipeline>,
    fog: Res<ParticleFogUniformBuffer>,
    images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
) {
    // Written by `prepare_particle_fog` earlier in the same frame, so this only
    // fails if that system did not run — and then no cloud can be drawn correctly.
    let Some(fog_buffer) = fog.buffer.buffer() else {
        return;
    };
    let fog_buffer_id = fog_buffer.id();
    for (entity, params, existing) in &clouds {
        let texture_id = params.texture.id();
        if existing.is_some_and(|bind_group| {
            bind_group.texture == texture_id && bind_group.fog_buffer == fog_buffer_id
        }) {
            continue;
        }
        let Some(gpu_image) = images.get(texture_id) else {
            continue;
        };
        let bind_group = render_device.create_bind_group(
            "particle_material_bind_group",
            &pipeline.material_layout,
            &BindGroupEntries::sequential((
                &gpu_image.texture_view,
                &gpu_image.sampler,
                fog_buffer.as_entire_binding(),
            )),
        );
        commands.entity(entity).insert(ParticleBindGroup {
            bind_group,
            texture: texture_id,
            fog_buffer: fog_buffer_id,
        });
    }
}

/// What specialising a particle cloud's pipeline needs, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the pipeline and its
/// specialisation cache, the pipeline cache a specialised id is queued in, and
/// the render meshes a cloud's key is read from.
#[derive(bevy::ecs::system::SystemParam)]
struct ParticlePipelines<'w> {
    /// The particle pipeline itself.
    pipeline: Res<'w, ParticlePipeline>,
    /// Its per-key specialisations.
    specialized: ResMut<'w, SpecializedMeshPipelines<ParticlePipeline>>,
    /// Where a specialised pipeline is queued for compilation.
    pipeline_cache: Res<'w, PipelineCache>,
    /// The render meshes a cloud's mesh key is read from.
    meshes: Res<'w, RenderAssets<RenderMesh>>,
    /// Which render mesh each cloud instance uses.
    render_mesh_instances: Res<'w, RenderMeshInstances>,
}

/// `QueueMeshes`: queue every visible particle cloud into each view's transparent phase,
/// specialized for its blend / lit variant. Only clouds in a view's
/// [`RenderVisibleEntities`] are queued, which is what scopes a HUD cloud to the HUD
/// camera and a world cloud to the fly camera (the render-layer filter already ran in
/// `check_visibility`).
fn queue_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipelines: ParticlePipelines,
    clouds: Query<(Entity, &MainEntity, &ParticleDrawParams)>,
    mut transparent_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    view_key_cache: Res<ViewKeyCache>,
    views: Query<(&ExtractedView, &RenderVisibleEntities, &ExtractedCamera)>,
) {
    let ParticlePipelines {
        pipeline,
        mut specialized,
        pipeline_cache,
        meshes,
        render_mesh_instances,
    } = pipelines;
    let draw_particles = draw_functions.read().id::<DrawParticles>();

    for (view, visible_entities, camera) in &views {
        // Skip the reflection-probe capture cameras (order `-1`, ahead of the world view
        // at order `0` and the HUD at order `2`). Drawing particles into the six cube
        // faces of a probe's refresh burst both froze a particle snapshot into the
        // probe's image-based lighting and cost a periodic multi-view render spike; the
        // reference viewer likewise does not draw particles into reflection captures.
        if camera.order < 0 {
            continue;
        }
        let Some(transparent_phase) = transparent_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        // The main entities this view actually sees (render-layer + visibility filtered);
        // for 3D meshes the render entity in the list is a placeholder, so key on the
        // `MainEntity` and match it against our clouds below. A `NoFrustumCulling` cloud
        // lands in the CPU-culling list; the GPU-culling table is folded in for safety.
        let mut visible: HashSet<MainEntity> = HashSet::new();
        if let Some(class) = visible_entities.get::<Mesh3d>() {
            for (_render_entity, main_entity) in &class.entities_cpu_culling {
                visible.insert(*main_entity);
            }
            for main_entity in class.entities_gpu_culling.keys() {
                visible.insert(*main_entity);
            }
        }

        for (entity, main_entity, params) in &clouds {
            if !visible.contains(main_entity) {
                continue;
            }
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*main_entity)
            else {
                continue;
            };
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let mesh_key = view_key
                | MeshPipelineKey::from_primitive_topology_and_strip_index(
                    mesh.primitive_topology(),
                    mesh.index_format(),
                );
            let key = ParticleKey {
                mesh_key,
                unlit: params.unlit,
                blend: params.blend,
            };
            let pipeline_id =
                match specialized.specialize(&pipeline_cache, &pipeline, key, &mesh.layout) {
                    Ok(id) => id,
                    Err(error) => {
                        error!("particle pipeline specialization failed: {error}");
                        continue;
                    }
                };
            transparent_phase.add_retained(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: params.sort_center,
                    depth_bias: 0.0,
                },
                entity: (entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: draw_particles,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: true,
            });
        }
    }
}

/// Bind a cloud's `@group(3)` material bind group (diffuse texture + sampler + fog), read from
/// the [`ParticleBindGroup`] component on the item entity. Modelled on Bevy's
/// `SetMaterialBindGroup`, but per-item rather than per-material-asset.
struct SetParticleMaterialBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetParticleMaterialBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<ParticleBindGroup>;

    /// Bind the cloud's texture bind group, or skip the draw if it is not ready yet.
    #[expect(
        clippy::renamed_function_params,
        reason = "the item-query param is a bind group here, clearer than the trait's `entity`"
    )]
    fn render<'w>(
        _item: &P,
        _view: (),
        bind_group: Option<&'w ParticleBindGroup>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bind_group.bind_group, &[]);
        RenderCommandResult::Success
    }
}

/// Issue the instanced draw: bind the shared quad's vertex / index slices and the cloud's
/// instance buffer, then `draw_indexed` the quad once per live particle. A direct draw
/// (not indirect) is correct because the shader never reads the mesh transform, so the
/// view keeps Bevy's GPU indirect drawing for everything else.
struct DrawParticleInstanced;

impl<P: PhaseItem> RenderCommand<P> for DrawParticleInstanced {
    type Param = (
        SRes<RenderAssets<RenderMesh>>,
        SRes<RenderMeshInstances>,
        SRes<MeshAllocator>,
    );
    type ViewQuery = ();
    type ItemQuery = Read<InstanceBuffer>;

    /// Bind the quad + instance buffer and draw one quad per particle.
    #[expect(
        clippy::renamed_function_params,
        reason = "the item-query param is the instance buffer here, clearer than the trait's `entity`"
    )]
    fn render<'w>(
        item: &P,
        _view: (),
        instance_buffer: Option<&'w InstanceBuffer>,
        (meshes, render_mesh_instances, mesh_allocator): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let mesh_allocator = mesh_allocator.into_inner();

        let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(item.main_entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(gpu_mesh) = meshes.into_inner().get(mesh_instance.mesh_asset_id()) else {
            return RenderCommandResult::Skip;
        };
        let Some(instance_buffer) = instance_buffer else {
            return RenderCommandResult::Skip;
        };
        let Some(buffer) = instance_buffer.buffer.buffer() else {
            return RenderCommandResult::Skip;
        };
        let instance_count = u32::try_from(instance_buffer.buffer.len()).unwrap_or(0);
        if instance_count == 0 {
            return RenderCommandResult::Skip;
        }
        let Some(vertex_slice) = mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id())
        else {
            return RenderCommandResult::Skip;
        };

        pass.set_vertex_buffer(0, vertex_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, buffer.slice(..));

        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(index_slice) =
                    mesh_allocator.mesh_index_slice(&mesh_instance.mesh_asset_id())
                else {
                    return RenderCommandResult::Skip;
                };
                let first = index_slice.range.start;
                let last = first.saturating_add(*count);
                let base_vertex = i32::try_from(vertex_slice.range.start).unwrap_or(0);
                pass.set_index_buffer(index_slice.buffer.slice(..), *index_format);
                pass.draw_indexed(first..last, base_vertex, 0..instance_count);
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_slice.range.clone(), 0..instance_count);
            }
        }
        RenderCommandResult::Success
    }
}

/// The full draw sequence for a particle cloud: pipeline, the mesh-view bind groups
/// (`@group(0..=1)`, supplying lights / view for the fragment's PBR lighting), the mesh
/// bind group (`@group(2)`), the cloud's material bind group (`@group(3)`), then the
/// instanced draw.
type DrawParticles = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetMeshBindGroup<2>,
    SetParticleMaterialBindGroup<3>,
    DrawParticleInstanced,
);

/// Loads the particle shader and wires the custom instanced render pipeline into the
/// render app. Add once to the [`App`] (after `DefaultPlugins`), like the other viewer
/// material plugins.
#[derive(Debug, Default)]
pub struct ParticleRenderPlugin;

impl Plugin for ParticleRenderPlugin {
    /// Compile `particle.wesl`, register the extract plugins for the per-cloud components,
    /// and set up the render-app pipeline / draw command / prepare / queue systems.
    fn build(&self, app: &mut App) {
        // The particle shader decodes its sRGB sprite samples with the shared module,
        // and fogs a billboard under the surface with the shared water-fog one.
        sl_client_bevy::load_srgb_shader(app);
        sl_client_bevy::load_water_fog_shader(app);
        load_internal_asset!(app, PARTICLE_SHADER_HANDLE, "particle.wesl", |source, _| {
            Shader::from_wesl(source, "embedded://sl_viewer_kit/particle.wesl")
        });
        app.add_plugins((
            ExtractComponentPlugin::<ParticleInstances>::default(),
            ExtractComponentPlugin::<ParticleDrawParams>::default(),
            ExtractResourcePlugin::<ParticleWaterFog>::default(),
        ));
        // No fog until the scene layer fills it in — and the gallery, which has no
        // water, never does.
        app.init_resource::<ParticleWaterFog>();
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_render_command::<Transparent3d, DrawParticles>()
            .init_resource::<SpecializedMeshPipelines<ParticlePipeline>>()
            .init_resource::<ParticleWaterFog>()
            .init_resource::<ParticleFogUniformBuffer>()
            .add_systems(
                RenderStartup,
                init_particle_pipeline.after(MeshPipelineSystems),
            )
            .add_systems(
                Render,
                (
                    queue_particles.in_set(RenderSystems::QueueMeshes),
                    prepare_instance_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_particle_fog.in_set(RenderSystems::PrepareResources),
                    prepare_particle_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParticleFogUniform, ParticleInstance, ParticleWaterFog, instance_buffer_layout,
        particle_quad_mesh,
    };
    use bevy::math::{Vec3, Vec4};
    use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
    use bevy::render::render_resource::ShaderType as _;
    use bevy::render::render_resource::{VertexFormat, VertexStepMode};
    use pretty_assertions::assert_eq;

    /// The shared quad is a single unit square: four corners in `[-0.5, 0.5]`, two
    /// triangles, with the position / normal / UV attributes the mesh pipeline needs.
    #[test]
    fn quad_is_one_unit_square() {
        let mesh = particle_quad_mesh();
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::TriangleList);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("the quad has float3 positions")
        };
        assert_eq!(positions.len(), 4);
        for position in positions {
            assert!(
                (position[0].abs() - 0.5).abs() < 1.0e-6,
                "corner x not ±0.5"
            );
            assert!(
                (position[1].abs() - 0.5).abs() < 1.0e-6,
                "corner y not ±0.5"
            );
            assert!(position[2].abs() < 1.0e-6, "corner not in the z=0 plane");
        }
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        assert_eq!(mesh.indices().map(Indices::len), Some(6));
    }

    /// The instance-rate vertex layout is packed and contiguous: its stride equals the
    /// `repr(C)` struct size (so the CPU buffer maps 1:1 to the GPU attributes), its
    /// attributes step per instance and occupy `@location(3..=7)`, and each field's
    /// declared offset+size is contiguous with the next (no overlap, no gap the shader
    /// would misread).
    #[test]
    fn instance_layout_is_packed_and_contiguous() {
        let layout = instance_buffer_layout();
        assert_eq!(layout.step_mode, VertexStepMode::Instance);
        assert_eq!(
            layout.array_stride,
            u64::try_from(size_of::<ParticleInstance>()).unwrap_or(0)
        );
        let locations: Vec<u32> = layout
            .attributes
            .iter()
            .map(|attribute| attribute.shader_location)
            .collect();
        assert_eq!(locations, vec![3, 4, 5, 6, 7, 8]);
        // Each attribute begins exactly where the previous one ended.
        let mut expected_offset = 0_u64;
        for attribute in &layout.attributes {
            assert_eq!(
                attribute.offset, expected_offset,
                "attribute at location {} is not contiguous",
                attribute.shader_location
            );
            expected_offset = expected_offset.saturating_add(attribute.format.size());
        }
        // …and the packed attributes fill the whole stride.
        assert_eq!(expected_offset, layout.array_stride);
    }

    /// The last attribute is the `u32` flags word (the shader reads `FOLLOW_VELOCITY`
    /// out of it) — a guard that the flags field did not silently become a float.
    #[test]
    fn flags_attribute_is_a_u32() {
        let layout = instance_buffer_layout();
        let Some(flags) = layout
            .attributes
            .iter()
            .find(|attribute| attribute.shader_location == 7)
        else {
            unreachable!("the layout has a location-7 attribute")
        };
        assert_eq!(flags.format, VertexFormat::Uint32);
    }

    /// The fog packs the way `particle.wesl` reads it — the above-water density in
    /// the colour's `w`, as `face_material.wesl` packs the same values — and the
    /// uniform is the 32 bytes the shader's `ParticleWaterFog` struct occupies.
    #[test]
    fn the_fog_packs_as_the_shader_reads_it() {
        let packed = ParticleFogUniform::from(ParticleWaterFog {
            color: Vec3::new(0.1, 0.2, 0.3),
            density_above: 4.0,
            density_submerged: 2.0,
            level: 20.0,
        });
        assert_eq!(
            packed,
            ParticleFogUniform {
                color: Vec4::new(0.1, 0.2, 0.3, 4.0),
                submerged_density: 2.0,
                level: 20.0,
            }
        );
        assert_eq!(ParticleFogUniform::min_size().get(), 32);
    }

    /// The default fog fogs nothing: a zero density is the shader's "no fog" gate, so
    /// a scene that never fills the resource in (the gallery) draws its particles
    /// exactly as before.
    #[test]
    fn the_default_fog_has_no_density() {
        let fog = ParticleWaterFog::default();
        assert_eq!(fog.density_above.to_bits(), 0.0_f32.to_bits());
        assert_eq!(fog.density_submerged.to_bits(), 0.0_f32.to_bits());
    }
}
