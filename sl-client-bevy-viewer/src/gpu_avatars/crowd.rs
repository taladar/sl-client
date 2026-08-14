//! The **synthetic-crowd debug spawner** (`SL_VIEWER_CROWD=N`, GPU-avatar
//! Phase 5, step 1): once the local avatar's rigged body has resolved, this
//! spawns `N` GPU-instanced copies of it laid out on a grid, so the payoff of
//! Phases 1–4 (extract → stage → compute → draw at crowd scale) can finally be
//! measured — every live test so far was 1–3 avatars.
//!
//! **Same-body instancing.** Each copy reuses the local avatar's **exact**
//! skinned submesh `Mesh3d` + `MeshMaterial3d` handles (captured once into
//! [`GpuCrowd::submeshes`]), so Bevy batches all copies of one submesh into a
//! single instanced draw rather than `N` draws — the whole point of the
//! instancing work. The copies are full GPU-avatar instances: dummy-joint
//! `SkinnedMesh` palettes, their own [`PoseSlotKey::Crowd`] slot, and their
//! pose fed through the same passes A–D as a real avatar (see
//! [`crate::gpu_avatars::stage::stage_gpu_avatars`]).
//!
//! **Desynced motion.** Body and appearance are necessarily shared (they are
//! copies of the local avatar), so the crowd's variety lives in animation
//! phase: every copy plays the local avatar's clips but starts at a distinct
//! per-index **phase offset** and advances at a slightly different **playback
//! rate** (both from a deterministic golden-ratio sequence — no RNG), so the
//! copies never sit in lockstep and Phase 5's phase-bucket temporal LOD has a
//! spread of phases to bucket.
//!
//! **Zero cost when unset.** With `SL_VIEWER_CROWD` unset or `0` the target is
//! zero, [`GpuCrowd`] stays empty, no [`PoseSlotKey::Crowd`] slot is ever
//! allocated, and every crowd system early-returns — a normal run is untouched.

use std::sync::Arc;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, SlIdentity};

use super::stage::{GpuAvatarPoseFeed, GpuSkinBinding, PoseSlotKey};
use crate::avatars::{AvatarBody, AvatarState};
use crate::face_material::FaceMaterial;

/// The env var selecting the synthetic-crowd copy count (`SL_VIEWER_CROWD=N`).
const ENV_CROWD: &str = "SL_VIEWER_CROWD";

/// Grid cell size, metres — compact enough that `ceil(sqrt(N))²` copies fit in
/// a ~15–20 m block a normal camera pull-back frames at once.
const CELL_METRES: f32 = 1.75;

/// How many copies to spawn per frame while ramping up to the target, so a
/// large `N` does not stall one frame with thousands of spawn commands.
const SPAWN_BATCH: u32 = 16;

/// How many consecutive frames the local avatar's visible skinned-submesh set
/// must stay unchanged before it is captured as the crowd template — long
/// enough to wait out the async BOM bake / attachment loads (a mesh body/head
/// is `Visibility::Hidden` until its bake resolves, so the visible set *grows*
/// as those flip on, then holds steady). ~1–1.5 s at typical frame rates.
const SETTLE_STABLE_FRAMES: u32 = 45;

/// The minimum wall-clock time (seconds) to watch the visible set before
/// capturing, even if it looks stable immediately — a floor under
/// [`SETTLE_STABLE_FRAMES`] so an early momentary lull (hair/clothing visible,
/// body still baking) cannot trigger an early capture.
const SETTLE_MIN_DELAY_SECS: f32 = 2.0;

/// The max-wait fallback (seconds): if the visible set has still not settled by
/// now (a stuck / never-baked avatar), capture whatever is visible and warn, so
/// the crowd still spawns rather than hanging forever.
const SETTLE_MAX_WAIT_SECS: f32 = 18.0;

/// The clip-phase spread, seconds: per-copy phase offsets fan out across this
/// window so the crowd starts visibly desynced (roughly one long clip length).
const PHASE_SPREAD_SECS: f32 = 4.0;

/// The peak per-copy playback-rate jitter (fraction): a copy's clip advances at
/// `1 ± PLAYBACK_RATE_JITTER`, so copies drift apart over time rather than
/// holding a fixed phase separation. A few percent keeps them recognisably the
/// same motion.
const PLAYBACK_RATE_JITTER: f32 = 0.06;

