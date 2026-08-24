//! The GPU-avatar pipeline's **main-world half**: the pose feed the CPU pose
//! driver publishes into, the per-avatar slot allocator, the §1.2(a) clip
//! arena + §2.1 sample-job scheduler, and [`stage_gpu_avatars`] — the system
//! that assembles one [`GpuAvatarStaging`] snapshot per frame for the render
//! world to upload.
//!
//! **In-place placement (Phase 4, the only path):** every skinned submesh of a
//! rigged avatar stages **its own** palette slot as a pass-D target, so the
//! rendered avatar is GPU-posed in place. The GPU also samples and blends the
//! keyframes itself (passes A+B): the scheduler here uploads each decoded clip
//! once, dedups this frame's `(clip, phase)` sample jobs (synced far dancers
//! collapse onto one job — the §3.4 animation-data instancing), builds the
//! per-avatar playback row blocks (uploaded only on content change), and
//! stages the sparse adjuster corrections the pose driver published. The pose
//! driver places only the socket subset (see
//! `crate::animations::write_socket_locals`) — the skinning joint entities are
//! gone. The readback's CPU-expected palette comes from the full mirror
//! pipeline ([`mirror_local_pose`] then [`reference_fk`]) over the same
//! uploaded data.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::camera::primitives::Aabb;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::pbr::ExternallyPosedSkin;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use sl_client_bevy::{AssetKey, SkeletalDeformations, VolumeDeformations};

use super::GpuAvatarsMode;
use super::crowd::{CrowdCopy, GpuCrowd};
use super::render::{GpuAvatarBounds, bounds_at};
use super::types::{
    ClipArena, GpuAvatarFrame, GpuClipHeader, GpuCorrection, GpuJointTrack, GpuLocalPose,
    GpuPlayState, GpuRestJoint, GpuSampleJob, JOINT_NONE, MAX_ACTIVE_CLIPS, MAX_GPU_JOINTS,
    PARAMS_FLAG_TPOSE, PLAY_STOPPED_NONE, compose_rest_joints, mirror_local_pose, reference_fk,
};
use crate::animations::{AnimationManager, AnimationPlayback};
use crate::animesh::ControlAvatarState;
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{AvatarBody, AvatarState};
use crate::world_api::PoseSlotKey;

/// The GPU-avatar **skin binding** written at skin-build time
/// (`roadmap/context/gpu-avatars.md` §1.1): which pose slot a skinned submesh
/// belongs to and, per palette slot, the canonical skeleton joint index its
/// vertices weight against — resolved once from the mesh skin's joint names (a
/// worn rig's `joint_names`, or a base part's joint map), so pass D's resolver
/// reads canonical indices straight from the component.
///
/// Present on every avatar base part, worn rigged mesh, and animesh control-
/// avatar submesh; absent on non-avatar scene skins, which the resolver skips.
///
/// Requires [`bevy::pbr::ExternallyPosedSkin`]: because pass D overwrites this
/// submesh's palette in `SkinUniforms` every frame, Bevy's `extract_skins` must
/// **not** also gather its (dummy) joint transforms — the marker excludes it
/// from that iterate-every-skin per-joint floor while leaving its palette
/// allocation / `current_skin_index` intact. Attaching it here guarantees every
/// GPU-posed skin (and only those) carries the marker.
#[derive(Component, Clone)]
#[require(ExternallyPosedSkin)]
pub(crate) struct GpuSkinBinding {
    /// The pose slot whose posed joint worlds pass D reads for this submesh.
    pub(crate) slot: PoseSlotKey,
    /// The canonical skeleton joint index of each palette slot, in the skin's
    /// own joint order (parallel to `SkinnedMesh.joints`).
    pub(crate) canonical: Arc<[u32]>,
}

/// One avatar's latest CPU-published pose data, as published by the pose
/// driver: the root matrix plus the sparse adjuster **corrections** — the GPU
/// samples and blends the keyframes itself (passes A+B).
#[derive(Debug)]
pub(crate) struct FeedEntry {
    /// The avatar root's Bevy-world matrix (SL→Bevy axis change + placement) —
    /// the matrix pass C composes the GPU pose under.
    pub(crate) root: Mat4,
    /// The sparse adjuster corrections (§5.3): the final CPU-computed local
    /// channels of each joint the look-at / reach / IK / physics folds changed
    /// this frame, sorted by joint, as `(canonical joint, replacement
    /// channels)`.
    pub(crate) corrections: Arc<Vec<(u32, GpuLocalPose)>>,
}

/// The channel between the CPU pose drivers and the GPU pipeline
/// (`roadmap/context/gpu-avatars.md`):
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons)
/// publishes each avatar's slot and
/// [`publish_control_avatars`](crate::animesh::publish_control_avatars) each
/// animesh's — the root matrix plus the sparse adjuster corrections (empty for
/// animesh); passes A+B blend the keyframes GPU-side.
#[derive(Debug, Resource, Default)]
pub struct GpuAvatarPoseFeed {
    /// The latest published entry per rigged pose slot.
    entries: HashMap<PoseSlotKey, FeedEntry>,
}

impl GpuAvatarPoseFeed {
    /// Record `slot`'s publish for this frame: the root matrix plus the sparse
    /// adjuster corrections, sorted by joint.
    pub(crate) fn publish_real(
        &mut self,
        slot: PoseSlotKey,
        root: Mat4,
        corrections: Vec<(u32, GpuLocalPose)>,
    ) {
        let _prev = self.entries.insert(
            slot,
            FeedEntry {
                root,
                corrections: Arc::new(corrections),
            },
        );
    }

    /// The latest entry for `slot`, if it has published at least once.
    fn get(&self, slot: PoseSlotKey) -> Option<&FeedEntry> {
        self.entries.get(&slot)
    }

    /// The root matrix and a clone of the sparse corrections of `slot` — the
    /// synthetic-crowd publisher ([`crate::gpu_avatars::crowd`]) reads the
    /// local avatar's freshly published entry to derive each copy's grid-offset
    /// root and share its corrections.
    pub(crate) fn template_entry(
        &self,
        slot: PoseSlotKey,
    ) -> Option<(Mat4, Vec<(u32, GpuLocalPose)>)> {
        self.get(slot)
            .map(|entry| (entry.root, (*entry.corrections).clone()))
    }

    /// Drop entries of slots that are no longer rigged.
    fn retain_rigged(&mut self, rigged: &HashSet<PoseSlotKey>) {
        self.entries.retain(|slot, _entry| rigged.contains(slot));
    }
}

