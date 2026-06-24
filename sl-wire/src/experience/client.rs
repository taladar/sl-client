//! Client side: experience cap request builders and response parsers.

use super::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate, PROPERTY_INVALID,
    SEARCH_PAGE_SIZE, uuid_array,
};
use crate::WireError;
use crate::llsd::{Llsd, push_escaped};
use sl_types::key::ExperienceKey;
use uuid::Uuid;

/// Percent-encodes `text` for a URL query value (RFC 3986 unreserved set kept,
/// everything else `%`-escaped). Used for the `FindExperienceByName` query.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        }
    }
    out
}

/// Maps a nibble (0–15) to its uppercase ASCII hex digit (a match, so no
/// arithmetic or indexing).
const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'A',
        11 => 'B',
        12 => 'C',
        13 => 'D',
        14 => 'E',
        _ => 'F',
    }
}

/// Builds the URL suffix for a `GetExperienceInfo` GET, to be appended directly
/// to the capability URL (`{cap}{suffix}` → `{cap}/id/?page_size=N&public_id=…`).
/// Each requested id is added as a `public_id` query parameter, batching the
/// lookup into one request as the viewer does.
#[must_use]
pub fn experience_info_query(ids: &[ExperienceKey]) -> String {
    let page_size = ids.len().max(1);
    let mut out = format!("/id/?page_size={page_size}");
    for id in ids {
        out.push_str("&public_id=");
        out.push_str(&id.to_string());
    }
    out
}

/// Builds the URL suffix for a `FindExperienceByName` GET (`{cap}?page=…&page_size=…&query=…`).
#[must_use]
pub fn find_experience_query(text: &str, page: i32) -> String {
    format!(
        "?page={page}&page_size={SEARCH_PAGE_SIZE}&query={}",
        percent_encode(text)
    )
}

/// Builds the URL suffix for a `GroupExperiences` GET (`{cap}?<group_id>`).
#[must_use]
pub fn group_experiences_query(group_id: Uuid) -> String {
    format!("?{group_id}")
}

/// Builds the URL suffix for an `IsExperienceAdmin` / `IsExperienceContributor`
/// GET (`{cap}?experience_id=<id>`).
#[must_use]
pub fn experience_id_query(experience_id: ExperienceKey) -> String {
    format!("?experience_id={experience_id}")
}

/// Builds the URL suffix for the `Forget` form of an `ExperiencePreferences`
/// change — an HTTP DELETE to `{cap}?<experience_id>` (no body).
#[must_use]
pub fn forget_experience_query(experience_id: ExperienceKey) -> String {
    format!("?{experience_id}")
}

/// Builds the LLSD-XML body for the `Allow`/`Block` form of an
/// `ExperiencePreferences` change — an HTTP PUT of `{ "<id>": { "permission":
/// "Allow"|"Block" } }`. The `Forget` form carries no body (see
/// [`forget_experience_query`]); passing [`ExperiencePermission::Forget`] here
/// yields an empty `permission`, which the caller should avoid by routing it to
/// the DELETE path instead.
#[must_use]
pub fn build_set_experience_permission_request(
    experience_id: ExperienceKey,
    permission: ExperiencePermission,
) -> String {
    format!(
        "<llsd><map><key>{experience_id}</key><map><key>permission</key><string>{}</string></map></map></llsd>",
        permission.as_str()
    )
}

/// Builds the LLSD-XML body for an `UpdateExperience` POST (the editable
/// metadata; `quota`/`expiration`/`agent_id` are server-controlled and omitted,
/// as the viewer does).
#[must_use]
pub fn build_update_experience_request(update: &ExperienceUpdate) -> String {
    let mut out = format!(
        "<llsd><map><key>public_id</key><uuid>{}</uuid><key>name</key><string>",
        update.public_id
    );
    push_escaped(&mut out, &update.name);
    out.push_str("</string><key>description</key><string>");
    push_escaped(&mut out, &update.description);
    out.push_str("</string><key>maturity</key><integer>");
    out.push_str(&update.maturity.to_string());
    out.push_str("</integer><key>properties</key><integer>");
    out.push_str(&update.properties.to_string());
    out.push_str("</integer><key>slurl</key><string>");
    push_escaped(&mut out, &update.slurl);
    out.push_str("</string><key>extended_metadata</key><string>");
    push_escaped(&mut out, &update.extended_metadata);
    out.push_str("</string></map></llsd>");
    out
}