/// A deterministic low-discrepancy fraction in `[0, 1)` for `index` — the
/// golden-ratio additive (Weyl) sequence, which spreads successive copies'
/// phases evenly without RNG (reproducible per index). Wraps every 65536
/// copies, far past any real crowd.
fn crowd_fraction(index: u32) -> f32 {
    /// The golden-ratio conjugate, the sequence's additive step.
    const GOLDEN: f32 = 0.618_034;
    let wrapped = u16::try_from(index % 0x1_0000).unwrap_or(0);
    (f32::from(wrapped) * GOLDEN).fract()
}

/// One skinned submesh captured from the local avatar, cloned into every crowd
/// copy. The `Mesh3d` + material handles are **shared** (not rebuilt), so all
/// copies of one submesh batch into a single instanced draw.
struct CrowdSubmesh {
    /// The shared render mesh handle (identical across every copy → batched).
    mesh: Mesh3d,
    /// The shared face-material handle (identical across every copy → batched).
    material: MeshMaterial3d<FaceMaterial>,
    /// The submesh's inverse-bindpose asset (shared; every copy's `SkinnedMesh`
    /// points at it).
    inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    /// The palette length (`SkinnedMesh.joints` count).
    joint_count: usize,
    /// The per-palette-slot canonical joint indices pass D resolves from, taken
    /// verbatim from the source submesh's [`GpuSkinBinding`].
    canonical: Arc<[u32]>,
}

/// One spawned crowd copy: the deterministic per-copy motion desync the stage
/// applies to its playback (its submesh entities hang off a root owned by the
/// ECS; the crowd never despawns, so no handle is kept).
pub(crate) struct CrowdCopy {
    /// The clip-phase offset (seconds) added to this copy's sampling clock.
    phase_offset: f32,
    /// The playback-rate multiplier (~1.0) this copy's sampling clock advances
    /// at, so it drifts away from its neighbours over time.
    rate: f32,
}

impl CrowdCopy {
    /// The copy's per-frame clip-phase desync `(offset seconds, rate)` the
    /// [`stage`](crate::gpu_avatars::stage) applies when building its sample
    /// jobs.
    pub(crate) const fn desync(&self) -> (f32, f32) {
        (self.phase_offset, self.rate)
    }
}

/// The settle watch that waits for the local avatar's visible skinned-submesh
/// set to stop changing before it is captured — the async BOM bake / attachment
/// loads flip body/head parts visible over the first seconds, so a first-rigged
/// capture would miss them (see [`resolve_template`]).
#[derive(Default)]
struct CrowdSettle {
    /// The visible-set signature last observed: `(count, XOR of entity bits)` —
    /// order-independent and cheap, changes whenever a submesh flips visibility.
    signature: Option<(usize, u64)>,
    /// Consecutive frames the visible set has been unchanged.
    stable_frames: u32,
    /// Wall-clock (`Time::elapsed_secs`) the watch began — the first frame the
    /// avatar was rigged with at least one visible submesh, for the min-delay
    /// floor and the max-wait fallback.
    watch_start: Option<f32>,
}

/// The synthetic-crowd debug state (`SL_VIEWER_CROWD`): the target copy count,
/// the resolved local-avatar template, the captured body submeshes, and the
/// spawned copies. Empty (target 0) unless the env selects a crowd, so every
/// consumer is a no-op on a normal run.
#[derive(Resource)]
pub(crate) struct GpuCrowd {
    /// The requested copy count (`0` = disabled).
    target: u32,
    /// The local avatar the crowd copies (its shape, clips and body submesh
    /// handles), once its rigged body has resolved.
    template: Option<AgentKey>,
    /// The captured body submeshes cloned into each copy (shared handles).
    submeshes: Vec<CrowdSubmesh>,
    /// The spawned copies, in crowd-index order (index = position).
    copies: Vec<CrowdCopy>,
    /// The visible-set settle watch, until the template is captured.
    settle: CrowdSettle,
}

impl Default for GpuCrowd {
    fn default() -> Self {
        let target = std::env::var(ENV_CROWD)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(0);
        Self {
            target,
            template: None,
            submeshes: Vec::new(),
            copies: Vec::new(),
            settle: CrowdSettle::default(),
        }
    }
}

impl GpuCrowd {
    /// Whether a crowd was requested (`SL_VIEWER_CROWD` ≥ 1).
    const fn enabled(&self) -> bool {
        self.target > 0
    }

    /// The local avatar whose shape / clips the crowd copies, once resolved.
    pub(crate) const fn template(&self) -> Option<AgentKey> {
        self.template
    }

