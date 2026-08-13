//! **The GPU-avatar pose pipeline, Phases 1a+1b+2**
//! (`roadmap/context/gpu-avatars.md` §1, §2.1–§2.4, §5.3, §5.4, §7): a
//! compute pipeline that samples each playing `.anim` clip (pass A, over the
//! upload-once clip arena, deduplicated per (clip, phase) sample job),
//! blends the contributions per joint by priority/ease with the procedural
//! idle adjusters and the sparse CPU adjuster corrections (pass B), re-runs
//! the Second Life skeletal recurrence over the CPU-composed rest skeletons
//! (pass C), and writes real skin palettes into Bevy's `SkinUniforms` buffer
//! at the offsets Bevy allocated (pass D), exactly as the keystone spike
//! proved (`crate::gpu_avatar_spike`).
//!
//! **The GPU in-place path is the DEFAULT** on a capable device (compute
//! shaders + storage-buffer skinning, checked once at startup against the
//! render device — [`select_gpu_avatar_path`] logs which path won); a
//! downlevel device falls back to the fully intact legacy CPU path
//! automatically. `SL_VIEWER_GPU_AVATARS` (read once at App build) is an
//! **override**, not an enable:
//!
//! - **unset / `1` / `real`** — the default **in-place path** (Phase 2):
//!   pass D writes the **real avatar's** skin slots (no offset, no ghosts) —
//!   the rendered avatar IS GPU-sampled, GPU-blended and GPU-FK-posed. The
//!   CPU is demoted to scheduling (playback reconcile + the §2.1 sample-job
//!   dedup) and the §5.3 **adjuster mini-pose** (look-at / reach / IK /
//!   physics run against a chain mini-FK over ~a tenth of the joints, and
//!   publish their channel changes as sparse `GpuCorrection`s pass B folds
//!   in); it stops writing the ~200 skinning joints' `GlobalTransform`s, so
//!   `extract_skins` sees no changed joints and its serial cost collapses;
//!   only the **socket subset** — worn attachment-point joints, the rigid
//!   eyeballs' eye joints, and `mHead` (the camera focus) — stays
//!   CPU-written, from the §5.4 mini-FK
//!   ([`sl_client_bevy::BevySkeleton::deformed_world_chain`]).
//!   Note Bevy's transform propagation still re-globals (and re-dirties) the
//!   whole joint tree on any frame the avatar's **anchor moves** — the
//!   collapse holds for stationary (dancing/idle) avatars, the crowd case;
//!   walking avatars pay the propagation + extract cost until Phase 4
//!   removes the joint entities.
//! - **`cpu` / `off` / `0`** — force the legacy CPU pose path (manual
//!   fallback / debugging); registers nothing, byte-for-byte today's path.
//! - **`ghost`** — the Phase 1a **side-by-side comparison harness**: the CPU
//!   path is fully live (joints written, `extract_skins` uploads), and each
//!   rigged avatar additionally gets a **GPU ghost** — a duplicate of its
//!   skinned submeshes whose own palette slots the compute overwrites with
//!   the GPU-FK result, root-offset ~2 m aside
//!   (`SL_VIEWER_GPU_AVATARS_OFFSET`, Bevy world +X, default 2), rigid
//!   eyeballs CPU-mirrored and a floating "GPU" label overhead. A correct FK
//!   renders the identical pose twice; a failed compute write leaves the
//!   ghost exactly on top of the original (no second avatar = the failure
//!   signature).
//!
//! **Live verdict channel (both placements):**
//! `SL_VIEWER_GPU_AVATARS_READBACK=1` copies the most-jointed target's
//! just-written palette back (a compute copy — the skin buffer carries no
//! `COPY_SRC`) next to a CPU-expected palette computed the same frame — in
//! ghost mode from the live joint globals (the true CPU path), in real mode
//! from the full CPU mirror pipeline ([`types::mirror_local_pose`] over the
//! very clip/playback/job/correction data uploaded this frame, then
//! [`types::reference_fk`] — the golden-tested pass A+B+C references; the
//! joint globals are frozen there) — and logs
//! `GPU-avatar palette readback: … GPU palette == CPU palette` (or `!=`, the
//! divergence signal) at ~1 Hz.
//!
//! Still deliberately **not** here: GPU picking (Phase 3 — the CPU pick
//! reads frozen joints in real mode and degrades to rest-pose accuracy
//! meanwhile), and animesh control avatars (fully CPU, per the design's
//! Phase 4 migration). The **ghost harness keeps the Phase 1 split** (CPU
//! sample+blend, `LocalPose` upload, passes C+D only): its purpose is
//! comparing the GPU FK against the live CPU pose path side by side.

