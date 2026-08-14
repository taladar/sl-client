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
//! render device — [`select_gpu_avatar_path`] logs which path won). Phase 4
//! removed the per-avatar joint entities, so the in-place path is now the
//! ONLY skinning path: pass D writes the **real avatar's** skin slots — the
//! rendered avatar IS GPU-sampled, GPU-blended and GPU-FK-posed. The CPU is
//! demoted to scheduling (playback reconcile + the §2.1 sample-job dedup) and
//! the §5.3 **adjuster mini-pose** (look-at / reach / IK / physics run
//! against a chain mini-FK over ~a tenth of the joints, and publish their
//! channel changes as sparse `GpuCorrection`s pass B folds in). The skinning
//! joints are gone entirely, so `extract_skins` sees no avatar joints at all;
//! only the **socket subset** — worn attachment-point nodes, the rigid
//! eyeballs, and the `mHead` camera focus — is CPU-written, from the §5.4
//! mini-FK ([`sl_client_bevy::BevySkeleton::deformed_world_chain`]).
//!
//! A **downlevel** device (no compute shaders / no storage-buffer skinning)
//! has no GPU path and — with the joint entities removed — no CPU skinner
//! either: [`select_gpu_avatar_path`] flips [`GpuAvatarsMode::active`] off and
//! warns once, and such avatars render at their bind pose.
//!
//! **Live verdict channel:** `SL_VIEWER_GPU_AVATARS_READBACK=1` copies the
//! most-jointed target's just-written palette back (a compute copy — the skin
//! buffer carries no `COPY_SRC`) next to a CPU-expected palette computed the
//! same frame from the full CPU mirror pipeline ([`types::mirror_local_pose`]
//! over the very clip/playback/job/correction data uploaded this frame, then
//! [`types::reference_fk`] — the golden-tested pass A+B+C references) — and
//! logs `GPU-avatar palette readback: … GPU palette == CPU palette` (or `!=`,
//! the divergence signal) at ~1 Hz.
//!
//! Still deliberately **not** here: animesh control avatars (fully CPU, per
//! the design's later migration — their control-avatar joints are a separate
//! joint set kept intact this phase).

pub(crate) mod crowd;
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

pub(crate) use stage::{GpuAvatarPoseFeed, GpuSkinBinding, PoseSlotKey};

/// The internal handle `pose.wgsl` is loaded under.
const POSE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7e3a9c15-24d8-4b6f-9a01-c85e2f7b4d39");

/// The debug sub-flag: `SL_VIEWER_GPU_AVATARS_READBACK=1` turns the per-frame
/// palette readback + verdict log on.
const ENV_READBACK: &str = "SL_VIEWER_GPU_AVATARS_READBACK";

/// How many storage buffers the pose pipeline binds in one compute stage
/// (`pose.wgsl` group 0, bindings 1..=8) — the device must allow at least
/// this many per stage.
const REQUIRED_STORAGE_BUFFERS: u32 = 8;

/// The pipeline's resolved run mode, read once from the environment at App
/// build and carried as a resource (never re-read per frame).
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct GpuAvatarsMode {
    /// Whether the pipeline is actually running: `true` from build, flipped
    /// to `false` by [`select_gpu_avatar_path`] when the device lacks the
    /// required capabilities — every main-world consumer gates on this. With
    /// the joint entities removed (Phase 4) a downlevel device has no CPU
    /// skinner fallback, so its avatars render at their bind pose.
    pub(crate) active: bool,
    /// Whether the palette readback + verdict log is on.
    pub(crate) readback: bool,
    /// Whether the live staging systems run (`false` only in the headless
    /// tests, which stage fixture data by hand).
    pub(crate) live: bool,
}

/// The GPU-avatar plugin. Phase 4 made the GPU in-place path the only
/// skinning path, so this always registers.
pub(crate) struct GpuAvatarsPlugin {
    /// The resolved mode.
    pub(crate) mode: GpuAvatarsMode,
}

impl GpuAvatarsPlugin {
    /// Build the plugin from the environment, read once here.
    #[must_use]
    pub(crate) fn from_env() -> Self {
        Self {
            mode: GpuAvatarsMode {
                active: true,
                readback: std::env::var(ENV_READBACK).as_deref() == Ok("1"),
                live: true,
            },
        }
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
            "GPU avatars: the device lacks compute-shader / storage-buffer support and \
             Phase 4 removed the CPU skinner — avatars render at their bind pose"
        );
        return;
    }
    info!(
        "GPU avatars: GPU in-place pose path ACTIVE: rendered avatars are GPU-FK-posed, \
         no skinning joint entities, sockets CPU-mini-FK'd (readback {})",
        if mode.readback { "on" } else { "off" },
    );
}

impl Plugin for GpuAvatarsPlugin {
    fn build(&self, app: &mut App) {
        let mode = self.mode;
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
            .init_resource::<crowd::GpuCrowd>()
            .add_plugins(ExtractResourcePlugin::<stage::GpuAvatarStaging>::default())
            // The device capability gate: runs once before anything else
            // (the render device exists after plugin finish) and demotes an
            // incapable device to the legacy CPU path via `mode.active`.
            .add_systems(PreStartup, select_gpu_avatar_path);
        if mode.live {
            app.init_resource::<render::GpuAvatarBounds>()
                .add_plugins(ExtractResourcePlugin::<render::GpuAvatarBoundsTarget>::default())
                // The Phase 5 posed-bounds destination + its `Readback` driver.
                .add_systems(Startup, render::init_gpu_avatar_bounds)
                .add_systems(
                    // The synthetic-crowd debug spawner (`SL_VIEWER_CROWD`): a no-op
                    // unless the env selected a crowd. Runs in `Update` so its spawn
                    // commands flush before `PostUpdate` stages the copies.
                    Update,
                    crowd::spawn_crowd,
                )
                .add_systems(
                    PostUpdate,
                    (
                        // After the pose driver, so the feed holds this frame's final
                        // blended poses and (in ghost mode) the joint globals are
                        // posed.
                        // After both pose feeds: avatars publish in `pose_avatar_skeletons`,
                        // animesh in `publish_control_avatars`.
                        // The crowd publisher runs between: after the template
                        // avatar published, before the stage reads the feed.
                        crowd::publish_crowd
                            .after(crate::animations::pose_avatar_skeletons)
                            .after(crate::animesh::publish_control_avatars),
                        stage::stage_gpu_avatars
                            .after(crate::animations::pose_avatar_skeletons)
                            .after(crate::animesh::publish_control_avatars)
                            .after(crowd::publish_crowd),
                        // Phase 5: set each avatar's `Aabb` from its read-back
                        // world bound (so off-screen avatars frustum-cull), after
                        // the stage refreshed the slot map and before Bevy's
                        // `CalculateBounds` would otherwise install the meaningless
                        // dummy-joint bind-pose AABB.
                        stage::apply_gpu_avatar_bounds
                            .after(stage::stage_gpu_avatars)
                            .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds),
                        // The `SL_VIEWER_LOG_AVATAR_BOUNDS` census (inert unless
                        // the env is set), after `CheckVisibility` so it reads
                        // the culled `ViewVisibility` `extract_skins` sees, not
                        // the pre-cull reset value.
                        stage::log_avatar_bounds
                            .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
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
