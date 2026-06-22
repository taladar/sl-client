//! The **`GetObjectCost`** and **`ResourceCostSelected`** capabilities: an
//! object's land-impact / physics "accounting" costs.
//!
//! The viewer's build tools and the "more info" object panel show how much of a
//! region's resource budget an object consumes. Two capabilities feed that
//! display:
//!
//! - `GetObjectCost` — POST `{ object_ids: [uuid, …] }`; the reply is a map keyed
//!   by each object's UUID, each value carrying the per-part and whole-linkset
//!   "resource" (land-impact) and physics costs.
//! - `ResourceCostSelected` — POST `{ selected_roots: [uuid, …] }` (or
//!   `selected_prims`); the reply sums the current selection's physics,
//!   streaming, and simulation costs under a single `selected` map.
//!
//! This module builds the request bodies and decodes the replies (client side),
//! and parses the requests and builds the replies (server side). The LLSD keys
//! are cross-checked against the Firestorm viewer's
//! `indra/newview/llaccountingcostmanager.cpp` and OpenSim's
//! `BunchOfCaps.cs` (`GetObjectCost` / `ResourceCostSelected`).

use std::collections::HashMap;

use sl_types::key::ObjectKey;
use uuid::Uuid;

use crate::llsd::Llsd;

/// The accounting costs of one object, as carried by a `GetObjectCost` reply.
/// Each value is reported per-part (`resource_cost` / `physics_cost`) and for the
/// whole linkset the part belongs to (`linked_set_*`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectCost {
    /// The land-impact ("resource") cost of the whole linkset
    /// (`linked_set_resource_cost`).
    pub linked_set_resource_cost: f32,
    /// The land-impact ("resource") cost of this part alone (`resource_cost`).
    pub resource_cost: f32,
    /// The physics cost of this part alone (`physics_cost`).
    pub physics_cost: f32,
    /// The physics cost of the whole linkset (`linked_set_physics_cost`).
    pub linked_set_physics_cost: f32,
    /// The accounting scheme in force (`resource_limiting_type`) — currently
    /// always `"legacy"`.
    pub resource_limiting_type: String,
}

/// The summed costs of the current selection, as carried by a
/// `ResourceCostSelected` reply (the `selected` map).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SelectedResourceCost {
    /// The total physics cost of the selection (`physics`).
    pub physics: f32,
    /// The total streaming (download) cost of the selection (`streaming`).
    pub streaming: f32,
    /// The total simulation cost of the selection (`simulation`).
    pub simulation: f32,
}

/// Which selection the `ResourceCostSelected` request asks about: the linkset
/// *roots* (`selected_roots`, summing each whole linkset) or individual *prims*
/// (`selected_prims`). The viewer normally sends roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectedCostKind {
    /// Sum each selected linkset by its root id (`selected_roots`).
    Roots,
    /// Sum the individually selected prims (`selected_prims`).
    Prims,
}

impl SelectedCostKind {
    /// The LLSD key this selection kind is carried under in the request body.
    const fn key(self) -> &'static str {
        match self {
            Self::Roots => "selected_roots",
            Self::Prims => "selected_prims",
        }
    }
}

/// Builds an LLSD `{ object_ids: [uuid, …] }` request body — the shape shared by
/// `GetObjectCost` and `GetObjectPhysicsData`.
#[must_use]
pub(crate) fn object_ids_request(object_ids: &[ObjectKey]) -> Llsd {
    Llsd::Map(HashMap::from([(
        "object_ids".to_owned(),
        Llsd::Array(object_ids.iter().map(|id| Llsd::Uuid(id.uuid())).collect()),
    )]))
}

