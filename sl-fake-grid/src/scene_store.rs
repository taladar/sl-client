//! The region's object store: a `bevy_ecs` [`World`] with one entity per
//! prim, found by an index rather than by a scan.
//!
//! A fixture states a region as a list of [`Object`]s, and for as long as a
//! region held a few dozen prims and nothing ticked a `Vec` of them was
//! enough. A script engine reads the scene per script per tick — `llGetPos`
//! on a root, `llGetLinkKey`, `llGetObjectDetails` on an arbitrary key — and
//! physics, sensors and the update stream query it per tick as well, so the
//! region is an ECS world:
//!
//! - every prim is an [`Entity`], spawned when it enters the region; an
//!   entity that left is never confused with one that came later, because a
//!   reused index carries a new generation;
//! - its wire record is its [`Body`] component, and what the region knows of
//!   it besides — its contents, what it costs the region, its edit history —
//!   are components of their own, so they leave with it;
//! - its region-local id, its full id and its link-set root are **indices
//!   into** the world, kept by the store as prims come, go and change;
//! - a write goes through `bevy_ecs`'s [`Mut`], so the world's change ticks
//!   say which prims changed, for the update stream to read.
//!
//! # Order
//!
//! An ECS world's iteration order is its own and shifts as entities leave, so
//! the store numbers each prim as it comes ([`Seq`]) and walks them in that
//! order. An arrival burst, a refetch and a linkset's unnumbered children
//! are all sent in it, and two runs of one scenario send them alike.
//!
//! # Changing a prim
//!
//! [`ObjectStore::get_mut`] hands out an [`ObjectMut`], which writes the prim
//! in place and, when it is dropped, files the prim again under whatever of
//! its ids the change moved: a prim relinked under another root, or given a
//! new full id, is found under the new one at once.
//!
//! # Copies
//!
//! A [`World`] cannot be cloned, and a scenario's world is copied into every
//! region that uses it when the grid starts. [`Clone`] for the store spawns
//! the same prims, with the same components and in the same order, into a
//! fresh world.

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::{Mut, World};
use sl_proto::{Object, RegionLocalObjectId};
use sl_types::key::ObjectKey;

use crate::world::{EditHistory, ObjectCost, TaskInventory};

/// A prim's wire record: what an object update states.
#[derive(Debug, Clone, Component)]
pub struct Body(pub Object);

/// A prim's task inventory. A prim nothing was ever put in has none, which
/// reads as empty.
#[derive(Debug, Clone, Component)]
pub struct Contents(pub TaskInventory);

/// What a prim costs the region, where a fixture said.
#[derive(Debug, Clone, Component)]
pub struct Cost(pub ObjectCost);

/// A prim's history of moves, turns and resizes, from its first edit.
#[derive(Debug, Clone, Component)]
pub(crate) struct History(pub(crate) EditHistory);

/// The order a prim came into the region in: the store walks prims by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Component)]
pub struct Seq(u64);

/// The ids a prim is filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Filed {
    /// Its region-local id.
    local: RegionLocalObjectId,
    /// Its full id.
    key: ObjectKey,
    /// What it hangs off; zero for nothing.
    parent: RegionLocalObjectId,
}

impl Filed {
    /// The ids `object` is filed under.
    const fn of(object: &Object) -> Self {
        Self {
            local: object.local_id,
            key: object.full_id,
            parent: object.parent_id,
        }
    }
}

/// The store's indices: where each prim is found, and in which order.
#[derive(Debug, Clone, Default)]
struct Indices {
    /// Every prim, by the order it came in.
    order: BTreeMap<Seq, Entity>,
    /// Each prim by its region-local id.
    by_local_id: HashMap<RegionLocalObjectId, Entity>,
    /// Each prim by its full id.
    by_key: HashMap<ObjectKey, Entity>,
    /// The prims hanging off each region-local id, in the store's order. A
    /// prim of a set whose root is not here is still filed under it.
    children: HashMap<RegionLocalObjectId, BTreeMap<Seq, Entity>>,
    /// The highest region-local id a prim in this store has ever had. Never
    /// lowered, so an id a removed prim held is still counted when a new one
    /// is minted.
    highest_local_id: u32,
    /// The number the next prim to come in is given.
    next_seq: u64,
}

