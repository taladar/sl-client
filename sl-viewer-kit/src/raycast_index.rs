//! An off-thread spatial index for raycasts against static world geometry, the
//! viewer's replacement for avian's `SpatialQuery`
//! ([[viewer-perf-custom-static-raycast-index]]).
//!
//! The viewer does not simulate physics — the solver is idle — yet avian's full
//! `PhysicsPlugins` set maintained its collider BVH *every fixed step over the
//! whole static set* on the frame thread, purely to answer the third-person
//! camera's one raycast per frame (and, later, other world-space raycasts). A
//! full-session Aditi trace measured that maintenance spiking `RunFixedMainLoop`
//! to 117 ms. This module replaces it with exactly what the viewer needs: a
//! [`parry3d`] BVH over the static prim colliders, **built on a background task**
//! and published as an immutable snapshot, queried lock-free from the main
//! thread.
//!
//! ## Shape
//!
//! - [`RaycastIndexColliders`] is the authoritative collider set, mutated *by
//!   change detection only* (a prim rez / derez / move), never a per-frame full
//!   scan.
//! - When it is dirty, [`rebuild_raycast_index`] spawns an
//!   [`AsyncComputeTaskPool`] task that rebuilds the [`Bvh`] off-thread and
//!   returns an `IndexSnapshot`; a poll installs it into
//!   [`StaticRaycastIndex`] via an [`ArcSwap`] (lock-free reads).
//! - [`StaticRaycastIndex::cast_ray`] reads the current snapshot and casts the
//!   ray through the BVH, doing the precise parry ray-vs-shape test only on the
//!   handful of colliders the broad phase keeps. Static geometry never moves, so
//!   a snapshot lagging a rez by a frame or two is imperceptible.

use std::collections::HashSet;
use std::sync::Arc;

use arc_swap::ArcSwap;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use parry3d::bounding_volume::Aabb;
use parry3d::math::{Pose, Rotation as ParryRot, Vector as ParryVec};
use parry3d::partitioning::{Bvh, BvhBuildStrategy};
use parry3d::query::{Ray, contact};
use parry3d::shape::SharedShape;

/// Convert a Bevy [`Vec3`] into the [`parry3d`] vector type (a `glam` vector,
/// possibly a different `glam` release than Bevy's, so round-trip through a plain
/// array rather than assuming the types unify).
#[must_use]
const fn to_parry_vec(v: Vec3) -> ParryVec {
    ParryVec::from_array(v.to_array())
}

/// Convert a [`parry3d`] vector back into a Bevy [`Vec3`] (see [`to_parry_vec`]).
#[must_use]
const fn to_bevy_vec(v: ParryVec) -> Vec3 {
    Vec3::from_array(v.to_array())
}

/// Build the [`parry3d`] pose (isometry) for a collider from a Bevy translation
/// and rotation. The prim's object scale is already baked into the collider
/// geometry (see `physics::build_static_colliders`), so the pose carries no
/// scale.
#[must_use]
fn to_parry_pose(translation: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(
        to_parry_vec(translation),
        ParryRot::from_array(rotation.to_array()),
    )
}

/// One collider in the authoritative set: its parry shape, world pose, and
/// whether it is physically collidable (`Solid`) as opposed to merely indexed
/// (phantom / physics-shape-`None`, which the camera still occludes on but a
/// physics-layer query filters out).
#[derive(Clone, Debug)]
struct ColliderRecord {
    /// The parry collision shape (object-local, object scale baked in).
    shape: SharedShape,
    /// The collider's world translation (Bevy frame).
    translation: Vec3,
    /// The collider's world rotation (Bevy frame).
    rotation: Quat,
    /// `true` for a physically-collidable prim, `false` for an indexed-only one.
    solid: bool,
}

/// The authoritative static-collider set the index is built from, keyed by prim
/// entity. Mutated only when a collider is added, moved, or removed (change
/// detection), so its upkeep is O(changes), not O(all prims) — the whole point
/// of replacing avian's per-step whole-set maintenance.
#[derive(Debug, Resource, Default)]
pub struct RaycastIndexColliders {
    /// One record per collidered prim entity.
    records: HashMap<Entity, ColliderRecord>,
    /// Set when `records` changed since the last rebuild, so a rebuild is queued.
    dirty: bool,
}

impl RaycastIndexColliders {
    /// Insert or replace a prim's collider, marking the set dirty.
    pub fn upsert(
        &mut self,
        entity: Entity,
        shape: SharedShape,
        translation: Vec3,
        rotation: Quat,
        solid: bool,
    ) {
        let _previous = self.records.insert(
            entity,
            ColliderRecord {
                shape,
                translation,
                rotation,
                solid,
            },
        );
        self.dirty = true;
    }