/// Builds the LLSD-XML body for a `RegionExperiences` POST (the estate update):
/// the three id lists the region allows / blocks / trusts.
#[must_use]
pub fn build_region_experiences_request(
    allowed: &[ExperienceKey],
    blocked: &[ExperienceKey],
    trusted: &[ExperienceKey],
) -> String {
    let mut out = String::from("<llsd><map>");
    for (key, ids) in [
        ("allowed", allowed),
        ("blocked", blocked),
        ("trusted", trusted),
    ] {
        out.push_str("<key>");
        out.push_str(key);
        out.push_str("</key><array>");
        for id in ids {
            out.push_str("<uuid>");
            out.push_str(&id.to_string());
            out.push_str("</uuid>");
        }
        out.push_str("</array>");
    }
    out.push_str("</map></llsd>");
    out
}

/// Decodes the `experience_keys` array of a `GetExperienceInfo` /
/// `FindExperienceByName` / `UpdateExperience` reply into [`ExperienceInfo`]
/// records. Any `error_ids` are folded in as `missing` placeholders (matching the
/// viewer, which inserts an [`PROPERTY_INVALID`] cache entry for each). A reply
/// that is itself a single flat experience map (as `UpdateExperience` returns) is
/// decoded as one record.
///
/// # Errors
///
/// Returns a [`WireError::MalformedField`] if a decoded LLSD field has the wrong
/// kind.
pub fn parse_experience_infos(body: &Llsd) -> Result<Vec<ExperienceInfo>, WireError> {
    let mut infos = Vec::new();
    if let Some(keys) = body.field_array("experience_keys", "experience_keys")? {
        for key in keys {
            infos.push(ExperienceInfo::from_llsd(key)?);
        }
    } else if body.get("public_id").is_some() {
        // A bare experience map (the `UpdateExperience` reply shape).
        infos.push(ExperienceInfo::from_llsd(body)?);
    }
    for id in uuid_array(body, "error_ids")? {
        infos.push(ExperienceInfo {
            public_id: ExperienceKey::from(id),
            properties: ExperienceProperties(PROPERTY_INVALID),
            missing: true,
            ..ExperienceInfo::default()
        });
    }
    Ok(infos)
}

/// Decodes the `experience_ids` array of an `AgentExperiences` /
/// `GetAdminExperiences` / `GetCreatorExperiences` / `GroupExperiences` reply.
///
/// # Errors
///
/// Returns a [`WireError::MalformedField`] if `experience_ids` is present but not
/// an LLSD array.
pub fn parse_experience_ids(body: &Llsd) -> Result<Vec<ExperienceKey>, WireError> {
    Ok(uuid_array(body, "experience_ids")?
        .into_iter()
        .map(ExperienceKey::from)
        .collect())
}

/// Decodes the `{ experiences, blocked }` of a `GetExperiences` /
/// `ExperiencePreferences` reply into the agent's allowed and blocked id lists.
///
/// # Errors
///
/// Returns a [`WireError::MalformedField`] if `experiences` or `blocked` is
/// present but not an LLSD array.
pub fn parse_experience_permissions(
    body: &Llsd,
) -> Result<(Vec<ExperienceKey>, Vec<ExperienceKey>), WireError> {
    Ok((
        uuid_array(body, "experiences")?
            .into_iter()
            .map(ExperienceKey::from)
            .collect(),
        uuid_array(body, "blocked")?
            .into_iter()
            .map(ExperienceKey::from)
            .collect(),
    ))
}

/// Decodes the `{ allowed, blocked, trusted }` of a `RegionExperiences` reply.
///
/// # Errors
///
/// Returns a [`WireError::MalformedField`] if `allowed`, `blocked`, or `trusted`
/// is present but not an LLSD array.
#[expect(
    clippy::type_complexity,
    reason = "the (allowed, blocked, trusted) tuple, wrapped in Result for the malformed-field error"
)]
pub fn parse_region_experiences(
    body: &Llsd,
) -> Result<(Vec<ExperienceKey>, Vec<ExperienceKey>, Vec<ExperienceKey>), WireError> {
    let keys = |name: &'static str| -> Result<Vec<ExperienceKey>, WireError> {
        Ok(uuid_array(body, name)?
            .into_iter()
            .map(ExperienceKey::from)
            .collect())
    };
    Ok((keys("allowed")?, keys("blocked")?, keys("trusted")?))
}

/// Decodes the `{ status }` boolean of an `IsExperienceAdmin` /
/// `IsExperienceContributor` reply.
///
/// # Errors
///
/// Returns a [`WireError::MalformedField`] if `status` is present but not an LLSD
/// boolean.
pub fn parse_experience_status(body: &Llsd) -> Result<bool, WireError> {
    Ok(body.field_bool("status", "status")?.unwrap_or(false))
}
