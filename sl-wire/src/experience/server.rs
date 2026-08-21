//! Server side: experience cap request parsers and response builders.

use super::{
    ExperienceInfo, ExperiencePermission, ExperienceUpdate, llsd_uuid, parse_region_experiences,
};
use crate::WireError;
use crate::llsd::{Llsd, LlsdError, parse_llsd_xml};
use crate::url::{percent_decode, query_param, url_query};
use sl_types::key::ExperienceKey;
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Server side — the inverse of the request builders / response parsers above.
//
// A grid's experience service parses the URL/body a viewer sends (the request
// parsers) and serializes the replies the viewer's response parsers above
// decode (the response builders). Each is the exact inverse of its client-side
// counterpart, so a request round-trips builder → parser and a reply round-trips
// builder → parser.
// ---------------------------------------------------------------------------

/// Parses the [`experience_info_query`](crate::experience_info_query) URL suffix back into the requested ids
/// (every `public_id` query parameter). Unparsable ids are skipped; an absent
/// query yields an empty list.
#[must_use]
pub fn parse_experience_info_query(suffix: &str) -> Vec<ExperienceKey> {
    let Some(query) = url_query(suffix) else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _value)| *key == "public_id")
        .filter_map(|(_key, value)| Uuid::parse_str(value).ok())
        .map(ExperienceKey::from)
        .collect()
}

/// Parses the [`find_experience_query`](crate::find_experience_query) URL suffix back into its
/// `(search text, page)` pair (the text percent-decoded), or `None` if it does
/// not match.
#[must_use]
pub fn parse_find_experience_query(suffix: &str) -> Option<(String, i32)> {
    let query = url_query(suffix)?;
    let page = query_param(query, "page")?.parse::<i32>().ok()?;
    let text = query_param(query, "query")?;
    Some((percent_decode(text), page))
}

/// Parses the bare-UUID query form (`?<id>`) shared by
/// [`group_experiences_query`] and [`forget_experience_query`].
fn parse_bare_uuid_query(suffix: &str) -> Option<Uuid> {
    Uuid::parse_str(url_query(suffix)?).ok()
}

/// Parses the [`group_experiences_query`](crate::group_experiences_query) URL suffix back into its group id
/// (`?<group_id>`), or `None` if it does not match.
#[must_use]
pub fn parse_group_experiences_query(suffix: &str) -> Option<Uuid> {
    parse_bare_uuid_query(suffix)
}

/// Parses the [`forget_experience_query`](crate::forget_experience_query) URL suffix back into its experience id
/// (the `Forget` DELETE target, `?<experience_id>`), or `None` if it does not
/// match.
#[must_use]
pub fn parse_forget_experience_query(suffix: &str) -> Option<ExperienceKey> {
    parse_bare_uuid_query(suffix).map(ExperienceKey::from)
}

/// Parses the [`experience_id_query`](crate::experience_id_query) URL suffix back into its experience id
/// (`?experience_id=<id>`), or `None` if it does not match.
#[must_use]
pub fn parse_experience_id_query(suffix: &str) -> Option<ExperienceKey> {
    let query = url_query(suffix)?;
    Uuid::parse_str(query_param(query, "experience_id")?)
        .ok()
        .map(ExperienceKey::from)
}

/// Parses an `ExperiencePreferences` PUT body
/// (`{ "<id>": { "permission": "Allow"|"Block" } }`) back into its
/// `(experience id, permission)` pair — the inverse of
/// [`build_set_experience_permission_request`](crate::build_set_experience_permission_request). Returns `Ok(None)` when the body
/// is well-formed XML but not a single id→permission entry.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_set_experience_permission_request(
    xml: &str,
) -> Result<Option<(ExperienceKey, ExperiencePermission)>, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let Some(map) = root.as_map() else {
        return Ok(None);
    };
    let Some((key, value)) = map.iter().next() else {
        return Ok(None);
    };
    let Ok(id) = Uuid::parse_str(key) else {
        return Ok(None);
    };
    let permission = value
        .get("permission")
        .and_then(Llsd::as_str)
        .and_then(ExperiencePermission::from_wire);
    Ok(permission.map(|permission| (ExperienceKey::from(id), permission)))
}

/// Parses an `UpdateExperience` POST body back into an [`ExperienceUpdate`] — the
/// inverse of [`build_update_experience_request`](crate::build_update_experience_request). Missing fields take their
/// defaults, mirroring the lenient decoding elsewhere in this module.
///
/// # Errors
///
/// Returns a [`LlsdError::MalformedField`] if the body is not well-formed XML or
/// a present field has the wrong LLSD kind.
pub fn parse_update_experience_request(xml: &str) -> Result<ExperienceUpdate, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| LlsdError::MalformedField {
        field: "UpdateExperience",
        value: error.to_string(),
    })?;
    let string = |key: &'static str| -> Result<String, WireError> {
        Ok(root.field_str(key, key)?.unwrap_or_default().to_owned())
    };
    Ok(ExperienceUpdate {
        public_id: ExperienceKey::from(
            root.get("public_id")
                .and_then(llsd_uuid)
                .unwrap_or_default(),
        ),
        name: string("name")?,
        description: string("description")?,
        maturity: root.field_i32("maturity", "maturity")?.unwrap_or(0),
        properties: root.field_i32("properties", "properties")?.unwrap_or(0),
        slurl: crate::optional_url_from_wire("slurl", &string("slurl")?)?,
        extended_metadata: string("extended_metadata")?,
    })
}