/// Decodes the `object_ids` array of an `{ object_ids: [...] }` request body.
#[must_use]
pub(crate) fn parse_object_ids(body: &Llsd) -> Vec<ObjectKey> {
    body.get("object_ids")
        .and_then(Llsd::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Llsd::as_uuid)
                .map(ObjectKey::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Serialises one [`ObjectCost`] to its LLSD map. Shared by the client decoder's
/// inverse (the server reply builder).
fn object_cost_to_llsd(cost: &ObjectCost) -> Llsd {
    Llsd::Map(HashMap::from([
        (
            "linked_set_resource_cost".to_owned(),
            Llsd::Real(f64::from(cost.linked_set_resource_cost)),
        ),
        (
            "resource_cost".to_owned(),
            Llsd::Real(f64::from(cost.resource_cost)),
        ),
        (
            "physics_cost".to_owned(),
            Llsd::Real(f64::from(cost.physics_cost)),
        ),
        (
            "linked_set_physics_cost".to_owned(),
            Llsd::Real(f64::from(cost.linked_set_physics_cost)),
        ),
        (
            "resource_limiting_type".to_owned(),
            Llsd::String(cost.resource_limiting_type.clone()),
        ),
    ]))
}

/// Decodes one [`ObjectCost`] from its LLSD map (lenient: absent keys default).
fn object_cost_from_llsd(value: &Llsd) -> ObjectCost {
    ObjectCost {
        linked_set_resource_cost: value
            .get("linked_set_resource_cost")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        resource_cost: value
            .get("resource_cost")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        physics_cost: value
            .get("physics_cost")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        linked_set_physics_cost: value
            .get("linked_set_physics_cost")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        resource_limiting_type: value
            .get("resource_limiting_type")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

// ---------------------------------------------------------------------------
// GetObjectCost
// ---------------------------------------------------------------------------

/// Builds the LLSD body for a `GetObjectCost` POST (`{ object_ids: [...] }`).
#[must_use]
pub fn build_get_object_cost_request(object_ids: &[ObjectKey]) -> String {
    object_ids_request(object_ids).to_llsd_xml()
}

/// Decodes a `GetObjectCost` reply: the per-object costs, keyed by object id.
/// The result is sorted by id so it is deterministic.
#[must_use]
pub fn parse_get_object_cost(body: &Llsd) -> Vec<(ObjectKey, ObjectCost)> {
    let mut costs: Vec<(ObjectKey, ObjectCost)> = body
        .as_map()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Uuid::parse_str(key)
                        .ok()
                        .map(|id| (ObjectKey::from(id), object_cost_from_llsd(value)))
                })
                .collect()
        })
        .unwrap_or_default();
    costs.sort_by_key(|(id, _cost)| id.uuid());
    costs
}

/// Builds a `GetObjectCost` reply from the per-object costs (server side) — the
/// inverse of [`parse_get_object_cost`].
#[must_use]
pub fn build_get_object_cost_response(costs: &[(ObjectKey, ObjectCost)]) -> String {
    Llsd::Map(
        costs
            .iter()
            .map(|(id, cost)| (id.uuid().to_string(), object_cost_to_llsd(cost)))
            .collect(),
    )
    .to_llsd_xml()
}

// ---------------------------------------------------------------------------
// ResourceCostSelected
// ---------------------------------------------------------------------------

/// Builds the LLSD body for a `ResourceCostSelected` POST. `kind` selects whether
/// the ids are linkset roots (`selected_roots`) or individual prims
/// (`selected_prims`).
#[must_use]
pub fn build_resource_cost_selected_request(
    kind: SelectedCostKind,
    object_ids: &[ObjectKey],
) -> String {
    Llsd::Map(HashMap::from([(
        kind.key().to_owned(),
        Llsd::Array(object_ids.iter().map(|id| Llsd::Uuid(id.uuid())).collect()),
    )]))
    .to_llsd_xml()
}

/// Decodes a `ResourceCostSelected` request: the selection kind and the ids.
/// Defaults to [`SelectedCostKind::Roots`] with no ids when neither key is set.
#[must_use]
pub fn parse_resource_cost_selected_request(body: &Llsd) -> (SelectedCostKind, Vec<ObjectKey>) {
    let (kind, key) = if body.get("selected_prims").is_some() {
        (SelectedCostKind::Prims, "selected_prims")
    } else {
        (SelectedCostKind::Roots, "selected_roots")
    };
    let ids = body
        .get(key)
        .and_then(Llsd::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Llsd::as_uuid)
                .map(ObjectKey::from)
                .collect()
        })
        .unwrap_or_default();
    (kind, ids)
}

