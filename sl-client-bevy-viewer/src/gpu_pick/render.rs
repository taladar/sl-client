//! The GPU pick pass's **render-world half**: the two specialized pipelines
//! (static / skinned over Bevy's mesh vertex layouts), the per-submission
//! prepare (dynamic per-draw uniforms + bind groups against this frame's
//! post-swap `SkinUniforms.current_buffer`), and the pass encoding — a tiny
//! [`super::CROP_SIZE`]² `Rgba32Uint` + `Depth32Float` render late in the
//! `Core3d` stream, **after** the GPU-avatar compute wrote this frame's
//! palettes (§6.3: the ID buffer reads the same palette buffer the visible
//! pass reads, so there is no second source of truth to drift).

use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::SkinUniforms;
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction, DepthBiasState,
    DepthStencilState, DynamicUniformBuffer, Extent3d, FragmentState, LoadOp, Operations,
    PipelineCache, PrimitiveState, PrimitiveTopology, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages,
    ShaderType, SpecializedMeshPipeline, SpecializedMeshPipelineError, SpecializedMeshPipelines,
    StencilState, StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderStartup, RenderSystems};

use super::{CROP_SIZE, GpuPickSubmission, GpuPickTargets, GpuPickWarmSet, PICK_SHADER_HANDLE};

/// The ID target's texel format: `(tag, depth bits, sequence, 0)`.
const ID_FORMAT: TextureFormat = TextureFormat::Rgba32Uint;

/// The pick depth attachment's format.
const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;

/// One per-draw uniform row (dynamic offsets into one buffer).
#[derive(Clone, Copy, ShaderType)]
struct GpuPickUniform {
    /// Static: cropped `clip_from_world * world_from_local`; skinned: the
    /// cropped `clip_from_world` alone.
    clip_from_local: Mat4,
    /// The encoded pick tag.
    tag: u32,
    /// The submission sequence.
    sequence: u32,
    /// Skinned: the entity's first palette row in the skin buffer.
    skin_base: u32,
    /// Padding to a 16-byte boundary.
    pad: u32,
}

/// The pipeline-variant key: one static and one skinned pipeline per mesh
/// vertex layout.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GpuPickKey {
    /// Whether the variant runs the palette-skinning vertex path.
    skinned: bool,
}

/// The pick pipelines' shared state: the two bind-group layouts.
#[derive(Resource)]
struct GpuPickPipeline {
    /// The static variant's layout: the per-draw uniform only.
    static_layout: BindGroupLayoutDescriptor,
    /// The skinned variant's layout: the uniform + the skin palette buffer.
    skinned_layout: BindGroupLayoutDescriptor,
}

impl SpecializedMeshPipeline for GpuPickPipeline {
    type Key = GpuPickKey;

    /// Build the descriptor for one (variant, mesh-layout) pair: position (+
    /// joint indices / weights for the skinned variant) against the mesh's
    /// own vertex layout, the `Rgba32Uint` ID target, reverse-Z depth
    /// (matching the main camera's projection the crop derives from), and no
    /// culling (avatar clothing and prim interiors pick from both sides,
    /// like the CPU ray test did).
    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
        let mut shader_defs = Vec::new();
        if key.skinned {
            attributes.push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(1));
            attributes.push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(2));
            shader_defs.push("SKINNED".into());
        }
        let vertex_layout = layout.0.get_layout(&attributes)?;
        Ok(RenderPipelineDescriptor {
            label: Some(if key.skinned {
                "gpu_pick_pipeline_skinned".into()
            } else {
                "gpu_pick_pipeline_static".into()
            }),
            layout: vec![if key.skinned {
                self.skinned_layout.clone()
            } else {
                self.static_layout.clone()
            }],
            vertex: VertexState {
                shader: PICK_SHADER_HANDLE,
                shader_defs: shader_defs.clone(),
                entry_point: Some("vertex".into()),
                buffers: vec![vertex_layout],
            },
            fragment: Some(FragmentState {
                shader: PICK_SHADER_HANDLE,
                shader_defs,
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: ID_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            ..Default::default()
        })
    }
}

