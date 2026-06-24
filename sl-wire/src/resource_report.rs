//! The **`AttachmentResources`** and **`LandResources`** capabilities: scripted-
//! object resource (memory / URL) usage reports.
//!
//! These two capabilities answer "what is using up my script resources" — one for
//! the agent's own attachments, one for a parcel's objects:
//!
//! - `AttachmentResources` — GET; the reply groups the agent's scripted
//!   attachments by attachment point, with each object's memory/URL use, plus a
//!   `summary` of total available and used resources.
//! - `LandResources` — POST `{ parcel_id: uuid }`; the reply hands back two
//!   *follow-up* capability URLs: a `ScriptResourceSummary` (the parcel's totals)
//!   and, when the agent may see it, a `ScriptResourceDetails` (a per-object
//!   breakdown grouped by parcel). The viewer then GETs those URLs.
//!
//! The summary report, the attachment report, and the detail report all share the
//! same building blocks — a [`ResourceSummary`] of `available`/`used`
//! [`ResourceAmount`]s, and the per-object [`ScriptedObjectInfo`]. This module
//! builds the request bodies and decodes the replies (client side) and parses the
//! requests and builds the replies (server side). The LLSD keys are cross-checked
//! against OpenSim's `BunchOfCaps.cs` (`AttachmentResources` / `LandResources` and
//! the `ScriptResourceSummary` / `ScriptResourceDetails` follow-up caps).

use std::collections::HashMap;

use sl_types::key::{AgentKey, OwnerKey, ParcelKey};
use sl_types::map::RegionCoordinates;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

/// One resource budget line in a [`ResourceSummary`]: how much of a named
/// resource is available or used. The `resource_type` is `"memory"` (bytes; an
/// `amount` of `-1` means "unlimited") or `"urls"` (a count).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceAmount {
    /// The resource being measured (`type`): `"memory"` or `"urls"`.
    pub resource_type: String,
    /// The amount available or used (`amount`).
    pub amount: i32,
}

/// A resource budget summary: the `available` and `used` amounts of each
/// resource. Shared by the attachment report and the land summary report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceSummary {
    /// The available (budget) amount of each resource (`available`).
    pub available: Vec<ResourceAmount>,
    /// The currently used amount of each resource (`used`).
    pub used: Vec<ResourceAmount>,
}

/// The script resources used by one object (`resources`): memory in bytes and the
/// number of `llRequestURL` URLs it holds. Each is [`None`] when the simulator
/// omits it (it reports a value only when non-zero, in the attachment report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScriptedObjectResources {
    /// The script memory the object uses, in bytes (`memory`).
    pub memory: Option<i32>,
    /// The number of URLs the object holds (`urls`).
    pub urls: Option<i32>,
}

/// One scripted object in a resource report, shared by the attachment report and
/// the land detail report.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedObjectInfo {
    /// The object's id (`id`).
    pub id: Uuid,
    /// The object's region position (`location`), in metres.
    pub location: RegionCoordinates,
    /// The object's name (`name`).
    pub name: String,
    /// The object's owner (`owner_id` + `is_group_owned`) — an agent, or a group
    /// when the object is deeded to a group.
    pub owner: OwnerKey,
    /// The script resources the object uses (`resources`).
    pub resources: ScriptedObjectResources,
}

/// Build an [`OwnerKey`] from the report's raw owner UUID and its group flag: a
/// set flag makes the UUID a [`GroupKey`](sl_types::key::GroupKey), otherwise an
/// [`AgentKey`]. The inverse on encode is `owner.uuid()` / `owner.is_group()`.
fn owner_key_from_wire(uuid: Uuid, is_group: bool) -> OwnerKey {
    if is_group {
        OwnerKey::Group(sl_types::key::GroupKey::from(uuid))
    } else {
        OwnerKey::Agent(AgentKey::from(uuid))
    }
}