impl Indices {
    /// Files `entity`, which came in as `seq`, under `ids`. An id another
    /// prim already holds stays that prim's.
    fn file(&mut self, entity: Entity, seq: Seq, ids: Filed) {
        self.highest_local_id = self.highest_local_id.max(ids.local.0);
        let local = self.by_local_id.entry(ids.local).or_insert(entity);
        if *local != entity {
            tracing::warn!(
                local_id = ids.local.0,
                "a second prim with one region-local id is in the region; the first keeps it"
            );
        }
        let keyed = self.by_key.entry(ids.key).or_insert(entity);
        if *keyed != entity {
            tracing::warn!(
                full_id = %ids.key,
                "a second prim with one full id is in the region; the first keeps it"
            );
        }
        if ids.parent.0 != 0 {
            let _new = self
                .children
                .entry(ids.parent)
                .or_default()
                .insert(seq, entity);
        }
    }

    /// Takes `entity`, which came in as `seq`, out from under `ids`.
    fn unfile(&mut self, entity: Entity, seq: Seq, ids: Filed) {
        if self.by_local_id.get(&ids.local) == Some(&entity) {
            let _gone = self.by_local_id.remove(&ids.local);
        }
        if self.by_key.get(&ids.key) == Some(&entity) {
            let _gone = self.by_key.remove(&ids.key);
        }
        if let Some(children) = self.children.get_mut(&ids.parent) {
            let _gone = children.remove(&seq);
            if children.is_empty() {
                let _empty = self.children.remove(&ids.parent);
            }
        }
    }
}

/// The prims of one region: an ECS world and the indices into it.
pub struct ObjectStore {
    /// One entity per prim.
    world: World,
    /// Where each prim is found.
    indices: Indices,
}

impl Default for ObjectStore {
    fn default() -> Self {
        Self {
            world: World::new(),
            indices: Indices::default(),
        }
    }
}

impl core::fmt::Debug for ObjectStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectStore")
            .field("prims", &self.len())
            .field("highest_local_id", &self.indices.highest_local_id)
            .finish_non_exhaustive()
    }
}

impl Clone for ObjectStore {
    fn clone(&self) -> Self {
        let mut copy = Self::new();
        for entity in self.indices.order.values() {
            let Some(body) = self.world.get::<Body>(*entity) else {
                continue;
            };
            let copied = copy.spawn(body.0.clone());
            let mut spawned = copy.world.entity_mut(copied);
            if let Some(contents) = self.world.get::<Contents>(*entity) {
                let _held = spawned.insert(contents.clone());
            }
            if let Some(cost) = self.world.get::<Cost>(*entity) {
                let _held = spawned.insert(cost.clone());
            }
            if let Some(history) = self.world.get::<History>(*entity) {
                let _held = spawned.insert(history.clone());
            }
        }
        copy.indices.highest_local_id = self.indices.highest_local_id;
        copy
    }
}