/// Create the pick bind-group layouts.
fn init_gpu_pick_pipeline(mut commands: Commands) {
    let static_layout = BindGroupLayoutDescriptor::new(
        "gpu_pick_static_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (uniform_buffer::<GpuPickUniform>(true),),
        ),
    );
    let skinned_layout = BindGroupLayoutDescriptor::new(
        "gpu_pick_skinned_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<GpuPickUniform>(true),
                storage_buffer_read_only_sized(false, None),
            ),
        ),
    );
    commands.insert_resource(GpuPickPipeline {
        static_layout,
        skinned_layout,
    });
}

/// The reusable GPU-side pieces: the per-draw uniform buffer and the (lazily
/// created) depth attachment.
#[derive(Resource, Default)]
struct GpuPickBuffers {
    /// The per-draw uniform rows, rewritten every submission.
    uniforms: DynamicUniformBuffer<GpuPickUniform>,
    /// The pick view's depth attachment, created once.
    depth: Option<TextureView>,
}

/// One prepared draw of this frame's pick pass.
struct PickDraw {
    /// The specialized pipeline (may still be compiling on first use).
    pipeline: CachedRenderPipelineId,
    /// The mesh whose vertex/index slabs to draw.
    mesh: AssetId<Mesh>,
    /// The draw's dynamic offset into the uniform buffer.
    uniform_offset: u32,
    /// Whether to bind the skinned bind group.
    skinned: bool,
}

/// This frame's prepared pick pass, or `None` when no pick was submitted.
/// Taken (consumed) by the first `Core3d` view that encodes it.
#[derive(Resource, Default)]
struct PreparedGpuPick(Option<PreparedPickData>);

/// The data behind [`PreparedGpuPick`].
struct PreparedPickData {
    /// The sequence rendered into the clear color (readback correlation).
    sequence: u32,
    /// The draws under the crop.
    draws: Vec<PickDraw>,
    /// The static variant's bind group.
    static_bind: BindGroup,
    /// The skinned variant's bind group (present when any draw needs it).
    skinned_bind: Option<BindGroup>,
}

/// Kick off compilation of both pick pipeline variants for every currently
/// pickable mesh layout, independent of whether a pick is pending — so
/// `PipelineCache` starts compiling a mesh's pick pipeline the moment it
/// rezzes (avatar submeshes, prim faces, terrain, water) rather than only on
/// the first pick over it, eliminating the "first pick may miss while the
/// pick pipeline compiles" latency. Uses the *same* [`GpuPickKey`] and the
/// mesh's *real* [`MeshVertexBufferLayoutRef`] (via `RenderAssets<RenderMesh>`)
/// that [`prepare_gpu_pick`] specializes against, so the warmed pipeline is
/// the one an actual pick draws with — never a synthetic layout that would
/// warm nothing. `specialize` is a cheap cache lookup once a layout is
/// compiled (or compiling), so calling it again every frame for an
/// already-warm layout is harmless; no separate warmed-set tracking is kept.
fn warm_gpu_pick_pipelines(
    warm_set: Res<GpuPickWarmSet>,
    pipeline: Option<Res<GpuPickPipeline>>,
    mut specialized: ResMut<SpecializedMeshPipelines<GpuPickPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
) {
    let Some(pipeline) = pipeline else {
        return;
    };
    for &(skinned, mesh_id) in &warm_set.0 {
        let Some(mesh) = meshes.get(mesh_id) else {
            // Not yet uploaded to the render world; the next frame's warm
            // set (or the pick that eventually targets it) retries.
            continue;
        };
        let key = GpuPickKey { skinned };
        let Ok(_pipeline_id) =
            specialized.specialize(&pipeline_cache, &pipeline, key, &mesh.layout)
        else {
            // A mesh without the variant's attributes cannot be picked
            // through this variant; nothing to warm.
            continue;
        };
    }
}