impl Default for ScriptedObjectInfo {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            location: RegionCoordinates::new(0.0, 0.0, 0.0),
            name: String::new(),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
            resources: ScriptedObjectResources::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Shared building blocks.
// ---------------------------------------------------------------------------

/// Serialises one `{ type, amount }` resource line to LLSD.
fn resource_amount_to_llsd(amount: &ResourceAmount) -> Llsd {
    Llsd::Map(HashMap::from([
        (
            "type".to_owned(),
            Llsd::String(amount.resource_type.clone()),
        ),
        ("amount".to_owned(), Llsd::Integer(amount.amount)),
    ]))
}

/// Decodes one `{ type, amount }` resource line from LLSD.
///
/// `type` and `amount` are both **required**: OpenSim writes both unconditionally
/// for every resource line (`BunchOfCaps.cs` `WriteScriptResourceData`,
/// `AddElem("amount", …)` / `AddElem("type", …)`), and a line missing either is a
/// nonsense record (an unnamed budget, or a named budget with no number), so an
/// absent key is a hard [`WireError::MissingField`] rather than a silent
/// `{ type: "", amount: 0 }`.
fn resource_amount_from_llsd(value: &Llsd) -> Result<ResourceAmount, WireError> {
    Ok(ResourceAmount {
        resource_type: value.require_str("type", "type")?.to_owned(),
        amount: value.require_i32("amount", "amount")?,
    })
}

/// Decodes an `available`/`used` array of resource lines.
fn resource_amounts_from_llsd(
    value: Option<&Llsd>,
    label: &'static str,
) -> Result<Vec<ResourceAmount>, WireError> {
    match value {
        None | Some(Llsd::Undef) => Ok(Vec::new()),
        Some(Llsd::Array(amounts)) => amounts
            .iter()
            .map(resource_amount_from_llsd)
            .collect::<Result<Vec<_>, _>>(),
        Some(other) => Err(WireError::MalformedField {
            field: label,
            value: other.kind().to_owned(),
        }),
    }
}

/// Serialises a [`ResourceSummary`] to its `{ available, used }` LLSD map.
fn resource_summary_to_llsd(summary: &ResourceSummary) -> Llsd {
    let to_array = |amounts: &[ResourceAmount]| {
        Llsd::Array(amounts.iter().map(resource_amount_to_llsd).collect())
    };
    Llsd::Map(HashMap::from([
        ("available".to_owned(), to_array(&summary.available)),
        ("used".to_owned(), to_array(&summary.used)),
    ]))
}

/// Decodes a [`ResourceSummary`] from its `{ available, used }` LLSD map.
fn resource_summary_from_llsd(value: &Llsd) -> Result<ResourceSummary, WireError> {
    Ok(ResourceSummary {
        available: resource_amounts_from_llsd(value.get("available"), "available")?,
        used: resource_amounts_from_llsd(value.get("used"), "used")?,
    })
}

/// Serialises a `[x, y, z]` vector to an LLSD array of three reals (the LLSD
/// encoding of a region position).
fn vector3_to_llsd(vector: [f32; 3]) -> Llsd {
    Llsd::Array(
        vector
            .iter()
            .map(|&component| Llsd::Real(f64::from(component)))
            .collect(),
    )
}

/// Decodes an LLSD array of three reals into a `[x, y, z]` vector.
fn vector3_from_llsd(value: Option<&Llsd>, label: &'static str) -> Result<[f32; 3], WireError> {
    let array = match value {
        None | Some(Llsd::Undef) => return Ok([0.0, 0.0, 0.0]),
        Some(Llsd::Array(array)) => array,
        Some(other) => {
            return Err(WireError::MalformedField {
                field: label,
                value: other.kind().to_owned(),
            });
        }
    };
    let component = |index: usize| -> Result<f32, WireError> {
        match array.get(index) {
            None | Some(Llsd::Undef) => Ok(0.0),
            Some(element) => element.as_f32().ok_or_else(|| WireError::MalformedField {
                field: label,
                value: element.kind().to_owned(),
            }),
        }
    };
    Ok([component(0)?, component(1)?, component(2)?])
}

/// Serialises one [`ScriptedObjectInfo`] to its LLSD map. Memory/URL fields are
/// emitted only when present (`Some`), matching the simulator's "non-zero only"
/// behaviour.
fn scripted_object_to_llsd(object: &ScriptedObjectInfo) -> Llsd {
    let mut resources: HashMap<String, Llsd> = HashMap::new();
    if let Some(memory) = object.resources.memory {
        let _previous = resources.insert("memory".to_owned(), Llsd::Integer(memory));
    }
    if let Some(urls) = object.resources.urls {
        let _previous = resources.insert("urls".to_owned(), Llsd::Integer(urls));
    }
    Llsd::Map(HashMap::from([
        ("id".to_owned(), Llsd::Uuid(object.id)),
        (
            "is_group_owned".to_owned(),
            Llsd::Integer(i32::from(object.owner.is_group())),
        ),
        (
            "location".to_owned(),
            vector3_to_llsd([
                object.location.x(),
                object.location.y(),
                object.location.z(),
            ]),
        ),
        ("name".to_owned(), Llsd::String(object.name.clone())),
        ("owner_id".to_owned(), Llsd::Uuid(object.owner.uuid())),
        ("resources".to_owned(), Llsd::Map(resources)),
    ]))
}

/// Decodes one [`ScriptedObjectInfo`] from its LLSD map.
///
/// `id` and `owner_id` are **required**: OpenSim writes both unconditionally for
/// every object in both the attachment report and the land detail report
/// (`BunchOfCaps.cs` — `AddElem("id", …)` / `AddElem("owner_id", …)`), and a
/// scripted-object record with no identity or no owner is meaningless, so an
/// absent key is a hard [`WireError::MissingField`]. The remaining fields stay
/// optional: `name` may be empty, `location` is `.has()`-guarded by Firestorm
/// (`llfloaterscriptlimits.cpp`), `is_group_owned` is explicitly tolerated as
/// absent there ("may not be sent by all server versions" → default `false`), and
/// the `resources` memory/URL counts are written only when non-zero in the
/// attachment report (`BunchOfCaps.cs` `if (asi.memory > 0)` / `if (asi.urls > 0)`).
fn scripted_object_from_llsd(value: &Llsd) -> Result<ScriptedObjectInfo, WireError> {
    let resources = match value.get("resources") {
        None | Some(Llsd::Undef) => ScriptedObjectResources::default(),
        Some(map @ Llsd::Map(_)) => ScriptedObjectResources {
            memory: map.field_i32("memory", "memory")?,
            urls: map.field_i32("urls", "urls")?,
        },
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "resources",
                value: other.kind().to_owned(),
            });
        }
    };
    Ok(ScriptedObjectInfo {
        id: value.require_uuid("id", "id")?,
        location: {
            let [x, y, z] = vector3_from_llsd(value.get("location"), "location")?;
            RegionCoordinates::new(x, y, z)
        },
        name: value
            .field_str("name", "name")?
            .unwrap_or_default()
            .to_owned(),
        owner: owner_key_from_wire(
            value.require_uuid("owner_id", "owner_id")?,
            value
                .field_bool("is_group_owned", "is_group_owned")?
                .unwrap_or(false),
        ),
        resources,
    })
}