pub(crate) mod render;
pub(crate) mod stage;
pub(crate) mod types;

#[cfg(test)]
mod tests;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::prepass::node::early_prepass;
use bevy::core_pipeline::schedule::Core3d;
use bevy::pbr::{EARLY_SHADOW_PASS, per_view_shadow_pass, shared_shadow_pass};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::render_resource::DownlevelFlags;
use bevy::render::renderer::{RenderAdapter, RenderDevice};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

pub(crate) use stage::GpuAvatarPoseFeed;

/// The internal handle `pose.wgsl` is loaded under.
const POSE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7e3a9c15-24d8-4b6f-9a01-c85e2f7b4d39");

/// The path **override** (the GPU in-place path is the default): `=cpu` /
/// `=off` / `=0` forces the legacy CPU pose path, `=ghost` the side-by-side
/// comparison harness; unset / `=1` / `=real` selects the default GPU
/// in-place path (which still falls back to the CPU path automatically on a
/// device without compute/storage-buffer support).
const ENV_FLAG: &str = "SL_VIEWER_GPU_AVATARS";

/// The debug sub-flag: `SL_VIEWER_GPU_AVATARS_READBACK=1` turns the per-frame
/// palette readback + verdict log on (any placement).
const ENV_READBACK: &str = "SL_VIEWER_GPU_AVATARS_READBACK";

/// The ghost display offset in metres along Bevy world +X (default
/// [`DEFAULT_GHOST_OFFSET`]; ghost placement only).
const ENV_OFFSET: &str = "SL_VIEWER_GPU_AVATARS_OFFSET";

/// The default ghost display offset, metres.
const DEFAULT_GHOST_OFFSET: f32 = 2.0;

/// How many storage buffers the pose pipeline binds in one compute stage
/// (`pose.wgsl` group 0, bindings 1..=8) — the device must allow at least
/// this many per stage.
const REQUIRED_STORAGE_BUFFERS: u32 = 8;

/// Where the compute-written palettes land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuAvatarPlacement {
    /// The side-by-side comparison harness (Phase 1a): spawned ghost
    /// duplicates, offset aside; the CPU path fully live underneath.
    Ghost,
    /// The in-place path (Phase 1b, the default): the real avatars' own skin
    /// slots; skinning joints frozen, sockets CPU-mini-FK'd.
    Real,
}

/// The pipeline's resolved run mode, read once from the environment at App
/// build and carried as a resource (never re-read per frame).
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct GpuAvatarsMode {
    /// Ghost harness or the in-place real path.
    pub(crate) placement: GpuAvatarPlacement,
    /// Whether the pipeline is actually running: `true` from build, flipped
    /// to `false` by [`select_gpu_avatar_path`] when the device lacks the
    /// required capabilities — every main-world consumer gates on this, so a
    /// downlevel device runs the legacy CPU pose path automatically.
    pub(crate) active: bool,
    /// Whether the palette readback + verdict log is on.
    pub(crate) readback: bool,
    /// Whether the live staging systems run (`false` only in the headless
    /// tests, which stage fixture data by hand).
    pub(crate) live: bool,
    /// The ghost display offset, metres along Bevy world +X (ghost placement
    /// only; the real path writes in place).
    pub(crate) ghost_offset: f32,
}