/// Prepare this frame's pick pass from the extracted submission: specialize
/// the pipelines against each candidate's mesh layout, resolve skinned
/// candidates' palette offsets against **this frame's post-swap**
/// `SkinUniforms` (this system runs in `PrepareBindGroups`, after Bevy's
/// `prepare_skins`), write the per-draw uniforms, and build the bind groups.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy render-world system's inputs are its parameters: the extracted \
              submission, the pipeline/buffer/prepared resources, the specializer + cache, \
              the mesh assets, Bevy's skin uniforms and the device/queue pair"
)]
fn prepare_gpu_pick(
    submission: Res<GpuPickSubmission>,
    pipeline: Option<Res<GpuPickPipeline>>,
    mut buffers: ResMut<GpuPickBuffers>,
    mut prepared: ResMut<PreparedGpuPick>,
    mut specialized: ResMut<SpecializedMeshPipelines<GpuPickPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    skin_uniforms: Res<SkinUniforms>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    prepared.0 = None;
    if !submission.active {
        return;
    }
    let Some(pipeline) = pipeline else {
        return;
    };

    buffers.uniforms.clear();
    let mut draws = Vec::with_capacity(submission.items.len());
    let mut any_skinned = false;
    for item in &submission.items {
        let Some(mesh) = meshes.get(item.mesh) else {
            continue;
        };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            continue;
        }
        let skin_base = if item.skinned {
            match skin_uniforms.skin_index(MainEntity::from(item.entity)) {
                Some(offset) => offset,
                // Not (yet) registered with Bevy's skin allocator: skip this
                // frame; the next pick re-resolves.
                None => continue,
            }
        } else {
            0
        };
        let key = GpuPickKey {
            skinned: item.skinned,
        };
        let Ok(pipeline_id) = specialized.specialize(&pipeline_cache, &pipeline, key, &mesh.layout)
        else {
            // A mesh without the variant's attributes (e.g. a skinned draw
            // over a mesh missing joint data) cannot be picked; skip it.
            continue;
        };
        let uniform_offset = buffers.uniforms.push(&GpuPickUniform {
            clip_from_local: item.clip_from_local,
            tag: item.tag,
            sequence: submission.sequence,
            skin_base,
            pad: 0,
        });
        any_skinned |= item.skinned;
        draws.push(PickDraw {
            pipeline: pipeline_id,
            mesh: item.mesh,
            uniform_offset,
            skinned: item.skinned,
        });
    }
    if draws.is_empty() {
        // Nothing under the crop: push one dummy row so the uniform binding
        // (and thus the bind group) exists — the pass still needs to render
        // the sequence-carrying clear for the miss readback.
        buffers.uniforms.push(&GpuPickUniform {
            clip_from_local: Mat4::IDENTITY,
            tag: 0,
            sequence: submission.sequence,
            skin_base: 0,
            pad: 0,
        });
    }
    buffers.uniforms.write_buffer(&render_device, &render_queue);

    // The depth attachment, created once (the crop size is constant).
    if buffers.depth.is_none() {
        let texture = render_device.create_texture(&TextureDescriptor {
            label: Some("gpu_pick_depth"),
            size: Extent3d {
                width: CROP_SIZE,
                height: CROP_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        buffers.depth = Some(texture.create_view(&TextureViewDescriptor::default()));
    }

    // Even a draw-less submission (nothing under the cursor) must render the
    // clear, so the readback sees this sequence and resolves the miss.
    let Some(uniform_binding) = buffers.uniforms.binding() else {
        return;
    };
    let static_bind = render_device.create_bind_group(
        "gpu_pick_static_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.static_layout),
        &BindGroupEntries::sequential((uniform_binding.clone(),)),
    );
    let skinned_bind = any_skinned.then(|| {
        render_device.create_bind_group(
            "gpu_pick_skinned_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipeline.skinned_layout),
            &BindGroupEntries::sequential((
                uniform_binding,
                skin_uniforms.current_buffer.as_entire_binding(),
            )),
        )
    });
    prepared.0 = Some(PreparedPickData {
        sequence: submission.sequence,
        draws,
        static_bind,
        skinned_bind,
    });
}

/// Encode the pick pass: clear the ID target to `(0, 0, sequence, 0)` and the
/// depth to the reverse-Z far plane, then draw every prepared candidate.
/// Scheduled late in `Core3d` (`PostProcess`), safely **after** the
/// GPU-avatar compute (which runs first, in `Prepass`) wrote this frame's
/// skin palettes. `Core3d` runs once per 3D view; the prepared data is
/// `take`n so the pass encodes exactly once per frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy render-world system's inputs are its parameters: the prepared draws, \
              the targets + GPU images, the buffers, the caches and allocator, and the \
              render context"
)]
fn run_gpu_pick_pass(
    mut prepared: ResMut<PreparedGpuPick>,
    targets: Option<Res<GpuPickTargets>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    buffers: Res<GpuPickBuffers>,
    pipeline_cache: Res<PipelineCache>,
    mesh_allocator: Res<MeshAllocator>,
    meshes: Res<RenderAssets<RenderMesh>>,
    mut ctx: RenderContext,
) {
    let Some(data) = prepared.0.take() else {
        return;
    };
    let Some(targets) = targets else {
        return;
    };
    let Some(id_view) = super::id_target_view(&targets, &gpu_images) else {
        return;
    };
    let Some(depth_view) = buffers.depth.as_ref() else {
        return;
    };

    // The clear color carries the sequence in the B channel, so even a pixel
    // no draw touches (a miss) correlates the readback to its submission.
    let clear = wgpu_types::Color {
        r: 0.0,
        g: 0.0,
        b: f64::from(data.sequence),
        a: 0.0,
    };
    let descriptor = RenderPassDescriptor {
        label: Some("gpu_pick_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: id_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(clear),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut pass = ctx.begin_tracked_render_pass(descriptor);
    for draw in &data.draws {
        // A still-compiling pipeline skips its draw (the pick resolves without
        // this candidate; the next request re-tries a compiled pipeline).
        let Some(pipeline) = pipeline_cache.get_render_pipeline(draw.pipeline) else {
            continue;
        };
        let Some(gpu_mesh) = meshes.get(draw.mesh) else {
            continue;
        };
        let Some(vertex_slice) = mesh_allocator.mesh_vertex_slice(&draw.mesh) else {
            continue;
        };
        let bind_group = if draw.skinned {
            match data.skinned_bind.as_ref() {
                Some(bind_group) => bind_group,
                None => continue,
            }
        } else {
            &data.static_bind
        };
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[draw.uniform_offset]);
        pass.set_vertex_buffer(0, vertex_slice.buffer.slice(..));
        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                count,
                index_format,
            } => {
                let Some(index_slice) = mesh_allocator.mesh_index_slice(&draw.mesh) else {
                    continue;
                };
                let Ok(base_vertex) = i32::try_from(vertex_slice.range.start) else {
                    continue;
                };
                let Some(end) = index_slice.range.start.checked_add(*count) else {
                    continue;
                };
                pass.set_index_buffer(index_slice.buffer.slice(..), *index_format);
                pass.draw_indexed(index_slice.range.start..end, base_vertex, 0..1);
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_slice.range.clone(), 0..1);
            }
        }
    }
}