/// Decodes an array of [`ScriptedObjectInfo`].
fn scripted_objects_from_llsd(
    value: Option<&Llsd>,
    label: &'static str,
) -> Result<Vec<ScriptedObjectInfo>, WireError> {
    match value {
        None | Some(Llsd::Undef) => Ok(Vec::new()),
        Some(Llsd::Array(objects)) => objects
            .iter()
            .map(scripted_object_from_llsd)
            .collect::<Result<Vec<_>, _>>(),
        Some(other) => Err(WireError::MalformedField {
            field: label,
            value: other.kind().to_owned(),
        }),
    }
}

// ---------------------------------------------------------------------------
// AttachmentResources
// ---------------------------------------------------------------------------

/// One attachment point in an [`AttachmentResourcesReport`]: the point's display
/// name and the scripted objects worn there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttachmentLocation {
    /// The attachment point's display name (`location`), e.g. `"Right Hand"`.
    pub location: String,
    /// The scripted objects attached at this point (`objects`).
    pub objects: Vec<ScriptedObjectInfo>,
}

/// An `AttachmentResources` reply: the agent's scripted attachments grouped by
/// attachment point, plus the total resource summary.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttachmentResourcesReport {
    /// The scripted attachments, grouped by attachment point (`attachments`).
    pub attachments: Vec<AttachmentLocation>,
    /// The total available/used resource summary (`summary`).
    pub summary: ResourceSummary,
}