/// The GPU-avatar plugin. `mode: None` (the `cpu` override) registers
/// **nothing** — the byte-for-byte legacy CPU path.
pub(crate) struct GpuAvatarsPlugin {
    /// The resolved mode, or `None` for the forced legacy CPU path.
    pub(crate) mode: Option<GpuAvatarsMode>,
}

impl GpuAvatarsPlugin {
    /// Build the plugin from the environment, read once here. The GPU
    /// in-place path is the **default**; the env var only overrides.
    #[must_use]
    pub(crate) fn from_env() -> Self {
        let placement = match std::env::var(ENV_FLAG) {
            Ok(value) if value == "cpu" || value == "off" || value == "0" => {
                warn!("GPU avatars: legacy CPU pose path forced by {ENV_FLAG}={value}");
                None
            }
            Ok(value) if value == "ghost" => Some(GpuAvatarPlacement::Ghost),
            Ok(value) if value == "1" || value == "real" => Some(GpuAvatarPlacement::Real),
            Ok(other) => {
                warn!(
                    "{ENV_FLAG}={other:?} is not a recognised value (expected `cpu`/`off`, \
                     `ghost`, or `1`/`real`); using the default GPU in-place path"
                );
                Some(GpuAvatarPlacement::Real)
            }
            // The default: GPU in-place, hardware permitting.
            Err(_unset) => Some(GpuAvatarPlacement::Real),
        };
        let mode = placement.map(|placement| GpuAvatarsMode {
            placement,
            active: true,
            readback: std::env::var(ENV_READBACK).as_deref() == Ok("1"),
            live: true,
            ghost_offset: std::env::var(ENV_OFFSET)
                .ok()
                .and_then(|raw| raw.parse::<f32>().ok())
                .filter(|offset| offset.is_finite())
                .unwrap_or(DEFAULT_GHOST_OFFSET),
        });
        Self { mode }
    }
}

/// Whether the device can run the pose pipeline: compute shaders, Bevy's
/// storage-buffer skinning path (the palette write-in binds
/// `SkinUniforms.current_buffer` as a storage buffer — impossible when Bevy
/// fell back to uniform-buffer skins, mirroring
/// [`bevy::pbr::skins_use_uniform_buffers`]), and enough storage-buffer
/// bindings for one compute stage.
fn gpu_avatar_device_capable(device: &RenderDevice, adapter: Option<&RenderAdapter>) -> bool {
    let limits = device.limits();
    if bevy::pbr::skins_use_uniform_buffers(&limits) {
        return false;
    }
    if limits.max_storage_buffers_per_shader_stage < REQUIRED_STORAGE_BUFFERS {
        return false;
    }
    adapter.is_none_or(|adapter| {
        adapter
            .get_downlevel_capabilities()
            .flags
            .contains(DownlevelFlags::COMPUTE_SHADERS)
    })
}

/// The startup path selection (`PreStartup`, after the render device
/// exists): keep the GPU path on a capable device, otherwise flip
/// [`GpuAvatarsMode::active`] off so the viewer runs the legacy CPU pose
/// path — and log which path won either way.
fn select_gpu_avatar_path(
    mut mode: ResMut<GpuAvatarsMode>,
    device: Option<Res<RenderDevice>>,
    adapter: Option<Res<RenderAdapter>>,
) {
    let capable = device
        .as_deref()
        .is_some_and(|device| gpu_avatar_device_capable(device, adapter.as_deref()));
    if !capable {
        mode.active = false;
        warn!(
            "GPU avatars: the device lacks compute-shader / storage-buffer support — \
             falling back to the legacy CPU pose path"
        );
        return;
    }
    match mode.placement {
        GpuAvatarPlacement::Real => info!(
            "GPU avatars: GPU in-place pose path ACTIVE (the default): rendered avatars \
             are GPU-FK-posed, skinning joints frozen, sockets CPU-mini-FK'd \
             (readback {})",
            if mode.readback { "on" } else { "off" },
        ),
        GpuAvatarPlacement::Ghost => warn!(
            "GPU avatars: side-by-side GHOST harness active: every rigged avatar renders \
             twice — the CPU pose in place and a GPU-FK ghost {} m to the side \
             (readback {})",
            mode.ghost_offset,
            if mode.readback { "on" } else { "off" },
        ),
    }
}

