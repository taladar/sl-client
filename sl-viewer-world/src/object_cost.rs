//! Shared **object land-impact** model — the `GetObjectCost` capability's
//! per-linkset resource cost (`ObjectCost.linked_set_resource_cost`), the number
//! the reference shows as an object's *Land Impact*.
//!
//! Both the in-world hover tooltip (`crate::hover_tooltip`) and the build /
//! edit floater (`crate::edit_params`) want it, so it lives in **one**
//! resource with **one** `ingest_object_costs` reader. The whole point is to
//! not spam requests: a value is requested **once** and every surface shares the
//! result. That is modelled as an explicit state machine rather than an
//! `Option`, so the distinct situations are represented rather than conflated:
//!
//! - [`LandImpact::CapUnavailable`] — the region seed omits the `GetObjectCost`
//!   cap (e.g. plain OpenSim); a request would be a silent no-op, so none is
//!   sent and no line is shown.
//! - [`LandImpact::NotRequested`] — no surface has asked yet.
//! - [`LandImpact::Pending`] — a request is in flight; **no** surface re-sends
//!   while it is (this is the anti-spam guard, and it holds across a lost /
//!   errored reply — the value simply stays `Pending` rather than re-requesting
//!   in a loop).
//! - [`LandImpact::Known`] — the reply landed.
//!
//! The single request is driven through [`ObjectCostModel::resolve`], which
//! advances `NotRequested → Pending` and sends the command exactly once. A
//! surface that only wants to *read* the current state (without triggering a
//! request) uses `ObjectCostModel::land_impact`.
//!
//! Note: the `GetObjectCost` reply carries only successful `(id, cost)` rows —
//! an object the simulator errors on is simply omitted, so an *error* is not
//! distinguishable from a still-in-flight `Pending` without surfacing the reply
//! layer's error array (not modelled here; it would be a wire-layer change).

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use sl_client_bevy::{
    CAP_GET_OBJECT_COST, Command, Object, ObjectKey, PrimShapeParams, RegionLocalObjectId,
    SlCapabilities, SlCommand, SlEvent, SlSessionEvent,
};

use crate::world_api::ObjectState;

/// The state of a linkset's land impact in the shared model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LandImpact {
    /// The region seed has no `GetObjectCost` cap — it cannot be requested.
    CapUnavailable,
    /// No surface has requested it yet.
    NotRequested,
    /// A request is in flight; awaiting the `GetObjectCost` reply.
    Pending,
    /// The reply carried the linkset's resource cost.
    Known(f32),
}

/// The shared land-impact cache + request state, keyed by linkset root.
#[derive(Resource, Debug, Default)]
pub struct ObjectCostModel {
    /// Whether the current region advertises the `GetObjectCost` cap (folded
    /// from [`SlCapabilities`]; `false` until the seed caps arrive).
    cap_available: bool,
    /// The resolved linkset land impact (`linked_set_resource_cost`) by root.
    known: HashMap<ObjectKey, f32>,
    /// Roots whose request is in flight (the [`LandImpact::Pending`] set).
    pending: HashSet<ObjectKey>,
}

impl ObjectCostModel {
    /// The current land-impact state for `root`, **without** issuing a request
    /// (a read-only peek).
    pub(crate) fn land_impact(&self, root: ObjectKey) -> LandImpact {
        if let Some(cost) = self.known.get(&root) {
            LandImpact::Known(*cost)
        } else if !self.cap_available {
            LandImpact::CapUnavailable
        } else if self.pending.contains(&root) {
            LandImpact::Pending
        } else {
            LandImpact::NotRequested
        }
    }

    /// Drop any cached / in-flight cost for `root`, so the next
    /// [`resolve`](Self::resolve) re-requests it — used when an edit changes the
    /// linkset's land impact (a relink, or a prim scale / shape change).
    pub fn invalidate(&mut self, root: ObjectKey) {
        self.known.remove(&root);
        self.pending.remove(&root);
    }

    /// Drop the whole cache — used on a relink, which can change the land impact
    /// of more than one linkset at once (and the cache only holds the few
    /// objects a surface has actually shown, so this is cheap).
    fn invalidate_all(&mut self) {
        self.known.clear();
        self.pending.clear();
    }

    /// The land impact for `root`, sending a **single** `GetObjectCost` request
    /// the first time it is wanted. A no-op (no command) once cached, already
    /// pending, or the cap is absent — so several surfaces wanting the same
    /// object share one request and nothing re-sends while a reply is awaited.
    pub fn resolve(
        &mut self,
        root: ObjectKey,
        commands: &mut MessageWriter<SlCommand>,
    ) -> LandImpact {
        match self.land_impact(root) {
            LandImpact::NotRequested => {
                self.pending.insert(root);
                commands.write(SlCommand(Command::RequestObjectCost {
                    object_ids: vec![root],
                }));
                LandImpact::Pending
            }
            other => other,
        }
    }
}

