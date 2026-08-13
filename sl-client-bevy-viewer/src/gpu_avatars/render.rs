//! The GPU-avatar pipeline's **render-world half**: buffer uploads, the two
//! compute pipelines (pass C `fk`, pass D `palettes`), their scheduling — the
//! spike-proven §2.4 placement: bind groups rebuilt after `prepare_skins`
//! swapped and re-staged `SkinUniforms`, the compute encoded first in `Core3d`
//! before prepass and shadow encoders — and the debug palette readback (a
//! compute copy, because `SkinUniforms.current_buffer` carries no `COPY_SRC`).

use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::pbr::SkinUniforms;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, storage_buffer_sized, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
    BufferDescriptor, BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
    ComputePipelineDescriptor, PipelineCache, ShaderStages, StorageBuffer, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::sync_world::MainEntity;

use super::POSE_SHADER_HANDLE;
use super::stage::GpuAvatarStaging;
use super::types::{
    GpuAvatarFrame, GpuComputeParams, GpuLocalPose, GpuRestJoint, GpuSkinInstance, MAX_GPU_JOINTS,
};

/// The WGSL `@workgroup_size` shared by all three entry points.
const WORKGROUP_SIZE: u32 = 64;

/// One `mat4x4<f32>` palette entry, in bytes.
pub(super) const MAT4_BYTES: usize = 64;

/// How far a read-back GPU palette component may sit from the CPU-path value
/// before the verdict flips to "diverges". The FK itself is golden-tested at
/// 1e-4 / bit-exact; this live channel additionally crosses the
/// `GlobalTransform` affine round-trip and GPU fma reassociation, so it gets
/// the spike's forgiving epsilon.
const PALETTE_EPSILON: f32 = 1.0e-3;

/// Minimum gap between two identical verdict log lines (~1 Hz); a changed
/// verdict logs immediately.
const VERDICT_REPEAT: Duration = Duration::from_secs(1);

/// The compute pipelines and bind-group layouts, created once in
/// [`init_gpu_avatar_pipelines`].
#[derive(Resource)]
pub(super) struct GpuAvatarPipelines {
    /// The pass C+D layout: params uniform + 8 storage buffers (sequential
    /// bindings 0..=8 — exactly wgpu's downlevel default of 8 storage buffers
    /// per stage, deliberately not one more).
    layout: BindGroupLayoutDescriptor,
    /// The pass C (hierarchical FK) pipeline, entry point `fk`.
    fk_pipeline: CachedComputePipelineId,
    /// The pass D (skin palettes) pipeline, entry point `palettes`.
    palettes_pipeline: CachedComputePipelineId,
    /// The debug readback layout: params + instances + palette + expected +
    /// destination, at the module's binding indices `{0, 7, 8, 9, 10}`.
    readback_layout: BindGroupLayoutDescriptor,
    /// The debug readback pipeline, entry point `readback_palette`.
    readback_pipeline: CachedComputePipelineId,
}

/// Create the bind-group layouts and queue the compute pipelines.
pub(super) fn init_gpu_avatar_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_pose_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<GpuComputeParams>(false),
                // frames
                storage_buffer_read_only_sized(false, None),
                // rest joints
                storage_buffer_read_only_sized(false, None),
                // local pose
                storage_buffer_read_only_sized(false, None),
                // joint world (pass C output / pass D input)
                storage_buffer_sized(false, None),
                // joint map pool
                storage_buffer_read_only_sized(false, None),
                // inverse bindpose pool
                storage_buffer_read_only_sized(false, None),
                // skin instances
                storage_buffer_read_only_sized(false, None),
                // Bevy's `SkinUniforms.current_buffer`, read-write — the
                // spike-proven binding.
                storage_buffer_sized(false, None),
            ),
        ),
    );
    let fk_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_fk_pipeline".into()),
        layout: vec![layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("fk".into()),
        ..default()
    });
    let palettes_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_palettes_pipeline".into()),
        layout: vec![layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("palettes".into()),
        ..default()
    });
    let readback_layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_readback_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<GpuComputeParams>(false)),
                (7, storage_buffer_read_only_sized(false, None)),
                (8, storage_buffer_sized(false, None)),
                (9, storage_buffer_read_only_sized(false, None)),
                (10, storage_buffer_sized(false, None)),
            ),
        ),
    );
    let readback_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_readback_pipeline".into()),
        layout: vec![readback_layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("readback_palette".into()),
        ..default()
    });
    commands.insert_resource(GpuAvatarPipelines {
        layout,
        fk_pipeline,
        palettes_pipeline,
        readback_layout,
        readback_pipeline,
    });
}

