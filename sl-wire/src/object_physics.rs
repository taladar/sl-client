//! The **`GetObjectPhysicsData`** capability and the **`ObjectPhysicsProperties`**
//! event-queue event: an object's per-prim physics material parameters.
//!
//! Every prim carries a small set of physics-material parameters — its physics
//! *shape type* (prim / none / convex hull), and the material's density,
//! friction, restitution (bounciness), and gravity multiplier. The viewer learns
//! them two ways:
//!
//! - `GetObjectPhysicsData` — POST `{ object_ids: [uuid, …] }`; the reply is a map
//!   keyed by object UUID, each value carrying that object's physics parameters.
//! - `ObjectPhysicsProperties` — a server-pushed event-queue event delivering the
//!   same parameters for a batch of objects, keyed by their region-local id (sent
//!   when a prim's physics material changes).
//!
//! This module builds the request body and decodes both the reply and the event
//! (client side), and parses the request and builds both the reply and the event
//! (server side). The LLSD keys are cross-checked against OpenSim's
//! `BunchOfCaps.cs` (`GetObjectPhysicsData`) and `LLClientView.cs` (the
//! `ObjectPhysicsProperties` event-queue builder).

use std::collections::HashMap;

use sl_types::key::ObjectKey;
use uuid::Uuid;

use crate::llsd::Llsd;
use crate::object_cost::{object_ids_request, parse_object_ids};

/// The physics-shape type of a prim (`PhysicsShapeType`): the collision shape the
/// simulator uses for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PhysicsShapeType {
    /// The exact prim shape (`0`) — the default.
    #[default]
    Prim,
    /// No collision shape — the prim is phantom-like to physics (`1`).
    None,
    /// A convex-hull approximation of the prim (`2`).
    ConvexHull,
    /// An unrecognised shape-type byte, preserved verbatim.
    Other(u8),
}

impl PhysicsShapeType {
    /// The raw `PhysicsShapeType` byte for this shape.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Prim => 0,
            Self::None => 1,
            Self::ConvexHull => 2,
            Self::Other(value) => value,
        }
    }

    /// Decodes a raw `PhysicsShapeType` byte.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Prim,
            1 => Self::None,
            2 => Self::ConvexHull,
            other => Self::Other(other),
        }
    }
}

/// One prim's physics-material parameters, as carried by `GetObjectPhysicsData`
/// and `ObjectPhysicsProperties`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObjectPhysicsData {
    /// The collision shape the simulator uses (`PhysicsShapeType`).
    pub physics_shape_type: PhysicsShapeType,
    /// The material density (`Density`), in kg/m³.
    pub density: f32,
    /// The surface friction coefficient (`Friction`).
    pub friction: f32,
    /// The restitution / bounciness (`Restitution`).
    pub restitution: f32,
    /// The gravity multiplier applied to the prim (`GravityMultiplier`).
    pub gravity_multiplier: f32,
}

/// Serialises one [`ObjectPhysicsData`] to its LLSD map. Shared by the
/// `GetObjectPhysicsData` reply builder and the `ObjectPhysicsProperties` event
/// builder (the `LocalID` key is added by the caller for the event).
fn object_physics_data_to_llsd(data: &ObjectPhysicsData) -> HashMap<String, Llsd> {
    HashMap::from([
        (
            "PhysicsShapeType".to_owned(),
            Llsd::Integer(i32::from(data.physics_shape_type.to_u8())),
        ),
        ("Density".to_owned(), Llsd::Real(f64::from(data.density))),
        ("Friction".to_owned(), Llsd::Real(f64::from(data.friction))),
        (
            "Restitution".to_owned(),
            Llsd::Real(f64::from(data.restitution)),
        ),
        (
            "GravityMultiplier".to_owned(),
            Llsd::Real(f64::from(data.gravity_multiplier)),
        ),
    ])
}

/// Decodes one [`ObjectPhysicsData`] from its LLSD map (lenient: absent keys
/// default).
fn object_physics_data_from_llsd(value: &Llsd) -> ObjectPhysicsData {
    let shape = value
        .get("PhysicsShapeType")
        .and_then(Llsd::as_i32)
        .and_then(|raw| u8::try_from(raw).ok())
        .map_or(PhysicsShapeType::Prim, PhysicsShapeType::from_u8);
    ObjectPhysicsData {
        physics_shape_type: shape,
        density: value.get("Density").and_then(Llsd::as_f32).unwrap_or(0.0),
        friction: value.get("Friction").and_then(Llsd::as_f32).unwrap_or(0.0),
        restitution: value
            .get("Restitution")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
        gravity_multiplier: value
            .get("GravityMultiplier")
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0),
    }
}

// ---------------------------------------------------------------------------
// GetObjectPhysicsData (HTTP capability)
// ---------------------------------------------------------------------------

/// Builds the LLSD body for a `GetObjectPhysicsData` POST
/// (`{ object_ids: [...] }`).
#[must_use]
pub fn build_get_object_physics_data_request(object_ids: &[ObjectKey]) -> String {
    object_ids_request(object_ids).to_llsd_xml()
}