    /// The number of spawned copies (the count of live [`PoseSlotKey::Crowd`]
    /// slots).
    pub(crate) fn copy_count(&self) -> u32 {
        u32::try_from(self.copies.len()).unwrap_or(u32::MAX)
    }

    /// The crowd indices of every spawned copy (`0 .. copy_count`).
    pub(crate) fn slots(&self) -> std::ops::Range<u32> {
        0..self.copy_count()
    }

    /// The copy at crowd `index`, for the stage's per-copy phase desync.
    pub(crate) fn copy(&self, index: u32) -> Option<&CrowdCopy> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.copies.get(index))
    }
}

/// The grid offset (Bevy world metres, on the ground plane) of crowd copy
/// `index` within a `cols × cols` square block centred on the origin — a
/// left-multiplied world translation lays the copies out around the local
/// avatar. Copies spread in X/Z (Bevy up is Y), so the block is flat on the
/// ground.
fn grid_offset(index: u32, cols: u32) -> Vec3 {
    let cols = cols.max(1);
    let col = index.checked_rem(cols).unwrap_or(0);
    let row = index.checked_div(cols).unwrap_or(0);
    let half = f32::from(u16::try_from(cols.saturating_sub(1)).unwrap_or(0)) * 0.5;
    let x = (f32::from(u16::try_from(col).unwrap_or(0)) - half) * CELL_METRES;
    let z = (f32::from(u16::try_from(row).unwrap_or(0)) - half) * CELL_METRES;
    Vec3::new(x, 0.0, z)
}

/// The query the crowd systems scan: every skinned avatar/worn submesh, with
/// its entity, shared render handles, GPU skin binding and inherited
/// visibility.
type CrowdSourceQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Mesh3d,
        &'static MeshMaterial3d<FaceMaterial>,
        &'static SkinnedMesh,
        &'static GpuSkinBinding,
        &'static InheritedVisibility,
    ),
>;

/// Gather the local avatar's currently **visible** skinned submeshes and a
/// signature of that set. A modern mesh-body avatar hides its system-body base
/// parts where a worn mesh covers the region (`apply_avatar_part_visibility`
/// sets them `Visibility::Hidden`), and its BOM mesh body/head stay hidden
/// until their bake resolves asynchronously — the crowd copies are not
/// `AvatarBodyPart` entities, so neither hide reaches them, so the visible set
/// is filtered here. The avatar parts carry `NoFrustumCulling`, so
/// `InheritedVisibility` reflects only the Hidden/Inherited chain, not culling.
/// The signature is `(count, XOR of entity bits)` — order-independent, changing
/// whenever any submesh flips visibility, so [`resolve_template`] can tell when
/// the set has settled.
fn visible_submeshes(
    local: AgentKey,
    sources: &CrowdSourceQuery<'_, '_>,
) -> (Vec<CrowdSubmesh>, (usize, u64)) {
    let slot = PoseSlotKey::Avatar(local);
    let mut submeshes: Vec<CrowdSubmesh> = Vec::new();
    let mut xor = 0_u64;
    for (entity, mesh, material, skin, binding, visibility) in sources {
        if binding.slot != slot || !visibility.get() {
            continue;
        }
        xor ^= entity.to_bits();
        submeshes.push(CrowdSubmesh {
            mesh: mesh.clone(),
            material: material.clone(),
            inverse_bindposes: skin.inverse_bindposes.clone(),
            joint_count: skin.joints.len(),
            canonical: Arc::clone(&binding.canonical),
        });
    }
    let signature = (submeshes.len(), xor);
    (submeshes, signature)
}