/// The persistent GPU buffers, uploaded from the extracted
/// [`GpuAvatarStaging`]: per-frame blocks rewritten every frame, the rest /
/// pool blocks only on a staged generation bump.
#[derive(Resource)]
pub(super) struct GpuAvatarBuffers {
    /// The [`GpuComputeParams`] uniform, rewritten every frame.
    params: UniformBuffer<GpuComputeParams>,
    /// One row per posed avatar, rewritten every frame.
    frames: StorageBuffer<Vec<GpuAvatarFrame>>,
    /// The slot-indexed composed rest rows, rewritten on generation bump.
    rest: StorageBuffer<Vec<GpuRestJoint>>,
    /// The staged generation [`Self::rest`] currently holds.
    rest_generation: Option<u64>,
    /// The slot-indexed local pose rows, rewritten every frame.
    local_pose: StorageBuffer<Vec<GpuLocalPose>>,
    /// The shared joint-map pool, rewritten on pool-generation bump.
    joint_map: StorageBuffer<Vec<u32>>,
    /// The shared inverse-bindpose pool, rewritten on pool-generation bump.
    ibps: StorageBuffer<Vec<Mat4>>,
    /// The staged pool generation the pools currently hold.
    pool_generation: Option<u64>,
    /// The resolved instance table, rewritten every frame (palette offsets
    /// can move every frame).
    instances: StorageBuffer<Vec<GpuSkinInstance>>,
    /// The GPU-only `JointWorld` working buffer and its current byte size.
    joint_world: Option<(Buffer, u64)>,
    /// The CPU-expected palette for the debug readback, rewritten every frame
    /// the readback is staged.
    expected: StorageBuffer<Vec<Mat4>>,
}

impl Default for GpuAvatarBuffers {
    fn default() -> Self {
        let mut buffers = Self {
            params: UniformBuffer::default(),
            frames: StorageBuffer::default(),
            rest: StorageBuffer::default(),
            rest_generation: None,
            local_pose: StorageBuffer::default(),
            joint_map: StorageBuffer::default(),
            ibps: StorageBuffer::default(),
            pool_generation: None,
            instances: StorageBuffer::default(),
            joint_world: None,
            expected: StorageBuffer::default(),
        };
        buffers.frames.set_label(Some("gpu_avatar_frames"));
        buffers.rest.set_label(Some("gpu_avatar_rest"));
        buffers.local_pose.set_label(Some("gpu_avatar_local_pose"));
        buffers.joint_map.set_label(Some("gpu_avatar_joint_map"));
        buffers.ibps.set_label(Some("gpu_avatar_ibps"));
        buffers.instances.set_label(Some("gpu_avatar_instances"));
        buffers.expected.set_label(Some("gpu_avatar_expected"));
        buffers
    }
}

/// This frame's dispatch, or `None` when nothing is staged / resolvable.
/// Rebuilt whole every frame: `current_buffer` is swapped (and on growth
/// reallocated) by `prepare_skins`, and skins can be moved by the allocator.
#[derive(Resource, Default)]
pub(super) struct PreparedGpuAvatars(Option<PreparedData>);