/// One in-place submesh's resolved-skin cache in the [`GpuAvatarRegistry`]:
/// pass D writes the submesh's own palette slot, so only the pool resolution
/// needs remembering.
struct RealSkinRecord {
    /// The pose slot whose posed joints this submesh skins to.
    slot: PoseSlotKey,
    /// A cheap invalidation fingerprint of the source's `SkinnedMesh`: the
    /// inverse-bindpose asset and the joint-list length (a swap re-resolves).
    fingerprint: (AssetId<SkinnedMeshInverseBindposes>, usize),
    /// The resolved skin `(joint_count, joint_map_offset, ibp_offset)`, or
    /// `None` while the inverse-bindpose asset is still loading.
    skin: Option<(u32, u32, u32)>,
}

/// The pool-deduplication key for a resolved mesh skin: the inverse-bindpose
/// asset plus the canonical joint map (identical for every wearer of the same
/// shared mesh asset).
type SkinPoolKey = (AssetId<SkinnedMeshInverseBindposes>, Vec<u32>);

/// The main-world bookkeeping of the GPU-avatar pipeline: the dense avatar
/// slot allocator (free-list), the per-avatar composed rest rows (re-composed
/// only on a `pose_inputs_generation` bump), the shared joint-map /
/// inverse-bindpose pools, and the per-submesh resolved-skin cache.
#[derive(Resource, Default)]
pub(crate) struct GpuAvatarRegistry {
    /// Rigged pose slot → its dense slot index.
    slots: HashMap<PoseSlotKey, u32>,
    /// Freed slots available for reuse.
    free: Vec<u32>,
    /// The slot high-water mark (the slot-indexed buffers' row-block count).
    slot_capacity: u32,
    /// Per-slot composed rest rows and the source generation they were composed
    /// at (an avatar's `pose_inputs_generation`, or an animesh's override
    /// generation).
    rest_rows: HashMap<PoseSlotKey, (u64, Arc<Vec<GpuRestJoint>>)>,
    /// The assembled slot-indexed rest buffer contents (holes default-filled),
    /// rebuilt only when [`Self::rest_dirty`] was set.
    assembled_rest: Arc<Vec<GpuRestJoint>>,
    /// Bumped every time [`Self::assembled_rest`] is rebuilt — the render
    /// side re-uploads only on a bump (the "rest uploads on change" contract).
    rest_generation: u64,
    /// Whether the assembled rest buffer must be rebuilt this frame.
    rest_dirty: bool,
    /// The shared canonical-joint-map pool (§1.2(b)), append-only.
    pool_joint_map: Arc<Vec<u32>>,
    /// The shared inverse-bindpose pool (§1.2(b)), append-only.
    pool_ibps: Arc<Vec<Mat4>>,
    /// Bumped on pool growth — the render side re-uploads only on a bump.
    pool_generation: u64,
    /// Deduplicated resolved skins: pool key → `(count, map_off, ibp_off)`.
    skin_dedup: HashMap<SkinPoolKey, (u32, u32, u32)>,
    /// Submesh entity → its resolved-skin cache.
    real_skins: HashMap<Entity, RealSkinRecord>,
    /// The §1.2(a) clip arena: every decoded `.anim` uploaded once as GPU
    /// keyframe data.
    clips: ClipArena,
    /// The last staged playback rows and their generation, so the render side
    /// re-uploads the playback buffer **only when its content changed**
    /// (§1.3(d): idle loops and dances cost zero upload per frame — the
    /// per-frame phases ride the tiny job list instead).
    playback_rows: Arc<Vec<GpuPlayState>>,
    /// Bumped when [`Self::playback_rows`] content changed.
    playback_generation: u64,
    /// Whether the too-many-joints warning already fired (once per run).
    warned_joint_overflow: bool,
}

impl GpuAvatarRegistry {
    /// The dense slot index a rigged pose slot currently holds, if allocated —
    /// the key the Phase 5 bounds readback is indexed by, so
    /// [`apply_gpu_avatar_bounds`] can look up each skinned submesh's posed
    /// world AABB.
    pub(crate) fn slot_index(&self, key: PoseSlotKey) -> Option<u32> {
        self.slots.get(&key).copied()
    }

    /// Pin a pose slot's dense index directly, for the headless bounds-culling
    /// test (which drives [`apply_gpu_avatar_bounds`] without running the full
    /// stage that would otherwise allocate the slot).
    #[cfg(test)]
    pub(crate) fn set_slot_for_test(&mut self, key: PoseSlotKey, slot: u32) {
        let _prev = self.slots.insert(key, slot);
    }

    /// Allocate a dense avatar slot (reusing a freed one when available).
    /// `None` only on `u32` overflow, which a real scene never reaches.
    fn alloc_slot(&mut self) -> Option<u32> {
        if self.free.is_empty() {
            let slot = self.slot_capacity;
            self.slot_capacity = self.slot_capacity.checked_add(1)?;
            self.rest_dirty = true;
            return Some(slot);
        }
        self.free.pop()
    }

    /// Intern a resolved canonical joint map + inverse bindposes into the
    /// shared pools, deduplicated by `(inverse-bindpose asset, canonical
    /// map)`. The palette length is the shorter of the two, since Bevy zips
    /// `SkinnedMesh.joints` with the bindposes. The canonical map comes
    /// straight from [`GpuSkinBinding`] (written at skin-build time). `None`
    /// only on a `u32` overflow a real scene never reaches.
    fn intern_skin_pools(
        &mut self,
        ibp_id: AssetId<SkinnedMeshInverseBindposes>,
        canonical: &[u32],
        ibp: &[Mat4],
    ) -> Option<(u32, u32, u32)> {
        let count = canonical.len().min(ibp.len());
        let map: Vec<u32> = canonical.iter().copied().take(count).collect();
        let count_u32 = u32::try_from(count).ok()?;
        let key: SkinPoolKey = (ibp_id, map.clone());
        if let Some(resolved) = self.skin_dedup.get(&key).copied() {
            return Some(resolved);
        }
        let (Ok(map_offset), Ok(ibp_offset)) = (
            u32::try_from(self.pool_joint_map.len()),
            u32::try_from(self.pool_ibps.len()),
        ) else {
            return None;
        };
        Arc::make_mut(&mut self.pool_joint_map).extend_from_slice(&map);
        Arc::make_mut(&mut self.pool_ibps).extend(ibp.iter().take(count).copied());
        self.pool_generation = self.pool_generation.wrapping_add(1);
        let resolved = (count_u32, map_offset, ibp_offset);
        let _prev = self.skin_dedup.insert(key, resolved);
        Some(resolved)
    }
}