/// Parses a `RegionExperiences` POST body back into its
/// `(allowed, blocked, trusted)` id lists — the inverse of
/// [`build_region_experiences_request`](crate::build_region_experiences_request). (The body and reply share a shape, so
/// this delegates to [`parse_region_experiences`].)
///
/// # Errors
///
/// Returns a [`LlsdError::MalformedField`] if the body is not well-formed XML or
/// a present `allowed`/`blocked`/`trusted` field has the wrong LLSD kind.
#[expect(
    clippy::type_complexity,
    reason = "mirrors parse_region_experiences' (allowed, blocked, trusted) tuple, wrapped in Result for the malformed-field error"
)]
pub fn parse_region_experiences_request(
    xml: &str,
) -> Result<(Vec<ExperienceKey>, Vec<ExperienceKey>, Vec<ExperienceKey>), WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| LlsdError::MalformedField {
        field: "RegionExperiences",
        value: error.to_string(),
    })?;
    parse_region_experiences(&root)
}

/// Builds an array-of-UUIDs LLSD value from experience ids.
fn uuid_array_llsd(ids: &[ExperienceKey]) -> Llsd {
    Llsd::Array(ids.iter().map(|id| Llsd::Uuid(id.uuid())).collect())
}

/// Builds a `GetExperienceInfo` / `FindExperienceByName` reply
/// (`{ experience_keys, error_ids }`) from a list of records — the inverse of
/// [`parse_experience_infos`](crate::parse_experience_infos). Records flagged [`missing`](ExperienceInfo::missing)
/// are emitted as bare ids in `error_ids` (the grid's "could not resolve" form),
/// the rest as full `experience_keys` maps; `error_ids` is omitted when empty.
/// Built on [`Llsd::to_llsd_xml`], so it round-trips through [`parse_llsd_xml`].
#[must_use]
pub fn build_experience_infos_response(infos: &[ExperienceInfo]) -> String {
    let mut keys = Vec::new();
    let mut errors = Vec::new();
    for info in infos {
        if info.missing {
            errors.push(Llsd::Uuid(info.public_id.uuid()));
        } else {
            keys.push(info.to_llsd());
        }
    }
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert("experience_keys".to_owned(), Llsd::Array(keys));
    if !errors.is_empty() {
        let _previous = map.insert("error_ids".to_owned(), Llsd::Array(errors));
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Builds an `AgentExperiences` / `GetAdminExperiences` / `GetCreatorExperiences`
/// / `GroupExperiences` reply (`{ experience_ids }`) — the inverse of
/// [`parse_experience_ids`](crate::parse_experience_ids).
#[must_use]
pub fn build_experience_ids_response(ids: &[ExperienceKey]) -> String {
    Llsd::Map(HashMap::from([(
        "experience_ids".to_owned(),
        uuid_array_llsd(ids),
    )]))
    .to_llsd_xml()
}

/// Builds a `GetExperiences` / `ExperiencePreferences` reply
/// (`{ experiences, blocked }`) — the inverse of
/// [`parse_experience_permissions`](crate::parse_experience_permissions).
#[must_use]
pub fn build_experience_permissions_response(
    allowed: &[ExperienceKey],
    blocked: &[ExperienceKey],
) -> String {
    Llsd::Map(HashMap::from([
        ("experiences".to_owned(), uuid_array_llsd(allowed)),
        ("blocked".to_owned(), uuid_array_llsd(blocked)),
    ]))
    .to_llsd_xml()
}

/// Builds a `RegionExperiences` reply (`{ allowed, blocked, trusted }`) — the
/// inverse of [`parse_region_experiences`]. (The reply shares its shape with the
/// POST body that [`build_region_experiences_request`](crate::build_region_experiences_request) writes.)
#[must_use]
pub fn build_region_experiences_response(
    allowed: &[ExperienceKey],
    blocked: &[ExperienceKey],
    trusted: &[ExperienceKey],
) -> String {
    Llsd::Map(HashMap::from([
        ("allowed".to_owned(), uuid_array_llsd(allowed)),
        ("blocked".to_owned(), uuid_array_llsd(blocked)),
        ("trusted".to_owned(), uuid_array_llsd(trusted)),
    ]))
    .to_llsd_xml()
}

/// Builds an `IsExperienceAdmin` / `IsExperienceContributor` reply
/// (`{ status }`) — the inverse of [`parse_experience_status`](crate::parse_experience_status).
#[must_use]
pub fn build_experience_status_response(status: bool) -> String {
    Llsd::Map(HashMap::from([(
        "status".to_owned(),
        Llsd::Boolean(status),
    )]))
    .to_llsd_xml()
}