/// The bind groups and dispatch sizes [`run_gpu_avatar_compute`] encodes.
struct PreparedData {
    /// The pass C+D bind group over this frame's post-swap `current_buffer`.
    bind_group: BindGroup,
    /// Workgroups covering the avatar count (pass C).
    fk_workgroups: u32,
    /// Workgroups covering `(max_skin_joints, instance_count)` (pass D).
    palette_workgroups: (u32, u32),
    /// The debug readback bind group and its workgroup count, when staged.
    readback: Option<(BindGroup, u32)>,
}

/// Upload the extracted staging into the GPU buffers, resolve each ghost's
/// palette offset from [`SkinUniforms::skin_index`] (fresh, after
/// `prepare_skins` ran in `PrepareResources`), and (re)build the bind groups
/// against this frame's post-swap `current_buffer` — step 2 of the spike's
/// §2.4 ordering.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy render-world system's inputs are its parameters: the extracted \
              staging, the pipeline's own buffer/prepared/pipeline resources, Bevy's \
              skin uniforms, the device/queue pair, and the two readback lookups"
)]
pub(super) fn prepare_gpu_avatars(
    staging: Res<GpuAvatarStaging>,
    mut buffers: ResMut<GpuAvatarBuffers>,
    mut prepared: ResMut<PreparedGpuAvatars>,
    pipelines: Res<GpuAvatarPipelines>,
    pipeline_cache: Res<PipelineCache>,
    skin_uniforms: Res<SkinUniforms>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    readback_target: Option<Res<GpuAvatarReadbackTarget>>,
    shader_buffers: Res<RenderAssets<GpuShaderBuffer>>,
) {
    prepared.0 = None;
    if staging.joint_count == 0 || staging.frames.is_empty() {
        return;
    }

    // Resolve the staged instances against Bevy's live skin allocation. An
    // instance whose skin is not (yet) registered is skipped this frame and
    // retried next frame — offsets are re-resolved every frame regardless.
    let readback_entity = staging.readback.as_ref().map(|request| request.target);
    let mut gpu_instances: Vec<GpuSkinInstance> = Vec::with_capacity(staging.instances.len());
    let mut max_skin_joints = 0_u32;
    let mut readback_instance = u32::MAX;
    for instance in &staging.instances {
        let Some(palette_offset) = skin_uniforms.skin_index(MainEntity::from(instance.target))
        else {
            continue;
        };
        if Some(instance.target) == readback_entity {
            readback_instance = u32::try_from(gpu_instances.len()).unwrap_or(u32::MAX);
        }
        max_skin_joints = max_skin_joints.max(instance.joint_count);
        gpu_instances.push(GpuSkinInstance {
            avatar_slot: instance.avatar_slot,
            palette_offset,
            joint_count: instance.joint_count,
            joint_map_offset: instance.joint_map_offset,
            ibp_offset: instance.ibp_offset,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        });
    }
    if gpu_instances.is_empty() || max_skin_joints == 0 {
        return;
    }
    let Ok(avatar_count) = u32::try_from(staging.frames.len()) else {
        return;
    };
    let Ok(instance_count) = u32::try_from(gpu_instances.len()) else {
        return;
    };
    let readback_joint_count = staging
        .readback
        .as_ref()
        .map_or(0, |request| request.joint_count);

    // Per-frame uploads.
    buffers.frames.set(staging.frames.clone());
    buffers.frames.write_buffer(&render_device, &render_queue);
    buffers.local_pose.set(staging.local_pose.clone());
    buffers
        .local_pose
        .write_buffer(&render_device, &render_queue);
    buffers.instances.set(gpu_instances);
    buffers
        .instances
        .write_buffer(&render_device, &render_queue);
    buffers.params.set(GpuComputeParams {
        avatar_count,
        joint_count: staging.joint_count,
        instance_count,
        max_skin_joints,
        readback_instance,
        readback_joint_count,
        pad0: 0,
        pad1: 0,
    });
    buffers.params.write_buffer(&render_device, &render_queue);

    // Change-driven uploads: the composed rest skeletons and the joint-map /
    // inverse-bindpose pools rewrite only when their staged generation moved.
    if buffers.rest_generation != Some(staging.rest_generation) {
        buffers.rest.set((*staging.rest).clone());
        buffers.rest.write_buffer(&render_device, &render_queue);
        buffers.rest_generation = Some(staging.rest_generation);
    }
    if buffers.pool_generation != Some(staging.pool_generation) {
        buffers.joint_map.set((*staging.joint_map).clone());
        buffers
            .joint_map
            .write_buffer(&render_device, &render_queue);
        buffers.ibps.set((*staging.ibps).clone());
        buffers.ibps.write_buffer(&render_device, &render_queue);
        buffers.pool_generation = Some(staging.pool_generation);
    }

    // The GPU-only JointWorld scratch: grow-on-demand (contents are transient
    // within one frame's pass C→D, so recreation needs no copy).
    let needed = u64::from(staging.slot_capacity)
        .saturating_mul(u64::from(staging.joint_count))
        .saturating_mul(u64::try_from(MAT4_BYTES).unwrap_or(64));
    if needed == 0 {
        return;
    }
    let recreate = buffers
        .joint_world
        .as_ref()
        .is_none_or(|(_buffer, size)| *size < needed);
    if recreate {
        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("gpu_avatar_joint_world"),
            size: needed,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        buffers.joint_world = Some((buffer, needed));
    }
    // Cloned handle (a cheap `Arc` bump) so the later mutable buffer writes
    // do not conflict with this borrow.
    let Some(joint_world) = buffers
        .joint_world
        .as_ref()
        .map(|(buffer, _size)| buffer.clone())
    else {
        return;
    };

    // The debug readback's expected palette (uploaded fresh every staged
    // frame — the readback pass copies it next to the GPU palette in the same
    // submission, making the CPU diff race-free).
    if let Some(request) = staging.readback.as_ref() {
        buffers.expected.set(request.expected.clone());
        buffers.expected.write_buffer(&render_device, &render_queue);
    }

    let (Some(params_binding), Some(frames_binding), Some(rest_binding)) = (
        buffers.params.binding(),
        buffers.frames.binding(),
        buffers.rest.binding(),
    ) else {
        return;
    };
    let (Some(local_pose_binding), Some(joint_map_binding), Some(ibps_binding)) = (
        buffers.local_pose.binding(),
        buffers.joint_map.binding(),
        buffers.ibps.binding(),
    ) else {
        return;
    };
    let Some(instances_binding) = buffers.instances.binding() else {
        return;
    };

    let bind_group = render_device.create_bind_group(
        "gpu_avatar_pose_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipelines.layout),
        &BindGroupEntries::sequential((
            params_binding.clone(),
            frames_binding,
            rest_binding,
            local_pose_binding,
            joint_world.as_entire_binding(),
            joint_map_binding,
            ibps_binding,
            instances_binding.clone(),
            skin_uniforms.current_buffer.as_entire_binding(),
        )),
    );

    // The readback bind group, when this frame staged a request whose
    // instance resolved and whose destination asset has prepared.
    let readback = if readback_instance != u32::MAX && readback_joint_count != 0 {
        staging
            .readback
            .as_ref()
            .and_then(|_request| buffers.expected.binding())
            .and_then(|expected_binding| {
                let destination = readback_target
                    .as_ref()
                    .and_then(|target| shader_buffers.get(&target.buffer))?;
                let bind_group = render_device.create_bind_group(
                    "gpu_avatar_readback_bind_group",
                    &pipeline_cache.get_bind_group_layout(&pipelines.readback_layout),
                    &BindGroupEntries::with_indices((
                        (0, params_binding),
                        (7, instances_binding),
                        (8, skin_uniforms.current_buffer.as_entire_binding()),
                        (9, expected_binding),
                        (10, destination.buffer.as_entire_binding()),
                    )),
                );
                Some((bind_group, readback_joint_count.div_ceil(WORKGROUP_SIZE)))
            })
    } else {
        None
    };

    prepared.0 = Some(PreparedData {
        bind_group,
        fk_workgroups: avatar_count.div_ceil(WORKGROUP_SIZE),
        palette_workgroups: (max_skin_joints.div_ceil(WORKGROUP_SIZE), instance_count),
        readback,
    });
}