/// Decodes an `AttachmentResources` reply.
///
/// # Errors
/// Returns [`WireError::MissingField`] if a required field of a scripted-object
/// record (`id`, `owner_id`) or a resource line (`type`, `amount`) is absent, or
/// [`WireError::MalformedField`] if a decoded LLSD field is present but of the
/// wrong kind.
pub fn parse_attachment_resources(body: &Llsd) -> Result<AttachmentResourcesReport, WireError> {
    let attachments = match body.get("attachments") {
        None | Some(Llsd::Undef) => Vec::new(),
        Some(Llsd::Array(points)) => points
            .iter()
            .map(|point| {
                Ok(AttachmentLocation {
                    location: point
                        .field_str("location", "location")?
                        .unwrap_or_default()
                        .to_owned(),
                    objects: scripted_objects_from_llsd(point.get("objects"), "objects")?,
                })
            })
            .collect::<Result<Vec<_>, WireError>>()?,
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "attachments",
                value: other.kind().to_owned(),
            });
        }
    };
    let summary = match body.get("summary") {
        None | Some(Llsd::Undef) => ResourceSummary::default(),
        Some(map @ Llsd::Map(_)) => resource_summary_from_llsd(map)?,
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "summary",
                value: other.kind().to_owned(),
            });
        }
    };
    Ok(AttachmentResourcesReport {
        attachments,
        summary,
    })
}

/// Builds an `AttachmentResources` reply (server side) — the inverse of
/// [`parse_attachment_resources`].
#[must_use]
pub fn build_attachment_resources_response(report: &AttachmentResourcesReport) -> String {
    let attachments = Llsd::Array(
        report
            .attachments
            .iter()
            .map(|point| {
                Llsd::Map(HashMap::from([
                    ("location".to_owned(), Llsd::String(point.location.clone())),
                    (
                        "objects".to_owned(),
                        Llsd::Array(point.objects.iter().map(scripted_object_to_llsd).collect()),
                    ),
                ]))
            })
            .collect(),
    );
    Llsd::Map(HashMap::from([
        ("attachments".to_owned(), attachments),
        (
            "summary".to_owned(),
            resource_summary_to_llsd(&report.summary),
        ),
    ]))
    .to_llsd_xml()
}

// ---------------------------------------------------------------------------
// LandResources (the POST handing back the follow-up cap URLs)
// ---------------------------------------------------------------------------

/// The follow-up capability URLs returned by a `LandResources` POST. The viewer
/// GETs [`script_resource_summary`](Self::script_resource_summary) for the
/// parcel's totals and, when present, [`script_resource_details`](Self::script_resource_details)
/// for the per-object breakdown (omitted when the agent may not see detail).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LandResourcesUrls {
    /// The URL of the parcel's resource-summary follow-up cap
    /// (`ScriptResourceSummary`). The empty/absent wire value decodes to [`None`].
    pub script_resource_summary: Option<url::Url>,
    /// The URL of the parcel's resource-detail follow-up cap
    /// (`ScriptResourceDetails`), when the agent may see it.
    pub script_resource_details: Option<url::Url>,
}

/// Builds the LLSD body for a `LandResources` POST (`{ parcel_id: uuid }`). The
/// `parcel_id` is the region's "fake" parcel id (see `RemoteParcelRequest`).
#[must_use]
pub fn build_land_resources_request(parcel_id: ParcelKey) -> String {
    Llsd::Map(HashMap::from([(
        "parcel_id".to_owned(),
        Llsd::Uuid(parcel_id.uuid()),
    )]))
    .to_llsd_xml()
}

/// Decodes a `LandResources` request: the requested parcel id.
///
/// # Errors
/// Returns [`WireError::MalformedField`] if `parcel_id` is present but of the
/// wrong LLSD kind.
pub fn parse_land_resources_request(body: &Llsd) -> Result<Option<ParcelKey>, WireError> {
    Ok(body
        .field_uuid("parcel_id", "parcel_id")?
        .map(ParcelKey::from))
}

/// Decodes a `LandResources` reply: the follow-up capability URLs.
///
/// # Errors
/// Returns [`WireError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_land_resources_reply(body: &Llsd) -> Result<LandResourcesUrls, WireError> {
    Ok(LandResourcesUrls {
        script_resource_summary: crate::optional_url_from_wire(
            "ScriptResourceSummary",
            body.field_str("ScriptResourceSummary", "ScriptResourceSummary")?
                .unwrap_or(""),
        )?,
        script_resource_details: crate::optional_url_from_wire(
            "ScriptResourceDetails",
            body.field_str("ScriptResourceDetails", "ScriptResourceDetails")?
                .unwrap_or(""),
        )?,
    })
}

