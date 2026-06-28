//! Second Life "Experiences" over the region capabilities.
//!
//! An *experience* is a named, grid- or land-scoped grant a script asks an agent
//! for (`llRequestExperiencePermissions`, surfaced over UDP as the `ScriptQuestion`
//! `Experience` block — see `ScriptPermissionRequest`). Once admitted, the agent's
//! per-experience allow/block preference and the experience's metadata are managed
//! entirely over a family of HTTP capabilities served only by Second Life (stock
//! OpenSim ships no experience module, so these caps are usually absent there).
//!
//! This module builds the cap request bodies / query strings and decodes the
//! replies. Field names, LLSD keys, the property bitflags, and the request/response
//! shapes are cross-checked against the Firestorm viewer's
//! `indra/llmessage/llexperiencecache.{h,cpp}` and `llfloaterexperiences.cpp`.
//!
//! The capabilities, by HTTP verb:
//!
//! - `GetExperienceInfo` — GET `…/id/?page_size=N&public_id=<id>&…`, batch metadata
//!   lookup → `{ experience_keys: [ … ], error_ids: [ … ] }`.
//! - `FindExperienceByName` — GET `…?page=N&page_size=M&query=<text>` → `{ experience_keys }`.
//! - `GetExperiences` — GET, the agent's admitted/blocked experiences → `{ experiences, blocked }`.
//! - `AgentExperiences` / `GetAdminExperiences` / `GetCreatorExperiences` — GET,
//!   the experiences the agent owns / administers / created → `{ experience_ids }`.
//! - `GroupExperiences` — GET `…?<group_id>` → `{ experience_ids }`.
//! - `ExperiencePreferences` — PUT `{ "<id>": { permission } }` to allow/block, or
//!   DELETE `…?<id>` to forget; both reply `{ experiences, blocked }`.
//! - `IsExperienceAdmin` / `IsExperienceContributor` — GET `…?experience_id=<id>` → `{ status }`.
//! - `UpdateExperience` — POST the editable metadata → the updated experience info.
//! - `RegionExperiences` — GET, or POST `{ allowed, blocked, trusted }` to update;
//!   both reply `{ allowed, blocked, trusted }`.

use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

mod client;
mod server;
#[cfg(test)]
mod tests;
mod types;

pub use client::{
    build_region_experiences_request, build_set_experience_permission_request,
    build_update_experience_request, experience_id_query, experience_info_query,
    find_experience_query, forget_experience_query, group_experiences_query, parse_experience_ids,
    parse_experience_infos, parse_experience_permissions, parse_experience_status,
    parse_region_experiences,
};
pub use server::{
    build_experience_ids_response, build_experience_infos_response,
    build_experience_permissions_response, build_experience_status_response,
    build_region_experiences_response, parse_experience_id_query, parse_experience_info_query,
    parse_find_experience_query, parse_forget_experience_query, parse_group_experiences_query,
    parse_region_experiences_request, parse_set_experience_permission_request,
    parse_update_experience_request,
};
pub use types::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
    PROPERTY_DISABLED, PROPERTY_GRID, PROPERTY_INVALID, PROPERTY_PRIVATE, PROPERTY_PRIVILEGED,
    PROPERTY_SUSPENDED, SEARCH_PAGE_SIZE,
};

/// Reads a UUID-valued LLSD value, accepting either a `uuid` or a `string`.
fn llsd_uuid(value: &Llsd) -> Option<Uuid> {
    value.as_uuid().or_else(|| {
        value
            .as_str()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
    })
}

/// Collects every UUID from the LLSD `array` at `map[key]` (skipping non-UUID
/// elements). An absent or `Undef` value yields an empty list; a present value
/// of the wrong LLSD kind is a [`WireError::Llsd`] labelled `key`.
fn uuid_array(map: &Llsd, key: &'static str) -> Result<Vec<Uuid>, WireError> {
    Ok(map
        .field_array(key, key)?
        .map(|array| array.iter().filter_map(llsd_uuid).collect())
        .unwrap_or_default())
}
