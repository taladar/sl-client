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
    GpuAvatarFrame, GpuClipHeader, GpuComputeParams, GpuCorrection, GpuJointTrack, GpuLocalPose,
    GpuPlayState, GpuRestJoint, GpuSampleJob, GpuSkinInstance, MAX_GPU_JOINTS,
};

/// The WGSL `@workgroup_size` shared by all three entry points.
const WORKGROUP_SIZE: u32 = 64;

/// One `mat4x4<f32>` palette entry, in bytes.
pub(super) const MAT4_BYTES: usize = 64;

/// One pose-cache / local-pose row (`GpuLocalPose`), in bytes — the std430
/// stride the packing test pins.
const POSE_ENTRY_BYTES: u64 = 32;

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
    /// The pass A layout: params + the sample-job / clip-arena / pose-cache
    /// bindings, at the module's binding indices `{0, 11, 12, 13, 15, 16, 17}`
    /// (6 storage buffers).
    sample_layout: BindGroupLayoutDescriptor,
    /// The pass A (clip sample) pipeline, entry point `sample`.
    sample_pipeline: CachedComputePipelineId,
    /// The pass B layout: params + frames / clip / playback / cache /
    /// corrections / local-pose bindings, at the module's binding indices
    /// `{0, 1, 12, 13, 14, 17, 18, 19, 20}` (8 storage buffers — the same
    /// per-stage floor the capability check requires).
    blend_layout: BindGroupLayoutDescriptor,
    /// The pass B (priority/ease blend + idle + corrections) pipeline, entry
    /// point `blend`.
    blend_pipeline: CachedComputePipelineId,
    /// The debug readback layout: params + instances + palette + expected +
    /// destination, at the module's binding indices `{0, 7, 8, 9, 10}`.
    readback_layout: BindGroupLayoutDescriptor,
    /// The debug readback pipeline, entry point `readback_palette`.
    readback_pipeline: CachedComputePipelineId,
    /// The posed-bounds layout (Phase 5 frustum culling): params + frames +
    /// joint world + the world-space bounds destination, at the module's
    /// binding indices `{0, 1, 4, 21}` (3 storage buffers — well under the
    /// per-stage floor).
    bounds_layout: BindGroupLayoutDescriptor,
    /// The posed-bounds pipeline, entry point `bounds`.
    bounds_pipeline: CachedComputePipelineId,
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
    let sample_layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_sample_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<GpuComputeParams>(false)),
                // jobs
                (11, storage_buffer_read_only_sized(false, None)),
                // clip headers
                (12, storage_buffer_read_only_sized(false, None)),
                // clip tracks
                (13, storage_buffer_read_only_sized(false, None)),
                // key times
                (15, storage_buffer_read_only_sized(false, None)),
                // key values
                (16, storage_buffer_read_only_sized(false, None)),
                // pose cache (written)
                (17, storage_buffer_sized(false, None)),
            ),
        ),
    );
    let sample_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_sample_pipeline".into()),
        layout: vec![sample_layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("sample".into()),
        ..default()
    });
    let blend_layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_blend_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<GpuComputeParams>(false)),
                // frames (slot lookup)
                (1, storage_buffer_read_only_sized(false, None)),
                // clip headers
                (12, storage_buffer_read_only_sized(false, None)),
                // clip tracks (priorities)
                (13, storage_buffer_read_only_sized(false, None)),
                // track-of-joint pool
                (14, storage_buffer_read_only_sized(false, None)),
                // pose cache (read; declared read_write in the module)
                (17, storage_buffer_sized(false, None)),
                // playback row blocks
                (18, storage_buffer_read_only_sized(false, None)),
                // corrections
                (19, storage_buffer_read_only_sized(false, None)),
                // local pose (written; pass C reads it at binding 3)
                (20, storage_buffer_sized(false, None)),
            ),
        ),
    );
    let blend_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_blend_pipeline".into()),
        layout: vec![blend_layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("blend".into()),
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
    let bounds_layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_bounds_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<GpuComputeParams>(false)),
                // frames (slot lookup)
                (1, storage_buffer_read_only_sized(false, None)),
                // joint world (pass C output; read here, so the module's
                // read_write declaration keeps this binding read-write).
                (4, storage_buffer_sized(false, None)),
                // world-space bounds destination
                (21, storage_buffer_sized(false, None)),
            ),
        ),
    );
    let bounds_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_bounds_pipeline".into()),
        layout: vec![bounds_layout.clone()],
        shader: POSE_SHADER_HANDLE,
        entry_point: Some("bounds".into()),
        ..default()
    });
    commands.insert_resource(GpuAvatarPipelines {
        layout,
        fk_pipeline,
        palettes_pipeline,
        sample_layout,
        sample_pipeline,
        blend_layout,
        blend_pipeline,
        readback_layout,
        readback_pipeline,
        bounds_layout,
        bounds_pipeline,
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
    /// The clip arena's headers, rewritten on clip-generation bump.
    clip_headers: StorageBuffer<Vec<GpuClipHeader>>,
    /// The clip arena's shared track pool, rewritten with the headers.
    clip_tracks: StorageBuffer<Vec<GpuJointTrack>>,
    /// The clip arena's joint→track lookup pool, rewritten with the headers.
    track_of_joint: StorageBuffer<Vec<u32>>,
    /// The clip arena's keyframe time pool, rewritten with the headers.
    key_times: StorageBuffer<Vec<f32>>,
    /// The clip arena's keyframe value pool, rewritten with the headers.
    key_values: StorageBuffer<Vec<Vec4>>,
    /// The staged clip generation the arena buffers currently hold.
    clip_generation: Option<u64>,
    /// This frame's sample jobs, rewritten every frame (tiny).
    jobs: StorageBuffer<Vec<GpuSampleJob>>,
    /// The playback row blocks, rewritten on playback-generation bump only
    /// (§1.3(d): steady-state loops upload nothing).
    playback: StorageBuffer<Vec<GpuPlayState>>,
    /// The staged playback generation the buffer currently holds.
    playback_generation: Option<u64>,
    /// The sparse corrections, rewritten every frame (sparse).
    corrections: StorageBuffer<Vec<GpuCorrection>>,
    /// The GPU-only pose-cache working buffer and its current byte size
    /// (pass A output / pass B input; transient within one frame).
    pose_cache: Option<(Buffer, u64)>,
    /// How many local-pose rows the buffer was last sized for in **blend**
    /// mode (where contents are GPU-written and only the allocation matters).
    local_pose_rows: usize,
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
            clip_headers: StorageBuffer::default(),
            clip_tracks: StorageBuffer::default(),
            track_of_joint: StorageBuffer::default(),
            key_times: StorageBuffer::default(),
            key_values: StorageBuffer::default(),
            clip_generation: None,
            jobs: StorageBuffer::default(),
            playback: StorageBuffer::default(),
            playback_generation: None,
            corrections: StorageBuffer::default(),
            pose_cache: None,
            local_pose_rows: 0,
        };
        buffers.frames.set_label(Some("gpu_avatar_frames"));
        buffers.rest.set_label(Some("gpu_avatar_rest"));
        buffers.local_pose.set_label(Some("gpu_avatar_local_pose"));
        buffers.joint_map.set_label(Some("gpu_avatar_joint_map"));
        buffers.ibps.set_label(Some("gpu_avatar_ibps"));
        buffers.instances.set_label(Some("gpu_avatar_instances"));
        buffers.expected.set_label(Some("gpu_avatar_expected"));
        buffers
            .clip_headers
            .set_label(Some("gpu_avatar_clip_headers"));
        buffers
            .clip_tracks
            .set_label(Some("gpu_avatar_clip_tracks"));
        buffers
            .track_of_joint
            .set_label(Some("gpu_avatar_track_of_joint"));
        buffers.key_times.set_label(Some("gpu_avatar_key_times"));
        buffers.key_values.set_label(Some("gpu_avatar_key_values"));
        buffers.jobs.set_label(Some("gpu_avatar_jobs"));
        buffers.playback.set_label(Some("gpu_avatar_playback"));
        buffers
            .corrections
            .set_label(Some("gpu_avatar_corrections"));
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
    /// The pass A bind group and its `(track, job)` workgroup grid, when this
    /// frame staged sample jobs.
    sample: Option<(BindGroup, (u32, u32))>,
    /// The pass B bind group and its `(joint, avatar)` workgroup grid, when
    /// the GPU blend is on (Phase 2 real placement).
    blend: Option<(BindGroup, (u32, u32))>,
    /// The debug readback bind group and its workgroup count, when staged.
    readback: Option<(BindGroup, u32)>,
    /// The posed-bounds bind group (Phase 5) and its workgroup count over the
    /// avatar frames, when the bounds destination has prepared.
    bounds: Option<(BindGroup, u32)>,
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
              skin uniforms, the device/queue pair, the readback + bounds targets, and \
              the shader-buffer lookup"
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
    bounds_target: Option<Res<GpuAvatarBoundsTarget>>,
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
    let rows_len = usize::try_from(staging.slot_capacity)
        .ok()
        .and_then(|slots| slots.checked_mul(usize::try_from(staging.joint_count).ok()?))
        .unwrap_or(0);
    if staging.blend {
        // Phase 2: the local pose is GPU-written by pass B — only the
        // allocation matters. (Re)size it on slot growth; never upload rows.
        if buffers.local_pose_rows < rows_len {
            buffers
                .local_pose
                .set(vec![GpuLocalPose::default(); rows_len]);
            buffers
                .local_pose
                .write_buffer(&render_device, &render_queue);
            buffers.local_pose_rows = rows_len;
        }
    } else {
        // Phase 1 upload (ghost placement / hand-staged tests): the CPU rows.
        buffers.local_pose.set(staging.local_pose.clone());
        buffers
            .local_pose
            .write_buffer(&render_device, &render_queue);
        buffers.local_pose_rows = staging.local_pose.len();
    }
    buffers.instances.set(gpu_instances);
    buffers
        .instances
        .write_buffer(&render_device, &render_queue);
    let job_count = u32::try_from(staging.jobs.len()).unwrap_or(0);
    let correction_count = u32::try_from(staging.corrections.len()).unwrap_or(0);
    buffers.params.set(GpuComputeParams {
        avatar_count,
        joint_count: staging.joint_count,
        instance_count,
        max_skin_joints,
        readback_instance,
        readback_joint_count,
        sample_job_count: job_count,
        correction_count,
        now: staging.now,
        idle_now: staging.idle_now,
        chest_joint: staging.chest_joint,
        torso_joint: staging.torso_joint,
        flags: staging.param_flags,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    });
    buffers.params.write_buffer(&render_device, &render_queue);

    // Phase 2 per-frame uploads (tiny): the job list and corrections; the
    // playback rows only on a content bump; the clip arena only on growth.
    // Empty vecs are padded with one default row so the storage bindings
    // exist (the params counts govern every GPU read).
    if staging.blend {
        let mut jobs = staging.jobs.clone();
        if jobs.is_empty() {
            jobs.push(GpuSampleJob::default());
        }
        buffers.jobs.set(jobs);
        buffers.jobs.write_buffer(&render_device, &render_queue);
        let mut corrections = staging.corrections.clone();
        if corrections.is_empty() {
            corrections.push(GpuCorrection::default());
        }
        buffers.corrections.set(corrections);
        buffers
            .corrections
            .write_buffer(&render_device, &render_queue);
        if buffers.playback_generation != Some(staging.playback_generation) {
            let mut playback = (*staging.playback).clone();
            if playback.is_empty() {
                playback.push(GpuPlayState::default());
            }
            buffers.playback.set(playback);
            buffers.playback.write_buffer(&render_device, &render_queue);
            buffers.playback_generation = Some(staging.playback_generation);
        }
        if buffers.clip_generation != Some(staging.clip_generation) {
            let mut headers = (*staging.clip_headers).clone();
            if headers.is_empty() {
                headers.push(GpuClipHeader::default());
            }
            buffers.clip_headers.set(headers);
            buffers
                .clip_headers
                .write_buffer(&render_device, &render_queue);
            let mut tracks = (*staging.clip_tracks).clone();
            if tracks.is_empty() {
                tracks.push(GpuJointTrack::default());
            }
            buffers.clip_tracks.set(tracks);
            buffers
                .clip_tracks
                .write_buffer(&render_device, &render_queue);
            let mut track_of_joint = (*staging.track_of_joint).clone();
            if track_of_joint.is_empty() {
                track_of_joint.push(0);
            }
            buffers.track_of_joint.set(track_of_joint);
            buffers
                .track_of_joint
                .write_buffer(&render_device, &render_queue);
            let mut key_times = (*staging.key_times).clone();
            if key_times.is_empty() {
                key_times.push(0.0);
            }
            buffers.key_times.set(key_times);
            buffers
                .key_times
                .write_buffer(&render_device, &render_queue);
            let mut key_values = (*staging.key_values).clone();
            if key_values.is_empty() {
                key_values.push(Vec4::ZERO);
            }
            buffers.key_values.set(key_values);
            buffers
                .key_values
                .write_buffer(&render_device, &render_queue);
            buffers.clip_generation = Some(staging.clip_generation);
        }
        // The GPU-only pose-cache scratch: grow-on-demand (transient within
        // one frame's A→B), never smaller than one row so the binding exists.
        let cache_bytes = u64::from(staging.cache_len.max(1)).saturating_mul(POSE_ENTRY_BYTES);
        let recreate_cache = buffers
            .pose_cache
            .as_ref()
            .is_none_or(|(_buffer, size)| *size < cache_bytes);
        if recreate_cache {
            let buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("gpu_avatar_pose_cache"),
                size: cache_bytes,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            buffers.pose_cache = Some((buffer, cache_bytes));
        }
    }

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
            frames_binding.clone(),
            rest_binding,
            local_pose_binding.clone(),
            joint_world.as_entire_binding(),
            joint_map_binding,
            ibps_binding,
            instances_binding.clone(),
            skin_uniforms.current_buffer.as_entire_binding(),
        )),
    );

    // The posed-bounds bind group (Phase 5): pass `bounds` reduces pass C's
    // joint world positions into a per-slot world-space AABB written straight
    // into the readback destination asset. Built only when that asset has
    // prepared; skipped otherwise (the avatars keep the CPU's generous default
    // AABB that frame — no cull until the first real bound lands).
    let bounds = bounds_target
        .as_ref()
        .and_then(|target| shader_buffers.get(&target.buffer))
        .map(|destination| {
            let bind_group = render_device.create_bind_group(
                "gpu_avatar_bounds_bind_group",
                &pipeline_cache.get_bind_group_layout(&pipelines.bounds_layout),
                &BindGroupEntries::with_indices((
                    (0, params_binding.clone()),
                    (1, frames_binding.clone()),
                    (4, joint_world.as_entire_binding()),
                    (21, destination.buffer.as_entire_binding()),
                )),
            );
            (bind_group, avatar_count.div_ceil(WORKGROUP_SIZE))
        });

    // The Phase 2 pass A/B bind groups (real placement): pass A over the
    // clip arena + jobs + pose cache; pass B over the playback / corrections
    // and the same local-pose buffer pass C reads.
    let mut sample = None;
    let mut blend = None;
    if staging.blend {
        let cache_binding = buffers
            .pose_cache
            .as_ref()
            .map(|(buffer, _size)| buffer.clone());
        let (Some(cache_buffer), Some(playback_binding), Some(corrections_binding)) = (
            cache_binding,
            buffers.playback.binding(),
            buffers.corrections.binding(),
        ) else {
            return;
        };
        let (
            Some(jobs_binding),
            Some(headers_binding),
            Some(tracks_binding),
            Some(track_of_joint_binding),
            Some(times_binding),
            Some(values_binding),
        ) = (
            buffers.jobs.binding(),
            buffers.clip_headers.binding(),
            buffers.clip_tracks.binding(),
            buffers.track_of_joint.binding(),
            buffers.key_times.binding(),
            buffers.key_values.binding(),
        )
        else {
            return;
        };
        if !staging.jobs.is_empty() {
            let sample_bind_group = render_device.create_bind_group(
                "gpu_avatar_sample_bind_group",
                &pipeline_cache.get_bind_group_layout(&pipelines.sample_layout),
                &BindGroupEntries::with_indices((
                    (0, params_binding.clone()),
                    (11, jobs_binding),
                    (12, headers_binding.clone()),
                    (13, tracks_binding.clone()),
                    (15, times_binding),
                    (16, values_binding),
                    (17, cache_buffer.as_entire_binding()),
                )),
            );
            // The widest staged clip bounds the x-dispatch over tracks.
            let max_tracks = staging
                .jobs
                .iter()
                .filter_map(|job| {
                    usize::try_from(job.clip_id)
                        .ok()
                        .and_then(|index| staging.clip_headers.get(index))
                })
                .map(|header| header.track_count)
                .max()
                .unwrap_or(0);
            if max_tracks > 0 {
                sample = Some((
                    sample_bind_group,
                    (max_tracks.div_ceil(WORKGROUP_SIZE), job_count),
                ));
            }
        }
        let blend_bind_group = render_device.create_bind_group(
            "gpu_avatar_blend_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipelines.blend_layout),
            &BindGroupEntries::with_indices((
                (0, params_binding.clone()),
                (1, frames_binding),
                (12, headers_binding),
                (13, tracks_binding),
                (14, track_of_joint_binding),
                (17, cache_buffer.as_entire_binding()),
                (18, playback_binding),
                (19, corrections_binding),
                (20, local_pose_binding),
            )),
        );
        blend = Some((
            blend_bind_group,
            (staging.joint_count.div_ceil(WORKGROUP_SIZE), avatar_count),
        ));
    }

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
        sample,
        blend,
        readback,
        bounds,
    });
}