impl ObjectStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many prims the region holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.indices.order.len()
    }

    /// Whether the region holds no prim.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.order.is_empty()
    }

    /// The highest region-local id any prim here has had, the removed ones
    /// included.
    #[must_use]
    pub const fn highest_local_id(&self) -> RegionLocalObjectId {
        RegionLocalObjectId(self.indices.highest_local_id)
    }

    /// Spawns `object` and files it, returning its entity.
    fn spawn(&mut self, object: Object) -> Entity {
        let seq = Seq(self.indices.next_seq);
        self.indices.next_seq = self.indices.next_seq.saturating_add(1);
        let ids = Filed::of(&object);
        let entity = self.world.spawn((Body(object), seq)).id();
        let _first = self.indices.order.insert(seq, entity);
        self.indices.file(entity, seq, ids);
        entity
    }

    /// Adds `object` to the region.
    ///
    /// A prim whose region-local id or full id another prim here already has
    /// is stored, and walked over, but not found by that id: the prim that
    /// had it first keeps it, as the first of a `Vec` was the one a lookup
    /// found.
    pub fn push(&mut self, object: Object) {
        let _entity = self.spawn(object);
    }

    /// The entity of the prim with the given region-local id.
    fn entity(&self, local_id: RegionLocalObjectId) -> Option<Entity> {
        self.indices.by_local_id.get(&local_id).copied()
    }

    /// The prim with the given region-local id.
    #[must_use]
    pub fn get(&self, local_id: RegionLocalObjectId) -> Option<&Object> {
        self.world
            .get::<Body>(self.entity(local_id)?)
            .map(|body| &body.0)
    }

    /// The prim with the given full id.
    #[must_use]
    pub fn get_by_key(&self, full_id: ObjectKey) -> Option<&Object> {
        self.world
            .get::<Body>(*self.indices.by_key.get(&full_id)?)
            .map(|body| &body.0)
    }

    /// The prim with the given region-local id, to change in place.
    pub fn get_mut(&mut self, local_id: RegionLocalObjectId) -> Option<ObjectMut<'_>> {
        let entity = self.entity(local_id)?;
        let seq = *self.world.get::<Seq>(entity)?;
        let body = self.world.get_mut::<Body>(entity)?;
        Some(ObjectMut::new(body, &mut self.indices, entity, seq))
    }

    /// Whether a prim with the given region-local id is here.
    #[must_use]
    pub fn contains(&self, local_id: RegionLocalObjectId) -> bool {
        self.indices.by_local_id.contains_key(&local_id)
    }

    /// The prims hanging off `root`, in the store's order.
    pub fn children_of(&self, root: RegionLocalObjectId) -> impl Iterator<Item = &Object> + '_ {
        self.indices
            .children
            .get(&root)
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter_map(|entity| self.world.get::<Body>(*entity).map(|body| &body.0))
    }

    /// Removes the prim with the given region-local id, and everything the
    /// region kept of it, returning its record.
    pub fn remove(&mut self, local_id: RegionLocalObjectId) -> Option<Object> {
        let entity = self.entity(local_id)?;
        self.take(entity)
    }

    /// Takes the prim `entity` out of the region.
    fn take(&mut self, entity: Entity) -> Option<Object> {
        let seq = *self.world.get::<Seq>(entity)?;
        let body = self.world.get::<Body>(entity)?.0.clone();
        let _was = self.indices.order.remove(&seq);
        self.indices.unfile(entity, seq, Filed::of(&body));
        let _despawned = self.world.despawn(entity);
        Some(body)
    }

    /// Keeps only the prims `keep` says to keep, in order.
    pub fn retain(&mut self, mut keep: impl FnMut(&Object) -> bool) {
        let leaving: Vec<Entity> = self
            .indices
            .order
            .values()
            .copied()
            .filter(|entity| {
                self.world
                    .get::<Body>(*entity)
                    .is_some_and(|body| !keep(&body.0))
            })
            .collect();
        for entity in leaving {
            let _gone = self.take(entity);
        }
    }

    /// Changes every prim with `change`, in order, filing each again.
    pub fn for_each_mut(&mut self, mut change: impl FnMut(&mut Object)) {
        let prims: Vec<(Seq, Entity)> = self
            .indices
            .order
            .iter()
            .map(|(seq, entity)| (*seq, *entity))
            .collect();
        for (seq, entity) in prims {
            let Some(body) = self.world.get_mut::<Body>(entity) else {
                continue;
            };
            let mut held = ObjectMut::new(body, &mut self.indices, entity, seq);
            change(&mut held);
        }
    }

    /// The prims, in the order they entered the region.
    pub fn iter(&self) -> impl Iterator<Item = &Object> + '_ {
        self.indices
            .order
            .values()
            .filter_map(|entity| self.world.get::<Body>(*entity).map(|body| &body.0))
    }

    /// The first prim in the store's order.
    #[must_use]
    pub fn first(&self) -> Option<&Object> {
        self.iter().next()
    }

    /// The task inventory of the prim with the given region-local id, if
    /// anything was ever put in it.
    #[must_use]
    pub fn contents(&self, local_id: RegionLocalObjectId) -> Option<&TaskInventory> {
        self.world
            .get::<Contents>(self.entity(local_id)?)
            .map(|contents| &contents.0)
    }

    /// The task inventory of the prim with the given region-local id, to
    /// change; [`None`] for a prim that is not here or holds none yet.
    pub fn contents_mut(&mut self, local_id: RegionLocalObjectId) -> Option<&mut TaskInventory> {
        let entity = self.entity(local_id)?;
        self.world
            .get_mut::<Contents>(entity)
            .map(|contents| &mut contents.into_inner().0)
    }

    /// The task inventory of the prim with the given region-local id, to
    /// change, an empty one made where it holds none yet; [`None`] only for a
    /// prim that is not here.
    pub fn contents_or_empty(
        &mut self,
        local_id: RegionLocalObjectId,
    ) -> Option<&mut TaskInventory> {
        let entity = self.entity(local_id)?;
        if self.world.get::<Contents>(entity).is_none() {
            let _held = self
                .world
                .get_entity_mut(entity)
                .ok()?
                .insert(Contents(TaskInventory::default()));
        }
        self.contents_mut(local_id)
    }

    /// States the task inventory of the prim with the given region-local id,
    /// returning the one it replaced.
    ///
    /// # Errors
    ///
    /// `contents` itself, when no prim here has that id.
    pub fn set_contents(
        &mut self,
        local_id: RegionLocalObjectId,
        contents: TaskInventory,
    ) -> Result<Option<TaskInventory>, TaskInventory> {
        let Some(mut prim) = self
            .entity(local_id)
            .and_then(|entity| self.world.get_entity_mut(entity).ok())
        else {
            return Err(contents);
        };
        let replaced = prim.take::<Contents>().map(|held| held.0);
        let _held = prim.insert(Contents(contents));
        Ok(replaced)
    }

    /// What the prim with the given region-local id costs the region, if a
    /// fixture said.
    #[must_use]
    pub fn cost(&self, local_id: RegionLocalObjectId) -> Option<&ObjectCost> {
        self.world
            .get::<Cost>(self.entity(local_id)?)
            .map(|cost| &cost.0)
    }

    /// States what the prim with the given region-local id costs the region.
    ///
    /// # Errors
    ///
    /// `cost` itself, when no prim here has that id.
    pub fn set_cost(
        &mut self,
        local_id: RegionLocalObjectId,
        cost: ObjectCost,
    ) -> Result<(), ObjectCost> {
        let Some(mut prim) = self
            .entity(local_id)
            .and_then(|entity| self.world.get_entity_mut(entity).ok())
        else {
            return Err(cost);
        };
        let _held = prim.insert(Cost(cost));
        Ok(())
    }

    /// The edit history of the prim with the given region-local id, to
    /// change, an empty one made where it has none yet; [`None`] for a prim
    /// that is not here.
    pub(crate) fn history_mut(
        &mut self,
        local_id: RegionLocalObjectId,
    ) -> Option<&mut EditHistory> {
        let entity = self.entity(local_id)?;
        let mut prim = self.world.get_entity_mut(entity).ok()?;
        if prim.get::<History>().is_none() {
            let _held = prim.insert(History(EditHistory::default()));
        }
        self.world
            .get_mut::<History>(entity)
            .map(|history| &mut history.into_inner().0)
    }
}