/// Decodes a `ResourceCostSelected` reply: the summed selection costs.
#[must_use]
pub fn parse_resource_cost_selected(body: &Llsd) -> SelectedResourceCost {
    let selected = body.get("selected");
    let read = |key: &str| {
        selected
            .and_then(|map| map.get(key))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    SelectedResourceCost {
        physics: read("physics"),
        streaming: read("streaming"),
        simulation: read("simulation"),
    }
}

/// Builds a `ResourceCostSelected` reply from the summed selection costs (server
/// side) — the inverse of [`parse_resource_cost_selected`].
#[must_use]
pub fn build_resource_cost_selected_response(cost: &SelectedResourceCost) -> String {
    Llsd::Map(HashMap::from([(
        "selected".to_owned(),
        Llsd::Map(HashMap::from([
            ("physics".to_owned(), Llsd::Real(f64::from(cost.physics))),
            (
                "streaming".to_owned(),
                Llsd::Real(f64::from(cost.streaming)),
            ),
            (
                "simulation".to_owned(),
                Llsd::Real(f64::from(cost.simulation)),
            ),
        ])),
    )]))
    .to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::ObjectKey;
    use uuid::Uuid;

    use super::{
        ObjectCost, SelectedCostKind, SelectedResourceCost, build_get_object_cost_request,
        build_get_object_cost_response, build_resource_cost_selected_request,
        build_resource_cost_selected_response, parse_get_object_cost, parse_resource_cost_selected,
        parse_resource_cost_selected_request,
    };
    use crate::llsd::parse_llsd_xml;
    use crate::object_cost::parse_object_ids;

    /// The per-object costs round-trip through the server reply builder and the
    /// client decoder, sorted by id.
    #[test]
    fn object_cost_round_trips() -> Result<(), String> {
        let id_a = ObjectKey::from(Uuid::from_u128(0x11));
        let id_b = ObjectKey::from(Uuid::from_u128(0x22));
        let costs = vec![
            (
                id_b,
                ObjectCost {
                    linked_set_resource_cost: 12.5,
                    resource_cost: 3.5,
                    physics_cost: 1.0,
                    linked_set_physics_cost: 4.0,
                    resource_limiting_type: "legacy".to_owned(),
                },
            ),
            (
                id_a,
                ObjectCost {
                    linked_set_resource_cost: 1.0,
                    resource_cost: 1.0,
                    physics_cost: 0.0,
                    linked_set_physics_cost: 0.0,
                    resource_limiting_type: "legacy".to_owned(),
                },
            ),
        ];
        let xml = build_get_object_cost_response(&costs);
        let parsed =
            parse_get_object_cost(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.first().map(|entry| entry.0), Some(id_a));
        assert_eq!(parsed.get(1).map(|entry| entry.0), Some(id_b));
        assert_eq!(
            parsed.get(1).map(|entry| entry.1.resource_cost.to_bits()),
            Some(3.5_f32.to_bits())
        );
        Ok(())
    }

    /// A `GetObjectCost` request carries the requested ids under `object_ids`.
    #[test]
    fn object_cost_request_carries_ids() -> Result<(), String> {
        let ids = [
            ObjectKey::from(Uuid::from_u128(0xaa)),
            ObjectKey::from(Uuid::from_u128(0xbb)),
        ];
        let body = build_get_object_cost_request(&ids);
        let parsed =
            parse_object_ids(&parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed, ids);
        Ok(())
    }

    /// A `ResourceCostSelected` request round-trips the selection kind and ids,
    /// and the reply round-trips the summed costs.
    #[test]
    fn resource_cost_selected_round_trips() -> Result<(), String> {
        let ids = [ObjectKey::from(Uuid::from_u128(0xc0))];
        let body = build_resource_cost_selected_request(SelectedCostKind::Roots, &ids);
        let (kind, parsed_ids) = parse_resource_cost_selected_request(
            &parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(kind, SelectedCostKind::Roots);
        assert_eq!(parsed_ids, ids);

        let cost = SelectedResourceCost {
            physics: 2.0,
            streaming: 3.0,
            simulation: 4.0,
        };
        let xml = build_resource_cost_selected_response(&cost);
        let parsed = parse_resource_cost_selected(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(parsed, cost);
        Ok(())
    }
}
