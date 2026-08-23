//! The GPU-avatar **keystone spike** (`roadmap/context/gpu-avatars.md` §2.4,
//! §9.1 risk 1; task `viewer-perf-gpu-avatar-keystone-skinuniforms-spike`): a
//! de-risking experiment, not a feature. It proves — or disproves — the one
//! assumption the whole GPU-avatar design leans on: that a compute pass can
//! bind Bevy's [`SkinUniforms`]`.current_buffer` as `storage, read_write` and
//! overwrite a skin's palette range **at the offset Bevy allocated**, with the
//! normal draw path (`skinning.wgsl`, batching, prepass, shadows) then
//! rendering from the compute-written matrices.
//!
//! **Flag-gated, additive, default OFF.** With `SL_VIEWER_GPU_AVATAR_SPIKE`
//! unset, [`GpuAvatarSpikePlugin::from_env`] builds a no-op plugin: nothing is
//! registered and the viewer is byte-for-byte the normal path. With the flag
//! set to `identity` or `marker` (read once at App build, the
//! `SL_VIEWER_DISABLE_*` idiom), one skinned mesh is targeted and its palette
//! stomped every frame:
//!
//! - `identity` — every joint matrix becomes the target mesh entity's own
//!   world matrix, so the mesh snaps to **bind pose at its own place**,
//!   ignoring all animation. Obvious and decisive.
//! - `marker` — the same, composed with a fixed 45°-roll + 1.5×-scale, so a
//!   clearly deformed render proves the compute write both landed and is what
//!   the draw read.
//!
//! **Live-run signals (grep these).** Two log lines make the run
//! eyeball-independent:
//!
//! - `GPU-avatar spike target: <entity> (<N> joints)` at INFO, re-logged on
//!   every target change: `pick_spike_target` converges on the
//!   **most-jointed** skin as the scene rezzes (a worn Bento mesh body binds
//!   ~130 joints seconds after the ~15-joint system parts it hides), so the
//!   final line names the mesh body, not an early hidden system part.
//! - `GPU-avatar spike readback: … write LANDED` / `write did NOT land` at
//!   WARN, on change or at ~1 Hz: `spike_readback_verdict` compares the
//!   copied `palette[0]` against the expected matrix the readback pass
//!   computed GPU-side from the same params — the decisive keystone verdict
//!   even when the stomped part is invisible under a mesh body.
//!
//! **The §2.4 ordering, concretely.** All on the render app's single command
//! stream:
//!
//! 1. `Render` / `RenderSystems::PrepareResources`: Bevy's `prepare_skins`
//!    swaps `current`/`prev` (skipped only when its staging is empty — a
//!    registered skin keeps it non-empty, ours included), then
//!    `queue.write_buffer`s the CPU-staged pose into the post-swap
//!    `current_buffer`. wgpu executes queue writes **before any subsequently
//!    submitted command buffer**.
//! 2. `Render` / `RenderSystems::PrepareBindGroups` (a later set in the same
//!    chain, so after `prepare_skins`): `prepare_spike` re-resolves the
//!    target's palette offset from [`SkinUniforms::skin_index`] (buffers can
//!    reallocate and the allocator can move skins, so every frame), uploads
//!    `SpikeParams`, and rebuilds the bind group against the **post-swap**
//!    `current_buffer` (rebuilt every frame for the same reason).
//! 3. `Core3d`, first in [`Core3dSystems::Prepass`] — explicitly before
//!    `early_prepass` and both shadow-pass systems, which in Bevy 0.19 encode
//!    inside `Core3d` too: `run_spike_compute` dispatches the overwrite.
//!    [`RenderContext`] flushes command buffers in topological system order,
//!    so the compute lands in the submitted stream before every pass that
//!    samples the palette.
//! 4. Prepass / shadow / main draws read the buffer; within one queue wgpu
//!    orders the compute write before those reads.
//!
//! The headless test at the bottom is the decisive deliverable: it renders a
//! real skinned mesh whose **CPU pose moves it visibly right of centre**, and
//! asserts the draw output shows the mesh where the **compute-written**
//! palette puts it instead — plus a byte-level readback of the palette range
//! at the Bevy-allocated offset (which must itself be a compute copy: the
//! skin buffer carries no `COPY_SRC` usage). If wgpu rejected the
//! `storage, read_write` binding, pipeline / bind
//! group creation would raise a wgpu validation error (a
//! `wgpu error: Validation Error` panic on the render thread naming
//! `gpu_avatar_spike_layout` or the spike pipeline), which is the fallback
//! signal (fork `skinning.wgsl` via a `MaterialExtension`, §9.1).
//!
//! Not verified here: motion vectors / TAA reading `prev_buffer` (last
//! frame's compute-written palette after the swap) — that needs the live
//! viewer with TAA on, per the roadmap task.