/// Encode passes C and D in one compute pass — first in the frame's `Core3d`
/// pass stream, before the prepass and both shadow encoders, so every palette
/// consumer this frame reads the compute-written ghost palettes (step 3 of the
/// spike's §2.4 ordering) — then the optional readback copy pass.
///
/// `Core3d` runs once per 3D camera; the extra dispatches on secondary views
/// recompute identical values — idempotent by construction.
pub(super) fn run_gpu_avatar_compute(
    pipelines: Res<GpuAvatarPipelines>,
    prepared: Res<PreparedGpuAvatars>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(prepared) = prepared.0.as_ref() else {
        return;
    };
    let (Some(fk), Some(palettes)) = (
        pipeline_cache.get_compute_pipeline(pipelines.fk_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.palettes_pipeline),
    ) else {
        return;
    };
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("gpu_avatar_pose_pass"),
                timestamp_writes: None,
            });
        pass.set_bind_group(0, &*prepared.bind_group, &[]);
        pass.set_pipeline(fk);
        pass.dispatch_workgroups(prepared.fk_workgroups, 1, 1);
        // Storage writes are visible between dispatches of one pass: pass D
        // reads the joint worlds pass C just wrote.
        pass.set_pipeline(palettes);
        let (x, y) = prepared.palette_workgroups;
        pass.dispatch_workgroups(x, y, 1);
    }
    if let Some((readback_bind_group, workgroups)) = prepared.readback.as_ref()
        && let Some(readback) = pipeline_cache.get_compute_pipeline(pipelines.readback_pipeline)
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("gpu_avatar_readback_pass"),
                timestamp_writes: None,
            });
        pass.set_pipeline(readback);
        pass.set_bind_group(0, &**readback_bind_group, &[]);
        pass.dispatch_workgroups(*workgroups, 1, 1);
    }
}

