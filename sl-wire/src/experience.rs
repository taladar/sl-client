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

use crate::llsd::{Llsd, push_escaped};

/// Experience [`properties`](ExperienceInfo::properties) bit: the experience id is
/// invalid (a placeholder for an `error_ids` entry the grid could not resolve).
pub const PROPERTY_INVALID: i32 = 1 << 0;
/// Experience properties bit: privileged (a Linden-blessed experience).
pub const PROPERTY_PRIVILEGED: i32 = 1 << 3;
/// Experience properties bit: grid-wide scope (vs. land-scoped). Mirrors the
/// viewer's grid-scope notification on a permission request.
pub const PROPERTY_GRID: i32 = 1 << 4;
/// Experience properties bit: the experience is private.
pub const PROPERTY_PRIVATE: i32 = 1 << 5;
/// Experience properties bit: the experience is disabled.
pub const PROPERTY_DISABLED: i32 = 1 << 6;
/// Experience properties bit: the experience is suspended by an admin.
pub const PROPERTY_SUSPENDED: i32 = 1 << 7;

/// The `FindExperienceByName` results-per-page count the reference viewer sends.
pub const SEARCH_PAGE_SIZE: i32 = 30;

/// The bitfield of [`ExperienceInfo::properties`] flags (the `PROPERTY_*`
/// constants). Mirrors the viewer's `LLExperienceCache` property bits, which it
/// notes should track the grid's `experience-api` model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExperienceProperties(pub i32);

impl ExperienceProperties {
    /// Whether all of the bits in `flag` are set.
    #[must_use]
    pub const fn contains(self, flag: i32) -> bool {
        self.0 & flag == flag
    }

    /// Whether the experience id is invalid ([`PROPERTY_INVALID`]).
    #[must_use]
    pub const fn is_invalid(self) -> bool {
        self.contains(PROPERTY_INVALID)
    }

    /// Whether the experience is privileged ([`PROPERTY_PRIVILEGED`]).
    #[must_use]
    pub const fn is_privileged(self) -> bool {
        self.contains(PROPERTY_PRIVILEGED)
    }

    /// Whether the experience is grid-wide ([`PROPERTY_GRID`]); otherwise it is
    /// land-scoped.
    #[must_use]
    pub const fn is_grid(self) -> bool {
        self.contains(PROPERTY_GRID)
    }

    /// Whether the experience is private ([`PROPERTY_PRIVATE`]).
    #[must_use]
    pub const fn is_private(self) -> bool {
        self.contains(PROPERTY_PRIVATE)
    }

    /// Whether the experience is disabled ([`PROPERTY_DISABLED`]).
    #[must_use]
    pub const fn is_disabled(self) -> bool {
        self.contains(PROPERTY_DISABLED)
    }

    /// Whether the experience is suspended ([`PROPERTY_SUSPENDED`]).
    #[must_use]
    pub const fn is_suspended(self) -> bool {
        self.contains(PROPERTY_SUSPENDED)
    }
}

/// The per-experience preference an agent can set over `ExperiencePreferences`.
/// `Allow`/`Block` are sent as a PUT body; `Forget` clears any prior preference
/// (sent as a DELETE — see [`build_set_experience_permission_request`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperiencePermission {
    /// Admit the experience for this agent.
    Allow,
    /// Block the experience for this agent.
    Block,
    /// Forget any prior preference (neither allowed nor blocked).
    Forget,
}

impl ExperiencePermission {
    /// The wire string the cap expects (`"Allow"` / `"Block"` / `"Forget"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Block => "Block",
            Self::Forget => "Forget",
        }
    }

    /// Whether this preference is set with an HTTP DELETE (`Forget`) rather than a
    /// PUT (`Allow`/`Block`).
    #[must_use]
    pub const fn is_forget(self) -> bool {
        matches!(self, Self::Forget)
    }
}

