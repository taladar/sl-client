//! The GPU-avatar pipeline's **main-world half**: the pose feed the CPU pose
//! driver publishes into, the per-avatar slot allocator, the ghost-entity
//! lifecycle, and [`stage_gpu_avatars`] — the system that assembles one
//! [`GpuAvatarStaging`] snapshot per frame for the render world to upload.
//!
//! **Real placement (Phase 1b, the default):** no ghosts — every skinned
//! submesh of a rigged avatar stages **its own** palette slot as a pass-D
//! target (offset identity), so the rendered avatar is GPU-FK-posed in place.
//! The pose driver freezes the skinning joints and CPU-writes only the socket
//! subset (see `crate::animations::write_socket_globals`); the readback's
//! CPU-expected palette comes from [`reference_fk`] over the same uploaded
//! pose, since the joint globals no longer carry the pose.
//!
//! **The ghost scheme (Phase 1a verification):** the real avatar's skin slots
//! are left untouched — it keeps rendering the normal CPU pose in place. For
//! every rigged skinned submesh a **GPU ghost** is spawned: a duplicate entity
//! sharing the same `Mesh`, `SkinnedMeshInverseBindposes`, per-face material
//! and — deliberately — the same `joints` entity list. The ghost registers its
//! own palette slot in Bevy's `SkinUniforms` (which Bevy keeps filling with
//! the CPU pose), and the compute pipeline overwrites **only the ghost's
//! slot** with the GPU-FK palette, its root affine offset ~2 m to the side.
//! With the flag on, every avatar therefore renders twice: the CPU-posed
//! original in place and the GPU-posed ghost beside it — a correct FK makes
//! the two poses identical, and any divergence is directly visible. If the
//! compute write fails entirely, Bevy's own CPU fill of the ghost's slot makes
//! the ghost render exactly on top of the original (no offset — the offset
//! lives only in the compute-written palette), so "no second avatar appears"
//! is the failure signature.
//!
//! Two companions complete the ghost: the **rigid ghosts** (the eyeballs —
//! rigid base parts carry no `SkinnedMesh`, so [`place_gpu_rigid_ghosts`]
//! CPU-places their duplicates at the ghost offset; without them the GPU copy
//! has no eyes) and the floating **"GPU" label** ([`sync_gpu_avatar_labels`],
//! one world-anchored text billboard per avatar over its ghost), because the
//! world-space offset flips sides with the camera and the two copies are
//! otherwise indistinguishable.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::text::TextBounds;
use sl_client_bevy::{AgentKey, AnimationPose, VolumeDeformations};

use super::GpuAvatarsMode;
use super::types::{
    GpuAvatarFrame, GpuLocalPose, GpuRestJoint, MAX_GPU_JOINTS, compose_rest_joints, pose_rows,
    reference_fk,
};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::{AvatarBody, AvatarBodyPart, AvatarState};
use crate::face_material::FaceMaterial;
use crate::name_tag_billboard::{
    NEUTRAL_MESH_TAG, NameTagPixelSize, NameTagPullRadius, TagText, WorldTextStyle,
    tag_render_layers,
};
use crate::name_tag_content::TagContent;

/// How many frames a fresh ghost's `Transform` is dirtied for after spawning:
/// Bevy bakes a mesh instance's `current_skin_index` into its GPU uniform only
/// when the instance (re)extracts, and a static instance can extract *before*
/// its skin registers in `SkinUniforms` — left stale, it renders nothing (the
/// spike's staleness finding). Dirtying the transform for a generous window
/// after spawn guarantees a re-extract lands after registration (async
/// pipeline warm-up included); after the window the baked index stays valid
/// (Bevy's skin allocator never moves a live skin).
const GHOST_CHURN_FRAMES: u32 = 300;

/// One avatar's latest CPU-blended pose, as published by the pose driver.
pub(crate) struct FeedEntry {
    /// The dense per-joint local pose rows (the §1.3(f) `LocalPose` upload).
    pub(crate) rows: Arc<Vec<GpuLocalPose>>,
    /// The avatar root's Bevy-world matrix (SL→Bevy axis change + placement)
    /// — the same matrix `write_joint_globals` composes the CPU pose under.
    pub(crate) root: Mat4,
}

/// The channel between the CPU pose driver and the GPU pipeline
/// (`roadmap/context/gpu-avatars.md` Phase 1: the CPU still samples, blends
/// and adjusts; the GPU re-runs FK from the blended result):
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons)
/// publishes each avatar's **final** blended local pose — keyframes, idle,
/// look-at, IK, physics, all folded — plus its root matrix, on every frame it
/// evaluates that avatar. A pose-gate-skipped frame publishes nothing and the
/// stored entry stays valid (the CPU path's joint globals are equally stale
/// then, so the two stay in lockstep). Exists only while
/// `SL_VIEWER_GPU_AVATARS` is on.
#[derive(Resource, Default)]
pub(crate) struct GpuAvatarPoseFeed {
    /// The latest published entry per rigged avatar.
    entries: HashMap<AgentKey, FeedEntry>,
}

impl GpuAvatarPoseFeed {
    /// Record `agent`'s final blended pose for this frame, densified to
    /// `joint_count` rows, along with its root matrix.
    pub(crate) fn publish(
        &mut self,
        agent: AgentKey,
        pose: &AnimationPose,
        joint_count: usize,
        root: Mat4,
    ) {
        let _prev = self.entries.insert(
            agent,
            FeedEntry {
                rows: Arc::new(pose_rows(pose, joint_count)),
                root,
            },
        );
    }