use std::time::{Duration, Instant};

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::prepass::node::early_prepass;
use bevy::core_pipeline::schedule::Core3d;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{
    EARLY_SHADOW_PASS, MAX_JOINTS, SkinUniforms, per_view_shadow_pass, shared_shadow_pass,
};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{storage_buffer_sized, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache,
    ShaderStages, ShaderType, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::sync_world::MainEntity;
use bevy::render::{Extract, Render, RenderApp, RenderStartup, RenderSystems};

/// The internal handle the spike compute shader (`gpu_avatar_spike.wgsl`) is
/// loaded under.
const SPIKE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5b8f2c47-91d3-4e6a-b052-7c4f8a1d93e6");

/// The env flag selecting the spike's write mode: `identity` or `marker`.
/// Unset (or unrecognised) leaves the viewer on the untouched normal path.
const ENV_SPIKE: &str = "SL_VIEWER_GPU_AVATAR_SPIKE";

/// The WGSL entry point's `@workgroup_size`.
const WORKGROUP_SIZE: u32 = 64;

/// The size of one `mat4x4<f32>` palette entry, in bytes.
const MAT4_BYTES: usize = 64;

/// How far a read-back palette component may sit from its expected value
/// before the verdict flips to "did NOT land". The expected value is computed
/// on the GPU from the very `params` the overwrite used, so a landed write is
/// bit-identical; this is pure paranoia margin.
const VERDICT_EPSILON: f32 = 1.0e-3;

/// The minimum gap between two identical verdict log lines (~1 Hz): a
/// changed verdict logs immediately, a repeated one at most this often.
const VERDICT_REPEAT: Duration = Duration::from_secs(1);

/// Which known transform the spike writes over the target skin's palette.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpikeMode {
    /// Write the target mesh entity's world matrix for every joint: the mesh
    /// snaps to bind pose at its own place, ignoring its animation.
    Identity,
    /// Write the world matrix composed with a fixed 45°-roll + 1.5×-scale
    /// (`MARKER` in the WGSL): an unmistakably deformed bind pose.
    Marker,
}

impl SpikeMode {
    /// The `params.mode` selector value the WGSL branches on.
    const fn shader_mode(self) -> u32 {
        match self {
            Self::Identity => 0,
            Self::Marker => 1,
        }
    }
}

/// The spike plugin. [`GpuAvatarSpikePlugin::from_env`] reads
/// `ENV_SPIKE` once at App build; `mode: None` (the default when the flag is
/// unset) registers **nothing**.
#[derive(Debug)]
pub struct GpuAvatarSpikePlugin {
    /// The write mode, or `None` for the byte-for-byte normal path.
    pub(crate) mode: Option<SpikeMode>,
}

impl GpuAvatarSpikePlugin {
    /// Build the plugin from `ENV_SPIKE`, read once here (the
    /// `SL_VIEWER_DISABLE_*` idiom: an env knob is sampled at startup and
    /// carried in a resource, never re-read per frame).
    #[must_use]
    pub fn from_env() -> Self {
        let mode = match std::env::var(ENV_SPIKE) {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "identity" => Some(SpikeMode::Identity),
                "marker" => Some(SpikeMode::Marker),
                other => {
                    warn!(
                        "{ENV_SPIKE}={other:?} is not a spike mode (expected `identity` or \
                         `marker`); the GPU-avatar spike stays off"
                    );
                    None
                }
            },
            Err(_unset) => None,
        };
        Self { mode }
    }
}

