//! What the session knows about the regions it is streaming, in one place.
//!
//! Nine of [`Session`](crate::Session)'s fields were per-circuit mirrors of the
//! simulator's world — the objects, the terrain patches, the region handle and
//! flags, the parcels, the time dilation, the agent's own avatar id, the
//! outstanding parent re-asks, and which circuit answered a script request —
//! each maintained by hand at its own call sites, and each having to be
//! remembered again in the two places that drop a region. They are
//! [`WorldCache`] now, and the two lifecycle operations that must touch all of
//! them at once ([`WorldCache::reset`] and [`WorldCache::forget_circuit`]) are
//! methods rather than a checklist.

use super::ParentRequest;
use crate::scoped_id::{CircuitId, ScopedObjectId};
use crate::types::{Object, ParcelInfo, TerrainPatch};
use sl_types::key::ObjectKey;
use sl_wire::{RegionHandle, RegionLocalObjectId, RegionLocalParcelId};
use std::collections::BTreeMap;
use std::time::Instant;

/// One region's object cache, keyed by region-local id.
pub(crate) type SimObjects = BTreeMap<RegionLocalObjectId, Object>;

/// One region's terrain patches, keyed by `(layer code, patch x, patch y)`.
pub(crate) type SimTerrain = BTreeMap<(u8, u32, u32), TerrainPatch>;

/// One region's parcels, keyed by region-local parcel id.
pub(crate) type SimParcels = BTreeMap<RegionLocalParcelId, ParcelInfo>;

/// The per-circuit mirror of every region the session is streaming: the root
/// region and each neighbour a child circuit is open to.
///
/// Everything here is keyed by [`CircuitId`] rather than by region handle, so a
/// circuit that is torn down and re-opened to the same region starts with a
/// clean cache instead of aliasing what the previous instance held.
#[derive(Debug)]
pub(crate) struct WorldCache {
    /// The scene-graph cache per circuit: every object the simulator has sent an
    /// `ObjectUpdate` for, kept current by the terse motion updates.
    objects: BTreeMap<CircuitId, SimObjects>,
    /// Linkset roots a tracked child names but the simulator has not sent,
    /// with the re-ask bookkeeping for each.
    requested_parents: BTreeMap<ScopedObjectId, ParentRequest>,
    /// The terrain patches per circuit, every layer (LAND / WATER / WIND /
    /// CLOUD).
    terrain: BTreeMap<CircuitId, SimTerrain>,
    /// The region handle each circuit's region sits at, learned from
    /// `EnableSimulator`, the login response, a teleport, or an object update.
    regions: BTreeMap<CircuitId, RegionHandle>,
    /// The raw `RegionFlags` each region's `RegionHandshake` carried.
    region_flags: BTreeMap<CircuitId, u32>,
    /// The parcels each region has pushed, keyed by region-local parcel id.
    parcels: BTreeMap<CircuitId, SimParcels>,
    /// The last raw time-dilation value seen per circuit, so a steady region
    /// does not re-emit the event on every object update.
    time_dilation: BTreeMap<CircuitId, u16>,
    /// The agent's own avatar's region-local id per circuit, set once: the id is
    /// stable for the life of a circuit.
    own_avatar: BTreeMap<CircuitId, RegionLocalObjectId>,
    /// Which circuit an object's script request arrived on, so the reply goes
    /// back to the simulator that asked.
    script_request_circuits: BTreeMap<ObjectKey, CircuitId>,
}