/// Builds a `LandResources` reply from the follow-up URLs (server side) — the
/// inverse of [`parse_land_resources_reply`].
#[must_use]
pub fn build_land_resources_response(urls: &LandResourcesUrls) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    if let Some(summary) = &urls.script_resource_summary {
        let _previous = map.insert(
            "ScriptResourceSummary".to_owned(),
            Llsd::String(crate::url_to_wire(summary)),
        );
    }
    if let Some(details) = &urls.script_resource_details {
        let _previous = map.insert(
            "ScriptResourceDetails".to_owned(),
            Llsd::String(crate::url_to_wire(details)),
        );
    }
    Llsd::Map(map).to_llsd_xml()
}

// ---------------------------------------------------------------------------
// LandResources follow-up reports: ScriptResourceSummary / ScriptResourceDetails
// ---------------------------------------------------------------------------

/// Decodes a `ScriptResourceSummary` follow-up report: the parcel's resource
/// totals (carried under a `summary` key).
///
/// # Errors
/// Returns [`WireError::MissingField`] if a resource line is missing its required
/// `type` or `amount`, or [`WireError::MalformedField`] if a decoded LLSD field
/// is present but of the wrong kind.
pub fn parse_land_resource_summary(body: &Llsd) -> Result<ResourceSummary, WireError> {
    match body.get("summary") {
        None | Some(Llsd::Undef) => Ok(ResourceSummary::default()),
        Some(map @ Llsd::Map(_)) => resource_summary_from_llsd(map),
        Some(other) => Err(WireError::MalformedField {
            field: "summary",
            value: other.kind().to_owned(),
        }),
    }
}

/// Builds a `ScriptResourceSummary` follow-up report (server side) — the inverse
/// of [`parse_land_resource_summary`].
#[must_use]
pub fn build_land_resource_summary_response(summary: &ResourceSummary) -> String {
    Llsd::Map(HashMap::from([(
        "summary".to_owned(),
        resource_summary_to_llsd(summary),
    )]))
    .to_llsd_xml()
}

/// One parcel's scripted-object breakdown in a `ScriptResourceDetails` report.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParcelScriptResources {
    /// The parcel's name (`name`).
    pub name: String,
    /// The parcel's grid-wide ("fake") id (`id`).
    pub id: Uuid,
    /// The parcel's region-local id (`local_id`).
    pub local_id: crate::RegionLocalParcelId,
    /// The scripted objects on the parcel (`objects`).
    pub objects: Vec<ScriptedObjectInfo>,
}

/// Decodes a `ScriptResourceDetails` follow-up report: the per-parcel scripted-
/// object breakdown (carried under a `parcels` array).
///
/// Within each parcel record, `id` and `local_id` are **required**: OpenSim
/// writes both unconditionally for every parcel (`BunchOfCaps.cs` —
/// `AddElem("id", …)` / `AddElem("local_id", …)`), and a parcel record with no
/// grid id or no region-local id can identify no parcel, so an absent key is a
/// hard [`WireError::MissingField`]. `name` stays optional (it may be empty and
/// Firestorm reads it blind).
///
/// # Errors
/// Returns [`WireError::MissingField`] if a required field (`id`, `local_id`) is
/// absent, or [`WireError::MalformedField`] if a decoded LLSD field is present
/// but of the wrong kind.
pub fn parse_land_resource_detail(body: &Llsd) -> Result<Vec<ParcelScriptResources>, WireError> {
    match body.get("parcels") {
        None | Some(Llsd::Undef) => Ok(Vec::new()),
        Some(Llsd::Array(parcels)) => parcels
            .iter()
            .map(|parcel| {
                Ok(ParcelScriptResources {
                    name: parcel
                        .field_str("name", "name")?
                        .unwrap_or_default()
                        .to_owned(),
                    id: parcel.require_uuid("id", "id")?,
                    local_id: crate::RegionLocalParcelId(
                        parcel.require_i32("local_id", "local_id")?,
                    ),
                    objects: scripted_objects_from_llsd(parcel.get("objects"), "objects")?,
                })
            })
            .collect::<Result<Vec<_>, WireError>>(),
        Some(other) => Err(WireError::MalformedField {
            field: "parcels",
            value: other.kind().to_owned(),
        }),
    }
}