impl Plugin for GpuAvatarSpikePlugin {
    fn build(&self, app: &mut App) {
        let Some(mode) = self.mode else {
            return;
        };
        warn!(
            "GPU-avatar keystone spike is ON ({mode:?}): one skinned mesh's palette in \
             SkinUniforms.current_buffer is overwritten by a compute pass every frame"
        );
        load_internal_asset!(
            app,
            SPIKE_SHADER_HANDLE,
            "gpu_avatar_spike.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(ExtractResourcePlugin::<SpikeReadbackTarget>::default())
            // The mode again in the main world, so the verdict observer can
            // name it in its log line (the render app has its own copy).
            .insert_resource(mode)
            .init_resource::<SpikeReadbackData>()
            .init_resource::<SpikeVerdictLog>()
            .add_systems(Startup, init_spike_readback)
            .add_systems(Update, pick_spike_target);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(mode)
            .init_resource::<ExtractedSpikeTarget>()
            .init_resource::<PreparedSpike>()
            .add_systems(RenderStartup, init_spike_pipeline)
            .add_systems(ExtractSchedule, extract_spike_target)
            .add_systems(
                Render,
                // `RenderSystems::PrepareBindGroups` is chained after
                // `PrepareResources` (where Bevy's `prepare_skins` swaps
                // current/prev and uploads its staging), so the offset is
                // resolved and the bind group built against the post-swap,
                // post-realloc `current_buffer` — step 2 of the §2.4 ordering
                // in the module docs.
                prepare_spike.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                // First in the frame's pass encoding: before the prepass and
                // before both shadow-pass encoders (which in Bevy 0.19 are
                // themselves `Core3d` systems), so every palette consumer
                // this frame — prepass, shadows, main — reads the
                // compute-written matrices. Step 3 of the §2.4 ordering.
                run_spike_compute
                    .in_set(Core3dSystems::Prepass)
                    .before(early_prepass)
                    .before(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                    .before(shared_shadow_pass::<EARLY_SHADOW_PASS>),
            );
    }
}

// ---------------------------------------------------------------------------
// Main world: pick one target skin.
// ---------------------------------------------------------------------------

/// Marks the one skinned mesh whose palette the spike overwrites.
#[derive(Component)]
pub(crate) struct GpuAvatarSpikeTarget;

/// Keep exactly one skinned mesh marked as the spike target, **converging on
/// the most-jointed skin as the scene rezzes**.
///
/// Not sticky-first, and that is a live-run lesson: an avatar rezzes in
/// stages — ~15-joint system base-body parts bind seconds before a worn
/// ~130-joint Bento mesh body — so a first-come target latches onto a system
/// part the mesh body then hides, and nothing visible changes no matter what
/// the compute writes. Instead the target follows the maximum: whenever a
/// skin with **strictly more** joints than the current target appears, the
/// marker moves to it. The strict inequality is the hysteresis — an
/// equal-jointed later arrival never steals the target — so once the
/// most-jointed skin (the mesh body) has bound, the target stays put. Ties on
/// the initial pick break to the lowest entity id. Every change logs at
/// INFO (`GPU-avatar spike target: …`).
fn pick_spike_target(
    mut commands: Commands,
    current: Query<(Entity, &SkinnedMesh), With<GpuAvatarSpikeTarget>>,
    candidates: Query<(Entity, &SkinnedMesh)>,
) {
    let Some((best_entity, best_skin)) = candidates
        .iter()
        .max_by_key(|(entity, skin)| (skin.joints.len(), core::cmp::Reverse(*entity)))
    else {
        return;
    };
    let best_joints = best_skin.joints.len();
    match current.iter().next() {
        Some((entity, _skin)) if entity == best_entity => {}
        Some((entity, skin)) if best_joints > skin.joints.len() => {
            commands.entity(entity).remove::<GpuAvatarSpikeTarget>();
            commands.entity(best_entity).insert(GpuAvatarSpikeTarget);
            info!(
                "GPU-avatar spike target: {best_entity} ({best_joints} joints), was {entity} \
                 ({} joints)",
                skin.joints.len()
            );
        }
        Some(_current_has_at_least_as_many) => {}
        None => {
            commands.entity(best_entity).insert(GpuAvatarSpikeTarget);
            info!("GPU-avatar spike target: {best_entity} ({best_joints} joints)");
        }
    }
}

// ---------------------------------------------------------------------------
// Render world.
// ---------------------------------------------------------------------------

/// The frame's extracted facts about the target skin, or `None` while no
/// target exists (nothing spawned yet, or the target despawned).
#[derive(Resource, Default)]
struct ExtractedSpikeTarget(Option<SpikeTargetData>);

/// What `prepare_spike` needs to know about the target.
struct SpikeTargetData {
    /// The target's main-world entity, the key [`SkinUniforms::skin_index`]
    /// resolves offsets by.
    main_entity: MainEntity,
    /// How many joint matrices the target's palette range holds.
    joint_count: u32,
    /// The target mesh entity's world matrix — the `base` every overwritten
    /// palette entry is built from, so the stomped mesh stays at its own
    /// place instead of collapsing to the world origin.
    base: Mat4,
}

/// The main-world query [`extract_spike_target`] reads: the marked target's
/// identity, skin, and world matrix.
type SpikeTargetQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static SkinnedMesh, &'static GlobalTransform),
    With<GpuAvatarSpikeTarget>,
>;

/// Copy the target's identity, joint count and world matrix into the render
/// world each frame.
fn extract_spike_target(
    mut extracted: ResMut<ExtractedSpikeTarget>,
    targets: Extract<SpikeTargetQuery<'_, '_>>,
) {
    extracted.0 = targets
        .iter()
        .next()
        .map(|(entity, skin, global)| SpikeTargetData {
            main_entity: MainEntity::from(entity),
            joint_count: u32::try_from(skin.joints.len()).unwrap_or(0),
            base: global.to_matrix(),
        });
}

/// The uniform block `gpu_avatar_spike.wgsl` reads; field order and types
/// mirror the WGSL `SpikeParams` struct exactly.
#[derive(ShaderType, Clone, Copy, Default)]
struct SpikeParams {
    /// The target mesh entity's world matrix (WGSL `base`).
    base: Mat4,
    /// The palette offset in `mat4x4` elements (WGSL `offset`).
    offset: u32,
    /// The palette entry count (WGSL `count`).
    count: u32,
    /// The [`SpikeMode::shader_mode`] selector (WGSL `mode`).
    mode: u32,
}

/// The compute pipelines and their per-frame uniform, created once in
/// [`init_spike_pipeline`].
#[derive(Resource)]
struct SpikePipeline {
    /// The overwrite pass's bind-group layout: palette storage (read-write) +
    /// params uniform.
    layout: BindGroupLayoutDescriptor,
    /// The queued overwrite pipeline (`spike` entry point).
    pipeline_id: CachedComputePipelineId,
    /// The verdict readback pass's layout: the overwrite layout plus the
    /// destination storage buffer.
    readback_layout: BindGroupLayoutDescriptor,
    /// The queued verdict readback pipeline (`spike_readback` entry point).
    readback_pipeline_id: CachedComputePipelineId,
    /// The `SpikeParams` uniform buffer, rewritten every frame.
    params: UniformBuffer<SpikeParams>,
}

/// Create the spike's bind-group layouts and queue its compute pipelines.
///
/// This is where a wgpu rejection of the `storage, read_write` binding on the
/// skin palette would first surface: as a validation error naming
/// `gpu_avatar_spike_layout` / `gpu_avatar_spike_pipeline`.
fn init_spike_pipeline(mut commands: Commands, pipeline_cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_spike_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // `SkinUniforms.current_buffer`: created by Bevy with
                // `BufferUsages::STORAGE | COPY_DST` on every storage-buffer
                // platform, so a read-write storage binding is usage-legal;
                // whether validation accepts it end-to-end is the experiment.
                storage_buffer_sized(false, None),
                uniform_buffer::<SpikeParams>(false),
            ),
        ),
    );
    let pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_spike_pipeline".into()),
        layout: vec![layout.clone()],
        shader: SPIKE_SHADER_HANDLE,
        entry_point: Some("spike".into()),
        ..default()
    });
    // The verdict readback copy. It has to be a compute pass rather than a
    // `copy_buffer_to_buffer`: Bevy creates the skin buffer WITHOUT
    // `COPY_SRC` (usage `STORAGE | COPY_DST`), so wgpu rejects buffer-copy
    // reads out of it — a real spike finding for any future palette
    // validation/debug readback.
    let readback_layout = BindGroupLayoutDescriptor::new(
        "gpu_avatar_spike_readback_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_sized(false, None),
                uniform_buffer::<SpikeParams>(false),
                storage_buffer_sized(false, None),
            ),
        ),
    );
    let readback_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("gpu_avatar_spike_readback_pipeline".into()),
        layout: vec![readback_layout.clone()],
        shader: SPIKE_SHADER_HANDLE,
        entry_point: Some("spike_readback".into()),
        ..default()
    });
    commands.insert_resource(SpikePipeline {
        layout,
        pipeline_id,
        readback_layout,
        readback_pipeline_id,
        params: UniformBuffer::default(),
    });
}