    /// The latest entry for `agent`, if it has published at least once.
    fn get(&self, agent: AgentKey) -> Option<&FeedEntry> {
        self.entries.get(&agent)
    }

    /// Drop entries of avatars that are no longer rigged.
    fn retain_rigged(&mut self, rigged: &HashSet<AgentKey>) {
        self.entries.retain(|agent, _entry| rigged.contains(agent));
    }
}

/// Marks a spawned GPU-ghost mesh entity and names the original submesh it
/// duplicates.
#[derive(Component)]
pub(crate) struct GpuAvatarGhost {
    /// The original skinned submesh this ghost mirrors.
    pub(crate) source: Entity,
}

/// Marks a spawned **rigid** ghost — the duplicate of a rigid base part (the
/// eyeballs, `PartBinding::Rigid`), which carries no `SkinnedMesh` and so
/// cannot ride the palette overwrite. It is placed CPU-side instead:
/// [`place_gpu_rigid_ghosts`] writes its `GlobalTransform` as
/// `ghost_offset * source_global` every frame — faithful to the end-state
/// design, where rigid parts follow CPU-owned socket joints (§5.4), and a
/// visual cross-check in itself: CPU-placed eyes only sit correctly in a
/// GPU-posed head if the GPU FK matches the CPU FK.
#[derive(Component)]
pub(crate) struct GpuAvatarRigidGhost {
    /// The original rigid part this ghost mirrors.
    pub(crate) source: Entity,
}

/// One **real** (in-place) submesh's resolved-skin cache in the
/// [`GpuAvatarRegistry`] — the Phase 1b counterpart of [`GhostRecord`]: in
/// real placement pass D writes the submesh's own palette slot, so no ghost
/// entity exists and only the pool resolution needs remembering.
struct RealSkinRecord {
    /// The wearer.
    agent: AgentKey,
    /// A cheap invalidation fingerprint of the source's `SkinnedMesh`: the
    /// inverse-bindpose asset and the joint-list length (a swap re-resolves).
    fingerprint: (AssetId<SkinnedMeshInverseBindposes>, usize),
    /// The resolved skin `(joint_count, joint_map_offset, ibp_offset)`, or
    /// `None` while the inverse-bindpose asset is still loading.
    skin: Option<(u32, u32, u32)>,
}

/// One rigid ghost's bookkeeping in the [`GpuAvatarRegistry`].
struct RigidGhostRecord {
    /// The spawned ghost entity.
    ghost: Entity,
    /// Whether the spawn commands may still be pending (cleared the first
    /// frame the entity is seen), so the retain sweep does not drop a
    /// just-spawned record.
    fresh: bool,
}

/// One ghost's bookkeeping in the [`GpuAvatarRegistry`].
struct GhostRecord {
    /// The spawned ghost entity.
    ghost: Entity,
    /// The wearer.
    agent: AgentKey,
    /// The resolved skin: `(joint_count, joint_map_offset, ibp_offset)` into
    /// the shared pools — `None` until the inverse-bindpose asset has loaded
    /// and every joint resolved to a canonical index.
    skin: Option<(u32, u32, u32)>,
    /// Frames of spawn-window transform churn left (see
    /// [`GHOST_CHURN_FRAMES`]).
    churn_left: u32,
}

impl GhostRecord {
    /// Whether the ghost was spawned so recently that its entity may not have
    /// materialised yet (spawn commands still pending): the churn window has
    /// not ticked, and it ticks on the first frame the entity exists.
    const fn fresh(&self) -> bool {
        self.churn_left == GHOST_CHURN_FRAMES
    }
}

/// The pool-deduplication key for a resolved mesh skin: the inverse-bindpose
/// asset plus the canonical joint map (identical for every wearer of the same
/// shared mesh asset).
type SkinPoolKey = (AssetId<SkinnedMeshInverseBindposes>, Vec<u32>);

/// The main-world bookkeeping of the GPU-avatar pipeline: the dense avatar
/// slot allocator (free-list), the per-avatar composed rest rows (re-composed
/// only on a `pose_inputs_generation` bump), the shared joint-map /
/// inverse-bindpose pools, and the ghost records.
#[derive(Resource, Default)]
pub(crate) struct GpuAvatarRegistry {
    /// Rigged agent → its dense avatar slot.
    slots: HashMap<AgentKey, u32>,
    /// Freed slots available for reuse.
    free: Vec<u32>,
    /// The slot high-water mark (the slot-indexed buffers' row-block count).
    slot_capacity: u32,
    /// Per-agent composed rest rows and the `pose_inputs_generation` they
    /// were composed at.
    rest_rows: HashMap<AgentKey, (u64, Arc<Vec<GpuRestJoint>>)>,
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
    /// Original submesh entity → its ghost bookkeeping (ghost placement).
    ghosts: HashMap<Entity, GhostRecord>,
    /// Original rigid-part entity → its rigid ghost bookkeeping (ghost
    /// placement).
    rigid_ghosts: HashMap<Entity, RigidGhostRecord>,
    /// Real submesh entity → its resolved-skin cache (real placement).
    real_skins: HashMap<Entity, RealSkinRecord>,
    /// Agent → its floating "GPU" label billboard over the ghost.
    labels: HashMap<AgentKey, Entity>,
    /// Whether the too-many-joints warning already fired (once per run).
    warned_joint_overflow: bool,
    /// Sources whose unresolved-joint diagnostic already fired (once per
    /// source, so a persistent anomaly does not spam the log every frame).
    warned_unresolved: HashSet<Entity>,
}

