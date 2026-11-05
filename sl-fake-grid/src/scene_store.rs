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
//!   it besides — its contents, what it costs the region, its edit history, a
//!   root's link order — are components of their own, so they leave with it;
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
//! # What changed
//!
//! [`ObjectStore::take_changed_contents`] names the prims whose contents were
//! written since it last answered — by the change tick `bevy_ecs` stamps a
//! component with when it is written through a [`Mut`] or inserted — so the
//! script engine follows what the region's prims hold without walking all of
//! them every tick. `ObjectStore::take_written` does the same for the prims'
//! records, for the update scheduler ([`crate::updates`]), which keeps beside
//! each prim what the region's viewers were last shown of it (`Shown`) to
//! tell a move from any other change.
//!
//! # Copies
//!
//! A [`World`] cannot be cloned, and a scenario's world is copied into every
//! region that uses it when the grid starts. [`Clone`] for the store spawns
//! the same prims, with the same components and in the same order, into a
//! fresh world.

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::change_detection::{DetectChanges as _, Tick};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::{Mut, Ref, World};
use sl_lsl_runtime::vm::seating::Seat;
use sl_proto::{Object, RegionLocalObjectId};
use sl_types::key::{AgentKey, ObjectKey};
use sl_types::lsl::{Rotation, Vector};

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

/// What the region's viewers were last shown of a prim: its record as the
/// update scheduler ([`crate::updates`]) last published it, or as a session
/// that changed it told the region. A prim the scheduler has not seen yet has
/// none, and its first update is a full one.
#[derive(Debug, Clone, Component)]
pub(crate) struct Shown(pub(crate) Object);

/// The order a prim came into the region in: the store walks prims by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Component)]
pub struct Seq(u64);

/// What a prim's scripts set of it as a seat: its sit target, its sitter's
/// camera. A prim no script set anything of has none, which reads as a seat
/// without a target.
#[derive(Debug, Clone, Component)]
pub struct SeatSettings(pub Seat);

/// What a prim's scripts last wrote of it, and the record that write gave it
/// ([`crate::prim_state`]). A prim no script wrote has none.
#[derive(Debug, Clone, Component)]
pub(crate) struct ScriptedPrim(pub(crate) crate::prim_state::Scripted);

/// One avatar sitting on a prim: where on it, and in which turn it sat.
#[derive(Debug, Clone, PartialEq)]
pub struct Sitter {
    /// The avatar.
    pub agent: AgentKey,
    /// Where it sits, from the prim's centre in the prim's frame — the
    /// position its own object update states, as a seated avatar's is.
    pub offset: Vector,
    /// Its turn, in the prim's frame.
    pub rotation: Rotation,
    /// Whether it took the prim's sit target, which a second avatar on the
    /// same prim does not (`llAvatarOnSitTarget`).
    pub on_target: bool,
    /// When it sat, among every sitter of the region: the link numbers a
    /// linkset's sitters take after its prims follow it.
    pub order: u64,
}

/// The avatars sitting on a prim, in the order they sat — the seat's half of
/// the index whose avatar's half the store keeps
/// ([`ObjectStore::seat_of`]). A prim nobody sits on has none.
#[derive(Debug, Clone, Default, Component)]
pub struct Sitters(pub Vec<Sitter>);

/// The order a linkset keeps its prims in, on its root
/// ([`crate::link_sets`]). A set nobody has linked or delinked has none and is
/// in the order its prims came into the region; a root that leaves takes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Component)]
pub(crate) struct LinkOrder {
    /// The children in the order of their link numbers, number 2 first,
    /// where a link or a delink set it.
    pub(crate) children: Option<Vec<RegionLocalObjectId>>,
    /// The group's part collection as OpenSim keeps it, the root included,
    /// where a link or a delink needed it.
    pub(crate) parts: Option<crate::link_sets::Parts>,
}