/// Encode passes A→D in one compute pass — first in the frame's `Core3d`
/// pass stream, before the prepass and both shadow encoders, so every palette
/// consumer this frame reads the compute-written palettes (step 3 of the
/// spike's §2.4 ordering) — then the optional readback copy pass.
///
/// A pass whose pipeline is still compiling (or whose data is not staged —
/// passes A/B outside the Phase 2 real placement) is skipped; storage writes
/// are visible between dispatches of one compute pass, so B reads A's cache,
/// C reads B's local pose, and D reads C's joint worlds.
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
    // The blend must not run against a still-compiling sample pipeline (it
    // would read a never-written pose cache); hold the whole pose pass until
    // every staged stage is ready.
    let sample_pipeline = prepared
        .sample
        .as_ref()
        .map(|_stage| pipeline_cache.get_compute_pipeline(pipelines.sample_pipeline));
    let blend_pipeline = prepared
        .blend
        .as_ref()
        .map(|_stage| pipeline_cache.get_compute_pipeline(pipelines.blend_pipeline));
    if sample_pipeline.as_ref().is_some_and(Option::is_none)
        || blend_pipeline.as_ref().is_some_and(Option::is_none)
    {
        return;
    }
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("gpu_avatar_pose_pass"),
                timestamp_writes: None,
            });
        if let (Some((bind_group, (x, y))), Some(Some(pipeline))) =
            (prepared.sample.as_ref(), sample_pipeline)
        {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &**bind_group, &[]);
            pass.dispatch_workgroups(*x, *y, 1);
        }
        if let (Some((bind_group, (x, y))), Some(Some(pipeline))) =
            (prepared.blend.as_ref(), blend_pipeline)
        {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &**bind_group, &[]);
            pass.dispatch_workgroups(*x, *y, 1);
        }
        pass.set_pipeline(fk);
        pass.set_bind_group(0, &*prepared.bind_group, &[]);
        pass.dispatch_workgroups(prepared.fk_workgroups, 1, 1);
        // The posed-bounds reduction (Phase 5): reads the joint worlds pass C
        // just wrote (visible between dispatches) into a per-slot world AABB.
        // It binds its OWN layout (the pose layout is pinned at the
        // 8-storage-buffer downlevel floor, so it cannot carry the extra bounds
        // output), leaving that incompatible bind group at slot 0 — pass D
        // below re-binds the pose bind group before it runs. Its pipeline may
        // still be compiling — skip it that frame (avatars stay unculled until
        // it lands); it never gates the pose.
        if let Some((bounds_bind_group, workgroups)) = prepared.bounds.as_ref()
            && let Some(bounds) = pipeline_cache.get_compute_pipeline(pipelines.bounds_pipeline)
        {
            pass.set_pipeline(bounds);
            pass.set_bind_group(0, bounds_bind_group, &[]);
            pass.dispatch_workgroups(*workgroups, 1, 1);
        }
        // Pass D — skin palettes. Re-bind the pose bind group: the bounds
        // dispatch above may have left its own (incompatible) bounds-layout
        // bind group at slot 0, and pass D's pipeline expects the pose layout —
        // binding the wrong layout is the wgpu validation error. Storage writes
        // are visible between dispatches of one pass: pass D reads the joint
        // worlds pass C just wrote.
        pass.set_pipeline(palettes);
        pass.set_bind_group(0, &*prepared.bind_group, &[]);
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
    for (component, chunk) in out.iter_mut().zip(slice.as_chunks::<4>().0) {
        *component = f32::from_ne_bytes(*chunk);
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

// ---------------------------------------------------------------------------
// The posed-bounds readback channel (Phase 5 frustum culling): the `bounds`
// pass writes a per-slot world-space AABB every frame; the CPU reads it back
// and sets each avatar's `Aabb` so off-screen avatars frustum-cull.
// ---------------------------------------------------------------------------

/// The fixed per-slot capacity of the bounds buffer (mirrors `pose.wgsl`'s
/// `BOUND_SLOT_CAP`): a slot at or beyond this is not written, so its avatar
/// keeps the generous default AABB (unculled). Sized well past any real region
/// avatar count or `SL_VIEWER_CROWD` harness.
pub(super) const BOUND_SLOT_CAP: u32 = 4096;

/// The byte stride of one slot's bounds entry: two `vec4<f32>` (min `xyz` +
/// pad, max `xyz` + pad) in std430.
const BOUND_ENTRY_BYTES: usize = 32;

/// The destination the `bounds` pass writes each posed slot's world-space AABB
/// into, so `Readback::buffer` can lift the whole slot-indexed block off the
/// GPU — the per-frame frustum-cull input consumed by
/// [`apply_gpu_avatar_bounds`](super::stage::apply_gpu_avatar_bounds).
#[derive(Resource, Clone)]
pub(super) struct GpuAvatarBoundsTarget {
    /// The destination buffer asset, sized for [`BOUND_SLOT_CAP`] slots.
    pub(super) buffer: Handle<ShaderBuffer>,
}

impl ExtractResource for GpuAvatarBoundsTarget {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The raw bytes of the last completed bounds readback: [`BOUND_SLOT_CAP`]
/// slot-indexed `(min, max)` world-space AABBs. Empty until the first
/// completes (before which every avatar keeps the generous default AABB).
#[derive(Resource, Default)]
pub(crate) struct GpuAvatarBounds {
    /// The last completed readback's bytes.
    pub(super) bytes: Vec<u8>,
}

/// Create the bounds destination buffer and its `Readback` driver.
pub(super) fn init_gpu_avatar_bounds(
    mut commands: Commands,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let slots = usize::try_from(BOUND_SLOT_CAP).unwrap_or(4096);
    let bytes = slots.saturating_mul(BOUND_ENTRY_BYTES);
    let buffer = buffers.add(ShaderBuffer::with_size(bytes, RenderAssetUsages::default()));
    commands
        .spawn(Readback::buffer(buffer.clone()))
        .observe(receive_gpu_avatar_bounds);
    commands.insert_resource(GpuAvatarBoundsTarget { buffer });
}

/// Stash each completed bounds readback's bytes for the main-world apply system
/// to consume next frame (a 1–2 frame latency the flesh + motion margin covers).
fn receive_gpu_avatar_bounds(readback: On<ReadbackComplete>, mut data: ResMut<GpuAvatarBounds>) {
    data.bytes.clone_from(&readback.data);
}

/// The three `f32` at byte `offset` of a bounds buffer as a [`Vec3`], if
/// present.
fn vec3_at(bytes: &[u8], offset: usize) -> Option<Vec3> {
    let slice = bytes.get(offset..offset.checked_add(12)?)?;
    let mut out = [0.0_f32; 3];
    for (component, chunk) in out.iter_mut().zip(slice.as_chunks::<4>().0) {
        *component = f32::from_ne_bytes(*chunk);
    }
    let [x, y, z] = out;
    Some(Vec3::new(x, y, z))
}

/// The world-space `(min, max)` AABB of pose slot `slot` in a completed bounds
/// readback, or `None` when the slot is past the buffer or was never written
/// this run. An unwritten slot reads back all-zeros — a degenerate zero-extent
/// box a real posed skeleton never produces (its joints always span), so a
/// zero-extent read means "no bound yet" and the caller keeps the avatar
/// unculled rather than culling by a point at the origin.
pub(super) fn bounds_at(bytes: &[u8], slot: u32) -> Option<(Vec3, Vec3)> {
    let start = usize::try_from(slot).ok()?.checked_mul(BOUND_ENTRY_BYTES)?;
    let min = vec3_at(bytes, start)?;
    let max = vec3_at(bytes, start.checked_add(16)?)?;
    // The widest axis span (component f32 arithmetic — the restriction lints
    // forbid glam's `Vec3` operator overloads): a zero span is an unwritten
    // slot, which a real posed skeleton never produces.
    let span = (max.x - min.x).max(max.y - min.y).max(max.z - min.z);
    if span <= f32::EPSILON {
        return None;
    }
    Some((min, max))
}