/// A single experience's metadata record, as carried in a cap reply's
/// `experience_keys` array (and decoded by [`ExperienceInfo::from_llsd`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperienceInfo {
    /// The experience's public id (`public_id`) — the key used everywhere else.
    pub public_id: Uuid,
    /// The experience's display name.
    pub name: String,
    /// The owning agent (`agent_id`); nil when group-owned.
    pub agent_id: Uuid,
    /// The owning group (`group_id`); nil when agent-owned.
    pub group_id: Uuid,
    /// The free-text description.
    pub description: String,
    /// The [`ExperienceProperties`] bitfield.
    pub properties: ExperienceProperties,
    /// The script-memory quota in megabytes (`quota`).
    pub quota: i32,
    /// The cache expiration, in seconds (`expiration`).
    pub expiration: f64,
    /// The content rating (`maturity`; `sim_access` codes: PG 13 / Mature 34 /
    /// Adult 42).
    pub maturity: i32,
    /// A SLURL to the experience's home location (`slurl`).
    pub slurl: String,
    /// Opaque extended metadata XML (`extended_metadata`).
    pub extended_metadata: String,
    /// `true` when this is a placeholder for an `error_ids` entry — the grid could
    /// not resolve the id (also flagged via [`PROPERTY_INVALID`]).
    pub missing: bool,
}