/// What a worn object's root is attached to: the wearer and the point, as
/// `llGetAttached` reads it. The object's record carries the same, as the
/// wire does — the wearer's avatar as its parent, the point in its state.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct AttachedTo {
    /// The avatar wearing it.
    pub agent: AgentKey,
    /// The attachment point.
    pub point: u8,
    /// Where the wearer stands in the region: what the object's region
    /// position is (`llGetPos`), and where it hears and speaks from. Set
    /// when it is put on and moved with its wearer's sits and stands, the
    /// only moves an avatar makes within a fake region.
    pub at: Vector,
}

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
    /// The prim each seated avatar sits on: the avatar's half of the index
    /// whose seat's half is each prim's [`Sitters`].
    seated: HashMap<AgentKey, Entity>,
    /// The roots each avatar wears, in the store's order: the wearer's half
    /// of the index whose object's half is each root's [`AttachedTo`].
    worn: HashMap<AgentKey, BTreeMap<Seq, Entity>>,
    /// The order the next avatar to sit is given.
    next_sit: u64,
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

    /// Takes the root that came in as `seq` off `agent`'s worn roots.
    fn unwear(&mut self, agent: AgentKey, seq: Seq) {
        if let Some(worn) = self.worn.get_mut(&agent) {
            let _gone = worn.remove(&seq);
            if worn.is_empty() {
                let _none = self.worn.remove(&agent);
            }
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
    /// The world's change tick when [`Self::take_changed_contents`] last
    /// answered: a contents write stamped later is one it has not named.
    contents_seen: Tick,
    /// The world's change tick when [`Self::take_written`] last answered.
    bodies_seen: Tick,
}

impl Default for ObjectStore {
    fn default() -> Self {
        Self {
            world: World::new(),
            indices: Indices::default(),
            contents_seen: Tick::new(0),
            bodies_seen: Tick::new(0),
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
            if let Some(seat) = self.world.get::<SeatSettings>(*entity) {
                let _held = spawned.insert(seat.clone());
            }
            if let Some(order) = self.world.get::<LinkOrder>(*entity) {
                let _held = spawned.insert(order.clone());
            }
            if let Some(scripted) = self.world.get::<ScriptedPrim>(*entity) {
                let _held = spawned.insert(scripted.clone());
            }
            // Who sits where, and who wears what, are the region's agents',
            // and a copy is a region nobody is in yet.
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
        // A prim that goes takes its sitters off it and its wearer's hold on
        // it: the region's own unseat and detach run first where they are
        // told of, and this keeps a removal they were not told of from
        // leaving the indices naming an entity that is gone.
        if let Some(sitters) = self.world.get::<Sitters>(entity) {
            for sitter in &sitters.0 {
                if self.indices.seated.get(&sitter.agent) == Some(&entity) {
                    let _gone = self.indices.seated.remove(&sitter.agent);
                }
            }
        }
        if let Some(attached) = self.world.get::<AttachedTo>(entity) {
            self.indices.unwear(attached.agent, seq);
        }
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

    /// The prims whose contents were written or stated since the last call,
    /// in the store's order — every prim holding contents, the first time.
    ///
    /// A write is one through [`Self::contents_mut`] or
    /// [`Self::contents_or_empty`] (whether or not it changed anything) or a
    /// [`Self::set_contents`]; a prim that left the region is not named.
    pub fn take_changed_contents(&mut self) -> Vec<RegionLocalObjectId> {
        // Writes from here on are stamped after `seen`.
        let seen = self.world.increment_change_tick();
        let now = self.world.change_tick();
        let since = self.contents_seen;
        let mut query = self.world.query::<(&Seq, &Body, Ref<'_, Contents>)>();
        let mut changed: Vec<(Seq, RegionLocalObjectId)> = query
            .iter(&self.world)
            .filter(|(_seq, _body, contents)| contents.last_changed().is_newer_than(since, now))
            .map(|(seq, body, _contents)| (*seq, body.0.local_id))
            .collect();
        changed.sort_unstable_by_key(|(seq, _local_id)| *seq);
        self.contents_seen = seen;
        changed
            .into_iter()
            .map(|(_seq, local_id)| local_id)
            .collect()
    }

    /// The prims whose record was written since the last call, in the store's
    /// order — every prim, the first time.
    ///
    /// A write is one through [`Self::get_mut`] or [`Self::for_each_mut`],
    /// whether or not it changed anything, or a prim's arrival; a prim that
    /// left the region is not named. Whether a write changed what a viewer
    /// sees is the update scheduler's question ([`Self::shown`]).
    pub(crate) fn take_written(&mut self) -> Vec<RegionLocalObjectId> {
        // Writes from here on are stamped after `seen`.
        let seen = self.world.increment_change_tick();
        let now = self.world.change_tick();
        let since = self.bodies_seen;
        let mut query = self.world.query::<(&Seq, Ref<'_, Body>)>();
        let mut written: Vec<(Seq, RegionLocalObjectId)> = query
            .iter(&self.world)
            .filter(|(_seq, body)| body.last_changed().is_newer_than(since, now))
            .map(|(seq, body)| (*seq, body.0.local_id))
            .collect();
        written.sort_unstable_by_key(|(seq, _local_id)| *seq);
        self.bodies_seen = seen;
        written
            .into_iter()
            .map(|(_seq, local_id)| local_id)
            .collect()
    }

    /// What the region's viewers were last shown of the prim with the given
    /// region-local id, or `None` for a prim the update scheduler has not seen.
    #[must_use]
    pub(crate) fn shown(&self, local_id: RegionLocalObjectId) -> Option<&Object> {
        self.world
            .get::<Shown>(self.entity(local_id)?)
            .map(|shown| &shown.0)
    }

    /// Records the prim with the given region-local id as shown as it now
    /// stands. Writing the record of what was shown is not a write of the
    /// prim: [`Self::take_written`] does not name it for this.
    pub(crate) fn mark_shown(&mut self, local_id: RegionLocalObjectId) {
        let Some(entity) = self.entity(local_id) else {
            return;
        };
        let Some(body) = self.world.get::<Body>(entity).map(|body| body.0.clone()) else {
            return;
        };
        let _held = self.world.entity_mut(entity).insert(Shown(body));
    }

    /// Changes what the region's viewers were shown of the prim with the
    /// given region-local id with `amend`: how a write the grid deliberately
    /// does not announce — a quiet flag — keeps the update scheduler from
    /// announcing it later, without hiding anything else written to the prim
    /// in the same tick. A prim with nothing shown yet is left so, its first
    /// update being a full one anyway.
    pub(crate) fn amend_shown(
        &mut self,
        local_id: RegionLocalObjectId,
        amend: impl FnOnce(&mut Object),
    ) {
        let Some(entity) = self.entity(local_id) else {
            return;
        };
        if let Some(mut shown) = self.world.get_mut::<Shown>(entity) {
            amend(&mut shown.0);
        }
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

    /// The seat settings of the prim with the given region-local id, where a
    /// script or a fixture set any.
    #[must_use]
    pub fn seat_settings(&self, local_id: RegionLocalObjectId) -> Option<&Seat> {
        self.world
            .get::<SeatSettings>(self.entity(local_id)?)
            .map(|seat| &seat.0)
    }

    /// The seat settings of the prim with the given region-local id, to
    /// change, empty ones made where it has none yet; [`None`] for a prim that
    /// is not here.
    pub fn seat_settings_mut(&mut self, local_id: RegionLocalObjectId) -> Option<&mut Seat> {
        let entity = self.entity(local_id)?;
        let mut prim = self.world.get_entity_mut(entity).ok()?;
        if prim.get::<SeatSettings>().is_none() {
            let _held = prim.insert(SeatSettings(Seat::default()));
        }
        self.world
            .get_mut::<SeatSettings>(entity)
            .map(|seat| &mut seat.into_inner().0)
    }

    /// The last prim a script wrote as the prim with the given region-local
    /// id, and the record it wrote, where a script wrote one.
    #[must_use]
    pub(crate) fn scripted(
        &self,
        local_id: RegionLocalObjectId,
    ) -> Option<&crate::prim_state::Scripted> {
        self.world
            .get::<ScriptedPrim>(self.entity(local_id)?)
            .map(|scripted| &scripted.0)
    }

    /// Keeps `scripted` as what a script last wrote as the prim with the
    /// given region-local id.
    pub(crate) fn set_scripted(
        &mut self,
        local_id: RegionLocalObjectId,
        scripted: crate::prim_state::Scripted,
    ) {
        let Some(entity) = self.entity(local_id) else {
            return;
        };
        if let Ok(mut prim) = self.world.get_entity_mut(entity) {
            let _held = prim.insert(ScriptedPrim(scripted));
        }
    }

    /// The order the linkset whose root has the given region-local id keeps
    /// its prims in, where a link or a delink set one.
    #[must_use]
    pub(crate) fn link_order(&self, root: RegionLocalObjectId) -> Option<&LinkOrder> {
        self.world.get::<LinkOrder>(self.entity(root)?)
    }

    /// The order of the linkset whose root has the given region-local id, to
    /// change, an empty one made where it has none yet; [`None`] for a prim
    /// that is not here.
    pub(crate) fn link_order_mut(&mut self, root: RegionLocalObjectId) -> Option<&mut LinkOrder> {
        let entity = self.entity(root)?;
        let mut prim = self.world.get_entity_mut(entity).ok()?;
        if prim.get::<LinkOrder>().is_none() {
            let _held = prim.insert(LinkOrder::default());
        }
        self.world.get_mut::<LinkOrder>(entity).map(Mut::into_inner)
    }

    /// Forgets the order kept for the prim with the given region-local id:
    /// it became a group of its own, which starts with a fresh collection.
    pub(crate) fn forget_link_order(&mut self, root: RegionLocalObjectId) {
        if let Some(entity) = self.entity(root)
            && let Ok(mut prim) = self.world.get_entity_mut(entity)
        {
            let _forgotten = prim.take::<LinkOrder>();
        }
    }

    /// The avatars sitting on the prim with the given region-local id, in
    /// the order they sat; empty for a prim nobody sits on or that is not
    /// here.
    #[must_use]
    pub fn sitters(&self, local_id: RegionLocalObjectId) -> &[Sitter] {
        self.entity(local_id)
            .and_then(|entity| self.world.get::<Sitters>(entity))
            .map_or(&[], |sitters| sitters.0.as_slice())
    }

    /// Seats `agent` on the prim with the given region-local id at `offset`,
    /// turned by `rotation`, on its sit target or beside it, standing it up
    /// from wherever it sat before; the sitter as stored, or [`None`] for a
    /// prim that is not here.
    pub fn sit(
        &mut self,
        agent: AgentKey,
        local_id: RegionLocalObjectId,
        (offset, rotation): (Vector, Rotation),
        on_target: bool,
    ) -> Option<Sitter> {
        let entity = self.entity(local_id)?;
        let _before = self.stand(agent);
        let sitter = Sitter {
            agent,
            offset,
            rotation,
            on_target,
            order: self.indices.next_sit,
        };
        self.indices.next_sit = self.indices.next_sit.saturating_add(1);
        let mut prim = self.world.get_entity_mut(entity).ok()?;
        if let Some(mut sitters) = prim.get_mut::<Sitters>() {
            sitters.0.push(sitter.clone());
        } else {
            let _held = prim.insert(Sitters(vec![sitter.clone()]));
        }
        let _was = self.indices.seated.insert(agent, entity);
        Some(sitter)
    }

    /// Stands `agent` up: the prim it sat on and where on it, or [`None`]
    /// when it sat nowhere here.
    pub fn stand(&mut self, agent: AgentKey) -> Option<(RegionLocalObjectId, Sitter)> {
        let entity = self.indices.seated.remove(&agent)?;
        let local_id = self.world.get::<Body>(entity)?.0.local_id;
        let mut prim = self.world.get_entity_mut(entity).ok()?;
        let (sitter, emptied) = {
            let mut sitters = prim.get_mut::<Sitters>()?;
            let index = sitters.0.iter().position(|sitter| sitter.agent == agent)?;
            let sitter = sitters.0.remove(index);
            (sitter, sitters.0.is_empty())
        };
        if emptied {
            let _empty = prim.take::<Sitters>();
        }
        Some((local_id, sitter))
    }

    /// The prim `agent` sits on and where on it, or [`None`] when it sits
    /// nowhere here.
    #[must_use]
    pub fn seat_of(&self, agent: AgentKey) -> Option<(&Object, &Sitter)> {
        let entity = *self.indices.seated.get(&agent)?;
        let body = &self.world.get::<Body>(entity)?.0;
        let sitter = self
            .world
            .get::<Sitters>(entity)?
            .0
            .iter()
            .find(|sitter| sitter.agent == agent)?;
        Some((body, sitter))
    }

    /// Puts the root with the given region-local id on `attached.agent`'s
    /// `attached.point`, taking it off whoever wore it before; `false` for a
    /// prim that is not here. The record's parent and state are the caller's
    /// to set, as the wire states them.
    pub fn wear(&mut self, local_id: RegionLocalObjectId, attached: AttachedTo) -> bool {
        let Some(entity) = self.entity(local_id) else {
            return false;
        };
        let Some(seq) = self.world.get::<Seq>(entity).copied() else {
            return false;
        };
        let _before = self.take_off(local_id);
        let Ok(mut prim) = self.world.get_entity_mut(entity) else {
            return false;
        };
        let agent = attached.agent;
        let _held = prim.insert(attached);
        let _new = self
            .indices
            .worn
            .entry(agent)
            .or_default()
            .insert(seq, entity);
        true
    }

    /// `agent` now stands at `at`: what it wears is there too.
    pub fn move_wearer(&mut self, agent: AgentKey, at: &Vector) {
        let worn: Vec<Entity> = self
            .indices
            .worn
            .get(&agent)
            .into_iter()
            .flat_map(BTreeMap::values)
            .copied()
            .collect();
        for entity in worn {
            if let Some(mut attached) = self.world.get_mut::<AttachedTo>(entity) {
                attached.at = at.clone();
            }
        }
    }

    /// Takes the root with the given region-local id off its wearer: who wore
    /// it where, or [`None`] for one nobody wears.
    pub fn take_off(&mut self, local_id: RegionLocalObjectId) -> Option<AttachedTo> {
        let entity = self.entity(local_id)?;
        let seq = *self.world.get::<Seq>(entity)?;
        let attached = self
            .world
            .get_entity_mut(entity)
            .ok()?
            .take::<AttachedTo>()?;
        self.indices.unwear(attached.agent, seq);
        Some(attached)
    }

    /// Who wears the root with the given region-local id, on which point;
    /// [`None`] for one nobody wears, a child among them.
    #[must_use]
    pub fn attached_to(&self, local_id: RegionLocalObjectId) -> Option<&AttachedTo> {
        self.world.get::<AttachedTo>(self.entity(local_id)?)
    }

    /// The roots `agent` wears, in the store's order.
    #[must_use]
    pub fn worn_by(&self, agent: AgentKey) -> Vec<RegionLocalObjectId> {
        self.indices
            .worn
            .get(&agent)
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter_map(|entity| self.world.get::<Body>(*entity).map(|body| body.0.local_id))
            .collect()
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

    /// A prim is named once for each round of contents writes: every prim
    /// holding contents the first time, then only those written since.
    #[test]
    fn changed_contents_are_named_once_per_write() -> Result<(), &'static str> {
        let mut store: ObjectStore = (1..=3).map(prim).collect();
        for local_id in [1, 3] {
            let _none = store
                .set_contents(RegionLocalObjectId(local_id), TaskInventory::default())
                .map_err(|_contents| "the prim is not in the store")?;
        }
        assert_eq!(
            store.take_changed_contents(),
            vec![RegionLocalObjectId(1), RegionLocalObjectId(3)]
        );
        assert_eq!(store.take_changed_contents(), Vec::new());
        store
            .contents_mut(RegionLocalObjectId(3))
            .ok_or("no contents")?
            .touch();
        let _made = store.contents_or_empty(RegionLocalObjectId(2));
        assert_eq!(
            store.take_changed_contents(),
            vec![RegionLocalObjectId(2), RegionLocalObjectId(3)]
        );
        // A copy names everything again: it is a fresh world.
        assert_eq!(store.clone().take_changed_contents().len(), 3);
        Ok(())
    }

    /// A prim is named once for each round of writes to its record, every
    /// prim the first time, and recording what was shown of it is not one.
    #[test]
    fn written_prims_are_named_once_per_write() -> Result<(), &'static str> {
        let mut store: ObjectStore = (1..=3).map(prim).collect();
        assert_eq!(
            store.take_written(),
            vec![
                RegionLocalObjectId(1),
                RegionLocalObjectId(2),
                RegionLocalObjectId(3)
            ]
        );
        assert_eq!(store.take_written(), Vec::new());
        store
            .get_mut(RegionLocalObjectId(3))
            .ok_or("no prim")?
            .motion
            .position
            .x = 9.0;
        store.mark_shown(RegionLocalObjectId(2));
        store.push(prim(4));
        assert_eq!(
            store.take_written(),
            vec![RegionLocalObjectId(3), RegionLocalObjectId(4)]
        );
        assert_eq!(store.shown(RegionLocalObjectId(3)), None);
        store.mark_shown(RegionLocalObjectId(3));
        assert_eq!(
            store
                .shown(RegionLocalObjectId(3))
                .map(|shown| shown.motion.position.x),
            Some(9.0)
        );
        store.amend_shown(RegionLocalObjectId(3), |shown| shown.update_flags = 7);
        assert_eq!(
            store
                .shown(RegionLocalObjectId(3))
                .map(|shown| shown.update_flags),
            Some(7)
        );
        assert_eq!(store.take_written(), Vec::new());
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