/// One staged skin instance: everything pass D needs except the palette
/// offset, which the render side re-resolves from `SkinUniforms` every frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StagedSkinInstance {
    /// The mesh entity whose `SkinUniforms` slot pass D overwrites (the
    /// in-place submesh itself).
    pub(crate) target: Entity,
    /// The wearer's avatar slot.
    pub(crate) avatar_slot: u32,
    /// The instance's palette entry count.
    pub(crate) joint_count: u32,
    /// Offset into the shared joint-map pool.
    pub(crate) joint_map_offset: u32,
    /// Offset into the shared inverse-bindpose pool.
    pub(crate) ibp_offset: u32,
}

/// The staged debug-readback request (`SL_VIEWER_GPU_AVATARS_READBACK=1`):
/// which instance to copy back, and the CPU-expected palette computed this
/// frame from [`reference_fk`] over the same uploaded local pose (the
/// golden-tested CPU reference).
#[derive(Clone)]
pub(crate) struct StagedReadback {
    /// The instance whose palette range the readback pass copies.
    pub(crate) target: Entity,
    /// A human-readable name for the verdict log line.
    pub(crate) label: String,
    /// The instance's palette entry count.
    pub(crate) joint_count: u32,
    /// The CPU-expected palette, one entry per skin joint.
    pub(crate) expected: Vec<Mat4>,
}

/// The per-frame snapshot the render world uploads: everything in one plain
/// resource so extraction is a single clone (the two big change-driven blocks
/// ride `Arc`s). Rebuilt whole by [`stage_gpu_avatars`] every frame.
#[derive(Resource, Clone, Default)]
pub(crate) struct GpuAvatarStaging {
    /// The canonical skeleton's joint count `N_J` (0 = nothing staged).
    pub(crate) joint_count: u32,
    /// The slot high-water mark (row blocks in the slot-indexed buffers).
    pub(crate) slot_capacity: u32,
    /// One row per posed avatar this frame (compact).
    pub(crate) frames: Vec<GpuAvatarFrame>,
    /// The slot-indexed local-pose rows (`slot_capacity * joint_count`).
    pub(crate) local_pose: Vec<GpuLocalPose>,
    /// The slot-indexed composed rest rows, re-uploaded on generation bump.
    pub(crate) rest: Arc<Vec<GpuRestJoint>>,
    /// Bumps when [`Self::rest`] content changed.
    pub(crate) rest_generation: u64,
    /// The shared joint-map pool, re-uploaded on generation bump.
    pub(crate) joint_map: Arc<Vec<u32>>,
    /// The shared inverse-bindpose pool, re-uploaded on generation bump.
    pub(crate) ibps: Arc<Vec<Mat4>>,
    /// Bumps when the pools grew.
    pub(crate) pool_generation: u64,
    /// The in-place skin instances pass D writes this frame.
    pub(crate) instances: Vec<StagedSkinInstance>,
    /// The debug readback request, when the sub-flag is on.
    pub(crate) readback: Option<StagedReadback>,
    /// Whether passes A+B run this frame: the GPU samples + blends into the
    /// local-pose buffer, and [`Self::local_pose`] is left empty. `true` in
    /// the live path; `false` only for the hand-staged headless FK tests,
    /// which upload a CPU-blended [`Self::local_pose`] directly.
    pub(crate) blend: bool,
    /// The clip arena's headers, re-uploaded on generation bump.
    pub(crate) clip_headers: Arc<Vec<GpuClipHeader>>,
    /// The clip arena's shared track pool.
    pub(crate) clip_tracks: Arc<Vec<GpuJointTrack>>,
    /// The clip arena's shared joint→track lookup pool.
    pub(crate) track_of_joint: Arc<Vec<u32>>,
    /// The clip arena's shared keyframe time pool.
    pub(crate) key_times: Arc<Vec<f32>>,
    /// The clip arena's shared keyframe value pool.
    pub(crate) key_values: Arc<Vec<Vec4>>,
    /// Bumps when the clip arena grew.
    pub(crate) clip_generation: u64,
    /// This frame's deduplicated sample jobs (pass A).
    pub(crate) jobs: Vec<GpuSampleJob>,
    /// The pose-cache length the jobs cover (elements).
    pub(crate) cache_len: u32,
    /// The frame-indexed playback row blocks (`MAX_ACTIVE` per frame row).
    pub(crate) playback: Arc<Vec<GpuPlayState>>,
    /// Bumps when [`Self::playback`] content changed (the render side
    /// re-uploads only then).
    pub(crate) playback_generation: u64,
    /// The sparse CPU corrections, sorted by (frame index, joint).
    pub(crate) corrections: Vec<GpuCorrection>,
    /// The wall clock pass B's ease weights run on.
    pub(crate) now: f32,
    /// The 15 Hz-quantised procedural idle clock.
    pub(crate) idle_now: f32,
    /// The canonical `mChest` index (or [`JOINT_NONE`]).
    pub(crate) chest_joint: u32,
    /// The canonical `mTorso` index (or [`JOINT_NONE`]).
    pub(crate) torso_joint: u32,
    /// The [`PARAMS_FLAG_TPOSE`] bit for the params uniform.
    pub(crate) param_flags: u32,
}

impl ExtractResource for GpuAvatarStaging {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The main-world queries [`stage_gpu_avatars`] scans: every skinned submesh
/// and, on it, the GPU skin binding written at skin-build time.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct StageQueries<'w, 's> {
    /// The skinned submeshes (avatar bodies, worn rigged meshes) with their
    /// GPU skin binding — the wearer + per-slot canonical joint index that
    /// pass D resolves the palette from. An animesh control-avatar skin or a
    /// non-avatar scene skin carries no binding and is skipped.
    sources: Query<'w, 's, (Entity, &'static SkinnedMesh, &'static GpuSkinBinding)>,
}

/// How far (metres) an avatar may sit from the camera before a **looping**
/// clip's sample phase is quantised to [`PHASE_BUCKET_HZ`] buckets (§2.1):
/// near avatars sample at their exact per-avatar phase so nothing visibly
/// snaps; far synced dancers collapse onto shared sample jobs.
const PHASE_SYNC_DISTANCE_METRES: f32 = 20.0;

/// The far-avatar phase-bucket rate, Hz (§2.1: `round(anim_elapsed × 30)`).
const PHASE_BUCKET_HZ: f32 = 30.0;

/// The Phase 2 scheduling inputs (§2.1 CPU frame prep), bundled so
/// [`stage_gpu_avatars`] stays within the argument-count lint: the wall
/// clock, the reconciled per-avatar playback sets, the decoded-motion cache,
/// and the camera the phase-bucket sync distance is measured from.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct BlendInputs<'w, 's> {
    /// The wall clock (`Time::elapsed_secs`) — the same `now` the playback
    /// reconcile stamped `start` / `stopped_at` with this frame.
    time: Res<'w, Time>,
    /// The reconciled avatar playback sets (`reconcile_playing` ran in
    /// `Update`).
    playback: Option<Res<'w, AnimationPlayback>>,
    /// The animesh control avatars' reconciled per-root motion sets + their
    /// rest overrides (`drive_control_avatars` ran in `Update`).
    control: Res<'w, ControlAvatarState>,
    /// The decoded-motion cache the clip arena uploads from.
    manager: Option<Res<'w, AnimationManager>>,
    /// The viewer camera, for the exact-phase sync distance.
    camera: Query<'w, 's, &'static GlobalTransform, With<crate::world_api::ViewerCamera>>,
}