// ---------------------------------------------------------------------------
// The debug readback channel (`SL_VIEWER_GPU_AVATARS_READBACK=1`).
// ---------------------------------------------------------------------------

/// The destination the readback pass copies the ghost's palette range (and the
/// CPU-expected palette after it) into, so `Readback::buffer` can lift both
/// off the GPU — the visibility-independent "GPU palette == CPU palette"
/// verdict channel, consumed by [`gpu_avatar_readback_verdict`] live and by
/// the headless test through [`GpuAvatarReadbackData`].
#[derive(Resource, Clone)]
pub(super) struct GpuAvatarReadbackTarget {
    /// The destination buffer asset, sized for two [`MAX_GPU_JOINTS`]-entry
    /// palette halves.
    pub(super) buffer: Handle<ShaderBuffer>,
}

impl ExtractResource for GpuAvatarReadbackTarget {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The raw bytes of the last completed readback: `joint_count` copied GPU
/// palette matrices followed by the same count of CPU-expected matrices.
/// Empty until the first completes.
#[derive(Resource, Default)]
pub(crate) struct GpuAvatarReadbackData {
    /// The last completed readback's bytes.
    pub(crate) bytes: Vec<u8>,
}

/// When the live verdict last logged and what it said, so the WARN line fires
/// immediately on a verdict change and otherwise at most ~1 Hz.
#[derive(Resource, Default)]
pub(super) struct GpuAvatarVerdictLog {
    /// The last logged verdict and when.
    last: Option<(bool, Instant)>,
}

/// Create the readback destination buffer and its `Readback` driver.
pub(super) fn init_gpu_avatar_readback(
    mut commands: Commands,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let bytes = usize::try_from(MAX_GPU_JOINTS)
        .unwrap_or(256)
        .saturating_mul(2)
        .saturating_mul(MAT4_BYTES);
    let buffer = buffers.add(ShaderBuffer::with_size(bytes, RenderAssetUsages::default()));
    commands
        .spawn(Readback::buffer(buffer.clone()))
        .observe(gpu_avatar_readback_verdict);
    commands.insert_resource(GpuAvatarReadbackTarget { buffer });
}

/// The 16 floats of the `index`th `mat4x4` in a readback byte buffer, if
/// present.
pub(super) fn mat_at(bytes: &[u8], index: usize) -> Option<[f32; 16]> {
    let start = index.checked_mul(MAT4_BYTES)?;
    let slice = bytes.get(start..start.checked_add(MAT4_BYTES)?)?;
    let mut out = [0.0_f32; 16];
    for (component, chunk) in out.iter_mut().zip(slice.chunks_exact(4)) {
        *component = f32::from_ne_bytes(chunk.try_into().ok()?);
    }
    Some(out)
}

/// The worst per-component difference between the GPU palette half and the
/// CPU-expected half of a completed readback holding `count` entries each, or
/// `None` when the buffer is too small or not yet plausibly written (the
/// expected half's first matrix must look affine — before the first dispatch
/// the buffer is all zeros, and zeros == zeros must not count as a verdict).
pub(super) fn palette_worst_diff(bytes: &[u8], count: usize) -> Option<f32> {
    if count == 0 {
        return None;
    }
    let first_expected = mat_at(bytes, count)?;
    let plausibly_written = first_expected
        .last()
        .is_some_and(|&last| (last - 1.0).abs() < 0.5);
    if !plausibly_written {
        return None;
    }
    let mut worst = 0.0_f32;
    for entry in 0..count {
        let gpu = mat_at(bytes, entry)?;
        let expected = mat_at(bytes, count.checked_add(entry)?)?;
        for (got, want) in gpu.iter().zip(&expected) {
            worst = worst.max((got - want).abs());
        }
    }
    Some(worst)
}

/// The live verdict: on every completed readback, diff the ghost's GPU-written
/// palette against the CPU-path palette computed the same frame, and log a
/// WARN verdict — immediately on change, else at ~1 Hz. Grep the live log for
/// `GPU-avatar palette readback:`; `GPU palette == CPU palette` is the
/// Phase 1a success signal per avatar.
fn gpu_avatar_readback_verdict(
    readback: On<ReadbackComplete>,
    mut data: ResMut<GpuAvatarReadbackData>,
    mut log: ResMut<GpuAvatarVerdictLog>,
    staging: Res<GpuAvatarStaging>,
) {
    data.bytes.clone_from(&readback.data);
    let Some(request) = staging.readback.as_ref() else {
        return;
    };
    let Ok(count) = usize::try_from(request.joint_count) else {
        return;
    };
    let Some(worst) = palette_worst_diff(&readback.data, count) else {
        return;
    };
    let matches = worst <= PALETTE_EPSILON;

    let now = Instant::now();
    let changed = log.last.is_none_or(|(previous, _at)| previous != matches);
    let repeat_due = log
        .last
        .is_none_or(|(_previous, at)| now.duration_since(at) >= VERDICT_REPEAT);
    if !(changed || repeat_due) {
        return;
    }
    log.last = Some((matches, now));
    if matches {
        warn!(
            "GPU-avatar palette readback: {} ({count} joints) worst diff {worst:e} — \
             GPU palette == CPU palette",
            request.label
        );
    } else {
        warn!(
            "GPU-avatar palette readback: {} ({count} joints) worst diff {worst:e} — \
             GPU palette != CPU palette (the GPU FK diverges from the CPU pose path)",
            request.label
        );
    }
}