impl WorldCache {
    /// An empty cache: nothing streamed yet. `const`, because
    /// [`Session::new`](crate::Session::new) is.
    pub(crate) const fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
            requested_parents: BTreeMap::new(),
            terrain: BTreeMap::new(),
            regions: BTreeMap::new(),
            region_flags: BTreeMap::new(),
            parcels: BTreeMap::new(),
            time_dilation: BTreeMap::new(),
            own_avatar: BTreeMap::new(),
            script_request_circuits: BTreeMap::new(),
        }
    }

    /// Drops everything: the world the session was streaming is gone, either
    /// because a fresh login is starting or because a distant teleport reset it.
    ///
    /// Every circuit the cache held is being torn down in both cases — the
    /// destination of a distant teleport is a freshly minted circuit — so no
    /// entry here can be reached again.
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Drops everything scoped to one retiring circuit, returning its object
    /// cache so the caller can announce each object's removal.
    ///
    /// The one operation that has to touch every map at once: a store forgotten
    /// here is a store that outlives its region, which is how a stale minimap
    /// dot or an unresolvable parent re-ask happens.
    pub(crate) fn forget_circuit(&mut self, circuit: CircuitId) -> SimObjects {
        self.terrain.remove(&circuit);
        self.regions.remove(&circuit);
        self.region_flags.remove(&circuit);
        self.parcels.remove(&circuit);
        self.time_dilation.remove(&circuit);
        self.own_avatar.remove(&circuit);
        self.script_request_circuits
            .retain(|_object, owner| *owner != circuit);
        // Outstanding parent re-asks are scoped to this circuit's region-local
        // ids, so they go stale with it.
        self.requested_parents
            .retain(|parent, _request| parent.circuit != circuit);
        self.objects.remove(&circuit).unwrap_or_default()
    }

    /// Records the region handle `circuit`'s region sits at.
    pub(crate) fn note_region(&mut self, circuit: CircuitId, handle: RegionHandle) {
        self.regions.insert(circuit, handle);
    }

    /// The region handle `circuit`'s region sits at, if known.
    pub(crate) fn region_handle(&self, circuit: CircuitId) -> Option<RegionHandle> {
        self.regions.get(&circuit).copied()
    }

    /// Records the raw `RegionFlags` `circuit`'s `RegionHandshake` carried.
    pub(crate) fn note_region_flags(&mut self, circuit: CircuitId, flags: u32) {
        self.region_flags.insert(circuit, flags);
    }

    /// The raw `RegionFlags` of `circuit`'s region, if its handshake has been
    /// seen.
    pub(crate) fn region_flags(&self, circuit: CircuitId) -> Option<u32> {
        self.region_flags.get(&circuit).copied()
    }

    /// Records the raw time dilation for `circuit`, returning `true` when it
    /// differs from the last value seen (the caller emits the event then).
    pub(crate) fn note_time_dilation(&mut self, circuit: CircuitId, raw: u16) -> bool {
        self.time_dilation.insert(circuit, raw) != Some(raw)
    }

    /// Records the agent's own avatar's region-local id on `circuit` the first
    /// time it is observed; a later observation never overwrites it.
    pub(crate) fn note_own_avatar(&mut self, circuit: CircuitId, local_id: RegionLocalObjectId) {
        self.own_avatar.entry(circuit).or_insert(local_id);
    }

    /// The agent's own avatar's region-local id on `circuit`, once observed.
    pub(crate) fn own_avatar(&self, circuit: CircuitId) -> Option<RegionLocalObjectId> {
        self.own_avatar.get(&circuit).copied()
    }

    /// Folds a `ParcelProperties` into `circuit`'s parcel cache.
    pub(crate) fn note_parcel(&mut self, circuit: CircuitId, parcel: &ParcelInfo) {
        self.parcels
            .entry(circuit)
            .or_default()
            .insert(parcel.local_id, parcel.clone());
    }

    /// The parcels `circuit`'s region has pushed.
    pub(crate) fn parcels_in(&self, circuit: CircuitId) -> Option<&SimParcels> {
        self.parcels.get(&circuit)
    }

    /// Remembers which circuit an object's script request arrived on.
    pub(crate) fn note_script_request_circuit(&mut self, object: ObjectKey, circuit: CircuitId) {
        self.script_request_circuits.insert(object, circuit);
    }

    /// The circuit `object`'s script request arrived on, if one has.
    pub(crate) fn script_request_circuit(&self, object: ObjectKey) -> Option<CircuitId> {
        self.script_request_circuits.get(&object).copied()
    }

    /// `circuit`'s object cache, if that circuit has streamed anything.
    pub(crate) fn objects_in(&self, circuit: CircuitId) -> Option<&SimObjects> {
        self.objects.get(&circuit)
    }

    /// `circuit`'s object cache for modification, if that circuit has streamed
    /// anything.
    pub(crate) fn objects_in_mut(&mut self, circuit: CircuitId) -> Option<&mut SimObjects> {
        self.objects.get_mut(&circuit)
    }

    /// `circuit`'s object cache, created empty if this is its first object.
    pub(crate) fn objects_in_or_default(&mut self, circuit: CircuitId) -> &mut SimObjects {
        self.objects.entry(circuit).or_default()
    }

    /// Every cached object, across every circuit.
    pub(crate) fn objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.values().flat_map(BTreeMap::values)
    }

    /// The cached object `id` names, resolved against that exact circuit
    /// instance.
    pub(crate) fn object(&self, id: ScopedObjectId) -> Option<&Object> {
        self.objects.get(&id.circuit)?.get(&id.id)
    }

    /// How many cached objects name `parent` as their linkset root — the
    /// orphans that are waiting on it, and the reason to keep re-asking.
    pub(crate) fn orphans_of(&self, parent: ScopedObjectId) -> usize {
        self.objects.get(&parent.circuit).map_or(0, |sim| {
            sim.values()
                .filter(|object| object.parent_id == parent.id)
                .count()
        })
    }

    /// `circuit`'s terrain patches, created empty on first use.
    pub(crate) fn terrain_in_or_default(&mut self, circuit: CircuitId) -> &mut SimTerrain {
        self.terrain.entry(circuit).or_default()
    }

    /// `circuit`'s terrain patches, if any have arrived.
    pub(crate) fn terrain_in(&self, circuit: CircuitId) -> Option<&SimTerrain> {
        self.terrain.get(&circuit)
    }

    /// Every cached terrain patch, across every circuit and layer.
    pub(crate) fn terrain_patches(&self) -> impl Iterator<Item = &TerrainPatch> {
        self.terrain.values().flat_map(BTreeMap::values)
    }

    /// Starts re-asking for the linkset root `parent`, unless it is already
    /// being asked for. Returns `true` when this is the first ask, which is the
    /// one the caller sends immediately.
    pub(crate) fn request_parent(&mut self, parent: ScopedObjectId, now: Instant) -> bool {
        if let std::collections::btree_map::Entry::Vacant(slot) =
            self.requested_parents.entry(parent)
        {
            let _request = slot.insert(ParentRequest::first(now));
            return true;
        }
        false
    }

    /// Stops re-asking for `parent`: it arrived, or nothing names it any more.
    pub(crate) fn forget_parent_request(&mut self, parent: ScopedObjectId) {
        let _forgotten = self.requested_parents.remove(&parent);
    }

    /// The parents whose next re-ask is due at `now`.
    pub(crate) fn parent_requests_due(&self, now: Instant) -> Vec<ScopedObjectId> {
        self.requested_parents
            .iter()
            .filter(|(_parent, request)| now >= request.next_ask())
            .map(|(parent, _request)| *parent)
            .collect()
    }

    /// The re-ask bookkeeping for `parent`, to stamp an ask onto.
    pub(crate) fn parent_request_mut(
        &mut self,
        parent: ScopedObjectId,
    ) -> Option<&mut ParentRequest> {
        self.requested_parents.get_mut(&parent)
    }

    /// The earliest instant at which some parent is due to be re-asked for.
    pub(crate) fn next_parent_reask(&self) -> Option<Instant> {
        self.requested_parents
            .values()
            .map(ParentRequest::next_ask)
            .min()
    }
}