/// Assemble this frame's [`GpuAvatarStaging`] snapshot: free/allocate avatar
/// slots, re-compose rest rows on a `pose_inputs_generation` bump, build the
/// per-avatar frame rows and roots, resolve each in-place skin into the shared
/// pools, schedule the sample jobs / playback rows / corrections (§2.1), and
/// stage the pass-D instance table plus the optional readback request.
///
/// Runs in `PostUpdate` after
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons), so the
/// feed holds this frame's corrections + roots.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries; the \
              scheduling inputs are already bundled into one `BlendInputs` param and \
              what remains is the avatar state, the asset library, the pipeline's \
              own three resources, the source query, the mode, the bindpose assets, \
              and the debug synthetic-crowd resource"
)]
pub(crate) fn stage_gpu_avatars(
    state: Res<AvatarState>,
    library: Option<Res<AvatarAssetLibrary>>,
    body: Option<Res<AvatarBody>>,
    mode: Res<GpuAvatarsMode>,
    mut feed: ResMut<GpuAvatarPoseFeed>,
    mut registry: ResMut<GpuAvatarRegistry>,
    mut staging: ResMut<GpuAvatarStaging>,
    queries: StageQueries<'_, '_>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
    blend_inputs: BlendInputs<'_, '_>,
    crowd: Res<GpuCrowd>,
) {
    // The startup capability check demoted this device (no compute / storage
    // skinning): stage nothing, so the render half never dispatches.
    if !mode.active {
        *staging = GpuAvatarStaging::default();
        return;
    }
    let Some(library) = library else {
        *staging = GpuAvatarStaging::default();
        return;
    };
    let skeleton = library.skeleton();
    let joint_count = skeleton.len();
    let Ok(joint_count_u32) = u32::try_from(joint_count) else {
        return;
    };
    if joint_count == 0 {
        *staging = GpuAvatarStaging::default();
        return;
    }
    if joint_count_u32 > MAX_GPU_JOINTS {
        if !registry.warned_joint_overflow {
            registry.warned_joint_overflow = true;
            warn!(
                "GPU avatars: the skeleton has {joint_count} joints, more than the \
                 shader's {MAX_GPU_JOINTS}-joint FK arrays — the GPU pose pipeline \
                 stays idle"
            );
        }
        *staging = GpuAvatarStaging::default();
        return;
    }

    // The combined rigged slot set: every rigged avatar plus every animesh
    // control avatar. Both ride the one passes-A–D pipeline.
    let mut rigged: HashSet<PoseSlotKey> = state
        .rigged_agents()
        .into_iter()
        .map(PoseSlotKey::Avatar)
        .collect();
    rigged.extend(
        blend_inputs
            .control
            .animesh_roots()
            .map(PoseSlotKey::Animesh),
    );
    // The synthetic debug crowd (`SL_VIEWER_CROWD`): each spawned copy is its
    // own rigged slot. Empty unless the env selected a crowd.
    rigged.extend(crowd.slots().map(PoseSlotKey::Crowd));
    feed.retain_rigged(&rigged);

    // Free the slots (and cached rest rows) of slots that de-rigged.
    let gone: Vec<(PoseSlotKey, u32)> = registry
        .slots
        .iter()
        .filter(|(slot_key, _slot)| !rigged.contains(*slot_key))
        .map(|(slot_key, slot)| (*slot_key, *slot))
        .collect();
    for (slot_key, slot) in gone {
        let _slot = registry.slots.remove(&slot_key);
        let _rows = registry.rest_rows.remove(&slot_key);
        registry.free.push(slot);
        registry.rest_dirty = true;
    }

    // Allocate slots and (re)compose rest rows for every rigged slot that has
    // published a pose. Composition re-runs only when the slot's source
    // generation moved — an avatar's `pose_inputs_generation` (shape edit,
    // appearance, override add/remove, volume morphs) or an animesh's override
    // generation (a rigged mesh binding / re-binding its joint positions).
    let avatar_generation = state.pose_inputs_generation();
    let no_volumes = VolumeDeformations::default();
    let no_deform = SkeletalDeformations::default();
    let mut active: Vec<(PoseSlotKey, u32)> = Vec::new();
    // Rigged avatars.
    for agent in state.rigged_agents() {
        let slot_key = PoseSlotKey::Avatar(agent);
        if feed.get(slot_key).is_none() {
            continue;
        }
        let Some(deform) = state.deformations(agent) else {
            continue;
        };
        let slot = match registry.slots.get(&slot_key).copied() {
            Some(slot) => slot,
            None => {
                let Some(slot) = registry.alloc_slot() else {
                    continue;
                };
                let _prev = registry.slots.insert(slot_key, slot);
                slot
            }
        };
        let stale = registry
            .rest_rows
            .get(&slot_key)
            .is_none_or(|(at, _rows)| *at != avatar_generation);
        if stale {
            let volumes = state.volume_deformations(agent).unwrap_or(&no_volumes);
            let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
            let rows = Arc::new(compose_rest_joints(skeleton, deform, volumes, &overrides));
            let _prev = registry
                .rest_rows
                .insert(slot_key, (avatar_generation, rows));
            registry.rest_dirty = true;
        }
        active.push((slot_key, slot));
    }
    // Animesh control avatars: no visual-param shape (rest deform / volumes),
    // only the joint position overrides the linkset's own rigged meshes impose.
    for object in blend_inputs.control.animesh_roots() {
        let slot_key = PoseSlotKey::Animesh(object);
        if feed.get(slot_key).is_none() {
            continue;
        }
        let generation = blend_inputs.control.overrides_generation(object);
        let slot = match registry.slots.get(&slot_key).copied() {
            Some(slot) => slot,
            None => {
                let Some(slot) = registry.alloc_slot() else {
                    continue;
                };
                let _prev = registry.slots.insert(slot_key, slot);
                slot
            }
        };
        let stale = registry
            .rest_rows
            .get(&slot_key)
            .is_none_or(|(at, _rows)| *at != generation);
        if stale {
            let overrides = blend_inputs.control.effective_overrides(object);
            let rows = Arc::new(compose_rest_joints(
                skeleton,
                &no_deform,
                &no_volumes,
                &overrides,
            ));
            let _prev = registry.rest_rows.insert(slot_key, (generation, rows));
            registry.rest_dirty = true;
        }
        active.push((slot_key, slot));
    }
    // Synthetic crowd copies (`SL_VIEWER_CROWD`): every copy reuses the local
    // template avatar's shape, so all crowd slots share one composed rest-row
    // block (composed at most once per frame, only when a slot is stale) at the
    // template's `pose_inputs_generation`.
    let crowd_template = crowd.template().and_then(|template| {
        state
            .deformations(template)
            .map(|deform| (template, deform))
    });
    if let Some((template, deform)) = crowd_template {
        let volumes = state.volume_deformations(template).unwrap_or(&no_volumes);
        let overrides = state
            .effective_joint_overrides(template)
            .unwrap_or_default();
        let mut shared_rest: Option<Arc<Vec<GpuRestJoint>>> = None;
        for index in crowd.slots() {
            let slot_key = PoseSlotKey::Crowd(index);
            if feed.get(slot_key).is_none() {
                continue;
            }
            let slot = match registry.slots.get(&slot_key).copied() {
                Some(slot) => slot,
                None => {
                    let Some(slot) = registry.alloc_slot() else {
                        continue;
                    };
                    let _prev = registry.slots.insert(slot_key, slot);
                    slot
                }
            };
            let stale = registry
                .rest_rows
                .get(&slot_key)
                .is_none_or(|(at, _rows)| *at != avatar_generation);
            if stale {
                let rows = shared_rest.get_or_insert_with(|| {
                    Arc::new(compose_rest_joints(skeleton, deform, volumes, &overrides))
                });
                let _prev = registry
                    .rest_rows
                    .insert(slot_key, (avatar_generation, Arc::clone(rows)));
                registry.rest_dirty = true;
            }
            active.push((slot_key, slot));
        }
    }
    // Deterministic frame order (HashSet iteration is not).
    active.sort_by_key(|(_slot_key, slot)| *slot);

    let capacity = usize::try_from(registry.slot_capacity).unwrap_or(0);
    let Some(rows_len) = capacity.checked_mul(joint_count) else {
        return;
    };

    // Reassemble the slot-indexed rest block only when something changed.
    if registry.rest_dirty {
        let mut assembled = vec![GpuRestJoint::default(); rows_len];
        for (slot_key, slot) in &active {
            let Some((_at, rows)) = registry.rest_rows.get(slot_key) else {
                continue;
            };
            let Some(start) = usize::try_from(*slot)
                .ok()
                .and_then(|slot| slot.checked_mul(joint_count))
            else {
                continue;
            };
            for (dst, src) in assembled.iter_mut().skip(start).zip(rows.iter()) {
                *dst = *src;
            }
        }
        registry.assembled_rest = Arc::new(assembled);
        registry.rest_generation = registry.rest_generation.wrapping_add(1);
        registry.rest_dirty = false;
    }

    // The per-frame uploads: one frame row per posed slot. The local pose is
    // GPU-computed by passes A+B, so only the frame rows are built here; the
    // keyframe data rides the clip arena / jobs / playback below. The palettes
    // land in place, so the root affine is the slot root itself.
    let mut frames: Vec<GpuAvatarFrame> = Vec::with_capacity(active.len());
    // The frame-row order of each staged slot (frame index = row index), for
    // the playback blocks / corrections / readback mirror below.
    let mut frame_of_slot: Vec<PoseSlotKey> = Vec::with_capacity(active.len());
    // The live path computes the local pose GPU-side (passes A+B); the buffer
    // is only filled by the hand-staged headless FK tests.
    let local_pose: Vec<GpuLocalPose> = Vec::new();
    for (slot_key, slot) in &active {
        let Some(entry) = feed.get(*slot_key) else {
            continue;
        };
        frames.push(GpuAvatarFrame {
            root: entry.root,
            slot: *slot,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        });
        frame_of_slot.push(*slot_key);
    }

    // The §2.1 CPU frame prep: upload newly decoded clips into the arena,
    // build the deduplicated sample-job list — distinct (clip, phase) across
    // all avatars' active slots, phases bucketed for far avatars — assign
    // cache bases, and build the frame-indexed playback row blocks plus the
    // sparse correction list.
    let t_pose = crate::animations::t_pose_enabled();
    let blend = true;
    let mut jobs: Vec<GpuSampleJob> = Vec::new();
    let mut cache_len = 0_u32;
    let mut corrections: Vec<GpuCorrection> = Vec::new();
    let now = blend_inputs.time.elapsed_secs();
    let idle_now =
        (now * crate::animations::POSE_IDLE_HZ).floor() / crate::animations::POSE_IDLE_HZ;
    let joint_of = |name: &str| -> u32 {
        body.as_deref()
            .and_then(|body| body.joint_index(name))
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(JOINT_NONE)
    };
    let (chest_joint, torso_joint) = (joint_of("mChest"), joint_of("mTorso"));
    if blend {
        let camera_pos = blend_inputs
            .camera
            .iter()
            .next()
            .map(|camera| camera.translation());
        let mut playback_rows =
            vec![GpuPlayState::default(); frames.len().saturating_mul(MAX_ACTIVE_CLIPS)];
        // (clip, phase-bits) → cache base: the §2.1 dedup that IS the
        // animation-data instancing (synced dancers share one job).
        let mut job_lookup: HashMap<(u32, u32), u32> = HashMap::new();
        for (frame_index, slot_key) in frame_of_slot.iter().enumerate() {
            if t_pose {
                break;
            }
            let (Some(playback), Some(manager), Some(body)) = (
                blend_inputs.playback.as_deref(),
                blend_inputs.manager.as_deref(),
                body.as_deref(),
            ) else {
                break;
            };
            // The slot's merged playing set — an avatar's from `AnimationPlayback`,
            // an animesh's from its control avatar — decoded motions only, capped
            // to the MAX_ACTIVE most recently activated, in id order for
            // deterministic cache-base assignment frame over frame.
            // A crowd copy replays the local template avatar's clips; its
            // desync `(phase offset, rate)` is applied to the sampling clock
            // below so the copies are never frame-locked. A real slot has none.
            let (crowd_offset, crowd_rate) = match slot_key {
                PoseSlotKey::Crowd(index) => {
                    crowd.copy(*index).map_or((0.0, 1.0), CrowdCopy::desync)
                }
                _other => (0.0, 1.0),
            };
            let mut states = match slot_key {
                PoseSlotKey::Avatar(agent) => playback.merged_active(*agent),
                PoseSlotKey::Animesh(object) => blend_inputs.control.merged_active(*object),
                PoseSlotKey::Crowd(_index) => match crowd.template() {
                    Some(template) => playback.merged_active(template),
                    None => Vec::new(),
                },
            };
            states.retain(|(id, _play)| manager.motion(AssetKey::from(*id)).is_some());
            if states.len() > MAX_ACTIVE_CLIPS {
                states.sort_by_key(|(_id, play)| core::cmp::Reverse(play.order()));
                states.truncate(MAX_ACTIVE_CLIPS);
            }
            states.sort_by_key(|(id, _play)| *id);
            let far = camera_pos.is_some_and(|camera| {
                feed.get(*slot_key).is_some_and(|entry| {
                    entry.root.w_axis.truncate().distance(camera) > PHASE_SYNC_DISTANCE_METRES
                })
            });
            for (slot_index, (anim_id, play)) in states.iter().enumerate() {
                let Some(motion) = manager.motion(AssetKey::from(*anim_id)) else {
                    continue;
                };
                let Some(clip_id) = registry.clips.ensure_clip(
                    AssetKey::from(*anim_id),
                    motion,
                    joint_count_u32,
                    |name| body.joint_index(name),
                ) else {
                    continue;
                };
                // The motion-elapsed sampling phase: the walk-speed clock skew
                // folds in here, so the GPU play state needs no anim_offset. A
                // crowd copy scales this by its rate and shifts it by its phase
                // offset, so it samples a different point of the same clip than
                // its neighbours (the ease weight still runs off the shared
                // `start`, so the readback mirror stays exact).
                let elapsed = (now - play.start() + play.anim_offset()) * crowd_rate + crowd_offset;
                let bucketed = far && motion.loops;
                let phase = if bucketed {
                    (elapsed * PHASE_BUCKET_HZ).round() / PHASE_BUCKET_HZ
                } else {
                    elapsed
                };
                let cache_base = match job_lookup.entry((clip_id, phase.to_bits())) {
                    std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let base = cache_len;
                        jobs.push(GpuSampleJob {
                            clip_id,
                            cache_base: base,
                            phase,
                            pad0: 0,
                        });
                        cache_len = cache_len.saturating_add(registry.clips.track_count(clip_id));
                        *entry.insert(base)
                    }
                };
                let row = GpuPlayState {
                    clip_id,
                    cache_base,
                    start: play.start(),
                    stopped_at: play.stopped_at().unwrap_or(PLAY_STOPPED_NONE),
                    // Truncated stamp: only the relative order within one
                    // avatar's ≤ 16 slots is ever compared.
                    order: u32::try_from(play.order() & u64::from(u32::MAX)).unwrap_or(0),
                    pad0: 0,
                    pad1: 0,
                    pad2: 0,
                };
                let index = frame_index
                    .checked_mul(MAX_ACTIVE_CLIPS)
                    .and_then(|base| base.checked_add(slot_index));
                if let Some(slot) = index.and_then(|index| playback_rows.get_mut(index)) {
                    *slot = row;
                }
            }
        }
        // The sparse corrections, sorted by (frame index, joint) so pass B
        // binary-searches its entry (frames are already in index order and
        // the per-slot lists sorted by joint at publish; animesh publishes
        // none).
        for (frame_index, slot_key) in frame_of_slot.iter().enumerate() {
            let Some(entry) = feed.get(*slot_key) else {
                continue;
            };
            let Ok(avatar) = u32::try_from(frame_index) else {
                continue;
            };
            for &(joint, value) in entry.corrections.iter() {
                corrections.push(GpuCorrection {
                    avatar,
                    joint,
                    flags: value.flags,
                    pad0: 0,
                    rot: value.rot,
                    pos: value.pos,
                    pad1: 0,
                });
            }
        }
        // Re-upload the playback buffer only when its content changed
        // (§1.3(d)): steady-state loops keep bit-identical rows.
        if *registry.playback_rows != playback_rows {
            registry.playback_rows = Arc::new(playback_rows.clone());
            registry.playback_generation = registry.playback_generation.wrapping_add(1);
        }
    }

    let slot_of: HashMap<PoseSlotKey, u32> = active.iter().copied().collect();

    // Stage the pass-D instance table over each in-place skinned source
    // carrying a `GpuSkinBinding` (its pose slot + per-slot canonical joint
    // indices, written at skin-build time) whose slot holds a dense slot. Prune
    // records of despawned sources, then intern each binding's canonical map
    // into the shared pools (cached per source, a cheap fingerprint
    // invalidating on a SkinnedMesh / binding swap).
    {
        let sources = &queries.sources;
        registry
            .real_skins
            .retain(|source, _record| sources.get(*source).is_ok());
    }
    let mut instances: Vec<StagedSkinInstance> = Vec::new();
    for (source, skin, binding) in &queries.sources {
        let slot_key = binding.slot;
        let Some(slot) = slot_of.get(&slot_key).copied() else {
            continue;
        };
        let fingerprint = (skin.inverse_bindposes.id(), binding.canonical.len());
        let cached = registry
            .real_skins
            .get(&source)
            .filter(|record| record.fingerprint == fingerprint && record.slot == slot_key)
            .and_then(|record| record.skin);
        let resolved = match cached {
            Some(resolved) => Some(resolved),
            None => {
                let resolved = bindposes.get(&skin.inverse_bindposes).and_then(|ibp| {
                    registry.intern_skin_pools(skin.inverse_bindposes.id(), &binding.canonical, ibp)
                });
                let _prev = registry.real_skins.insert(
                    source,
                    RealSkinRecord {
                        slot: slot_key,
                        fingerprint,
                        skin: resolved,
                    },
                );
                resolved
            }
        };
        if let Some((count, map_offset, ibp_offset)) = resolved {
            instances.push(StagedSkinInstance {
                target: source,
                avatar_slot: slot,
                joint_count: count,
                joint_map_offset: map_offset,
                ibp_offset,
            });
        }
    }
    instances.sort_by_key(|instance| instance.target);

    // The debug readback: pick the most-jointed staged instance (the spike's
    // convergence idiom — the mesh body, not an early system part) and stage
    // its CPU-expected palette alongside.
    let param_flags = if t_pose { PARAMS_FLAG_TPOSE } else { 0 };
    let registry = &*registry;
    let readback = if mode.readback {
        instances
            .iter()
            .max_by_key(|instance| instance.joint_count)
            .and_then(|instance| {
                real_readback_expected(
                    instance,
                    registry,
                    &feed,
                    &RealReadbackFrame {
                        frame_of_slot: &frame_of_slot,
                        jobs: &jobs,
                        cache_len,
                        joint_count: joint_count_u32,
                        now,
                        idle: (!t_pose).then_some(idle_now),
                        chest_joint,
                        torso_joint,
                    },
                )
            })
    } else {
        None
    };

    let (clip_headers, clip_tracks, track_of_joint, key_times, key_values, clip_generation) =
        registry.clips.staged();
    *staging = GpuAvatarStaging {
        joint_count: joint_count_u32,
        slot_capacity: registry.slot_capacity,
        frames,
        local_pose,
        rest: Arc::clone(&registry.assembled_rest),
        rest_generation: registry.rest_generation,
        joint_map: Arc::clone(&registry.pool_joint_map),
        ibps: Arc::clone(&registry.pool_ibps),
        pool_generation: registry.pool_generation,
        instances,
        readback,
        blend,
        clip_headers,
        clip_tracks,
        track_of_joint,
        key_times,
        key_values,
        clip_generation,
        jobs,
        cache_len,
        playback: Arc::clone(&registry.playback_rows),
        playback_generation: registry.playback_generation,
        corrections,
        now,
        idle_now,
        chest_joint,
        torso_joint,
        param_flags,
    };
}