    /// Remove a prim's collider (a derez / a prim that stopped qualifying),
    /// marking the set dirty only if it actually held one.
    pub fn remove(&mut self, entity: Entity) {
        if self.records.remove(&entity).is_some() {
            self.dirty = true;
        }
    }
}

/// An immutable, queryable snapshot of the index: a [`Bvh`] whose leaf data is an
/// index into `entries`. Published behind an [`ArcSwap`] so a query reads it
/// without locking while the background task builds the next one.
#[derive(Debug)]
struct IndexSnapshot {
    /// The bounding-volume hierarchy over the entries' world AABBs.
    bvh: Bvh,
    /// The colliders, parallel to the BVH leaf indices.
    entries: Vec<SnapshotEntry>,
}

/// One collider inside an `IndexSnapshot`: the parry shape at its world pose,
/// plus the source entity and its solidity, so a query can return the hit prim
/// and honour a solid-only filter.
#[derive(Debug)]
struct SnapshotEntry {
    /// The parry collision shape.
    shape: SharedShape,
    /// The shape's world pose.
    pose: Pose,
    /// The prim entity this collider belongs to.
    entity: Entity,
    /// Whether the prim is physically collidable.
    solid: bool,
}

impl IndexSnapshot {
    /// An empty snapshot (no colliders): every raycast misses.
    #[must_use]
    fn empty() -> Self {
        let no_leaves: [Aabb; 0] = [];
        Self {
            bvh: Bvh::from_leaves(BvhBuildStrategy::Binned, &no_leaves),
            entries: Vec::new(),
        }
    }

    /// Build a snapshot from a collider set: compute each shape's world AABB,
    /// build the BVH over them, and keep the shapes parallel to the leaf indices.
    #[must_use]
    fn build(records: Vec<(Entity, ColliderRecord)>) -> Self {
        let mut entries = Vec::with_capacity(records.len());
        let mut aabbs = Vec::with_capacity(records.len());
        for (entity, record) in records {
            let pose = to_parry_pose(record.translation, record.rotation);
            aabbs.push(record.shape.compute_aabb(&pose));
            entries.push(SnapshotEntry {
                shape: record.shape,
                pose,
                entity,
                solid: record.solid,
            });
        }
        let bvh = Bvh::from_leaves(BvhBuildStrategy::Binned, &aabbs);
        Self { bvh, entries }
    }

    /// Cast a ray through the snapshot, returning the distance to the nearest hit
    /// (see [`StaticRaycastIndex::cast_ray`] for the semantics of the arguments).
    #[must_use]
    fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        solid: bool,
        solid_only: bool,
        exclude: &HashSet<Entity>,
    ) -> Option<f32> {
        if self.entries.is_empty() {
            return None;
        }
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(direction));
        let (_leaf, distance) = self.bvh.cast_ray(&ray, max_distance, |leaf_index, best| {
            let entry = self.leaf(leaf_index)?;
            if exclude.contains(&entry.entity) || (solid_only && !entry.solid) {
                return None;
            }
            // `dyn Shape: RayCast`, so a `SharedShape` casts directly. Bound to
            // `best` so a leaf farther than the current nearest hit is skipped.
            entry.shape.cast_ray(&entry.pose, &ray, best, solid)
        })?;
        Some(distance)
    }

    /// The entry a BVH leaf index refers to, if in range.
    #[must_use]
    fn leaf(&self, leaf_index: u32) -> Option<&SnapshotEntry> {
        self.entries.get(usize::try_from(leaf_index).ok()?)
    }
}

/// The published, lock-free raycast index. Read from any main-thread system via
/// [`cast_ray`](StaticRaycastIndex::cast_ray).
#[derive(Debug, Resource)]
pub struct StaticRaycastIndex {
    /// The current snapshot, swapped in by [`rebuild_raycast_index`].
    snapshot: ArcSwap<IndexSnapshot>,
}

impl Default for StaticRaycastIndex {
    fn default() -> Self {
        Self {
            snapshot: ArcSwap::from_pointee(IndexSnapshot::empty()),
        }
    }
}

impl StaticRaycastIndex {
    /// Cast a ray from `origin` along `direction` (a unit vector), up to
    /// `max_distance`, against the current snapshot.
    ///
    /// - `solid`: treat colliders as solid volumes (`true`) or hollow surfaces
    ///   (`false`). Camera collision casts hollow so a ray originating *inside* a
    ///   collider reports the far surface rather than the origin.
    /// - `solid_only`: restrict to physically-collidable (`Solid`) colliders,
    ///   skipping indexed-only phantom / physics-`None` prims. Camera collision
    ///   passes `false` (it occludes on everything visible).
    /// - `exclude`: prim entities to ignore (e.g. the own avatar's attachments).
    ///
    /// Returns the distance along the ray to the nearest hit, or `None` on a miss.
    #[must_use]
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        solid: bool,
        solid_only: bool,
        exclude: &HashSet<Entity>,
    ) -> Option<f32> {
        self.snapshot
            .load()
            .cast_ray(origin, direction, max_distance, solid, solid_only, exclude)
    }
}