impl<'store> IntoIterator for &'store ObjectStore {
    type Item = &'store Object;
    type IntoIter = Box<dyn Iterator<Item = &'store Object> + 'store>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl Extend<Object> for ObjectStore {
    fn extend<T: IntoIterator<Item = Object>>(&mut self, iter: T) {
        for object in iter {
            self.push(object);
        }
    }
}

impl FromIterator<Object> for ObjectStore {
    fn from_iter<T: IntoIterator<Item = Object>>(iter: T) -> Self {
        let mut store = Self::new();
        store.extend(iter);
        store
    }
}

impl From<Vec<Object>> for ObjectStore {
    fn from(objects: Vec<Object>) -> Self {
        objects.into_iter().collect()
    }
}

/// A prim being changed in place, through `bevy_ecs`'s [`Mut`] so the
/// world's change ticks see it. Dropping it files the prim again under any id
/// the change moved.
pub struct ObjectMut<'store> {
    /// The prim.
    body: Mut<'store, Body>,
    /// The store's indices, to file it again in.
    indices: &'store mut Indices,
    /// Which prim it is.
    entity: Entity,
    /// The order it came in.
    seq: Seq,
    /// The ids it was filed under when the change began.
    filed: Filed,
}

impl core::fmt::Debug for ObjectMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectMut")
            .field("entity", &self.entity)
            .field("body", &self.body.0)
            .finish_non_exhaustive()
    }
}