/// Decodes a `GetObjectPhysicsData` request: the requested object ids.
#[must_use]
pub fn parse_get_object_physics_data_request(body: &Llsd) -> Vec<ObjectKey> {
    parse_object_ids(body)
}

/// Decodes a `GetObjectPhysicsData` reply: the per-object physics data, keyed by
/// object id and sorted by id so it is deterministic.
#[must_use]
pub fn parse_get_object_physics_data(body: &Llsd) -> Vec<(ObjectKey, ObjectPhysicsData)> {
    let mut data: Vec<(ObjectKey, ObjectPhysicsData)> = body
        .as_map()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Uuid::parse_str(key)
                        .ok()
                        .map(|id| (ObjectKey::from(id), object_physics_data_from_llsd(value)))
                })
                .collect()
        })
        .unwrap_or_default();
    data.sort_by_key(|(id, _data)| id.uuid());
    data
}

/// Builds a `GetObjectPhysicsData` reply from the per-object physics data (server
/// side) — the inverse of [`parse_get_object_physics_data`].
#[must_use]
pub fn build_get_object_physics_data_response(data: &[(ObjectKey, ObjectPhysicsData)]) -> String {
    Llsd::Map(
        data.iter()
            .map(|(id, value)| {
                (
                    id.uuid().to_string(),
                    Llsd::Map(object_physics_data_to_llsd(value)),
                )
            })
            .collect(),
    )
    .to_llsd_xml()
}

// ---------------------------------------------------------------------------
// ObjectPhysicsProperties (event-queue event)
// ---------------------------------------------------------------------------

/// Decodes an `ObjectPhysicsProperties` event-queue body: the per-object physics
/// data keyed by region-local id (`ObjectData[].LocalID`).
#[must_use]
pub fn parse_object_physics_properties(
    body: &Llsd,
) -> Vec<(crate::RegionLocalObjectId, ObjectPhysicsData)> {
    body.get("ObjectData")
        .and_then(Llsd::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let local_id = entry.get("LocalID").and_then(Llsd::as_i32)?.cast_unsigned();
                    Some((
                        crate::RegionLocalObjectId(local_id),
                        object_physics_data_from_llsd(entry),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Builds an `ObjectPhysicsProperties` event-queue body from the per-object
/// physics data keyed by region-local id (server side) — the inverse of
/// [`parse_object_physics_properties`].
#[must_use]
pub fn build_object_physics_properties(
    data: &[(crate::RegionLocalObjectId, ObjectPhysicsData)],
) -> Llsd {
    Llsd::Map(HashMap::from([(
        "ObjectData".to_owned(),
        Llsd::Array(
            data.iter()
                .map(|(local_id, value)| {
                    let mut map = object_physics_data_to_llsd(value);
                    let _previous = map.insert(
                        "LocalID".to_owned(),
                        Llsd::Integer(local_id.0.cast_signed()),
                    );
                    Llsd::Map(map)
                })
                .collect(),
        ),
    )]))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::ObjectKey;
    use uuid::Uuid;

    use super::{
        ObjectPhysicsData, PhysicsShapeType, build_get_object_physics_data_request,
        build_get_object_physics_data_response, build_object_physics_properties,
        parse_get_object_physics_data, parse_get_object_physics_data_request,
        parse_object_physics_properties,
    };
    use crate::llsd::parse_llsd_xml;

    /// The per-object physics data round-trips through the reply builder and the
    /// client decoder, sorted by id, preserving the shape type.
    #[test]
    fn physics_data_round_trips() -> Result<(), String> {
        let id = ObjectKey::from(Uuid::from_u128(0x42));
        let data = vec![(
            id,
            ObjectPhysicsData {
                physics_shape_type: PhysicsShapeType::ConvexHull,
                density: 1000.0,
                friction: 0.6,
                restitution: 0.5,
                gravity_multiplier: 1.0,
            },
        )];
        let xml = build_get_object_physics_data_response(&data);
        let parsed = parse_get_object_physics_data(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(parsed, data);
        Ok(())
    }

    /// A `GetObjectPhysicsData` request carries the requested ids.
    #[test]
    fn physics_request_carries_ids() -> Result<(), String> {
        let ids = [
            ObjectKey::from(Uuid::from_u128(0x1)),
            ObjectKey::from(Uuid::from_u128(0x2)),
        ];
        let body = build_get_object_physics_data_request(&ids);
        let parsed = parse_get_object_physics_data_request(
            &parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(parsed, ids);
        Ok(())
    }

    /// The event-queue body round-trips through the builder and decoder, keyed by
    /// the region-local id (including the full u32 range).
    #[test]
    fn physics_event_round_trips() -> Result<(), String> {
        let data = vec![
            (
                crate::RegionLocalObjectId(7),
                ObjectPhysicsData {
                    physics_shape_type: PhysicsShapeType::None,
                    density: 1.0,
                    friction: 0.1,
                    restitution: 0.2,
                    gravity_multiplier: 0.5,
                },
            ),
            (
                crate::RegionLocalObjectId(0xFFFF_FF00),
                ObjectPhysicsData::default(),
            ),
        ];
        let xml = build_object_physics_properties(&data).to_llsd_xml();
        let parsed = parse_object_physics_properties(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(parsed, data);
        Ok(())
    }
}
