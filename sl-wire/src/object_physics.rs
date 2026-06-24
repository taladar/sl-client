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

use crate::WireError;
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

/// Decodes one [`ObjectPhysicsData`] from its LLSD map.
///
/// `PhysicsShapeType` is the structural datum the record is about: every
/// conforming emitter always sends it (OpenSim `BunchOfCaps.cs` /
/// `EventQueueGetHandlers.cs` / `LLClientView.cs`) and both Firestorm readers
/// (`llviewerobjectlist.cpp`, `llviewerobject.cpp`) read it unconditionally, so
/// its absence is a hard [`WireError::MissingField`].
///
/// The four material fields (`Density`, `Friction`, `Restitution`,
/// `GravityMultiplier`) are written as an all-or-nothing group. For
/// `ObjectPhysicsProperties` OpenSim always sends all four; for
/// `GetObjectPhysicsData` the Firestorm reader gates the trio on the presence of
/// `Density`, so a shape-type-only object (all four absent) is legitimate. We
/// honour both: if `Density` is present the other three are required, and if it
/// is absent the whole group defaults to `0.0`. A partial material set (e.g.
/// `Density` present but `Friction` absent) is therefore a hard
/// [`WireError::MissingField`] rather than being silently masked.
fn object_physics_data_from_llsd(value: &Llsd) -> Result<ObjectPhysicsData, WireError> {
    let shape = value.require_i32("PhysicsShapeType", "PhysicsShapeType")?;
    let physics_shape_type =
        u8::try_from(shape).map_or(PhysicsShapeType::Prim, PhysicsShapeType::from_u8);
    let (density, friction, restitution, gravity_multiplier) =
        if let Some(density) = value.field_f32("Density", "Density")? {
            (
                density,
                value.require_f32("Friction", "Friction")?,
                value.require_f32("Restitution", "Restitution")?,
                value.require_f32("GravityMultiplier", "GravityMultiplier")?,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
    Ok(ObjectPhysicsData {
        physics_shape_type,
        density,
        friction,
        restitution,
        gravity_multiplier,
    })
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
///
/// # Errors
/// Returns [`WireError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_get_object_physics_data_request(body: &Llsd) -> Result<Vec<ObjectKey>, WireError> {
    parse_object_ids(body)
}

/// Decodes a `GetObjectPhysicsData` reply: the per-object physics data, keyed by
/// object id and sorted by id so it is deterministic. Objects absent from the
/// reply map (the "no such object" signal) are simply not present in the result.
///
/// # Errors
/// Returns [`WireError::MissingField`] if a present per-object map omits the
/// required `PhysicsShapeType` (or a partial material set), or
/// [`WireError::MalformedField`] if a decoded LLSD field is present but of the
/// wrong kind.
pub fn parse_get_object_physics_data(
    body: &Llsd,
) -> Result<Vec<(ObjectKey, ObjectPhysicsData)>, WireError> {
    let mut data: Vec<(ObjectKey, ObjectPhysicsData)> = body
        .as_map()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Uuid::parse_str(key).ok().map(|id| {
                        object_physics_data_from_llsd(value)
                            .map(|value| (ObjectKey::from(id), value))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    data.sort_by_key(|(id, _data)| id.uuid());
    Ok(data)
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
///
/// # Errors
/// Returns [`WireError::MissingField`] if the `ObjectData` array, an entry's
/// `LocalID`, its `PhysicsShapeType`, or a material field is absent (every field
/// of this event is always sent by a conforming emitter), or
/// [`WireError::MalformedField`] if a decoded LLSD field is present but of the
/// wrong kind.
pub fn parse_object_physics_properties(
    body: &Llsd,
) -> Result<Vec<(crate::RegionLocalObjectId, ObjectPhysicsData)>, WireError> {
    // `ObjectData` is the array the whole event exists to carry; every emitter
    // (OpenSim `EventQueueGetHandlers.cs` / `LLClientView.cs`) writes it and the
    // Firestorm reader (`llviewerobject.cpp`) reads it unconditionally, so its
    // absence is a hard `MissingField`.
    let entries = body.require_array("ObjectData", "ObjectData")?;
    entries
        .iter()
        .map(|entry| {
            // `LocalID` is the per-entry id that names which object the
            // parameters belong to; it is always sent and read unconditionally,
            // so a missing one is a hard `MissingField` rather than a silently
            // dropped entry.
            let local_id = entry.require_i32("LocalID", "LocalID")?.cast_unsigned();
            object_physics_data_from_llsd(entry)
                .map(|data| (crate::RegionLocalObjectId(local_id), data))
        })
        .collect::<Result<Vec<_>, _>>()
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
    use crate::WireError;
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
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, data);
        Ok(())
    }

    /// A present per-object physics map that omits `PhysicsShapeType` is a hard
    /// `MissingField` error.
    #[test]
    fn physics_data_missing_shape_type_is_error() -> Result<(), String> {
        let xml = concat!(
            "<llsd><map>",
            "<key>42424242-4242-4242-4242-424242424242</key><map>",
            // PhysicsShapeType deliberately omitted
            "<key>Density</key><real>1000.0</real>",
            "<key>Friction</key><real>0.6</real>",
            "<key>Restitution</key><real>0.5</real>",
            "<key>GravityMultiplier</key><real>1.0</real>",
            "</map></map></llsd>"
        );
        let body = parse_llsd_xml(xml).map_err(|error| format!("{error:?}"))?;
        match parse_get_object_physics_data(&body) {
            Err(WireError::MissingField {
                field: "PhysicsShapeType",
            }) => Ok(()),
            other => Err(format!(
                "expected MissingField PhysicsShapeType, got {other:?}"
            )),
        }
    }

    /// A partial material set (`Density` present but a sibling material field
    /// absent) is a hard `MissingField` error rather than being masked to a
    /// default, while a shape-type-only object (all four material fields absent)
    /// decodes successfully with zeroed material parameters.
    #[test]
    fn physics_data_partial_material_set_is_error() -> Result<(), String> {
        let partial = concat!(
            "<llsd><map>",
            "<key>42424242-4242-4242-4242-424242424242</key><map>",
            "<key>PhysicsShapeType</key><integer>0</integer>",
            "<key>Density</key><real>1000.0</real>",
            // Friction deliberately omitted
            "<key>Restitution</key><real>0.5</real>",
            "<key>GravityMultiplier</key><real>1.0</real>",
            "</map></map></llsd>"
        );
        let body = parse_llsd_xml(partial).map_err(|error| format!("{error:?}"))?;
        match parse_get_object_physics_data(&body) {
            Err(WireError::MissingField { field: "Friction" }) => {}
            other => return Err(format!("expected MissingField Friction, got {other:?}")),
        }

        let shape_only = concat!(
            "<llsd><map>",
            "<key>42424242-4242-4242-4242-424242424242</key><map>",
            "<key>PhysicsShapeType</key><integer>2</integer>",
            "</map></map></llsd>"
        );
        let body = parse_llsd_xml(shape_only).map_err(|error| format!("{error:?}"))?;
        let parsed = parse_get_object_physics_data(&body).map_err(|error| format!("{error:?}"))?;
        let entry = parsed.first().ok_or("expected one entry")?;
        assert_eq!(entry.1.physics_shape_type, PhysicsShapeType::ConvexHull);
        assert_eq!(entry.1.density.to_bits(), 0.0_f32.to_bits());
        Ok(())
    }

    /// An `ObjectPhysicsProperties` event entry that omits `LocalID` is a hard
    /// `MissingField` error rather than being silently dropped.
    #[test]
    fn physics_event_missing_local_id_is_error() -> Result<(), String> {
        let xml = concat!(
            "<llsd><map><key>ObjectData</key><array><map>",
            // LocalID deliberately omitted
            "<key>PhysicsShapeType</key><integer>0</integer>",
            "<key>Density</key><real>1.0</real>",
            "<key>Friction</key><real>0.1</real>",
            "<key>Restitution</key><real>0.2</real>",
            "<key>GravityMultiplier</key><real>0.5</real>",
            "</map></array></map></llsd>"
        );
        let body = parse_llsd_xml(xml).map_err(|error| format!("{error:?}"))?;
        match parse_object_physics_properties(&body) {
            Err(WireError::MissingField { field: "LocalID" }) => Ok(()),
            other => Err(format!("expected MissingField LocalID, got {other:?}")),
        }
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
        )
        .map_err(|error| format!("{error:?}"))?;
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
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, data);
        Ok(())
    }
}