/// One moving (physical-prim) collider: rebuilt into [`DynamicColliders`] every
/// frame, so — unlike the static BVH — it never triggers an off-thread rebuild.
#[derive(Clone, Debug)]
struct DynamicCollider {
    /// The parry collision shape (object scale baked in).
    shape: SharedShape,
    /// The collider's current world pose (Bevy frame → parry).
    pose: Pose,
    /// The prim entity.
    entity: Entity,
    /// Whether the prim is physically collidable.
    solid: bool,
}

/// The small set of **moving** physical-prim colliders, refilled each frame (see
/// `physics::sync_dynamic_colliders`). Kept out of the static BVH — which is
/// change-rebuilt off-thread — because these move continuously; there are only
/// ever a handful, so a linear scan is cheaper than churning the BVH. Serves both
/// camera collision (cast alongside the static index) and the collision-sound
/// contact test.
#[derive(Debug, Resource, Default)]
pub struct DynamicColliders {
    /// The current frame's moving colliders.
    colliders: Vec<DynamicCollider>,
}

impl DynamicColliders {
    /// Drop the previous frame's colliders (called before refilling).
    pub fn clear(&mut self) {
        self.colliders.clear();
    }

    /// Add a moving collider at its current world pose.
    pub fn push(
        &mut self,
        entity: Entity,
        shape: SharedShape,
        translation: Vec3,
        rotation: Quat,
        solid: bool,
    ) {
        self.colliders.push(DynamicCollider {
            shape,
            pose: to_parry_pose(translation, rotation),
            entity,
            solid,
        });
    }

    /// Cast a ray against the moving colliders, returning the nearest hit distance
    /// (linear — the set is tiny). Arguments mirror [`StaticRaycastIndex::cast_ray`].
    #[must_use]
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        solid: bool,
        solid_only: bool,
        exclude: &HashSet<Entity>,
    ) -> Option<f32> {
        let ray = Ray::new(to_parry_vec(origin), to_parry_vec(direction));
        let mut best: Option<f32> = None;
        for collider in &self.colliders {
            if exclude.contains(&collider.entity) || (solid_only && !collider.solid) {
                continue;
            }
            let limit = best.unwrap_or(max_distance);
            if let Some(distance) = collider.shape.cast_ray(&collider.pose, &ray, limit, solid) {
                best = Some(distance);
            }
        }
        best
    }

    /// Every pair of **solid** moving colliders currently touching (parry contact
    /// distance ≤ 0), with a world-space contact point — the input to the
    /// viewer-synthesised prim–prim collision sounds. O(n²) over a handful.
    #[must_use]
    pub fn contact_pairs(&self) -> Vec<(Entity, Entity, Vec3)> {
        let mut pairs = Vec::new();
        for (index, first) in self.colliders.iter().enumerate() {
            if !first.solid {
                continue;
            }
            for second in self.colliders.iter().skip(index.saturating_add(1)) {
                if !second.solid {
                    continue;
                }
                if let Ok(Some(hit)) = contact(
                    &first.pose,
                    &*first.shape,
                    &second.pose,
                    &*second.shape,
                    0.0,
                ) && hit.dist <= 0.0
                {
                    pairs.push((first.entity, second.entity, to_bevy_vec(hit.point1)));
                }
            }
        }
        pairs
    }
}

/// The in-flight off-thread snapshot rebuild, so only one runs at a time and the
/// poll can install its result.
#[derive(Debug, Resource, Default)]
pub struct IndexRebuild {
    /// The running rebuild task, if any.
    task: Option<Task<IndexSnapshot>>,
}