impl GpuAvatarRegistry {
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

    /// Resolve one submesh's skin into the shared joint-map / inverse-bindpose
    /// pools (deduplicated across wearers of the same shared mesh asset):
    /// `(joint_count, joint_map_offset, ibp_offset)`, or `None` while the
    /// inverse-bindpose asset has not loaded (normal during rez — retried).
    /// Never all-or-nothing: an unresolvable joint takes the `root_fallback`
    /// canonical index and is logged at WARN once per `source` (the
    /// missing-eyes class of bug), instead of silently dropping the submesh
    /// from the pass-D table.
    fn resolve_skin_into_pools(
        &mut self,
        source: Entity,
        agent: AgentKey,
        skin: &SkinnedMesh,
        joint_lookup: &HashMap<Entity, (AgentKey, u32)>,
        root_fallback: u32,
        bindposes: &Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<(u32, u32, u32)> {
        let ibp = bindposes.get(&skin.inverse_bindposes)?;
        let (mapped, unresolved) =
            resolve_joint_map(&skin.joints, agent, joint_lookup, root_fallback);
        if !unresolved.is_empty() && self.warned_unresolved.insert(source) {
            let details: Vec<String> = unresolved
                .iter()
                .take(4)
                .map(|entry| match entry.owner {
                    Some(owner) => format!(
                        "#{} {} (belongs to agent {owner}, not the wearer)",
                        entry.position, entry.joint
                    ),
                    None => format!(
                        "#{} {} (not an avatar skeleton joint)",
                        entry.position, entry.joint
                    ),
                })
                .collect();
            warn!(
                "GPU avatars: skin for source {source} (agent {agent}, {} skin joints) \
                 has {} unresolvable joint(s), mapped to the skeleton-root fallback \
                 (canonical {root_fallback}): {}",
                skin.joints.len(),
                unresolved.len(),
                details.join(", "),
            );
        }
        // Bevy zips joints with bindposes, so the palette length is the
        // shorter of the two.
        let count = mapped.len().min(ibp.len());
        let map: Vec<u32> = mapped.into_iter().take(count).collect();
        let count_u32 = u32::try_from(count).ok()?;
        let key: SkinPoolKey = (skin.inverse_bindposes.id(), map.clone());
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

/// Why one of a submesh's skin joints failed to resolve to a canonical index:
/// the entity is not an avatar joint at all, or it belongs to a different
/// avatar than the submesh's wearer. Reported by [`resolve_joint_map`] so the
/// diagnostic can name the exact joint and reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnresolvedJoint {
    /// The joint's position in the submesh's `SkinnedMesh::joints` list.
    pub(crate) position: usize,
    /// The unresolvable joint entity.
    pub(crate) joint: Entity,
    /// The avatar the entity actually belongs to, when it is *some* avatar's
    /// joint but not the wearer's; `None` when it is no avatar joint at all.
    pub(crate) owner: Option<AgentKey>,
}

/// Resolve a submesh's skin-joint entities to canonical skeleton indices for
/// `agent`. **Never all-or-nothing**: a joint that fails to resolve maps to
/// the `fallback` canonical index (the synthetic root — its palette entry
/// pins the affected vertices to the avatar root instead of garbage) and is
/// reported, so one bad joint degrades that joint's vertices instead of
/// silently dropping the whole submesh's ghost from the pass-D table (the
/// missing-eyes class of bug).
pub(crate) fn resolve_joint_map(
    joints: &[Entity],
    agent: AgentKey,
    lookup: &HashMap<Entity, (AgentKey, u32)>,
    fallback: u32,
) -> (Vec<u32>, Vec<UnresolvedJoint>) {
    let mut unresolved = Vec::new();
    let map = joints
        .iter()
        .enumerate()
        .map(|(position, &joint)| match lookup.get(&joint) {
            Some((owner, canonical)) if *owner == agent => *canonical,
            other => {
                unresolved.push(UnresolvedJoint {
                    position,
                    joint,
                    owner: other.map(|(owner, _canonical)| *owner),
                });
                fallback
            }
        })
        .collect();
    (map, unresolved)
}

/// One staged skin instance: everything pass D needs except the palette
/// offset, which the render side re-resolves from `SkinUniforms` every frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StagedSkinInstance {
    /// The mesh entity whose `SkinUniforms` slot pass D overwrites — the
    /// spawned ghost in ghost placement, the real submesh itself in real
    /// placement.
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
/// frame — in ghost placement from the original's posed joint
/// `GlobalTransform`s (the true CPU path) composed with the ghost offset, in
/// real placement from [`reference_fk`] over the same uploaded local pose
/// (the golden-tested CPU reference; the joint globals are frozen there).
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
    /// The ghost skin instances pass D writes this frame.
    pub(crate) instances: Vec<StagedSkinInstance>,
    /// The debug readback request, when the sub-flag is on.
    pub(crate) readback: Option<StagedReadback>,
}

impl ExtractResource for GpuAvatarStaging {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The component tuple of one rigid base-part candidate (see
/// [`GhostQueries::rigid_sources`]).
type RigidSourceData<'a> = (
    Entity,
    &'a AvatarBodyPart,
    &'a Mesh3d,
    &'a MeshMaterial3d<FaceMaterial>,
);

/// The rigid-source scan's filter: a rigid part carries no `SkinnedMesh`, and
/// spawned ghosts of either kind are never sources themselves.
type RigidSourceFilter = (
    Without<SkinnedMesh>,
    Without<GpuAvatarGhost>,
    Without<GpuAvatarRigidGhost>,
);

/// The main-world queries [`stage_gpu_avatars`] scans: every real (non-ghost)
/// skinned mesh with its shared handles, and the ghosts' current components
/// for the cheap handle sync.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct GhostQueries<'w, 's> {
    /// The real skinned submeshes (avatar bodies, worn rigged meshes, system
    /// parts) — everything a ghost can be spawned for.
    sources: Query<
        'w,
        's,
        (
            Entity,
            &'static SkinnedMesh,
            &'static Mesh3d,
            &'static MeshMaterial3d<FaceMaterial>,
        ),
        Without<GpuAvatarGhost>,
    >,
    /// The live ghosts' identity (which source each mirrors) and current
    /// handles, for the source→ghost handle sync.
    ghosts: Query<
        'w,
        's,
        (
            Entity,
            &'static GpuAvatarGhost,
            &'static SkinnedMesh,
            &'static Mesh3d,
            &'static MeshMaterial3d<FaceMaterial>,
        ),
    >,
    /// Joint `GlobalTransform`s, read for the readback's CPU-expected palette
    /// (posed this frame — this system runs after the pose driver).
    globals: Query<'w, 's, &'static GlobalTransform>,
    /// The rigid base parts (the eyeballs, `PartBinding::Rigid`) — they carry
    /// no `SkinnedMesh`, so without a dedicated rigid ghost the GPU copy is
    /// missing its eyes.
    rigid_sources: Query<'w, 's, RigidSourceData<'static>, RigidSourceFilter>,
    /// The live rigid ghosts' identity and handles, for sync + liveness.
    rigid_ghosts: Query<
        'w,
        's,
        (
            Entity,
            &'static GpuAvatarRigidGhost,
            &'static Mesh3d,
            &'static MeshMaterial3d<FaceMaterial>,
        ),
    >,
}

/// Assemble this frame's [`GpuAvatarStaging`] snapshot (and keep the ghost
/// population in sync): free/allocate avatar slots, re-compose rest rows on a
/// `pose_inputs_generation` bump, densify the published poses and roots,
/// spawn/sync/despawn ghosts, resolve their skins into the shared pools, and
/// stage the pass-D instance table plus the optional readback request.
///
/// Runs in `PostUpdate` after
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons), so the
/// feed holds this frame's final poses and the joint globals are posed.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries; the \
              entity queries are already bundled into one `GhostQueries` param and \
              what remains is the avatar state, the asset library, the pipeline's \
              own three resources, the mode, and the bindpose assets"
)]
pub(crate) fn stage_gpu_avatars(
    mut commands: Commands,
    state: Res<AvatarState>,
    library: Option<Res<AvatarAssetLibrary>>,
    body: Option<Res<AvatarBody>>,
    mode: Res<GpuAvatarsMode>,
    mut feed: ResMut<GpuAvatarPoseFeed>,
    mut registry: ResMut<GpuAvatarRegistry>,
    mut staging: ResMut<GpuAvatarStaging>,
    queries: GhostQueries<'_, '_>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
) {
    // The startup capability check demoted this device to the CPU path:
    // stage nothing, so the render half never dispatches.
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

    let rigged: HashSet<AgentKey> = state.rigged_agents().into_iter().collect();
    feed.retain_rigged(&rigged);

    // Free the slots (and cached rest rows) of avatars that de-rigged.
    let gone: Vec<(AgentKey, u32)> = registry
        .slots
        .iter()
        .filter(|(agent, _slot)| !rigged.contains(*agent))
        .map(|(agent, slot)| (*agent, *slot))
        .collect();
    for (agent, slot) in gone {
        let _slot = registry.slots.remove(&agent);
        let _rows = registry.rest_rows.remove(&agent);
        registry.free.push(slot);
        registry.rest_dirty = true;
    }

    // Allocate slots and (re)compose rest rows for every rigged avatar that
    // has published a pose. Composition re-runs only when the shared
    // pose-inputs generation moved — the same invalidation the CPU pose gate
    // keys on (shape edit, appearance, override add/remove, volume morphs).
    let generation = state.pose_inputs_generation();
    let no_volumes = VolumeDeformations::default();
    let mut active: Vec<(AgentKey, u32)> = Vec::new();
    for &agent in &rigged {
        if feed.get(agent).is_none() {
            continue;
        }
        let Some(deform) = state.deformations(agent) else {
            continue;
        };
        let slot = match registry.slots.get(&agent).copied() {
            Some(slot) => slot,
            None => {
                let Some(slot) = registry.alloc_slot() else {
                    continue;
                };
                let _prev = registry.slots.insert(agent, slot);
                slot
            }
        };
        let stale = registry
            .rest_rows
            .get(&agent)
            .is_none_or(|(at, _rows)| *at != generation);
        if stale {
            let volumes = state.volume_deformations(agent).unwrap_or(&no_volumes);
            let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
            let rows = Arc::new(compose_rest_joints(skeleton, deform, volumes, &overrides));
            let _prev = registry.rest_rows.insert(agent, (generation, rows));
            registry.rest_dirty = true;
        }
        active.push((agent, slot));
    }
    // Deterministic frame order (HashSet iteration is not).
    active.sort_by_key(|(_agent, slot)| *slot);

    let capacity = usize::try_from(registry.slot_capacity).unwrap_or(0);
    let Some(rows_len) = capacity.checked_mul(joint_count) else {
        return;
    };

    // Reassemble the slot-indexed rest block only when something changed.
    if registry.rest_dirty {
        let mut assembled = vec![GpuRestJoint::default(); rows_len];
        for (agent, slot) in &active {
            let Some((_at, rows)) = registry.rest_rows.get(agent) else {
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

    // The per-frame uploads: one frame row per avatar and the dense
    // local-pose block. In ghost placement the root affine carries the
    // side-by-side display offset; in real placement the palettes land in
    // place, so the offset is identity.
    let ghost_mode = mode.placement == super::GpuAvatarPlacement::Ghost;
    let offset_mat = if ghost_mode {
        Mat4::from_translation(Vec3::new(mode.ghost_offset, 0.0, 0.0))
    } else {
        Mat4::IDENTITY
    };
    let mut frames: Vec<GpuAvatarFrame> = Vec::with_capacity(active.len());
    let mut local_pose = vec![GpuLocalPose::default(); rows_len];
    for (agent, slot) in &active {
        let Some(entry) = feed.get(*agent) else {
            continue;
        };
        frames.push(GpuAvatarFrame {
            root: offset_mat.mul_mat4(&entry.root),
            slot: *slot,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        });
        let Some(start) = usize::try_from(*slot)
            .ok()
            .and_then(|slot| slot.checked_mul(joint_count))
        else {
            continue;
        };
        for (dst, src) in local_pose.iter_mut().skip(start).zip(entry.rows.iter()) {
            *dst = *src;
        }
    }

    // The joint-entity → (wearer, canonical index) lookup for ghost spawning
    // and skin resolution. Rebuilt per frame — O(avatars × joints) hash
    // inserts, acceptable at Phase 1a scale (a 1b optimisation is to key it
    // on the rigged-set generation).
    let mut joint_lookup: HashMap<Entity, (AgentKey, u32)> = HashMap::new();
    for (agent, _slot) in &active {
        if let Some(joints) = state.joint_entities_of(*agent) {
            for (index, &joint) in joints.iter().enumerate() {
                if let Ok(index) = u32::try_from(index) {
                    let _prev = joint_lookup.insert(joint, (*agent, index));
                }
            }
        }
    }
    let slot_of: HashMap<AgentKey, u32> = active.iter().copied().collect();

    if ghost_mode {
        maintain_ghosts(
            &mut commands,
            &mut registry,
            &queries,
            body.as_deref(),
            &joint_lookup,
            &slot_of,
        );
    }

    // The canonical index an unresolvable skin joint falls back to: the
    // synthetic root (identity local transform), pinning the affected
    // vertices to the avatar root instead of dropping the whole submesh.
    let root_fallback = skeleton
        .find("mRoot")
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(0);

    // Stage the pass-D instance table: in ghost placement over the ghost
    // entities' palette slots, in real placement over the sources' own.
    let mut instances: Vec<StagedSkinInstance> = Vec::new();
    if ghost_mode {
        // Resolve pending ghost skins through the shared pool resolver
        // (collected first so the resolver can borrow the registry whole).
        let pending: Vec<(Entity, AgentKey)> = registry
            .ghosts
            .iter()
            .filter(|(_source, record)| record.skin.is_none())
            .map(|(source, record)| (*source, record.agent))
            .collect();
        for (source, agent) in pending {
            let Ok((_entity, src_skin, _mesh, _material)) = queries.sources.get(source) else {
                continue;
            };
            let resolved = registry.resolve_skin_into_pools(
                source,
                agent,
                src_skin,
                &joint_lookup,
                root_fallback,
                &bindposes,
            );
            if resolved.is_some()
                && let Some(record) = registry.ghosts.get_mut(&source)
            {
                record.skin = resolved;
            }
        }
        for record in registry.ghosts.values() {
            let Some(slot) = slot_of.get(&record.agent).copied() else {
                continue;
            };
            if let Some((count, map_offset, ibp_offset)) = record.skin {
                instances.push(StagedSkinInstance {
                    target: record.ghost,
                    avatar_slot: slot,
                    joint_count: count,
                    joint_map_offset: map_offset,
                    ibp_offset,
                });
            }
        }
    } else {
        // Real placement: every skinned source whose joints belong to a
        // slotted avatar stages in place. Prune records of despawned sources,
        // then resolve through the same pool resolver (cached per source, a
        // cheap fingerprint invalidating on a SkinnedMesh swap).
        {
            let sources = &queries.sources;
            registry
                .real_skins
                .retain(|source, _record| sources.get(*source).is_ok());
        }
        for (source, skin, _mesh, _material) in &queries.sources {
            let Some(&(agent, _index)) = skin
                .joints
                .first()
                .and_then(|joint| joint_lookup.get(joint))
            else {
                // Not an avatar skeleton skin (an animesh control skeleton) —
                // stays on the CPU path, out of Phase 1 scope.
                continue;
            };
            let Some(slot) = slot_of.get(&agent).copied() else {
                continue;
            };
            let fingerprint = (skin.inverse_bindposes.id(), skin.joints.len());
            let cached = registry
                .real_skins
                .get(&source)
                .filter(|record| record.fingerprint == fingerprint && record.agent == agent)
                .and_then(|record| record.skin);
            let resolved = match cached {
                Some(resolved) => Some(resolved),
                None => {
                    let resolved = registry.resolve_skin_into_pools(
                        source,
                        agent,
                        skin,
                        &joint_lookup,
                        root_fallback,
                        &bindposes,
                    );
                    let _prev = registry.real_skins.insert(
                        source,
                        RealSkinRecord {
                            agent,
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
    }
    instances.sort_by_key(|instance| instance.target);

    // The debug readback: pick the most-jointed staged instance (the spike's
    // convergence idiom — the mesh body, not an early system part) and stage
    // its CPU-expected palette alongside.
    let registry = &*registry;
    let readback = if mode.readback {
        instances
            .iter()
            .max_by_key(|instance| instance.joint_count)
            .and_then(|instance| {
                if ghost_mode {
                    ghost_readback_expected(instance, registry, &queries, &bindposes, offset_mat)
                } else {
                    real_readback_expected(instance, registry, &feed)
                }
            })
    } else {
        None
    };

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
    };
}

/// The ghost placement's entity maintenance (Phase 1a harness): sync each
/// live ghost's shared handles to its source, retire ghosts whose source lost
/// its components, spawn ghosts for new sources, and mirror the rigid base
/// parts (the eyeballs). No-op in real placement, which writes the sources'
/// own palette slots and needs no duplicates.
fn maintain_ghosts(
    commands: &mut Commands,
    registry: &mut GpuAvatarRegistry,
    queries: &GhostQueries<'_, '_>,
    body: Option<&AvatarBody>,
    joint_lookup: &HashMap<Entity, (AgentKey, u32)>,
    slot_of: &HashMap<AgentKey, u32>,
) {
    // Ghost housekeeping. Sync the shared handles each live ghost mirrors —
    // the source swaps them on a LOD change or a bake-material replace — by
    // walking the ghosts and following their [`GpuAvatarGhost::source`] link.
    for (ghost_entity, ghost, ghost_skin, ghost_mesh, ghost_material) in &queries.ghosts {
        let Ok((_entity, src_skin, src_mesh, src_material)) = queries.sources.get(ghost.source)
        else {
            // The source lost its skinned-mesh components while alive (a
            // despawned source tears the ghost down through `ChildOf`
            // already): retire the ghost and its record.
            let _record = registry.ghosts.remove(&ghost.source);
            if let Ok(mut entity) = commands.get_entity(ghost_entity) {
                entity.despawn();
            }
            continue;
        };
        if ghost_mesh.0 != src_mesh.0 {
            commands
                .entity(ghost_entity)
                .insert(Mesh3d(src_mesh.0.clone()));
        }
        if ghost_material.0 != src_material.0 {
            commands
                .entity(ghost_entity)
                .insert(MeshMaterial3d(src_material.0.clone()));
        }
        if ghost_skin.inverse_bindposes != src_skin.inverse_bindposes
            || ghost_skin.joints != src_skin.joints
        {
            commands.entity(ghost_entity).insert(SkinnedMesh {
                inverse_bindposes: src_skin.inverse_bindposes.clone(),
                joints: src_skin.joints.clone(),
            });
            if let Some(record) = registry.ghosts.get_mut(&ghost.source) {
                record.skin = None;
            }
        }
    }
    // Drop records whose ghost (or source) despawned underneath them, so a
    // re-rezzed source gets a fresh ghost.
    {
        let ghosts_query = &queries.ghosts;
        registry
            .ghosts
            .retain(|_source, record| ghosts_query.get(record.ghost).is_ok() || record.fresh());
    }
    for (source, skin, mesh, material) in &queries.sources {
        if registry.ghosts.contains_key(&source) {
            continue;
        }
        // Only avatar-skeleton skins get a ghost: the first joint must belong
        // to a rigged avatar with a slot (an animesh control skeleton, whose
        // joints are not avatar joints, is out of Phase 1 scope).
        let Some(&(agent, _index)) = skin
            .joints
            .first()
            .and_then(|joint| joint_lookup.get(joint))
        else {
            continue;
        };
        if !slot_of.contains_key(&agent) {
            continue;
        }
        let ghost = commands
            .spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(material.0.clone()),
                SkinnedMesh {
                    inverse_bindposes: skin.inverse_bindposes.clone(),
                    joints: skin.joints.clone(),
                },
                // Skinned meshes render wherever their palette puts them, not
                // where their bounds sit — never frustum-cull one (matches the
                // originals).
                NoFrustumCulling,
                Transform::default(),
                Visibility::Inherited,
                GpuAvatarGhost { source },
                // Lifecycle + visibility + render layers ride the original.
                ChildOf(source),
            ))
            .id();
        let _prev = registry.ghosts.insert(
            source,
            GhostRecord {
                ghost,
                agent,
                skin: None,
                churn_left: GHOST_CHURN_FRAMES,
            },
        );
    }

    // Rigid-ghost housekeeping (the eyeballs): a rigid base part carries no
    // `SkinnedMesh` — its pose comes from the driver writing its
    // `GlobalTransform` from the eye joint — so it can never ride the palette
    // overwrite and needs a CPU-placed duplicate instead
    // ([`place_gpu_rigid_ghosts`]). Without these the GPU copy has no eyes.
    for (ghost_entity, ghost, ghost_mesh, ghost_material) in &queries.rigid_ghosts {
        let Ok((_entity, _part, src_mesh, src_material)) = queries.rigid_sources.get(ghost.source)
        else {
            let _record = registry.rigid_ghosts.remove(&ghost.source);
            if let Ok(mut entity) = commands.get_entity(ghost_entity) {
                entity.despawn();
            }
            continue;
        };
        if let Some(record) = registry.rigid_ghosts.get_mut(&ghost.source) {
            record.fresh = false;
        }
        if ghost_mesh.0 != src_mesh.0 {
            commands
                .entity(ghost_entity)
                .insert(Mesh3d(src_mesh.0.clone()));
        }
        if ghost_material.0 != src_material.0 {
            commands
                .entity(ghost_entity)
                .insert(MeshMaterial3d(src_material.0.clone()));
        }
    }
    {
        let rigid_query = &queries.rigid_ghosts;
        registry
            .rigid_ghosts
            .retain(|_source, record| rigid_query.get(record.ghost).is_ok() || record.fresh);
    }
    if let Some(body) = body {
        for (source, part, mesh, material) in &queries.rigid_sources {
            if registry.rigid_ghosts.contains_key(&source) {
                continue;
            }
            // Only genuinely rigid-bound parts (skinned parts carry a
            // `SkinnedMesh` and are excluded by the query filter already, but
            // the binding check keeps this honest against future part kinds).
            if body.rigid_joint_index(part.part()).is_none() {
                continue;
            }
            if !slot_of.contains_key(&part.agent()) {
                continue;
            }
            let ghost = commands
                .spawn((
                    Mesh3d(mesh.0.clone()),
                    MeshMaterial3d(material.0.clone()),
                    Transform::default(),
                    Visibility::Inherited,
                    NoFrustumCulling,
                    GpuAvatarRigidGhost { source },
                    // Lifecycle + visibility ride the original part.
                    ChildOf(source),
                ))
                .id();
            info!(
                "GPU avatars: spawned rigid ghost {ghost} for rigid part {source} \
                 (agent {}) — CPU-placed at the ghost offset",
                part.agent()
            );
            let _prev = registry
                .rigid_ghosts
                .insert(source, RigidGhostRecord { ghost, fresh: true });
        }
    }
}

/// The ghost placement's readback expectation: the CPU-path palette computed
/// from the ORIGINAL's posed joint `GlobalTransform`s — the exact matrices
/// `extract_skins` uploads (`joint_global * ibp`) — composed with the ghost
/// display offset. The strongest cross-check: it compares the GPU pipeline
/// end-to-end against the live CPU pose path.
fn ghost_readback_expected(
    instance: &StagedSkinInstance,
    registry: &GpuAvatarRegistry,
    queries: &GhostQueries<'_, '_>,
    bindposes: &Assets<SkinnedMeshInverseBindposes>,
    offset_mat: Mat4,
) -> Option<StagedReadback> {
    let (source, record) = registry
        .ghosts
        .iter()
        .find(|(_source, record)| record.ghost == instance.target)?;
    let (_entity, src_skin, _mesh, _material) = queries.sources.get(*source).ok()?;
    let ibp = bindposes.get(&src_skin.inverse_bindposes)?;
    let count = usize::try_from(instance.joint_count).ok()?;
    let expected: Vec<Mat4> = src_skin
        .joints
        .iter()
        .zip(ibp.iter())
        .take(count)
        .map(|(&joint, bindpose)| {
            let global = queries
                .globals
                .get(joint)
                .map_or(Mat4::IDENTITY, GlobalTransform::to_matrix);
            offset_mat.mul_mat4(&global).mul_mat4(bindpose)
        })
        .collect();
    Some(StagedReadback {
        target: instance.target,
        label: format!(
            "agent={} ghost={} source={source}",
            record.agent, record.ghost
        ),
        joint_count: instance.joint_count,
        expected,
    })
}

/// The real placement's readback expectation: the joint globals are frozen
/// there, so the CPU-path truth is [`reference_fk`] — the golden-tested Rust
/// mirror of the WGSL recurrence — over the very rest rows, local-pose rows
/// and root the pipeline uploaded this frame, times the pooled inverse
/// bindposes. A mismatch therefore isolates a GPU-side fault (upload, layout,
/// shader), not a pose-source difference.
fn real_readback_expected(
    instance: &StagedSkinInstance,
    registry: &GpuAvatarRegistry,
    feed: &GpuAvatarPoseFeed,
) -> Option<StagedReadback> {
    let record = registry.real_skins.get(&instance.target)?;
    let entry = feed.get(record.agent)?;
    let (_generation, rest_rows) = registry.rest_rows.get(&record.agent)?;
    let world = reference_fk(rest_rows, &entry.rows, entry.root);
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
            "agent={} target={} (in-place)",
            record.agent, instance.target
        ),
        joint_count: instance.joint_count,
        expected,
    })
}

/// Keep freshly spawned ghosts' `Transform`s dirty for a spawn window (see
/// [`GHOST_CHURN_FRAMES`]): a same-value `set_changed`, so nothing moves, but
/// the mesh instance re-extracts and re-bakes its `current_skin_index` after
/// its skin registers in `SkinUniforms`. Runs before transform propagation so
/// the re-extract lands the same frame.
pub(crate) fn churn_gpu_ghost_transforms(
    mut registry: ResMut<GpuAvatarRegistry>,
    mut transforms: Query<&mut Transform, With<GpuAvatarGhost>>,
) {
    for record in registry.ghosts.values_mut() {
        if record.churn_left == 0 {
            continue;
        }
        if let Ok(mut transform) = transforms.get_mut(record.ghost) {
            transform.set_changed();
            record.churn_left = record.churn_left.saturating_sub(1);
        }
    }
}

/// Place every **rigid** ghost (the eyeballs): its `GlobalTransform` becomes
/// `ghost_offset * source_global`, written directly like the pose driver's own
/// rigid-part placement — the source global was just written by
/// [`pose_avatar_skeletons`](crate::animations::pose_avatar_skeletons) from the
/// posed eye joint, so this runs after it (via
/// [`stage_gpu_avatars`]' ordering) and lands before render extraction.
///
/// CPU-placed by design: rigid parts follow joints CPU-side in the end-state
/// architecture too (§5.4 socket joints). Visually this is a cross-check —
/// CPU-placed eyes only sit correctly in the GPU-posed ghost head if the GPU
/// FK matches the CPU FK.
pub(crate) fn place_gpu_rigid_ghosts(
    mode: Res<GpuAvatarsMode>,
    registry: Res<GpuAvatarRegistry>,
    mut globals: Query<&mut GlobalTransform>,
) {
    let offset = Mat4::from_translation(Vec3::new(mode.ghost_offset, 0.0, 0.0));
    for (&source, record) in &registry.rigid_ghosts {
        // Copy the source matrix out first so the mutable ghost write below
        // does not alias the read (the `write_joint_globals` idiom).
        let source_matrix = {
            let Ok(source_global) = globals.get(source) else {
                continue;
            };
            source_global.to_matrix()
        };
        if let Ok(mut ghost_global) = globals.get_mut(record.ghost) {
            *ghost_global =
                GlobalTransform::from(Affine3A::from_mat4(offset.mul_mat4(&source_matrix)));
        }
    }
}

/// How far above the avatar root the floating "GPU" label hovers, metres in
/// Bevy world up — clear of the head so it reads like a name tag.
const GPU_LABEL_LIFT_METRES: f32 = 2.3;

/// Marks one avatar's floating **"GPU" label** billboard — the world-anchored
/// text hovering over the avatar's GPU ghost, so the two copies are
/// unambiguously tellable apart (the world-space +X offset flips sides with
/// the camera).
///
/// The label is **not** anchored to any ghost entity's transform: a ghost is
/// a skinned mesh that renders wherever its compute-written palette puts it
/// (avatar root + offset), while its entity `Transform` sits under the CPU
/// avatar — anchoring there would put "GPU" over the CPU copy. The label is
/// placed at `avatar_root_world + ghost_offset + lift` instead, from the same
/// per-frame feed root the palettes are computed under.
#[derive(Component)]
pub(crate) struct GpuAvatarLabel {
    /// The avatar this label's ghost belongs to.
    pub(crate) agent: AgentKey,
}

/// The world-anchored-text components of one "GPU" label, mirroring the
/// hover-text render bundle (`crate::hover_text`): the shared name-tag
/// billboard pipeline lays it out, meshes it and billboards it; no anti-overlap
/// solve runs over it (neutral mesh tag), and it starts hidden so it never
/// flashes at the origin before its first placement.
fn gpu_label_bundle(agent: AgentKey) -> impl Bundle {
    (
        TagText::default(),
        TagContent::plain_name("GPU"),
        TextLayout {
            justify: Justify::Center,
            linebreak: LineBreak::WordOrCharacter,
        },
        TextBounds {
            width: Some(200.0),
            height: None,
        },
        WorldTextStyle::HOVER_TEXT,
        NameTagPullRadius(0.0),
        NameTagPixelSize::default(),
        bevy::mesh::MeshTag(NEUTRAL_MESH_TAG),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        tag_render_layers(),
        GpuAvatarLabel { agent },
    )
}

/// Keep one floating "GPU" label per posed avatar, hovering over its ghost:
/// spawn on first sight, place at `feed root + ghost offset + lift` every
/// frame (the same root affine the ghost's palettes are composed under, so the
/// label tracks the ghost exactly), reveal once placed, and despawn with the
/// avatar. Runs with [`stage_gpu_avatars`]' ordering (after the pose driver).
pub(crate) fn sync_gpu_avatar_labels(
    mut commands: Commands,
    mode: Res<GpuAvatarsMode>,
    feed: Res<GpuAvatarPoseFeed>,
    mut registry: ResMut<GpuAvatarRegistry>,
    mut labels: Query<(Entity, &GpuAvatarLabel, &mut Transform, &mut Visibility)>,
) {
    // Place every live label from its avatar's feed root; reap a label whose
    // avatar lost its slot (de-rigged / despawned).
    for (entity, label, mut transform, mut visibility) in &mut labels {
        if !registry.slots.contains_key(&label.agent) {
            let _prev = registry.labels.remove(&label.agent);
            if let Ok(mut label_entity) = commands.get_entity(entity) {
                label_entity.despawn();
            }
            continue;
        }
        let Some(entry) = feed.get(label.agent) else {
            continue;
        };
        // Component-wise (not the glam operator) to stay clear of the
        // workspace `arithmetic_side_effects` lint: the avatar root's world
        // position, pushed to the ghost's display offset and lifted over the
        // head.
        let root = entry.root.w_axis;
        let anchor = Vec3::new(
            root.x + mode.ghost_offset,
            root.y + GPU_LABEL_LIFT_METRES,
            root.z,
        );
        if transform.translation != anchor {
            transform.translation = anchor;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
    // Spawn a label for every slotted avatar that has none yet.
    let slots: Vec<AgentKey> = registry.slots.keys().copied().collect();
    for agent in slots {
        if registry.labels.contains_key(&agent) || feed.get(agent).is_none() {
            continue;
        }
        let label = commands.spawn(gpu_label_bundle(agent)).id();
        let _prev = registry.labels.insert(agent, label);
    }
}