/// Builds a `ScriptResourceDetails` follow-up report (server side) — the inverse
/// of [`parse_land_resource_detail`].
#[must_use]
pub fn build_land_resource_detail_response(parcels: &[ParcelScriptResources]) -> String {
    Llsd::Map(HashMap::from([(
        "parcels".to_owned(),
        Llsd::Array(
            parcels
                .iter()
                .map(|parcel| {
                    Llsd::Map(HashMap::from([
                        ("name".to_owned(), Llsd::String(parcel.name.clone())),
                        ("id".to_owned(), Llsd::Uuid(parcel.id)),
                        ("local_id".to_owned(), Llsd::Integer(parcel.local_id.0)),
                        (
                            "objects".to_owned(),
                            Llsd::Array(
                                parcel.objects.iter().map(scripted_object_to_llsd).collect(),
                            ),
                        ),
                    ]))
                })
                .collect(),
        ),
    )]))
    .to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::map::RegionCoordinates;
    use uuid::Uuid;

    use super::{
        AttachmentLocation, AttachmentResourcesReport, LandResourcesUrls, ParcelScriptResources,
        ResourceAmount, ResourceSummary, ScriptedObjectInfo, ScriptedObjectResources,
        build_attachment_resources_response, build_land_resource_detail_response,
        build_land_resource_summary_response, build_land_resources_request,
        build_land_resources_response, parse_attachment_resources, parse_land_resource_detail,
        parse_land_resource_summary, parse_land_resources_reply, parse_land_resources_request,
    };
    use crate::llsd::parse_llsd_xml;
    use sl_types::key::{AgentKey, OwnerKey, ParcelKey};

    fn sample_object() -> ScriptedObjectInfo {
        ScriptedObjectInfo {
            id: Uuid::from_u128(0xabc),
            location: RegionCoordinates::new(128.0, 64.5, 25.0),
            name: "Scripted thing".to_owned(),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xdef))),
            resources: ScriptedObjectResources {
                memory: Some(0x1_0000),
                urls: Some(2),
            },
        }
    }

    /// The attachment report round-trips through the reply builder and decoder,
    /// preserving the per-point objects and the summary.
    #[test]
    fn attachment_resources_round_trips() -> Result<(), String> {
        let report = AttachmentResourcesReport {
            attachments: vec![AttachmentLocation {
                location: "Right Hand".to_owned(),
                objects: vec![sample_object()],
            }],
            summary: ResourceSummary {
                available: vec![ResourceAmount {
                    resource_type: "urls".to_owned(),
                    amount: 38,
                }],
                used: vec![ResourceAmount {
                    resource_type: "memory".to_owned(),
                    amount: 0x1_0000,
                }],
            },
        };
        let xml = build_attachment_resources_response(&report);
        let parsed = parse_attachment_resources(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, report);
        Ok(())
    }

    /// The `LandResources` POST round-trips the parcel id, and the reply
    /// round-trips the follow-up URLs (with detail present).
    #[test]
    fn land_resources_handoff_round_trips() -> Result<(), String> {
        let parcel_id = ParcelKey::from(Uuid::from_u128(0x1234));
        let body = build_land_resources_request(parcel_id);
        let parsed = parse_land_resources_request(
            &parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, Some(parcel_id));

        let urls = LandResourcesUrls {
            script_resource_summary: Some(
                url::Url::parse("http://sim/cap/srs").map_err(|e| e.to_string())?,
            ),
            script_resource_details: Some(
                url::Url::parse("http://sim/cap/srd").map_err(|e| e.to_string())?,
            ),
        };
        let xml = build_land_resources_response(&urls);
        let parsed = parse_land_resources_reply(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, urls);
        Ok(())
    }

    /// The follow-up summary and detail reports round-trip.
    #[test]
    fn land_resource_reports_round_trip() -> Result<(), String> {
        let summary = ResourceSummary {
            available: vec![ResourceAmount {
                resource_type: "memory".to_owned(),
                amount: -1,
            }],
            used: vec![ResourceAmount {
                resource_type: "memory".to_owned(),
                amount: 0x2_0000,
            }],
        };
        let xml = build_land_resource_summary_response(&summary);
        let parsed = parse_land_resource_summary(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, summary);

        let parcels = vec![ParcelScriptResources {
            name: "Home".to_owned(),
            id: Uuid::from_u128(0x55),
            local_id: crate::RegionLocalParcelId(3),
            objects: vec![sample_object()],
        }];
        let xml = build_land_resource_detail_response(&parcels);
        let parsed = parse_land_resource_detail(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, parcels);
        Ok(())
    }
}