/// Capture the local avatar's visible skinned submeshes (shared handles) into
/// [`GpuCrowd::submeshes`] and record it as the crowd template — once its
/// **final draw set has settled**: rigged, appearance resolved, and the visible
/// set unchanged for [`SETTLE_STABLE_FRAMES`] frames past
/// [`SETTLE_MIN_DELAY_SECS`] (waiting out the async BOM bake / attachment
/// loads), or the [`SETTLE_MAX_WAIT_SECS`] fallback for a stuck avatar. Returns
/// whether the template is now ready.
fn resolve_template(
    crowd: &mut GpuCrowd,
    identity: &SlIdentity,
    state: &AvatarState,
    sources: &CrowdSourceQuery<'_, '_>,
    now: f32,
) -> bool {
    if crowd.template.is_some() {
        return true;
    }
    let Some(local) = identity.agent_id else {
        return false;
    };
    // Wait until the local avatar is rigged and its appearance has resolved
    // (a shape is present) before watching its visible set.
    if !state.is_rigged(local) || state.deformations(local).is_none() {
        return false;
    }
    let (submeshes, signature) = visible_submeshes(local, sources);
    // Nothing drawn yet: keep waiting (do not start the watch on an empty set).
    if submeshes.is_empty() {
        return false;
    }
    // Advance the settle watch: reset the stable-frame count whenever the
    // visible set changed (a bake / attachment just flipped a part on).
    let watch_start = *crowd.settle.watch_start.get_or_insert(now);
    if crowd.settle.signature == Some(signature) {
        crowd.settle.stable_frames = crowd.settle.stable_frames.saturating_add(1);
    } else {
        crowd.settle.signature = Some(signature);
        crowd.settle.stable_frames = 0;
    }
    let elapsed = now - watch_start;
    let settled =
        elapsed >= SETTLE_MIN_DELAY_SECS && crowd.settle.stable_frames >= SETTLE_STABLE_FRAMES;
    let timed_out = elapsed >= SETTLE_MAX_WAIT_SECS;
    if !settled && !timed_out {
        return false;
    }
    if timed_out && !settled {
        warn!(
            "SL_VIEWER_CROWD={}: local-avatar visible set did not settle within {:.0}s \
             (still baking / loading?); capturing {} visible submesh(es) anyway",
            crowd.target,
            SETTLE_MAX_WAIT_SECS,
            submeshes.len()
        );
    }
    info!(
        "SL_VIEWER_CROWD={}: captured {} visible local-avatar submesh(es) as the crowd \
         template after {:.1}s (settled; hidden system-body parts excluded, BOM \
         body/head included once baked)",
        crowd.target,
        submeshes.len(),
        elapsed
    );
    crowd.submeshes = submeshes;
    crowd.template = Some(local);
    true
}

/// Spawn synthetic-crowd copies up to `SL_VIEWER_CROWD`, `SPAWN_BATCH` per
/// frame, each a full GPU-avatar instance reusing the local avatar's shared
/// submesh handles under its own [`PoseSlotKey::Crowd`] slot. A no-op when the
/// env is unset or the template has not resolved.
pub(crate) fn spawn_crowd(
    mut crowd: ResMut<GpuCrowd>,
    identity: Res<SlIdentity>,
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    time: Res<Time>,
    sources: CrowdSourceQuery<'_, '_>,
    mut commands: Commands,
) {
    if !crowd.enabled() || crowd.copy_count() >= crowd.target {
        return;
    }
    let Some(body) = body else {
        return;
    };
    if !resolve_template(&mut crowd, &identity, &state, &sources, time.elapsed_secs()) {
        return;
    }
    let dummy = body.dummy_joint();
    // The square-ish grid: ceil(sqrt(N)) columns, so 100 → 10×10.
    let cols = isqrt_ceil(crowd.target).max(1);
    let end = crowd
        .target
        .min(crowd.copy_count().saturating_add(SPAWN_BATCH));
    for index in crowd.copy_count()..end {
        let root = commands
            .spawn((
                // The root transform is organisational only: a skinned mesh is
                // rendered from its world-space skin palette (which pass C
                // composes under the fed crowd root), so the copy's on-screen
                // position comes from `publish_crowd`, not this transform.
                Transform::from_translation(grid_offset(index, cols)),
                Visibility::default(),
            ))
            .id();
        for submesh in &crowd.submeshes {
            commands.spawn((
                submesh.mesh.clone(),
                submesh.material.clone(),
                Transform::default(),
                Visibility::Inherited,
                SkinnedMesh {
                    inverse_bindposes: submesh.inverse_bindposes.clone(),
                    joints: vec![dummy; submesh.joint_count],
                },
                // A skinned mesh's bind-pose bounds do not track its posed
                // vertices, and the palette is GPU-written after culling — so,
                // like a real avatar submesh, never frustum-cull it.
                NoFrustumCulling,
                GpuSkinBinding {
                    slot: PoseSlotKey::Crowd(index),
                    canonical: Arc::clone(&submesh.canonical),
                },
                ChildOf(root),
            ));
        }
        crowd.copies.push(CrowdCopy {
            phase_offset: crowd_fraction(index) * PHASE_SPREAD_SECS,
            // A second, decorrelated sequence term for the rate jitter.
            rate: 1.0
                + (crowd_fraction(index.wrapping_mul(3).wrapping_add(1)) - 0.5)
                    * 2.0
                    * PLAYBACK_RATE_JITTER,
        });
    }
    if crowd.copy_count() >= crowd.target {
        info!(
            "SL_VIEWER_CROWD: {} synthetic crowd copies spawned ({} submesh instances each)",
            crowd.copy_count(),
            crowd.submeshes.len()
        );
    }
}