/// Everything `run_spike_compute` needs this frame, or `None` when the
/// target is missing or not (yet) registered in [`SkinUniforms`].
#[derive(Resource, Default)]
struct PreparedSpike(Option<PreparedSpikeData>);

/// The per-frame dispatch, rebuilt whole because both of its inputs can move
/// every frame: `current_buffer` is swapped (and on growth reallocated) by
/// `prepare_skins`, and the skin's offset can be moved by the allocator.
struct PreparedSpikeData {
    /// The bind group over this frame's post-swap `current_buffer`.
    bind_group: BindGroup,
    /// How many workgroups cover the target's joint count.
    workgroups: u32,
    /// The verdict readback pass's bind group (palette + params + the
    /// [`SpikeReadbackTarget`] destination), once the destination asset has
    /// prepared.
    readback_bind_group: Option<BindGroup>,
}

/// Resolve the target's palette offset (fresh from [`SkinUniforms`], after
/// `prepare_skins` ran), upload `SpikeParams`, and rebuild the bind group
/// against this frame's `current_buffer`.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy render-world system's inputs are its parameters; splitting this \
              spike-only system into two would spread one frame's bind-group build \
              over shared state for no reader's benefit"
)]
fn prepare_spike(
    mut prepared: ResMut<PreparedSpike>,
    mut pipeline: ResMut<SpikePipeline>,
    extracted: Res<ExtractedSpikeTarget>,
    skin_uniforms: Res<SkinUniforms>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mode: Res<SpikeMode>,
    readback: Option<Res<SpikeReadbackTarget>>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
) {
    prepared.0 = None;
    let Some(target) = extracted.0.as_ref() else {
        return;
    };
    if target.joint_count == 0 {
        return;
    }
    // Re-resolved every frame: skins can be moved by the offset allocator
    // whenever meshes (de)register, and the buffers themselves reallocate on
    // growth.
    let Some(offset) = skin_uniforms.skin_index(target.main_entity) else {
        return;
    };

    pipeline.params.set(SpikeParams {
        base: target.base,
        offset,
        count: target.joint_count,
        mode: mode.shader_mode(),
    });
    pipeline.params.write_buffer(&render_device, &render_queue);
    let Some(params_binding) = pipeline.params.binding() else {
        return;
    };

    let bind_group = render_device.create_bind_group(
        "gpu_avatar_spike_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            skin_uniforms.current_buffer.as_entire_binding(),
            params_binding.clone(),
        )),
    );

    // The verdict readback pass's bind group; [`init_spike_readback`]
    // registers the destination at startup, so with the spike on this exists
    // as soon as the buffer asset has prepared.
    let readback_bind_group = readback
        .and_then(|readback| buffers.get(&readback.buffer))
        .map(|destination| {
            render_device.create_bind_group(
                "gpu_avatar_spike_readback_bind_group",
                &pipeline_cache.get_bind_group_layout(&pipeline.readback_layout),
                &BindGroupEntries::sequential((
                    skin_uniforms.current_buffer.as_entire_binding(),
                    params_binding,
                    destination.buffer.as_entire_binding(),
                )),
            )
        });

    prepared.0 = Some(PreparedSpikeData {
        bind_group,
        workgroups: target.joint_count.div_ceil(WORKGROUP_SIZE),
        readback_bind_group,
    });
}

/// The destination the spike's readback pass copies the overwritten palette
/// range into (plus the expected value in the slot after it), so
/// `Readback::buffer` can lift it off the GPU — the **visibility-independent
/// verdict channel**, consumed by `spike_readback_verdict` live and by the
/// headless test through [`SpikeReadbackData`]. Owned by the plugin
/// ([`init_spike_readback`]); exists exactly when the spike is on.
#[derive(Resource, Clone)]
struct SpikeReadbackTarget {
    /// The destination buffer asset (sized for the largest palette range Bevy
    /// supports plus the expected-value slot, with the default
    /// `STORAGE | COPY_SRC | COPY_DST` usages).
    buffer: Handle<ShaderBuffer>,
}