/// Rebuild the published snapshot when the collider set has changed: poll any
/// in-flight build and install it, then — if the set is dirty and nothing is
/// building — snapshot the current records and spawn a fresh off-thread build.
///
/// The heavy BVH construction runs on the [`AsyncComputeTaskPool`], never the
/// frame thread; the main-thread cost here is one map clone when a rebuild
/// starts, only on frames where the collider set actually changed.
pub fn rebuild_raycast_index(
    mut colliders: ResMut<RaycastIndexColliders>,
    mut rebuild: ResMut<IndexRebuild>,
    index: Res<StaticRaycastIndex>,
) {
    if let Some(task) = rebuild.task.as_mut() {
        if let Some(snapshot) = block_on(poll_once(task)) {
            index.snapshot.store(Arc::new(snapshot));
            rebuild.task = None;
        } else {
            // A build is still running; let it finish before starting another.
            return;
        }
    }
    if !colliders.dirty {
        return;
    }
    colliders.dirty = false;
    let records: Vec<(Entity, ColliderRecord)> = colliders
        .records
        .iter()
        .map(|(entity, record)| (*entity, record.clone()))
        .collect();
    rebuild.task =
        Some(AsyncComputeTaskPool::get().spawn(async move { IndexSnapshot::build(records) }));
}

/// Registers the raycast-index resources and the rebuild system.
#[derive(Debug)]
pub struct RaycastIndexPlugin;

impl Plugin for RaycastIndexPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RaycastIndexColliders>()
            .init_resource::<StaticRaycastIndex>()
            .init_resource::<DynamicColliders>()
            .init_resource::<IndexRebuild>()
            .add_systems(Update, rebuild_raycast_index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a snapshot from a single-collider authoring set (a test helper, so
    /// the `RaycastIndexColliders` → `IndexSnapshot` plumbing is exercised).
    fn snapshot_of(entity: Entity, shape: SharedShape, at: Vec3, solid: bool) -> IndexSnapshot {
        let mut colliders = RaycastIndexColliders::default();
        colliders.upsert(entity, shape, at, Quat::IDENTITY, solid);
        let records: Vec<(Entity, ColliderRecord)> = colliders
            .records
            .iter()
            .map(|(entity, record)| (*entity, record.clone()))
            .collect();
        IndexSnapshot::build(records)
    }

    /// A ray straight down the Bevy -Z axis hits a unit cuboid parked ahead of it
    /// at the expected distance.
    #[test]
    fn cast_ray_hits_a_cuboid() {
        let snapshot = snapshot_of(
            Entity::PLACEHOLDER,
            SharedShape::cuboid(1.0, 1.0, 1.0),
            Vec3::new(0.0, 0.0, -5.0),
            true,
        );
        // Cuboid half-extent 1 at z=-5 → near face at z=-4 → distance 4.
        let distance = snapshot.cast_ray(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
            100.0,
            false,
            false,
            &HashSet::new(),
        );
        assert!(
            distance.is_some_and(|distance| (distance - 4.0).abs() < 1.0e-3),
            "distance {distance:?}"
        );
    }

    /// A ray that misses every collider returns `None`, and an empty index never
    /// hits.
    #[test]
    fn cast_ray_misses_and_empty_index() {
        assert!(
            IndexSnapshot::empty()
                .cast_ray(
                    Vec3::ZERO,
                    Vec3::new(0.0, 0.0, -1.0),
                    100.0,
                    false,
                    false,
                    &HashSet::new()
                )
                .is_none()
        );

        // Ray down -Z never reaches a collider parked on +X.
        let snapshot = snapshot_of(
            Entity::PLACEHOLDER,
            SharedShape::cuboid(0.5, 0.5, 0.5),
            Vec3::new(10.0, 0.0, 0.0),
            true,
        );
        assert!(
            snapshot
                .cast_ray(
                    Vec3::ZERO,
                    Vec3::new(0.0, 0.0, -1.0),
                    100.0,
                    false,
                    false,
                    &HashSet::new()
                )
                .is_none()
        );
    }

    /// The `exclude` set skips a collider that would otherwise be hit, and
    /// `solid_only` skips a non-solid (indexed-only) collider.
    #[test]
    fn exclude_and_solid_only_filters() {
        let excluded = Entity::PLACEHOLDER;
        // A non-solid (indexed-only) collider dead ahead.
        let snapshot = snapshot_of(
            excluded,
            SharedShape::cuboid(1.0, 1.0, 1.0),
            Vec3::new(0.0, 0.0, -5.0),
            false,
        );
        let down = Vec3::new(0.0, 0.0, -1.0);

        // Excluded → miss.
        let mut exclude = HashSet::new();
        exclude.insert(excluded);
        assert!(
            snapshot
                .cast_ray(Vec3::ZERO, down, 100.0, false, false, &exclude)
                .is_none()
        );
        // solid_only with a non-solid collider → miss.
        assert!(
            snapshot
                .cast_ray(Vec3::ZERO, down, 100.0, false, true, &HashSet::new())
                .is_none()
        );
        // No filter → hit.
        assert!(
            snapshot
                .cast_ray(Vec3::ZERO, down, 100.0, false, false, &HashSet::new())
                .is_some()
        );
    }
}