/// Track whether the current region advertises the `GetObjectCost` cap, from
/// the [`SlCapabilities`] the session emits once the seed caps resolve.
pub(crate) fn ingest_capabilities(
    mut capabilities: MessageReader<SlCapabilities>,
    mut model: ResMut<ObjectCostModel>,
) {
    for SlCapabilities(map) in capabilities.read() {
        model.cap_available = map.contains_key(CAP_GET_OBJECT_COST);
    }
}

/// Fold every [`SlSessionEvent::ObjectCosts`] reply into the shared cache,
/// keyed by object root, clearing the in-flight (`Pending`) guard.
pub(crate) fn ingest_object_costs(
    mut events: MessageReader<SlEvent>,
    mut model: ResMut<ObjectCostModel>,
) {
    for event in events.read() {
        if let SlSessionEvent::ObjectCosts(costs) = &event.0 {
            for (object_id, cost) in costs {
                model.pending.remove(object_id);
                model
                    .known
                    .insert(*object_id, cost.linked_set_resource_cost);
            }
        }
    }
}

/// The land-impact-affecting fingerprint of one prim: its scale, its shape and
/// its linkset parent. Land impact changes when any of these change (a mesh's
/// scale, a shape/physics edit, or a relink), but **not** when the prim merely
/// moves — so comparing it drops the stale cost without re-requesting on every
/// position update (the anti-spam guard extends to edits).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CostFingerprint {
    /// The prim's scale.
    scale: [f32; 3],
    /// The prim's shape parameters.
    shape: PrimShapeParams,
    /// The prim's linkset parent (0 for a root) — changes on link / unlink.
    parent: RegionLocalObjectId,
}

impl CostFingerprint {
    /// The fingerprint of an object from its decoded update.
    const fn of(object: &Object) -> Self {
        Self {
            scale: [object.scale.x, object.scale.y, object.scale.z],
            shape: object.shape,
            parent: object.parent_id,
        }
    }
}

/// Invalidate cached costs whose linkset's land impact an edit changed: a prim
/// scale / shape change drops just that prim's linkset-root cost, while a relink
/// (a changed parent) clears the cache (it can re-cost several linksets). Runs
/// after [`crate::objects::update_objects`] so the linkset lookups see the new
/// structure. Position-only (terse) updates leave the fingerprint unchanged and
/// so never invalidate — the point of the fingerprint.
fn invalidate_stale_costs(
    mut events: MessageReader<SlEvent>,
    state: Res<ObjectState>,
    mut model: ResMut<ObjectCostModel>,
    mut seen: Local<HashMap<ObjectKey, CostFingerprint>>,
) {
    for event in events.read() {
        let object = match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => object,
            _other => continue,
        };
        let fingerprint = CostFingerprint::of(object);
        let previous = seen.insert(object.full_id, fingerprint);
        match previous {
            // Unchanged geometry / linkset (e.g. a terse move): nothing to do.
            Some(previous) if previous == fingerprint => {}
            // A relink: the land impact of more than one linkset can change.
            Some(previous) if previous.parent != fingerprint.parent => model.invalidate_all(),
            // A scale / shape change (or first sighting): re-cost just this
            // prim's linkset root.
            _other => {
                let scoped = object.scoped_id();
                let root = state.linkset_root_of(&scoped).unwrap_or(scoped);
                if let Some(root_key) = state.full_key(&root) {
                    model.invalidate(root_key);
                }
            }
        }
    }
}

/// The shared object-cost plugin: the [`ObjectCostModel`] resource, the cap
/// tracker, the reply reader, and the edit-invalidation pass.
#[derive(Debug, Default)]
pub struct ObjectCostPlugin;

impl Plugin for ObjectCostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObjectCostModel>().add_systems(
            Update,
            (
                ingest_capabilities,
                ingest_object_costs,
                invalidate_stale_costs.after(crate::objects::update_objects),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{LandImpact, ObjectCostModel};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ObjectKey, Uuid};

    fn key(n: u128) -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(n))
    }

    /// Without the cap, the state is `CapUnavailable` and never becomes pending.
    #[test]
    fn no_cap_is_unavailable() {
        let model = ObjectCostModel::default();
        assert_eq!(model.land_impact(key(1)), LandImpact::CapUnavailable);
    }

    /// With the cap, an unrequested root reads `NotRequested`, a reply makes it
    /// `Known`, and the pending guard clears.
    #[test]
    fn cap_then_reply_transitions() {
        let mut model = ObjectCostModel {
            cap_available: true,
            ..ObjectCostModel::default()
        };
        assert_eq!(model.land_impact(key(1)), LandImpact::NotRequested);
        model.pending.insert(key(1));
        assert_eq!(model.land_impact(key(1)), LandImpact::Pending);
        model.pending.remove(&key(1));
        model.known.insert(key(1), 7.0);
        assert_eq!(model.land_impact(key(1)), LandImpact::Known(7.0));
    }
}