/// Publish each spawned crowd copy's GPU-pose feed entry — the local avatar's
/// current root matrix translated to the copy's grid cell, plus the template's
/// sparse adjuster corrections — so [`stage_gpu_avatars`](crate::gpu_avatars::stage::stage_gpu_avatars)
/// stages the copy this frame. Runs after the pose driver (the template feed is
/// published) and before the stage. A no-op when no copies exist.
pub(crate) fn publish_crowd(crowd: Res<GpuCrowd>, mut feed: ResMut<GpuAvatarPoseFeed>) {
    if crowd.copy_count() == 0 {
        return;
    }
    let Some(template) = crowd.template else {
        return;
    };
    // The local avatar's just-published root + corrections this frame.
    let Some((base_root, corrections)) = feed.template_entry(PoseSlotKey::Avatar(template)) else {
        return;
    };
    let cols = isqrt_ceil(crowd.target).max(1);
    for index in crowd.slots() {
        let root = Mat4::from_translation(grid_offset(index, cols)).mul_mat4(&base_root);
        feed.publish_real(PoseSlotKey::Crowd(index), root, corrections.clone());
    }
}

/// `ceil(sqrt(n))` for a `u32`, without floats — the square-grid column count.
const fn isqrt_ceil(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut root = 0_u32;
    while root.saturating_mul(root) < n {
        root = root.saturating_add(1);
    }
    root
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{CELL_METRES, crowd_fraction, grid_offset, isqrt_ceil};

    #[test]
    fn isqrt_ceil_is_the_ceiling_of_the_square_root() {
        assert_eq!(isqrt_ceil(0), 0);
        assert_eq!(isqrt_ceil(1), 1);
        assert_eq!(isqrt_ceil(4), 2);
        assert_eq!(isqrt_ceil(5), 3);
        assert_eq!(isqrt_ceil(100), 10);
        assert_eq!(isqrt_ceil(101), 11);
    }

    #[test]
    fn crowd_fraction_is_in_unit_range_and_deterministic() {
        for index in [0_u32, 1, 7, 99, 1000, 0x1_0001] {
            let value = crowd_fraction(index);
            assert!((0.0..1.0).contains(&value), "index {index} → {value}");
            assert!((crowd_fraction(index) - value).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn crowd_fractions_spread_across_the_unit_interval() {
        // The first 100 offsets, sorted, cover the interval with no large gap
        // (a low-discrepancy sequence), so the crowd is not phase-clustered.
        let mut values: Vec<f32> = (0..100_u32).map(crowd_fraction).collect();
        values.sort_by(f32::total_cmp);
        let max_gap = values
            .windows(2)
            .map(|pair| pair.get(1).copied().unwrap_or(0.0) - pair.first().copied().unwrap_or(0.0))
            .fold(0.0_f32, f32::max);
        assert!(max_gap < 0.1, "no phase-cluster gap > 0.1: {max_gap}");
        assert!(
            values.first().copied().unwrap_or(1.0) < 0.05,
            "covers the low end"
        );
        assert!(
            values.last().copied().unwrap_or(0.0) > 0.95,
            "covers the high end"
        );
    }

    #[test]
    fn grid_is_square_centred_and_flat() {
        // A 10×10 block: first cell is the top-left corner, and the block is
        // centred on the origin (corners symmetric about zero), flat on Y.
        let corner = grid_offset(0, 10);
        let far = grid_offset(99, 10);
        assert!(corner.y.abs() < f32::EPSILON, "flat on the ground plane");
        assert!(far.y.abs() < f32::EPSILON, "flat on the ground plane");
        assert!((corner.x + far.x).abs() < f32::EPSILON, "centred in X");
        assert!((corner.z + far.z).abs() < f32::EPSILON, "centred in Z");
        // Neighbouring columns are exactly one cell apart.
        let step = grid_offset(1, 10).x - grid_offset(0, 10).x;
        assert!((step - CELL_METRES).abs() < f32::EPSILON);
    }
}