impl ExtractResource for SpikeReadbackTarget {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The most recent completed verdict readback, raw: `count` copied palette
/// matrices followed by the expected matrix at index `count`. Updated by
/// `spike_readback_verdict` every completion; the headless test reads its
/// palette half instead of running readback machinery of its own.
#[derive(Resource, Default)]
pub(crate) struct SpikeReadbackData {
    /// The raw bytes of the last completed readback (empty until the first
    /// completes).
    pub(crate) bytes: Vec<u8>,
}

/// When the live verdict last logged, and what it said — so the WARN line
/// fires on a verdict **change** immediately and otherwise repeats at most
/// once per [`VERDICT_REPEAT`], never per frame.
#[derive(Resource, Default)]
struct SpikeVerdictLog {
    /// The last logged verdict and when it was logged.
    last: Option<(bool, Instant)>,
}

/// Create the verdict readback destination and its `Readback` driver.
///
/// Sized for [`MAX_JOINTS`] palette entries plus one expected-value slot, so
/// it never needs resizing when the target converges onto a bigger skin.
fn init_spike_readback(mut commands: Commands, mut buffers: ResMut<Assets<ShaderBuffer>>) {
    let bytes = MAX_JOINTS.saturating_add(1).saturating_mul(MAT4_BYTES);
    let buffer = buffers.add(ShaderBuffer::with_size(bytes, RenderAssetUsages::default()));
    commands
        .spawn(Readback::buffer(buffer.clone()))
        .observe(spike_readback_verdict);
    commands.insert_resource(SpikeReadbackTarget { buffer });
}

/// The 16 floats of the `index`th `mat4x4` in a readback byte buffer, if the
/// buffer holds one there.
fn mat_at(bytes: &[u8], index: usize) -> Option<[f32; 16]> {
    let start = index.checked_mul(MAT4_BYTES)?;
    let slice = bytes.get(start..start.checked_add(MAT4_BYTES)?)?;
    let mut out = [0.0_f32; 16];
    for (component, &chunk) in out.iter_mut().zip(slice.as_chunks::<4>().0) {
        *component = f32::from_ne_bytes(chunk);
    }
    Some(out)
}

/// The live, visibility-independent keystone verdict: on every completed
/// readback, compare the copied `palette[0]` against the expected matrix the
/// readback pass wrote alongside it (computed on the GPU from the same
/// `params` as the overwrite, so a moving avatar cannot fake a mismatch),
/// and log a WARN verdict — immediately on change, else at ~1 Hz.
///
/// Grep the live log for `GPU-avatar spike readback:`; `write LANDED` is the
/// §9.1-risk-1 success signal regardless of whether the stomped part is
/// visible under a mesh body.
fn spike_readback_verdict(
    readback: On<ReadbackComplete>,
    mut data: ResMut<SpikeReadbackData>,
    mut log: ResMut<SpikeVerdictLog>,
    mode: Res<SpikeMode>,
    target: Query<(Entity, &SkinnedMesh), With<GpuAvatarSpikeTarget>>,
) {
    data.bytes.clone_from(&readback.data);

    let Some((entity, skin)) = target.iter().next() else {
        return;
    };
    let joints = skin.joints.len();
    let Some(palette0) = mat_at(&readback.data, 0) else {
        return;
    };
    let Some(expected) = mat_at(&readback.data, joints) else {
        return;
    };
    // Before the first dispatch completes the buffer is all zeros, and zeros
    // == zeros must not read as a landed write. Any affine expected matrix
    // has 1 in its last component; wait until one does. (A target switch can
    // also briefly point `joints` at a not-yet-written slot — same guard.)
    let plausibly_written = expected
        .last()
        .is_some_and(|&last| (last - 1.0).abs() < 0.5);
    if !plausibly_written {
        return;
    }

    let worst = palette0
        .iter()
        .zip(&expected)
        .map(|(got, want)| (got - want).abs())
        .fold(0.0_f32, f32::max);
    let landed = worst <= VERDICT_EPSILON;

    let now = Instant::now();
    let changed = log.last.is_none_or(|(previous, _at)| previous != landed);
    let repeat_due = log
        .last
        .is_none_or(|(_previous, at)| now.duration_since(at) >= VERDICT_REPEAT);
    if !(changed || repeat_due) {
        return;
    }
    log.last = Some((landed, now));
    if landed {
        warn!(
            "GPU-avatar spike readback: target {entity} ({joints} joints) palette[0] == \
             expected ({:?} mode, worst diff {worst:e}) — write LANDED",
            *mode
        );
    } else {
        warn!(
            "GPU-avatar spike readback: target {entity} ({joints} joints) palette[0] != \
             expected ({:?} mode, worst diff {worst:e}) — write did NOT land",
            *mode
        );
    }
}

/// Encode the palette overwrite: one compute dispatch, first in the frame's
/// `Core3d` pass stream.
///
/// `Core3d` runs once per 3D camera; the extra dispatches on secondary views
/// (reflection-probe captures) rewrite the same constants — idempotent by
/// construction, so the spike does not bother de-duplicating them.
fn run_spike_compute(
    pipeline: Res<SpikePipeline>,
    prepared: Res<PreparedSpike>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(prepared) = prepared.0.as_ref() else {
        return;
    };
    let Some(compute) = pipeline_cache.get_compute_pipeline(pipeline.pipeline_id) else {
        return;
    };
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("gpu_avatar_spike_pass"),
                timestamp_writes: None,
            });
        pass.set_pipeline(compute);
        pass.set_bind_group(0, &*prepared.bind_group, &[]);
        pass.dispatch_workgroups(prepared.workgroups, 1, 1);
    }

    // The verdict readback: copy the just-written palette range (plus the
    // frame's expected value) out into the registered destination buffer. A
    // **second compute pass** (not a `copy_buffer_to_buffer` — the skin
    // buffer has no `COPY_SRC` usage, see `init_spike_pipeline`) on the same
    // encoder, so it observes the overwrite's result.
    if let Some(readback_bind_group) = prepared.readback_bind_group.as_ref()
        && let Some(readback_pipeline) =
            pipeline_cache.get_compute_pipeline(pipeline.readback_pipeline_id)
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("gpu_avatar_spike_readback_pass"),
                timestamp_writes: None,
            });
        pass.set_pipeline(readback_pipeline);
        pass.set_bind_group(0, &**readback_bind_group, &[]);
        pass.dispatch_workgroups(prepared.workgroups, 1, 1);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bevy::app::ScheduleRunnerPlugin;
    use bevy::asset::RenderAssetUsages;
    use bevy::camera::RenderTarget;
    use bevy::camera::visibility::NoFrustumCulling;
    use bevy::log::LogPlugin;
    use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
    use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
    use bevy::prelude::*;
    use bevy::render::gpu_readback::{Readback, ReadbackComplete};
    use bevy::render::render_resource::{TextureFormat, TextureUsages};
    use bevy::winit::WinitPlugin;

    use super::{GpuAvatarSpikePlugin, MAT4_BYTES, SpikeMode, SpikeReadbackData};
    use crate::face_material::{FaceMaterial, SlFaceMaterialPlugin, inert_face_material};
    use sl_viewer_testkit::TestError;

    /// The rendered frame's edge, in pixels. 128 keeps a readback row at 512
    /// bytes — already a multiple of wgpu's 256-byte row alignment.
    const FRAME: u32 = 128;

    /// How many joints the fixture skeleton binds. Two, so the spike's
    /// palette range spans more than one matrix and the offset arithmetic is
    /// exercised beyond the trivial single-entry case.
    const JOINT_COUNT: usize = 2;

    /// Where the **CPU pose** translates the skeleton to: +2 m on X, chosen
    /// so the CPU-staged palette renders the quad clearly **right of centre
    /// but still in frame**. That makes the fixture self-proving: the control
    /// must show the quad at [`CPU_POSE_PX`] (the skinned draw works and
    /// follows the CPU pose), and the spiked identity run must move it to the
    /// centre (the draw followed the compute-written palette instead). A pose
    /// pushed fully out of frame could not tell "compute didn't drive the
    /// draw" from "the mesh never rendered at all".
    const CPU_POSE_X: f32 = 2.0;

    /// The pixel column where the CPU-posed quad's centre lands: the camera
    /// sits 6 m out with a 45-degree vertical FOV over a square frame, so the
    /// z=0 plane spans ±(6·tan 22.5°) ≈ ±2.485 m and world x=+2 projects to
    /// 64·(1 + 2/2.485) ≈ 115. The quad is ±1 m (≈ ±26 px), so this column is
    /// comfortably inside it — and comfortably outside the bind-pose quad at
    /// the origin (whose right edge sits at ≈ 90).
    const CPU_POSE_PX: u32 = 115;

    /// The frame's centre pixel — inside the bind-pose quad (± ≈ 26 px around
    /// it), outside the CPU-posed quad (whose left edge sits at ≈ 90).
    const CENTRE_PX: u32 = 64;

    /// Frames to run before reading back. Measured, not guessed, and the
    /// number is about **pipeline compilation**, not probes: on this Mesa /
    /// RADV setup the mesh material's render pipeline compiles asynchronously
    /// over many headless frames (frames here take ~ms), and until it is
    /// ready the mesh is silently skipped — no log line, a clean clear-colour
    /// frame. At 30 frames nothing has ever rendered; `render_readback`'s
    /// passing tests warm up 400 frames for the same underlying reason.
    const FRAMES_TO_RUN: usize = 400;

    /// The WGSL `MARKER` constant's mirror: a 45-degree roll about +Z
    /// composed with a uniform 1.5x scale, the exact literals
    /// `gpu_avatar_spike.wgsl` writes in `marker` mode (column-major).
    fn marker_matrix() -> Mat4 {
        Mat4::from_cols_array(&[
            1.060_660_2,
            1.060_660_2,
            0.0,
            0.0,
            -1.060_660_2,
            1.060_660_2,
            0.0,
            0.0,
            0.0,
            0.0,
            1.5,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }

    /// Where a completed readback's bytes land, shared with the observer that
    /// receives them in the render world a frame after they were asked for.
    type Cell = Arc<Mutex<Option<Vec<u8>>>>;

    /// The last value a cell captured, if any.
    fn take(cell: &Cell) -> Option<Vec<u8>> {
        cell.lock().ok()?.take()
    }

    /// A quad mesh (±1 m in XY, facing +Z) fully weighted onto joint 0 of a
    /// two-joint skin — the minimal real skinned mesh: it takes Bevy's
    /// skinned pipeline, registers in `SkinUniforms`, and renders wherever
    /// its palette says.
    fn skinned_quad() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-1.0_f32, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; 4])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0_f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 4]),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
            vec![[1.0_f32, 0.0, 0.0, 0.0]; 4],
        )
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
    }

    /// Build the headless spike app: a render-to-texture camera at +Z looking
    /// at the origin, one skinned quad whose **CPU pose** moves it to
    /// [`CPU_POSE_X`] (right of centre, still in frame), the spike plugin in
    /// `mode` (or absent for the control), and a pixel readback into the
    /// returned cell. Palette bytes come from the plugin's own verdict
    /// readback ([`SpikeReadbackData`]) — the exact channel the live run
    /// logs its verdict from, so the test exercises it rather than a
    /// parallel test-only copy.
    fn spike_app(mode: Option<SpikeMode>) -> (App, Cell) {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    // Headless: no window, and the app must not exit for the
                    // lack of one.
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                // No event loop: the test drives `update` itself.
                .disable::<WinitPlugin>()
                // The test harness owns the tracing subscriber.
                .disable::<LogPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO))
        // The viewer's own face material — what a real avatar part is drawn
        // with, and the material the readback tier already proves renders
        // headlessly on this harness.
        .add_plugins(SlFaceMaterialPlugin)
        .add_plugins(GpuAvatarSpikePlugin { mode });

        // Keep the skinned mesh's transform *dirty* every frame (a same-value
        // write, so nothing moves). Bevy bakes `current_skin_index` into the
        // mesh instance's GPU uniform only when the instance (re)extracts; a
        // fully static mesh extracts once, **before** its skin registers in
        // `SkinUniforms`, and is left pointing at `u32::MAX` forever — it
        // renders nothing, spike or no spike. A live avatar's transform
        // changes every frame, so this mirrors the case the spike targets
        // rather than papering over anything.
        app.add_systems(
            Update,
            |mut meshes: Query<&mut Transform, With<SkinnedMesh>>| {
                for mut transform in &mut meshes {
                    transform.set_changed();
                }
            },
        );

        let pixels: Cell = Cell::default();
        let pixels_in_observer = Arc::clone(&pixels);

        app.add_systems(
            Startup,
            move |mut commands: Commands,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<FaceMaterial>>,
                  mut images: ResMut<Assets<Image>>,
                  mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
                // The render target, plus COPY_SRC so the readback can lift
                // the frame back off the GPU.
                let mut target =
                    Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
                target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
                let target = images.add(target);
                commands.spawn((
                    Camera3d::default(),
                    RenderTarget::Image(target.clone().into()),
                    bevy::camera::Hdr,
                    Msaa::Off,
                    Transform::from_xyz(0.0, 0.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
                ));
                let pixels_cell = Arc::clone(&pixels_in_observer);
                commands.spawn(Readback::texture(target)).observe(
                    move |readback: On<ReadbackComplete>| {
                        if let Ok(mut slot) = pixels_cell.lock() {
                            *slot = Some(readback.data.clone());
                        }
                    },
                );

                // The CPU pose: both joints at +X, so the staged palette
                // renders the quad right of centre (see `CPU_POSE_X`).
                let joints = vec![
                    commands
                        .spawn(Transform::from_xyz(CPU_POSE_X, 0.0, 0.0))
                        .id(),
                    commands
                        .spawn(Transform::from_xyz(CPU_POSE_X, 1.0, 0.0))
                        .id(),
                ];
                let inverse_bindposes =
                    bindposes.add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; 2]));
                commands.spawn((
                    Mesh3d(meshes.add(skinned_quad())),
                    MeshMaterial3d(materials.add(inert_face_material(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.0, 0.0),
                        unlit: true,
                        ..default()
                    }))),
                    Transform::IDENTITY,
                    SkinnedMesh {
                        inverse_bindposes,
                        joints,
                    },
                    // A skinned mesh's frustum bounds come from its bind
                    // pose; keep the skin registered (and drawn) regardless
                    // of where a palette moves it.
                    NoFrustumCulling,
                ));
            },
        );

        (app, pixels)
    }

    /// Build `spike_app(mode)`, run it [`FRAMES_TO_RUN`] frames, and return
    /// the last captured pixel frame and the plugin's last verdict-readback
    /// bytes (`None` when the plugin is off or no readback ever completed).
    fn run_spike_app(mode: Option<SpikeMode>) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let (mut app, pixels) = spike_app(mode);
        app.finish();
        app.cleanup();
        for _frame in 0..FRAMES_TO_RUN {
            app.update();
        }
        let palette = app
            .world()
            .get_resource::<SpikeReadbackData>()
            .map(|data| data.bytes.clone())
            .filter(|bytes| !bytes.is_empty());
        (take(&pixels), palette)
    }

    /// The RGBA texel at `(x, y)` — on the frame's vertical midline when `y`
    /// is [`CENTRE_PX`].
    fn pixel_at(frame: &[u8], x: u32, y: u32) -> Option<[u8; 4]> {
        let index = usize::try_from(y.checked_mul(FRAME)?.checked_add(x)?.checked_mul(4)?).ok()?;
        let texel = frame.get(index..index.checked_add(4)?)?;
        texel.try_into().ok()
    }

    /// Whether a texel is unmistakably the fixture's unlit red quad rather
    /// than the clear colour.
    fn is_red(texel: [u8; 4]) -> bool {
        let [r, g, b, _a] = texel;
        r >= 200 && g <= 80 && b <= 80
    }

    /// Where the frame's red pixels are, as `(min_x, min_y, max_x, max_y)` —
    /// `None` when the quad rendered nowhere. Failure messages quote this so
    /// a wrong pose says where the quad actually went instead of only where
    /// it is not.
    fn red_bounds(frame: &[u8]) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for y in 0..FRAME {
            for x in 0..FRAME {
                let Some(texel) = pixel_at(frame, x, y) else {
                    continue;
                };
                if !is_red(texel) {
                    continue;
                }
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                });
            }
        }
        bounds
    }

    /// Reinterpret readback bytes as the `f32`s the GPU wrote (native
    /// endianness, which is what the GPU shares with the host).
    fn floats(data: &[u8]) -> Vec<f32> {
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|&chunk| f32::from_ne_bytes(chunk))
            .collect()
    }

    /// The fixture's palette range — the first [`JOINT_COUNT`] matrices of the
    /// verdict readback (the buffer is sized for [`super::MAX_JOINTS`], so
    /// everything past the fixture's two entries is padding / the expected
    /// slot).
    fn palette_range(bytes: &[u8]) -> Option<&[u8]> {
        bytes.get(..JOINT_COUNT.saturating_mul(MAT4_BYTES))
    }

    /// **The keystone claim, end to end through the draw.** With the spike in
    /// `identity` mode, the compute pass overwrites the skin's palette with
    /// the mesh's own world matrix (identity here), so the quad must render
    /// at the **origin** — even though the CPU-staged pose places it at
    /// x = +2 m ([`CPU_POSE_PX`]). Red at the centre and nothing at the
    /// CPU-pose column can only mean the draw read the compute-written
    /// palette: binding `SkinUniforms.current_buffer` as
    /// `storage, read_write` was accepted, the write landed after
    /// `prepare_skins`' staging upload and before the draw, and the offset
    /// resolution held across the per-frame buffer swap.
    ///
    /// The control (spike off) must show the exact opposite — the quad at the
    /// CPU-pose column and nothing at the centre — which proves the fixture
    /// has teeth twice over: the skinned draw renders at all, and it follows
    /// the CPU pose when nobody stomps the palette.
    ///
    /// The spiked run's palette bytes are asserted too (identity matrices),
    /// so a failure distinguishes "the compute never wrote" from "the compute
    /// wrote but the draw read something else" — the two very different
    /// §9.1-risk-1 failure modes.
    ///
    /// Skips (loudly) when no frame comes back: a machine with no GPU adapter
    /// cannot answer, mirroring the rest of the readback tier.
    #[test]
    fn the_draw_renders_the_compute_written_palette() -> Result<(), TestError> {
        let (control_pixels, _control_palette) = run_spike_app(None);
        let Some(control_frame) = control_pixels else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        let control_posed = pixel_at(&control_frame, CPU_POSE_PX, CENTRE_PX)
            .ok_or("the control frame is too small to hold the CPU-pose pixel")?;
        assert!(
            is_red(control_posed),
            "the control (spike off) must show the quad at the CPU pose (x = {CPU_POSE_X} m, \
             pixel column {CPU_POSE_PX}), but that pixel is {control_posed:?} and the frame's \
             red pixels span {:?} — the skinned fixture did not render where the CPU pose \
             puts it, so the spiked comparison below would prove nothing",
            red_bounds(&control_frame)
        );
        let control_centre = pixel_at(&control_frame, CENTRE_PX, CENTRE_PX)
            .ok_or("the control frame is too small to hold its own centre pixel")?;
        assert!(
            !is_red(control_centre),
            "the control (spike off) shows the quad at the frame's centre ({control_centre:?}) — \
             the CPU pose was supposed to move it right of centre, so the fixture cannot \
             discriminate the compute-written pose from the CPU one"
        );

        let (spiked_pixels, spiked_palette) = run_spike_app(Some(SpikeMode::Identity));
        let spiked_frame =
            spiked_pixels.ok_or("the control run rendered but the spiked run returned no frame")?;

        // The palette bytes first, so a pixel failure below is attributable:
        // identity matrices here mean the compute write landed; the pixels
        // then say whether the draw read them.
        let palette = spiked_palette.ok_or(
            "the spiked run rendered but no palette bytes came back — the plugin's verdict \
             readback never completed, so nothing below is attributable",
        )?;
        let got = floats(
            palette_range(&palette)
                .ok_or("the verdict readback holds fewer bytes than the fixture's palette")?,
        );
        let expected: Vec<f32> = std::iter::repeat_n(Mat4::IDENTITY.to_cols_array(), JOINT_COUNT)
            .flatten()
            .collect();
        let worst = got
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            got.len() == expected.len() && worst <= 1.0e-5,
            "in identity mode the palette range must hold identity matrices, but the readback \
             is {got:?} — the compute write itself did not land"
        );

        let spiked_centre = pixel_at(&spiked_frame, CENTRE_PX, CENTRE_PX)
            .ok_or("the spiked frame is too small to hold its own centre pixel")?;
        let spiked_posed = pixel_at(&spiked_frame, CPU_POSE_PX, CENTRE_PX)
            .ok_or("the spiked frame is too small to hold the CPU-pose pixel")?;
        assert!(
            is_red(spiked_centre) && !is_red(spiked_posed),
            "with the spike in identity mode the quad must render at the origin — the \
             compute-written palette — but centre is {spiked_centre:?} and the CPU-pose \
             column is {spiked_posed:?}: the palette held the compute-written matrices \
             (asserted above), so the draw read something else — the §9.1 risk-1 \
             ordering/consumption failure (fork-skinning.wgsl fallback territory)"
        );
        Ok(())
    }

    /// **The write lands at the Bevy-allocated offset, with the exact
    /// payload.** In `marker` mode the spike writes `base * MARKER` (base is
    /// identity here, so exactly `MARKER`) over both palette entries; the
    /// plugin's verdict readback (`run_spike_compute`'s second pass — the
    /// same channel the live WARN verdict reads) then lifts the palette
    /// range — addressed by `SkinUniforms::skin_index`'s offset — back off
    /// the GPU. Reading `MARKER` (and not the CPU-staged `translate(+2 m)`
    /// pose) proves the compute overwrote the staged upload at the right
    /// offset in the post-swap buffer.
    #[test]
    fn the_compute_write_lands_at_the_bevy_allocated_offset() -> Result<(), TestError> {
        let (pixels, palette) = run_spike_app(Some(SpikeMode::Marker));
        if pixels.is_none() {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        }
        let palette = palette.ok_or(
            "the frame rendered but no palette bytes came back — the plugin's verdict \
             readback never completed, so the offset/content claim is untested",
        )?;
        let got = floats(
            palette_range(&palette)
                .ok_or("the verdict readback holds fewer bytes than the fixture's palette")?,
        );
        let expected: Vec<f32> = std::iter::repeat_n(marker_matrix().to_cols_array(), JOINT_COUNT)
            .flatten()
            .collect();
        assert!(
            got.len() == expected.len(),
            "expected {} palette floats (two mat4 entries), read back {}",
            expected.len(),
            got.len()
        );
        let worst = got
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst <= 1.0e-5,
            "the palette range at the Bevy-allocated offset does not hold the marker matrices \
             (worst component error {worst}): read {got:?}, expected two copies of \
             {expected:?} — the CPU-staged pose (a translate to x = {CPU_POSE_X}) would mean \
             the compute write missed or was overwritten"
        );
        Ok(())
    }
}
