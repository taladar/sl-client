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
use uuid::Uuid;

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
    /// The object's region position (`location`), as `[x, y, z]` metres.
    pub location: [f32; 3],
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
            location: [0.0, 0.0, 0.0],
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
fn resource_amount_from_llsd(value: &Llsd) -> ResourceAmount {
    ResourceAmount {
        resource_type: value
            .get("type")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        amount: value.get("amount").and_then(Llsd::as_i32).unwrap_or(0),
    }
}

/// Decodes an `available`/`used` array of resource lines.
fn resource_amounts_from_llsd(value: Option<&Llsd>) -> Vec<ResourceAmount> {
    value
        .and_then(Llsd::as_array)
        .map(|amounts| amounts.iter().map(resource_amount_from_llsd).collect())
        .unwrap_or_default()
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
fn resource_summary_from_llsd(value: &Llsd) -> ResourceSummary {
    ResourceSummary {
        available: resource_amounts_from_llsd(value.get("available")),
        used: resource_amounts_from_llsd(value.get("used")),
    }
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
fn vector3_from_llsd(value: Option<&Llsd>) -> [f32; 3] {
    let component = |index: usize| {
        value
            .and_then(|array| array.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    [component(0), component(1), component(2)]
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
        ("location".to_owned(), vector3_to_llsd(object.location)),
        ("name".to_owned(), Llsd::String(object.name.clone())),
        ("owner_id".to_owned(), Llsd::Uuid(object.owner.uuid())),
        ("resources".to_owned(), Llsd::Map(resources)),
    ]))
}

/// Decodes one [`ScriptedObjectInfo`] from its LLSD map.
fn scripted_object_from_llsd(value: &Llsd) -> ScriptedObjectInfo {
    let resources = value.get("resources");
    ScriptedObjectInfo {
        id: value.get("id").and_then(Llsd::as_uuid).unwrap_or_default(),
        location: vector3_from_llsd(value.get("location")),
        name: value
            .get("name")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        owner: owner_key_from_wire(
            value
                .get("owner_id")
                .and_then(Llsd::as_uuid)
                .unwrap_or_default(),
            value
                .get("is_group_owned")
                .and_then(Llsd::as_bool)
                .unwrap_or(false),
        ),
        resources: ScriptedObjectResources {
            memory: resources
                .and_then(|map| map.get("memory"))
                .and_then(Llsd::as_i32),
            urls: resources
                .and_then(|map| map.get("urls"))
                .and_then(Llsd::as_i32),
        },
    }
}

/// Decodes an array of [`ScriptedObjectInfo`].
fn scripted_objects_from_llsd(value: Option<&Llsd>) -> Vec<ScriptedObjectInfo> {
    value
        .and_then(Llsd::as_array)
        .map(|objects| objects.iter().map(scripted_object_from_llsd).collect())
        .unwrap_or_default()
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
#[must_use]
pub fn parse_attachment_resources(body: &Llsd) -> AttachmentResourcesReport {
    let attachments = body
        .get("attachments")
        .and_then(Llsd::as_array)
        .map(|points| {
            points
                .iter()
                .map(|point| AttachmentLocation {
                    location: point
                        .get("location")
                        .and_then(Llsd::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    objects: scripted_objects_from_llsd(point.get("objects")),
                })
                .collect()
        })
        .unwrap_or_default();
    let summary = body
        .get("summary")
        .map(resource_summary_from_llsd)
        .unwrap_or_default();
    AttachmentResourcesReport {
        attachments,
        summary,
    }
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
    /// (`ScriptResourceSummary`).
    pub script_resource_summary: String,
    /// The URL of the parcel's resource-detail follow-up cap
    /// (`ScriptResourceDetails`), when the agent may see it.
    pub script_resource_details: Option<String>,
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
#[must_use]
pub fn parse_land_resources_request(body: &Llsd) -> Option<ParcelKey> {
    body.get("parcel_id")
        .and_then(Llsd::as_uuid)
        .map(ParcelKey::from)
}

/// Decodes a `LandResources` reply: the follow-up capability URLs.
#[must_use]
pub fn parse_land_resources_reply(body: &Llsd) -> LandResourcesUrls {
    LandResourcesUrls {
        script_resource_summary: body
            .get("ScriptResourceSummary")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned(),
        script_resource_details: body
            .get("ScriptResourceDetails")
            .and_then(Llsd::as_str)
            .map(str::to_owned),
    }
}

/// Builds a `LandResources` reply from the follow-up URLs (server side) — the
/// inverse of [`parse_land_resources_reply`].
#[must_use]
pub fn build_land_resources_response(urls: &LandResourcesUrls) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::from([(
        "ScriptResourceSummary".to_owned(),
        Llsd::String(urls.script_resource_summary.clone()),
    )]);
    if let Some(details) = &urls.script_resource_details {
        let _previous = map.insert(
            "ScriptResourceDetails".to_owned(),
            Llsd::String(details.clone()),
        );
    }
    Llsd::Map(map).to_llsd_xml()
}

// ---------------------------------------------------------------------------
// LandResources follow-up reports: ScriptResourceSummary / ScriptResourceDetails
// ---------------------------------------------------------------------------

/// Decodes a `ScriptResourceSummary` follow-up report: the parcel's resource
/// totals (carried under a `summary` key).
#[must_use]
pub fn parse_land_resource_summary(body: &Llsd) -> ResourceSummary {
    body.get("summary")
        .map(resource_summary_from_llsd)
        .unwrap_or_default()
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
#[must_use]
pub fn parse_land_resource_detail(body: &Llsd) -> Vec<ParcelScriptResources> {
    body.get("parcels")
        .and_then(Llsd::as_array)
        .map(|parcels| {
            parcels
                .iter()
                .map(|parcel| ParcelScriptResources {
                    name: parcel
                        .get("name")
                        .and_then(Llsd::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    id: parcel.get("id").and_then(Llsd::as_uuid).unwrap_or_default(),
                    local_id: crate::RegionLocalParcelId(
                        parcel.get("local_id").and_then(Llsd::as_i32).unwrap_or(0),
                    ),
                    objects: scripted_objects_from_llsd(parcel.get("objects")),
                })
                .collect()
        })
        .unwrap_or_default()
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
            location: [128.0, 64.5, 25.0],
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
        );
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
        );
        assert_eq!(parsed, Some(parcel_id));

        let urls = LandResourcesUrls {
            script_resource_summary: "http://sim/cap/srs".to_owned(),
            script_resource_details: Some("http://sim/cap/srd".to_owned()),
        };
        let xml = build_land_resources_response(&urls);
        let parsed = parse_land_resources_reply(
            &parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?,
        );
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
        );
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
        );
        assert_eq!(parsed, parcels);
        Ok(())
    }
}