/// The staged frame context [`real_readback_expected`] mirrors the GPU passes
/// over: the frame-row order, this frame's sample jobs, and the pass-B frame
/// params — everything needed to re-run passes A+B on the CPU for one avatar.
struct RealReadbackFrame<'a> {
    /// The staged frame rows' pose slots, in frame-index order.
    frame_of_slot: &'a [PoseSlotKey],
    /// This frame's deduplicated sample jobs.
    jobs: &'a [GpuSampleJob],
    /// The pose-cache length the jobs cover.
    cache_len: u32,
    /// The canonical skeleton's joint count.
    joint_count: u32,
    /// The wall clock the ease weights run on.
    now: f32,
    /// The quantised idle clock, or `None` under the T-pose freeze.
    idle: Option<f32>,
    /// The canonical `mChest` index (or [`JOINT_NONE`]).
    chest_joint: u32,
    /// The canonical `mTorso` index (or [`JOINT_NONE`]).
    torso_joint: u32,
}

/// The real placement's readback expectation (Phase 2): the joint globals are
/// frozen there and the local pose is GPU-computed, so the CPU-path truth is
/// the full mirror pipeline — [`mirror_local_pose`] (the golden-tested Rust
/// mirror of passes A+B: sample, priority/ease blend, idle, corrections) over
/// the very clip/playback/job data the pipeline uploaded this frame, then
/// [`reference_fk`] (the pass-C mirror) under the staged root, times the
/// pooled inverse bindposes. A mismatch therefore isolates a GPU-side fault
/// (upload, layout, shader), not a pose-source difference.
fn real_readback_expected(
    instance: &StagedSkinInstance,
    registry: &GpuAvatarRegistry,
    feed: &GpuAvatarPoseFeed,
    frame: &RealReadbackFrame<'_>,
) -> Option<StagedReadback> {
    let record = registry.real_skins.get(&instance.target)?;
    let entry = feed.get(record.slot)?;
    let (_generation, rest_rows) = registry.rest_rows.get(&record.slot)?;
    let frame_index = frame
        .frame_of_slot
        .iter()
        .position(|slot_key| *slot_key == record.slot)?;
    let play_start = frame_index.checked_mul(MAX_ACTIVE_CLIPS)?;
    let plays = registry
        .playback_rows
        .get(play_start..play_start.checked_add(MAX_ACTIVE_CLIPS)?)
        .unwrap_or(&[]);
    let rows = mirror_local_pose(
        registry.clips.slices(),
        plays,
        frame.jobs,
        frame.cache_len,
        frame.joint_count,
        frame.now,
        frame.idle,
        frame.chest_joint,
        frame.torso_joint,
        &entry.corrections,
    );
    let world = reference_fk(rest_rows, &rows, entry.root);
    let count = usize::try_from(instance.joint_count).ok()?;
    let map_start = usize::try_from(instance.joint_map_offset).ok()?;
    let ibp_start = usize::try_from(instance.ibp_offset).ok()?;
    let map = registry
        .pool_joint_map
        .get(map_start..map_start.checked_add(count)?)?;
    let ibps = registry
        .pool_ibps
        .get(ibp_start..ibp_start.checked_add(count)?)?;
    let expected: Vec<Mat4> = map
        .iter()
        .zip(ibps)
        .map(|(&canonical, bindpose)| {
            let canonical = usize::try_from(canonical).unwrap_or(usize::MAX);
            world
                .get(canonical)
                .copied()
                .unwrap_or(Mat4::IDENTITY)
                .mul_mat4(bindpose)
        })
        .collect();
    Some(StagedReadback {
        target: instance.target,
        label: format!(
            "slot={:?} target={} (in-place)",
            record.slot, instance.target
        ),
        joint_count: instance.joint_count,
        expected,
    })
}

