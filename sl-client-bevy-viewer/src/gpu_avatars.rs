//! **Phase 1a of the GPU-avatar pipeline** (`roadmap/context/gpu-avatars.md`
//! §1, §2.2 passes C+D, §2.3, §2.4): a compute pipeline that re-runs the
//! Second Life skeletal recurrence on the GPU — from CPU-composed rest
//! skeletons and the CPU-blended local pose — and writes real skin palettes
//! into Bevy's `SkinUniforms` buffer at the offsets Bevy allocated, exactly as
//! the keystone spike proved (`crate::gpu_avatar_spike`), but with the real
//! pose instead of a marker.
//!
//! **Purely additive and A/B-testable.** The CPU pose path is untouched and
//! still owns every real avatar: joints are still written, `extract_skins`
//! still uploads CPU palettes. Behind `SL_VIEWER_GPU_AVATARS=1` (read once at
//! App build, default OFF — the `SL_VIEWER_GPU_AVATAR_SPIKE` idiom), each
//! rigged avatar additionally gets a **GPU ghost**: a duplicate of its skinned
//! submeshes whose own palette slot the compute pipeline overwrites with the
//! GPU-FK result, root-offset ~2 m to the side
//! (`SL_VIEWER_GPU_AVATARS_OFFSET`, metres along Bevy world +X, default 2).
//! The original (CPU) and the ghost (GPU) animate side by side: a correct FK
//! renders the identical pose twice, and any divergence is directly visible.
//! If the compute write fails, Bevy's own CPU fill of the ghost's slot leaves
//! the ghost exactly on top of the original — "no second avatar appears" is
//! the failure signature.
//!
//! **Live verdict channel:** `SL_VIEWER_GPU_AVATARS_READBACK=1` additionally
//! copies the most-jointed ghost's just-written palette back (a compute copy —
//! the skin buffer carries no `COPY_SRC`) next to the CPU-path palette
//! computed the same frame, and logs
//! `GPU-avatar palette readback: … GPU palette == CPU palette` (or `!=`, the
//! divergence signal) at ~1 Hz.
//!
//! What is deliberately **not** here (Phase 1b+): freezing the joint
//! entities, the socket-joint CPU mini-FK, `extract_skins` collapse
//! measurement, and GPU sampling/blending (Phase 2 — the CPU supplies the
//! blended pose via [`GpuAvatarPoseFeed`]).

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
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

pub(crate) use stage::GpuAvatarPoseFeed;

/// The internal handle `pose.wgsl` is loaded under.
const POSE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7e3a9c15-24d8-4b6f-9a01-c85e2f7b4d39");

/// The master flag: `SL_VIEWER_GPU_AVATARS=1` enables the pipeline (and the
/// ghosts). Unset / anything else leaves the viewer byte-for-byte on the
/// normal path.
const ENV_FLAG: &str = "SL_VIEWER_GPU_AVATARS";

/// The debug sub-flag: `SL_VIEWER_GPU_AVATARS_READBACK=1` turns the per-frame
/// palette readback + verdict log on.
const ENV_READBACK: &str = "SL_VIEWER_GPU_AVATARS_READBACK";

/// The ghost display offset in metres along Bevy world +X (default
/// [`DEFAULT_GHOST_OFFSET`]).
const ENV_OFFSET: &str = "SL_VIEWER_GPU_AVATARS_OFFSET";

/// The default ghost display offset, metres.
const DEFAULT_GHOST_OFFSET: f32 = 2.0;

/// The pipeline's resolved run mode, read once from the environment at App
/// build and carried as a resource (never re-read per frame).
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct GpuAvatarsMode {
    /// Whether the palette readback + verdict log is on.
    pub(crate) readback: bool,
    /// Whether the live staging systems run (`false` only in the headless
    /// tests, which stage fixture data by hand).
    pub(crate) live: bool,
    /// The ghost display offset, metres along Bevy world +X.
    pub(crate) ghost_offset: f32,
}

/// The Phase 1a GPU-avatar plugin. `mode: None` (the flag unset) registers
/// **nothing**.
pub(crate) struct GpuAvatarsPlugin {
    /// The resolved mode, or `None` for the byte-for-byte normal path.
    pub(crate) mode: Option<GpuAvatarsMode>,
}

impl GpuAvatarsPlugin {
    /// Build the plugin from the environment, read once here.
    #[must_use]
    pub(crate) fn from_env() -> Self {
        let mode = match std::env::var(ENV_FLAG) {
            Ok(value) if value == "1" => {
                let readback = std::env::var(ENV_READBACK).as_deref() == Ok("1");
                let ghost_offset = std::env::var(ENV_OFFSET)
                    .ok()
                    .and_then(|raw| raw.parse::<f32>().ok())
                    .filter(|offset| offset.is_finite())
                    .unwrap_or(DEFAULT_GHOST_OFFSET);
                Some(GpuAvatarsMode {
                    readback,
                    live: true,
                    ghost_offset,
                })
            }
            Ok(other) => {
                warn!(
                    "{ENV_FLAG}={other:?} is not a recognised value (expected `1`); \
                     the GPU-avatar pipeline stays off"
                );
                None
            }
            Err(_unset) => None,
        };
        Self { mode }
    }
}

impl Plugin for GpuAvatarsPlugin {
    fn build(&self, app: &mut App) {
        let Some(mode) = self.mode else {
            return;
        };
        warn!(
            "GPU-avatar pipeline (Phase 1a) is ON: every rigged avatar renders twice — \
             the CPU pose in place and a GPU-FK ghost {} m to the side (readback {})",
            mode.ghost_offset,
            if mode.readback { "on" } else { "off" },
        );
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
            .add_plugins(ExtractResourcePlugin::<stage::GpuAvatarStaging>::default());
        if mode.live {
            app.add_systems(
                PostUpdate,
                (
                    // Before propagation, so a fresh ghost's re-extract (and
                    // its baked `current_skin_index`) lands the same frame.
                    stage::churn_gpu_ghost_transforms.before(TransformSystems::Propagate),
                    // After the pose driver, so the feed holds this frame's
                    // final blended poses and the joint globals are posed.
                    stage::stage_gpu_avatars.after(crate::animations::pose_avatar_skeletons),
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
                // frame reads the compute-written ghost palettes. The spike's
                // §2.4 step 3.
                render::run_gpu_avatar_compute
                    .in_set(Core3dSystems::Prepass)
                    .before(early_prepass)
                    .before(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                    .before(shared_shadow_pass::<EARLY_SHADOW_PASS>),
            );
    }
}