impl Plugin for GpuAvatarsPlugin {
    fn build(&self, app: &mut App) {
        let Some(mode) = self.mode else {
            return;
        };
        load_internal_asset!(
            app,
            POSE_SHADER_HANDLE,
            "gpu_avatars/pose.wgsl",
            Shader::from_wgsl
        );
        app.insert_resource(mode)
            .init_resource::<GpuAvatarPoseFeed>()
            .init_resource::<stage::GpuAvatarRegistry>()
            .init_resource::<stage::GpuAvatarStaging>()
            .add_plugins(ExtractResourcePlugin::<stage::GpuAvatarStaging>::default())
            // The device capability gate: runs once before anything else
            // (the render device exists after plugin finish) and demotes an
            // incapable device to the legacy CPU path via `mode.active`.
            .add_systems(PreStartup, select_gpu_avatar_path);
        if mode.live {
            app.add_systems(
                PostUpdate,
                // After the pose driver, so the feed holds this frame's final
                // blended poses and (in ghost mode) the joint globals are
                // posed.
                stage::stage_gpu_avatars.after(crate::animations::pose_avatar_skeletons),
            );
        }
        if mode.live && mode.placement == GpuAvatarPlacement::Ghost {
            // The ghost-harness companions; the real path has no ghost
            // entities, no rigid mirrors and no labels.
            app.add_systems(
                PostUpdate,
                (
                    // Before propagation, so a fresh ghost's re-extract (and
                    // its baked `current_skin_index`) lands the same frame.
                    stage::churn_gpu_ghost_transforms.before(TransformSystems::Propagate),
                    // Rigid ghosts (the eyeballs) are CPU-placed from the
                    // posed source globals — after the staging system so a
                    // just-spawned record is already tracked.
                    stage::place_gpu_rigid_ghosts.after(stage::stage_gpu_avatars),
                    // The floating "GPU" label over each ghost, placed from
                    // the same feed roots the ghost palettes are composed
                    // under.
                    stage::sync_gpu_avatar_labels.after(stage::stage_gpu_avatars),
                ),
            );
        }
        if mode.readback {
            app.init_resource::<render::GpuAvatarReadbackData>()
                .init_resource::<render::GpuAvatarVerdictLog>()
                .add_plugins(ExtractResourcePlugin::<render::GpuAvatarReadbackTarget>::default())
                .add_systems(Startup, render::init_gpu_avatar_readback);
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<stage::GpuAvatarStaging>()
            .init_resource::<render::GpuAvatarBuffers>()
            .init_resource::<render::PreparedGpuAvatars>()
            .add_systems(RenderStartup, render::init_gpu_avatar_pipelines)
            .add_systems(
                Render,
                // After `PrepareResources` (where Bevy's `prepare_skins` swaps
                // current/prev and uploads its CPU staging), so palette
                // offsets resolve against — and the bind group binds — the
                // post-swap, post-realloc `current_buffer`. The spike's §2.4
                // step 2.
                render::prepare_gpu_avatars.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                // First in the frame's pass encoding: before the prepass and
                // both shadow-pass encoders, so every palette consumer this
                // frame reads the compute-written palettes. The spike's §2.4
                // step 3.
                render::run_gpu_avatar_compute
                    .in_set(Core3dSystems::Prepass)
                    .before(early_prepass)
                    .before(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                    .before(shared_shadow_pass::<EARLY_SHADOW_PASS>),
            );
    }
}
