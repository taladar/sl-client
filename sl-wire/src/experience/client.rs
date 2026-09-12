//! Client side: experience cap request builders and response parsers.

use super::{
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceSearchPage,
    ExperienceUpdate, PROPERTY_INVALID, RegionExperienceLists, SEARCH_PAGE_SIZE, llsd_uuid,
    uuid_array,
};
use crate::WireError;
use crate::llsd::{Llsd, push_escaped};
use crate::url::percent_encode;
use sl_types::key::ExperienceKey;
use uuid::Uuid;

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

/// Builds the URL suffix for an `ExperienceQuery` GET
/// (`{cap}?parcelid=<id>&experiences=<id>,<id>,…`) — "of the experiences
/// currently injecting something, which does this parcel admit?".
///
/// The `experiences` parameter is omitted entirely when the list is empty,
/// which is what the reference's own string building does
/// (`DayInjection::testExperiencesOnParcelCoro`, `indra/newview/llenvironment.cpp`):
/// it writes the parameter name only before the *first* id.
#[must_use]
pub fn experience_query(parcel_id: i32, experiences: &[ExperienceKey]) -> String {
    let mut out = format!("?parcelid={parcel_id}");
    for (index, id) in experiences.iter().enumerate() {
        out.push_str(if index == 0 { "&experiences=" } else { "," });
        out.push_str(&id.to_string());
    }
    out
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
    push_escaped(
        &mut out,
        &crate::optional_url_to_wire(update.slurl.as_ref()),
    );
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
/// Returns a [`WireError::Llsd`] if a decoded LLSD field has the wrong
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

/// Decodes a `FindExperienceByName` reply into one [`ExperienceSearchPage`]:
/// the `experience_keys` records ([`parse_experience_infos`]) plus the
/// `next_page_url` / `previous_page_url` markers, read — as the reference reads
/// them — by presence alone.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] if a decoded LLSD field has the wrong kind.
pub fn parse_experience_search_page(body: &Llsd) -> Result<ExperienceSearchPage, WireError> {
    // `Undef` is how an LLSD-XML `<undef/>` arrives, and the reference's
    // `has()` is false for a key that is not in the map at all; a key present
    // but undefined names no page either, so both count as absent.
    let offered = |field: &str| !matches!(body.get(field), None | Some(Llsd::Undef));
    Ok(ExperienceSearchPage {
        infos: parse_experience_infos(body)?,
        has_next_page: offered("next_page_url"),
        has_previous_page: offered("previous_page_url"),
    })
}

/// Decodes the `experience_ids` array of an `AgentExperiences` /
/// `GetAdminExperiences` / `GetCreatorExperiences` / `GroupExperiences` reply.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] if `experience_ids` is present but not
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
/// Returns a [`WireError::Llsd`] if `experiences` or `blocked` is
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

/// Decodes the `{ allowed, blocked, trusted }` of a `RegionExperiences` reply,
/// plus the estate's `default` experience when the reply names one.
///
/// The `default` key is read leniently — absent, `<undef/>`, or a value that is
/// not a UUID all decode as "no default" — because the reference reads it the
/// same way (`content.has("default")`, then `asUUID()`), and a grid that has no
/// estate default simply does not send it.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] if `allowed`, `blocked`, or `trusted`
/// is present but not an LLSD array.
pub fn parse_region_experiences(body: &Llsd) -> Result<RegionExperienceLists, WireError> {
    let keys = |name: &'static str| -> Result<Vec<ExperienceKey>, WireError> {
        Ok(uuid_array(body, name)?
            .into_iter()
            .map(ExperienceKey::from)
            .collect())
    };
    Ok(RegionExperienceLists {
        allowed: keys("allowed")?,
        blocked: keys("blocked")?,
        trusted: keys("trusted")?,
        default_experience: body
            .get("default")
            .and_then(llsd_uuid)
            .map(ExperienceKey::from),
    })
}

/// Decodes the `{ experiences: { "<id>": bool, … } }` of an `ExperienceQuery`
/// reply: for each queried experience, whether the parcel admits it. Sorted by
/// id, so a caller comparing two replies compares two identical orders.
///
/// An entry whose key is not a UUID, or whose value is not a boolean, is
/// skipped: the reference reads each entry with `asBoolean()` and acts only on
/// the ones that say *no*, so an unreadable entry must not become a clear.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] if `experiences` is present but not an LLSD
/// map.
pub fn parse_experience_query_reply(body: &Llsd) -> Result<Vec<(ExperienceKey, bool)>, WireError> {
    let Some(entries) = body.field_map("experiences", "experiences")? else {
        return Ok(Vec::new());
    };
    let mut admitted: Vec<(ExperienceKey, bool)> = entries
        .iter()
        .filter_map(|(id, allowed)| {
            Some((
                ExperienceKey::from(Uuid::parse_str(id.trim()).ok()?),
                allowed.as_bool()?,
            ))
        })
        .collect();
    admitted.sort_unstable();
    Ok(admitted)
}

/// Decodes the `{ status }` boolean of an `IsExperienceAdmin` /
/// `IsExperienceContributor` reply.
///
/// # Errors
///
/// Returns a [`WireError::Llsd`] if `status` is present but not an LLSD
/// boolean.
pub fn parse_experience_status(body: &Llsd) -> Result<bool, WireError> {
    Ok(body.field_bool("status", "status")?.unwrap_or(false))
}