// ---------------------------------------------------------------------------
// Phase 5: GPU-computed posed bounds → per-avatar `Aabb` (retires
// `NoFrustumCulling`), so off-screen avatars frustum-cull from the draw.
// ---------------------------------------------------------------------------

/// Metres the posed joint-span AABB is grown by to cover the skinned flesh
/// (hands, feet, hair, close-fitting rigged attachments) that renders past the
/// bones the GPU bound is reduced from. A conservative constant: under-
/// inclusive wrongly culls a visible avatar, while over-inclusive only draws
/// one just off-screen — so the bound is deliberately generous.
const BOUND_FLESH_MARGIN_METRES: f32 = 0.75;

/// Extra metres covering the 1–2 frame bounds-readback latency: a fast-moving
/// avatar (or, for a crowd copy, a fast-moving *template* avatar) is a couple
/// frames ahead of its last read-back world box, so the box is grown by roughly
/// its per-latency travel and is never culled a frame early.
const BOUND_MOTION_MARGIN_METRES: f32 = 0.5;

/// The half-extent (metres) of the generous default AABB an avatar carries
/// until its first bounds readback lands — large enough to always intersect the
/// frustum (never culled), so nothing pops before the real bound arrives. It is
/// the frustum-cull equivalent of the retired `NoFrustumCulling`.
const BOUND_DEFAULT_HALF_EXTENT_METRES: f32 = 1.0e4;