impl<'store> ObjectMut<'store> {
    /// A change to `body`, the prim `entity` that came in as `seq`, about to
    /// begin.
    fn new(
        body: Mut<'store, Body>,
        indices: &'store mut Indices,
        entity: Entity,
        seq: Seq,
    ) -> Self {
        let filed = Filed::of(&body.0);
        Self {
            body,
            indices,
            entity,
            seq,
            filed,
        }
    }
}

impl core::ops::Deref for ObjectMut<'_> {
    type Target = Object;

    fn deref(&self) -> &Object {
        &self.body.0
    }
}

impl core::ops::DerefMut for ObjectMut<'_> {
    fn deref_mut(&mut self) -> &mut Object {
        &mut self.body.0
    }
}

impl Drop for ObjectMut<'_> {
    fn drop(&mut self) {
        let now = Filed::of(&self.body.0);
        if now != self.filed {
            self.indices.unfile(self.entity, self.seq, self.filed);
            self.indices.file(self.entity, self.seq, now);
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::RegionLocalObjectId;
    use sl_types::key::{AgentKey, ObjectKey};
    use sl_types::lsl::Vector;

    use super::ObjectStore;
    use crate::world::{TaskInventory, box_prim};

    /// The origin, and a unit size.
    const ZERO: Vector = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// A box with the given region-local id and a full id made from it.
    fn prim(local_id: u32) -> sl_proto::Object {
        box_prim(
            RegionLocalObjectId(local_id),
            key(local_id),
            AgentKey::from(uuid::Uuid::from_u128(1)),
            ZERO,
            ZERO,
        )
    }

    /// The full id [`prim`] gives the prim `local_id`.
    fn key(local_id: u32) -> ObjectKey {
        ObjectKey::from(uuid::Uuid::from_u128(0x1000 + u128::from(local_id)))
    }

    /// A prim is found by either of its ids, and walked over in the order it
    /// came in, a removed one leaving no gap in the walk.
    #[test]
    fn a_prim_is_found_by_either_id_in_order() {
        let mut store: ObjectStore = (1..=4).map(prim).collect();
        assert_eq!(store.len(), 4);
        assert_eq!(
            store.get_by_key(key(3)).map(|object| object.local_id),
            Some(RegionLocalObjectId(3))
        );
        let _two = store.remove(RegionLocalObjectId(2));
        assert_eq!(store.get(RegionLocalObjectId(2)), None);
        assert_eq!(store.get_by_key(key(2)), None);
        store.push(prim(9));
        let order: Vec<u32> = store.iter().map(|object| object.local_id.0).collect();
        assert_eq!(order, vec![1, 3, 4, 9]);
        assert_eq!(store.len(), 4);
    }

    /// A change that moves a prim's ids files it under the new ones when the
    /// change is done, its root's children included.
    #[test]
    fn a_change_files_the_prim_again() {
        let mut store: ObjectStore = (1..=3).map(prim).collect();
        if let Some(mut child) = store.get_mut(RegionLocalObjectId(3)) {
            child.parent_id = RegionLocalObjectId(1);
        }
        let children: Vec<u32> = store
            .children_of(RegionLocalObjectId(1))
            .map(|object| object.local_id.0)
            .collect();
        assert_eq!(children, vec![3]);
        if let Some(mut child) = store.get_mut(RegionLocalObjectId(3)) {
            child.parent_id = RegionLocalObjectId(2);
            child.full_id = key(30);
        }
        assert_eq!(store.children_of(RegionLocalObjectId(1)).count(), 0);
        assert_eq!(store.children_of(RegionLocalObjectId(2)).count(), 1);
        assert_eq!(store.get_by_key(key(3)), None);
        assert_eq!(
            store.get_by_key(key(30)).map(|object| object.local_id),
            Some(RegionLocalObjectId(3))
        );
    }

    /// What the region keeps of a prim goes with it: a prim later given its
    /// id does not hold its contents.
    #[test]
    fn a_removed_prims_components_go_with_it() -> Result<(), &'static str> {
        let mut store: ObjectStore = std::iter::once(prim(4)).collect();
        let _none = store
            .set_contents(RegionLocalObjectId(4), TaskInventory::stated(3, Vec::new()))
            .map_err(|_contents| "the prim is not in the store")?;
        let _gone = store.remove(RegionLocalObjectId(4));
        store.push(prim(4));
        assert_eq!(store.contents(RegionLocalObjectId(4)), None);
        let absent = store.set_contents(RegionLocalObjectId(5), TaskInventory::default());
        assert_eq!(absent, Err(TaskInventory::default()));
        assert_eq!(store.highest_local_id(), RegionLocalObjectId(4));
        Ok(())
    }

    /// `retain` drops what it is told to and keeps the rest in order.
    #[test]
    fn retain_keeps_the_order() {
        let mut store: ObjectStore = (1..=5).map(prim).collect();
        store.retain(|object| object.local_id.0 % 2 == 1);
        let order: Vec<u32> = store.iter().map(|object| object.local_id.0).collect();
        assert_eq!(order, vec![1, 3, 5]);
        assert_eq!(store.get(RegionLocalObjectId(2)), None);
    }

    /// A lookup in a region of a thousand prims takes no longer than one in a
    /// region of ten: it is an index, not a walk.
    #[test]
    fn a_lookup_does_not_grow_with_the_region() {
        /// How many lookups each measurement makes.
        const LOOKUPS: u32 = 20_000;
        // The best of five, so a run preempted by a busy machine does not
        // count.
        let time = |prims: u32| {
            let store: ObjectStore = (1..=prims).map(prim).collect();
            let last = key(prims);
            std::iter::repeat_with(|| {
                let started = Instant::now();
                for _ in 0..LOOKUPS {
                    assert!(std::hint::black_box(&store).get_by_key(last).is_some());
                }
                started.elapsed()
            })
            .take(5)
            .min()
            .unwrap_or_default()
        };
        let small = time(10);
        let large = time(1_000);
        // A walk would make the large region a hundred times slower; an
        // index keeps it within noise. Ten times is far above the noise and
        // far below a walk.
        assert!(
            large < small.saturating_mul(10).max(Duration::from_millis(5)),
            "a lookup among 1000 prims took {large:?} against {small:?} among 10"
        );
    }
}