impl ExperienceInfo {
    /// Decodes an [`ExperienceInfo`] from one `experience_keys` map element.
    /// Missing fields take their defaults rather than failing.
    #[must_use]
    pub fn from_llsd(map: &Llsd) -> Self {
        let string = |key: &str| {
            map.get(key)
                .and_then(Llsd::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Self {
            public_id: map.get("public_id").and_then(llsd_uuid).unwrap_or_default(),
            name: string("name"),
            agent_id: map.get("agent_id").and_then(llsd_uuid).unwrap_or_default(),
            group_id: map.get("group_id").and_then(llsd_uuid).unwrap_or_default(),
            description: string("description"),
            properties: ExperienceProperties(
                map.get("properties").and_then(Llsd::as_i32).unwrap_or(0),
            ),
            quota: map.get("quota").and_then(Llsd::as_i32).unwrap_or(0),
            expiration: map.get("expiration").and_then(Llsd::as_f64).unwrap_or(0.0),
            maturity: map.get("maturity").and_then(Llsd::as_i32).unwrap_or(0),
            slurl: string("slurl"),
            extended_metadata: string("extended_metadata"),
            missing: map
                .get("DoesNotExist")
                .and_then(Llsd::as_bool)
                .unwrap_or(false),
        }
    }
}

/// The editable metadata sent to the `UpdateExperience` cap (see
/// [`build_update_experience_request`]). The viewer omits `quota`, `expiration`
/// and `agent_id` (server-controlled), so this carries only the editable fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExperienceUpdate {
    /// The experience to update (`public_id`).
    pub public_id: Uuid,
    /// The new display name.
    pub name: String,
    /// The new description.
    pub description: String,
    /// The new content rating (`maturity`).
    pub maturity: i32,
    /// The new [`ExperienceProperties`] bits (only admins may change them).
    pub properties: i32,
    /// The new home-location SLURL.
    pub slurl: String,
    /// The new extended-metadata XML.
    pub extended_metadata: String,
}

/// Reads a UUID-valued LLSD value, accepting either a `uuid` or a `string`.
fn llsd_uuid(value: &Llsd) -> Option<Uuid> {
    value.as_uuid().or_else(|| {
        value
            .as_str()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
    })
}

/// Collects every UUID from an LLSD `array` value (skipping non-UUID elements).
fn uuid_array(value: Option<&Llsd>) -> Vec<Uuid> {
    value
        .and_then(Llsd::as_array)
        .map(|array| array.iter().filter_map(llsd_uuid).collect())
        .unwrap_or_default()
}

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
pub fn experience_info_query(ids: &[Uuid]) -> String {
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
pub fn experience_id_query(experience_id: Uuid) -> String {
    format!("?experience_id={experience_id}")
}

/// Builds the URL suffix for the `Forget` form of an `ExperiencePreferences`
/// change — an HTTP DELETE to `{cap}?<experience_id>` (no body).
#[must_use]
pub fn forget_experience_query(experience_id: Uuid) -> String {
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
    experience_id: Uuid,
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
    allowed: &[Uuid],
    blocked: &[Uuid],
    trusted: &[Uuid],
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
#[must_use]
pub fn parse_experience_infos(body: &Llsd) -> Vec<ExperienceInfo> {
    let mut infos = Vec::new();
    if let Some(keys) = body.get("experience_keys").and_then(Llsd::as_array) {
        infos.extend(keys.iter().map(ExperienceInfo::from_llsd));
    } else if body.get("public_id").is_some() {
        // A bare experience map (the `UpdateExperience` reply shape).
        infos.push(ExperienceInfo::from_llsd(body));
    }
    for id in uuid_array(body.get("error_ids")) {
        infos.push(ExperienceInfo {
            public_id: id,
            properties: ExperienceProperties(PROPERTY_INVALID),
            missing: true,
            ..ExperienceInfo::default()
        });
    }
    infos
}

/// Decodes the `experience_ids` array of an `AgentExperiences` /
/// `GetAdminExperiences` / `GetCreatorExperiences` / `GroupExperiences` reply.
#[must_use]
pub fn parse_experience_ids(body: &Llsd) -> Vec<Uuid> {
    uuid_array(body.get("experience_ids"))
}

/// Decodes the `{ experiences, blocked }` of a `GetExperiences` /
/// `ExperiencePreferences` reply into the agent's allowed and blocked id lists.
#[must_use]
pub fn parse_experience_permissions(body: &Llsd) -> (Vec<Uuid>, Vec<Uuid>) {
    (
        uuid_array(body.get("experiences")),
        uuid_array(body.get("blocked")),
    )
}

/// Decodes the `{ allowed, blocked, trusted }` of a `RegionExperiences` reply.
#[must_use]
pub fn parse_region_experiences(body: &Llsd) -> (Vec<Uuid>, Vec<Uuid>, Vec<Uuid>) {
    (
        uuid_array(body.get("allowed")),
        uuid_array(body.get("blocked")),
        uuid_array(body.get("trusted")),
    )
}

/// Decodes the `{ status }` boolean of an `IsExperienceAdmin` /
/// `IsExperienceContributor` reply.
#[must_use]
pub fn parse_experience_status(body: &Llsd) -> bool {
    body.get("status").and_then(Llsd::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
        PROPERTY_GRID, build_region_experiences_request, build_set_experience_permission_request,
        build_update_experience_request, experience_info_query, find_experience_query,
        parse_experience_ids, parse_experience_infos, parse_experience_permissions,
        parse_experience_status, parse_region_experiences,
    };
    use crate::llsd::parse_llsd_xml;

    /// `GetExperienceInfo` batches every id as a `public_id` query parameter under
    /// the `id/` path, and its `experience_keys` decode into full records while
    /// `error_ids` become `missing` placeholders.
    #[test]
    fn experience_info_query_and_decode() -> Result<(), String> {
        let id =
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
        let suffix = experience_info_query(&[id]);
        assert_eq!(
            suffix,
            "/id/?page_size=1&public_id=11111111-1111-1111-1111-111111111111"
        );

        let reply = parse_llsd_xml(concat!(
            "<llsd><map><key>experience_keys</key><array><map>",
            "<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
            "<key>name</key><string>My Experience</string>",
            "<key>agent_id</key><uuid>22222222-2222-2222-2222-222222222222</uuid>",
            "<key>properties</key><integer>16</integer>",
            "<key>maturity</key><integer>13</integer>",
            "<key>description</key><string>fun</string>",
            "<key>slurl</key><string>http://maps/x</string>",
            "</map></array>",
            "<key>error_ids</key><array>",
            "<uuid>33333333-3333-3333-3333-333333333333</uuid></array>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let infos = parse_experience_infos(&reply);
        let [first, second] = infos.as_slice() else {
            return Err(format!("expected 2 infos, got {}", infos.len()));
        };
        assert_eq!(first.public_id, id);
        assert_eq!(first.name, "My Experience");
        assert!(first.properties.is_grid());
        assert_eq!(first.maturity, 13);
        assert!(!first.missing);
        assert!(second.missing);
        assert!(second.properties.is_invalid());
        Ok(())
    }

    /// The search query escapes its text and carries the page / page-size.
    #[test]
    fn find_experience_query_escapes() {
        assert_eq!(
            find_experience_query("a b&c", 2),
            "?page=2&page_size=30&query=a%20b%26c"
        );
    }

    /// `experience_ids` and `{ experiences, blocked }` replies decode to id lists.
    #[test]
    fn id_list_and_permission_decode() -> Result<(), String> {
        let ids_reply = parse_llsd_xml(concat!(
            "<llsd><map><key>experience_ids</key><array>",
            "<uuid>11111111-1111-1111-1111-111111111111</uuid>",
            "<uuid>22222222-2222-2222-2222-222222222222</uuid>",
            "</array></map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parse_experience_ids(&ids_reply).len(), 2);

        let prefs = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>experiences</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>",
            "<key>blocked</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let (allowed, blocked) = parse_experience_permissions(&prefs);
        assert_eq!(allowed.len(), 1);
        assert_eq!(blocked.len(), 1);
        Ok(())
    }

    /// The `Allow` permission PUT body nests the permission under the id key.
    #[test]
    fn set_permission_body() -> Result<(), String> {
        let id =
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
        let body = build_set_experience_permission_request(id, ExperiencePermission::Allow);
        assert_eq!(
            body,
            "<llsd><map><key>11111111-1111-1111-1111-111111111111</key><map><key>permission</key><string>Allow</string></map></map></llsd>"
        );
        Ok(())
    }

    /// The `UpdateExperience` POST body carries the editable fields and round-trips
    /// the reply back through the info decoder (a bare experience map).
    #[test]
    fn update_experience_round_trip() -> Result<(), String> {
        let id =
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?;
        let update = ExperienceUpdate {
            public_id: id,
            name: "Renamed".to_owned(),
            description: "desc".to_owned(),
            maturity: 13,
            properties: PROPERTY_GRID,
            slurl: "http://maps/y".to_owned(),
            extended_metadata: String::new(),
        };
        let body = build_update_experience_request(&update);
        assert!(
            body.contains("<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>")
        );
        assert!(body.contains("<key>name</key><string>Renamed</string>"));
        assert!(body.contains("<key>properties</key><integer>16</integer>"));
        assert!(!body.contains("quota"));

        let reply = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
            "<key>name</key><string>Renamed</string>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let infos = parse_experience_infos(&reply);
        let [info] = infos.as_slice() else {
            return Err(format!("expected 1 info, got {}", infos.len()));
        };
        assert_eq!(info.name, "Renamed");
        Ok(())
    }

    /// `RegionExperiences` round-trips its three id lists through the body builder
    /// and the reply decoder.
    #[test]
    fn region_experiences_round_trip() -> Result<(), String> {
        let allowed = [
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").map_err(|e| e.to_string())?
        ];
        let trusted = [
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").map_err(|e| e.to_string())?
        ];
        let body = build_region_experiences_request(&allowed, &[], &trusted);
        assert!(body.contains(
            "<key>allowed</key><array><uuid>11111111-1111-1111-1111-111111111111</uuid></array>"
        ));
        assert!(body.contains("<key>blocked</key><array></array>"));
        assert!(body.contains(
            "<key>trusted</key><array><uuid>22222222-2222-2222-2222-222222222222</uuid></array>"
        ));

        let reply = parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?;
        let (allowed_out, blocked_out, trusted_out) = parse_region_experiences(&reply);
        assert_eq!(allowed_out, allowed);
        assert!(blocked_out.is_empty());
        assert_eq!(trusted_out, trusted);
        Ok(())
    }

    /// The `{ status }` boolean decodes, and the property helpers read the bits.
    #[test]
    fn status_and_properties() -> Result<(), String> {
        let reply = parse_llsd_xml("<llsd><map><key>status</key><boolean>1</boolean></map></llsd>")
            .map_err(|error| format!("{error:?}"))?;
        assert!(parse_experience_status(&reply));

        let props = ExperienceProperties(PROPERTY_GRID);
        assert!(props.is_grid());
        assert!(!props.is_private());
        assert_eq!(
            ExperienceInfo::default().properties,
            ExperienceProperties(0)
        );
        Ok(())
    }
}