/// The generous default AABB (centred on the entity origin) — see
/// [`BOUND_DEFAULT_HALF_EXTENT_METRES`].
const fn generous_default_aabb() -> Aabb {
    Aabb {
        center: Vec3A::ZERO,
        half_extents: Vec3A::splat(BOUND_DEFAULT_HALF_EXTENT_METRES),
    }
}

/// Convert a read-back world-space AABB into the entity-local AABB the frustum
/// test wants, grown by `margin` metres on every side: expand the world box,
/// transform its 8 corners through the inverse of the entity's
/// `GlobalTransform`, and re-bound them. The cull re-applies that same
/// transform, so this round-trips the world box **regardless** of what the
/// transform is — a real avatar submesh's transform is its body root (equal to
/// the GPU root the bound was posed under), a synthetic crowd copy's is its
/// static grid cell (the base-root translation the copy actually renders at
/// lives only in the GPU root); either way the re-application reproduces the
/// world box. Axis-aligning the inverse-rotated box is over-inclusive, which is
/// safe. Component f32 arithmetic throughout — the restriction lints forbid
/// glam's `Vec3` operator overloads.
fn world_aabb_to_local(min: Vec3, max: Vec3, margin: f32, global: &GlobalTransform) -> Aabb {
    let inv = global.affine().inverse();
    let lo_x = min.x - margin;
    let lo_y = min.y - margin;
    let lo_z = min.z - margin;
    let hi_x = max.x + margin;
    let hi_y = max.y + margin;
    let hi_z = max.z + margin;
    let corners = [
        Vec3::new(lo_x, lo_y, lo_z),
        Vec3::new(hi_x, lo_y, lo_z),
        Vec3::new(lo_x, hi_y, lo_z),
        Vec3::new(lo_x, lo_y, hi_z),
        Vec3::new(hi_x, hi_y, lo_z),
        Vec3::new(hi_x, lo_y, hi_z),
        Vec3::new(lo_x, hi_y, hi_z),
        Vec3::new(hi_x, hi_y, hi_z),
    ];
    let seed = inv.transform_point3(Vec3::new(lo_x, lo_y, lo_z));
    let (lo, hi) = corners.iter().fold((seed, seed), |(lo, hi), &corner| {
        let point = inv.transform_point3(corner);
        (lo.min(point), hi.max(point))
    });
    Aabb::from_min_max(lo, hi)
}