/// Register the render-world half onto the render sub-app.
pub(crate) fn build_render_app(render_app: &mut bevy::app::SubApp) {
    render_app
        .init_resource::<GpuPickSubmission>()
        .init_resource::<GpuPickWarmSet>()
        .init_resource::<GpuPickBuffers>()
        .init_resource::<PreparedGpuPick>()
        .init_resource::<SpecializedMeshPipelines<GpuPickPipeline>>()
        .add_systems(RenderStartup, init_gpu_pick_pipeline)
        .add_systems(
            Render,
            (
                // Warming has no dependency on this frame's submission or the
                // skin-palette swap, but shares `SpecializedMeshPipelines`
                // with `prepare_gpu_pick`; either order is correct since both
                // only read/insert cache entries.
                warm_gpu_pick_pipelines,
                // After `PrepareResources` (Bevy's `prepare_skins` swap/upload),
                // so skinned candidates bind this frame's `current_buffer` — the
                // same ordering contract the GPU-avatar prepare uses.
                prepare_gpu_pick,
            )
                .in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(
            Core3d,
            // Late in the pass stream: the GPU-avatar compute (Prepass set)
            // has already written this frame's palettes by the time the pick
            // pass samples them (§6.3).
            run_gpu_pick_pass.in_set(Core3dSystems::PostProcess),
        );
}