/// Set each GPU-posed skinned submesh's `Aabb` from its pose slot's read-back
/// world bound, so off-screen avatars frustum-cull now that the avatar parts no
/// longer carry `NoFrustumCulling` (Phase 5).
///
/// One per-slot bound is applied to **all** that avatar's submeshes
/// (conservative but correct — they share a skeleton). Until a slot's first
/// bound lands — or on any device where the GPU path is inactive (the readback
/// stays all-zeros) — the submesh keeps a [`generous_default_aabb`] so nothing
/// pops. Ordered before Bevy's `CalculateBounds` so its `Without<Aabb>` pass
/// never installs the meaningless single-dummy-joint bind-pose AABB on these
/// entities.
pub(crate) fn apply_gpu_avatar_bounds(
    bounds: Res<GpuAvatarBounds>,
    registry: Res<GpuAvatarRegistry>,
    mut targets: Query<(Entity, &GpuSkinBinding, &GlobalTransform, Option<&mut Aabb>)>,
    mut commands: Commands,
) {
    let margin = BOUND_FLESH_MARGIN_METRES + BOUND_MOTION_MARGIN_METRES;
    for (entity, binding, global, existing) in &mut targets {
        let world = registry
            .slot_index(binding.slot)
            .and_then(|slot| bounds_at(&bounds.bytes, slot));
        let aabb = match world {
            Some((min, max)) => world_aabb_to_local(min, max, margin, global),
            None => generous_default_aabb(),
        };
        match existing {
            Some(mut existing) => *existing = aabb,
            None => {
                commands.entity(entity).insert(aabb);
            }
        }
    }
}

/// The env flag turning the once-per-second applied-bounds census on
/// (`SL_VIEWER_LOG_AVATAR_BOUNDS=1`): a diagnostic for the Phase 5 frustum
/// culling — it reports, over the avatar/crowd skinned submeshes, how many
/// carry a **real** read-back `Aabb` (posed half-extent ~1–2 m) vs the
/// **generous default** ([`BOUND_DEFAULT_HALF_EXTENT_METRES`], never culled),
/// plus the real half-extent spread. A run where every entity shows the default
/// means the readback never landed / the slot never resolved (culling can't
/// engage); real ~2 m half-extents with no observed cull point instead at the
/// view/visibility side.
const ENV_LOG_BOUNDS: &str = "SL_VIEWER_LOG_AVATAR_BOUNDS";

/// Log a once-per-second census of the applied avatar `Aabb`s when
/// [`ENV_LOG_BOUNDS`] is set — see its docs. Cheap and inert otherwise (the env
/// is read once into a `Local`).
pub(crate) fn log_avatar_bounds(
    bounds: Res<GpuAvatarBounds>,
    registry: Res<GpuAvatarRegistry>,
    targets: Query<(
        &GpuSkinBinding,
        &Aabb,
        &bevy::camera::visibility::ViewVisibility,
    )>,
    time: Res<Time>,
    mut enabled: Local<Option<bool>>,
    mut next_log: Local<f32>,
) {
    let on = *enabled.get_or_insert_with(|| std::env::var(ENV_LOG_BOUNDS).as_deref() == Ok("1"));
    if !on {
        return;
    }
    let now = time.elapsed_secs();
    if now < *next_log {
        return;
    }
    *next_log = now + 1.0;

    let default_threshold = BOUND_DEFAULT_HALF_EXTENT_METRES * 0.5;
    let mut real: u32 = 0;
    let mut default: u32 = 0;
    let mut resolved_slots: u32 = 0;
    let mut visible: u32 = 0;
    let mut real_extents: Vec<f32> = Vec::new();
    for (binding, aabb, view_visibility) in &targets {
        if registry.slot_index(binding.slot).is_some() {
            resolved_slots = resolved_slots.saturating_add(1);
        }
        if view_visibility.get() {
            visible = visible.saturating_add(1);
        }
        let half = aabb.half_extents.max_element();
        if half >= default_threshold {
            default = default.saturating_add(1);
        } else {
            real = real.saturating_add(1);
            real_extents.push(half);
        }
    }
    real_extents.sort_by(f32::total_cmp);
    let smallest = real_extents.first().copied().unwrap_or(0.0);
    let largest = real_extents.last().copied().unwrap_or(0.0);
    let median = real_extents
        .get(real_extents.len() / 2)
        .copied()
        .unwrap_or(0.0);
    info!(
        "avatar bounds census: {visible} ViewVisible / {} total; {real} real / {default} \
         default AABBs; {resolved_slots} have a resolved slot; readback {} ({} bytes); real \
         half-extents min {smallest:.2} m median {median:.2} m max {largest:.2} m (default is \
         {BOUND_DEFAULT_HALF_EXTENT_METRES:e} m)",
        real.saturating_add(default),
        if bounds.bytes.is_empty() {
            "EMPTY (never landed)"
        } else {
            "present"
        },
        bounds.bytes.len(),
    );
}
