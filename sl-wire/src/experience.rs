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

use std::collections::HashMap;

use uuid::Uuid;

use crate::llsd::{Llsd, parse_llsd_xml, push_escaped};

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

    /// Decodes the wire string (`"Allow"` / `"Block"` / `"Forget"`) back into a
    /// preference — the inverse of [`as_str`](Self::as_str). Any other text yields
    /// `None`.
    #[must_use]
    pub fn from_wire(text: &str) -> Option<Self> {
        match text {
            "Allow" => Some(Self::Allow),
            "Block" => Some(Self::Block),
            "Forget" => Some(Self::Forget),
            _ => None,
        }
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

    /// Encodes this record as one `experience_keys` map element — the inverse of
    /// [`from_llsd`](Self::from_llsd). A `missing` record carries the
    /// `DoesNotExist` marker so it decodes back as a placeholder; the server-side
    /// [`build_experience_infos_response`] instead routes missing ids to the
    /// reply's `error_ids` array (which decodes to the same placeholder).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let mut map: HashMap<String, Llsd> = HashMap::from([
            ("public_id".to_owned(), Llsd::Uuid(self.public_id)),
            ("name".to_owned(), Llsd::String(self.name.clone())),
            ("agent_id".to_owned(), Llsd::Uuid(self.agent_id)),
            ("group_id".to_owned(), Llsd::Uuid(self.group_id)),
            (
                "description".to_owned(),
                Llsd::String(self.description.clone()),
            ),
            ("properties".to_owned(), Llsd::Integer(self.properties.0)),
            ("quota".to_owned(), Llsd::Integer(self.quota)),
            ("expiration".to_owned(), Llsd::Real(self.expiration)),
            ("maturity".to_owned(), Llsd::Integer(self.maturity)),
            ("slurl".to_owned(), Llsd::String(self.slurl.clone())),
            (
                "extended_metadata".to_owned(),
                Llsd::String(self.extended_metadata.clone()),
            ),
        ]);
        if self.missing {
            let _previous = map.insert("DoesNotExist".to_owned(), Llsd::Boolean(true));
        }
        Llsd::Map(map)
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

// ---------------------------------------------------------------------------
// Server side — the inverse of the request builders / response parsers above.
//
// A grid's experience service parses the URL/body a viewer sends (the request
// parsers) and serializes the replies the viewer's response parsers above
// decode (the response builders). Each is the exact inverse of its client-side
// counterpart, so a request round-trips builder → parser and a reply round-trips
// builder → parser.
// ---------------------------------------------------------------------------

/// Returns the query string of a `{cap}{suffix}` URL — everything after the
/// first `?` — or `None` when the suffix carries no query.
fn url_query(suffix: &str) -> Option<&str> {
    suffix.split_once('?').map(|(_path, query)| query)
}

/// Returns the value of query parameter `name` within a `key=value&…` query
/// string, if present.
fn query_param<'query>(query: &'query str, name: &str) -> Option<&'query str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

/// Maps an ASCII hex digit (`0-9`, `a-f`, `A-F`) to its nibble value, or `None`.
const fn from_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(byte.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(byte.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// Decodes a percent-encoded URL query value — the inverse of [`percent_encode`].
/// A `%XX` pair becomes its byte; a malformed `%` (not followed by two hex
/// digits) is kept verbatim. The resulting bytes are interpreted as UTF-8
/// (lossily, since the encoder only ever emits valid UTF-8).
fn percent_decode(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut iter = text.bytes();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let high = iter.next();
            let low = iter.next();
            match (high.and_then(from_hex_digit), low.and_then(from_hex_digit)) {
                (Some(high), Some(low)) => bytes.push(high.wrapping_shl(4) | low),
                _ => {
                    bytes.push(b'%');
                    if let Some(high) = high {
                        bytes.push(high);
                    }
                    if let Some(low) = low {
                        bytes.push(low);
                    }
                }
            }
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Parses the [`experience_info_query`] URL suffix back into the requested ids
/// (every `public_id` query parameter). Unparsable ids are skipped; an absent
/// query yields an empty list.
#[must_use]
pub fn parse_experience_info_query(suffix: &str) -> Vec<Uuid> {
    let Some(query) = url_query(suffix) else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _value)| *key == "public_id")
        .filter_map(|(_key, value)| Uuid::parse_str(value).ok())
        .collect()
}

/// Parses the [`find_experience_query`] URL suffix back into its
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

/// Parses the [`group_experiences_query`] URL suffix back into its group id
/// (`?<group_id>`), or `None` if it does not match.
#[must_use]
pub fn parse_group_experiences_query(suffix: &str) -> Option<Uuid> {
    parse_bare_uuid_query(suffix)
}

/// Parses the [`forget_experience_query`] URL suffix back into its experience id
/// (the `Forget` DELETE target, `?<experience_id>`), or `None` if it does not
/// match.
#[must_use]
pub fn parse_forget_experience_query(suffix: &str) -> Option<Uuid> {
    parse_bare_uuid_query(suffix)
}

/// Parses the [`experience_id_query`] URL suffix back into its experience id
/// (`?experience_id=<id>`), or `None` if it does not match.
#[must_use]
pub fn parse_experience_id_query(suffix: &str) -> Option<Uuid> {
    let query = url_query(suffix)?;
    Uuid::parse_str(query_param(query, "experience_id")?).ok()
}

/// Parses an `ExperiencePreferences` PUT body
/// (`{ "<id>": { "permission": "Allow"|"Block" } }`) back into its
/// `(experience id, permission)` pair — the inverse of
/// [`build_set_experience_permission_request`]. Returns `Ok(None)` when the body
/// is well-formed XML but not a single id→permission entry.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_set_experience_permission_request(
    xml: &str,
) -> Result<Option<(Uuid, ExperiencePermission)>, roxmltree::Error> {
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
    Ok(permission.map(|permission| (id, permission)))
}

/// Parses an `UpdateExperience` POST body back into an [`ExperienceUpdate`] — the
/// inverse of [`build_update_experience_request`]. Missing fields take their
/// defaults, mirroring the lenient decoding elsewhere in this module.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_update_experience_request(xml: &str) -> Result<ExperienceUpdate, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let string = |key: &str| {
        root.get(key)
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(ExperienceUpdate {
        public_id: root
            .get("public_id")
            .and_then(llsd_uuid)
            .unwrap_or_default(),
        name: string("name"),
        description: string("description"),
        maturity: root.get("maturity").and_then(Llsd::as_i32).unwrap_or(0),
        properties: root.get("properties").and_then(Llsd::as_i32).unwrap_or(0),
        slurl: string("slurl"),
        extended_metadata: string("extended_metadata"),
    })
}

/// Parses a `RegionExperiences` POST body back into its
/// `(allowed, blocked, trusted)` id lists — the inverse of
/// [`build_region_experiences_request`]. (The body and reply share a shape, so
/// this delegates to [`parse_region_experiences`].)
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
#[expect(
    clippy::type_complexity,
    reason = "mirrors parse_region_experiences' (allowed, blocked, trusted) tuple, wrapped in Result for the XML parse error"
)]
pub fn parse_region_experiences_request(
    xml: &str,
) -> Result<(Vec<Uuid>, Vec<Uuid>, Vec<Uuid>), roxmltree::Error> {
    Ok(parse_region_experiences(&parse_llsd_xml(xml)?))
}

/// Builds an array-of-UUIDs LLSD value.
fn uuid_array_llsd(ids: &[Uuid]) -> Llsd {
    Llsd::Array(ids.iter().copied().map(Llsd::Uuid).collect())
}

/// Builds a `GetExperienceInfo` / `FindExperienceByName` reply
/// (`{ experience_keys, error_ids }`) from a list of records — the inverse of
/// [`parse_experience_infos`]. Records flagged [`missing`](ExperienceInfo::missing)
/// are emitted as bare ids in `error_ids` (the grid's "could not resolve" form),
/// the rest as full `experience_keys` maps; `error_ids` is omitted when empty.
/// Built on [`Llsd::to_llsd_xml`], so it round-trips through [`parse_llsd_xml`].
#[must_use]
pub fn build_experience_infos_response(infos: &[ExperienceInfo]) -> String {
    let mut keys = Vec::new();
    let mut errors = Vec::new();
    for info in infos {
        if info.missing {
            errors.push(Llsd::Uuid(info.public_id));
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
/// [`parse_experience_ids`].
#[must_use]
pub fn build_experience_ids_response(ids: &[Uuid]) -> String {
    Llsd::Map(HashMap::from([(
        "experience_ids".to_owned(),
        uuid_array_llsd(ids),
    )]))
    .to_llsd_xml()
}

/// Builds a `GetExperiences` / `ExperiencePreferences` reply
/// (`{ experiences, blocked }`) — the inverse of
/// [`parse_experience_permissions`].
#[must_use]
pub fn build_experience_permissions_response(allowed: &[Uuid], blocked: &[Uuid]) -> String {
    Llsd::Map(HashMap::from([
        ("experiences".to_owned(), uuid_array_llsd(allowed)),
        ("blocked".to_owned(), uuid_array_llsd(blocked)),
    ]))
    .to_llsd_xml()
}

/// Builds a `RegionExperiences` reply (`{ allowed, blocked, trusted }`) — the
/// inverse of [`parse_region_experiences`]. (The reply shares its shape with the
/// POST body that [`build_region_experiences_request`] writes.)
#[must_use]
pub fn build_region_experiences_response(
    allowed: &[Uuid],
    blocked: &[Uuid],
    trusted: &[Uuid],
) -> String {
    Llsd::Map(HashMap::from([
        ("allowed".to_owned(), uuid_array_llsd(allowed)),
        ("blocked".to_owned(), uuid_array_llsd(blocked)),
        ("trusted".to_owned(), uuid_array_llsd(trusted)),
    ]))
    .to_llsd_xml()
}

/// Builds an `IsExperienceAdmin` / `IsExperienceContributor` reply
/// (`{ status }`) — the inverse of [`parse_experience_status`].
#[must_use]
pub fn build_experience_status_response(status: bool) -> String {
    Llsd::Map(HashMap::from([(
        "status".to_owned(),
        Llsd::Boolean(status),
    )]))
    .to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
        PROPERTY_GRID, PROPERTY_INVALID, build_experience_ids_response,
        build_experience_infos_response, build_experience_permissions_response,
        build_experience_status_response, build_region_experiences_request,
        build_region_experiences_response, build_set_experience_permission_request,
        build_update_experience_request, experience_id_query, experience_info_query,
        find_experience_query, forget_experience_query, group_experiences_query,
        parse_experience_id_query, parse_experience_ids, parse_experience_info_query,
        parse_experience_infos, parse_experience_permissions, parse_experience_status,
        parse_find_experience_query, parse_forget_experience_query, parse_group_experiences_query,
        parse_region_experiences, parse_region_experiences_request,
        parse_set_experience_permission_request, parse_update_experience_request,
    };
    use crate::llsd::parse_llsd_xml;

    /// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
    fn uuid(text: &str) -> Result<Uuid, String> {
        Uuid::parse_str(text).map_err(|error| error.to_string())
    }

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

        assert_eq!(
            build_experience_status_response(true),
            "<llsd><map><key>status</key><boolean>true</boolean></map></llsd>"
        );

        let props = ExperienceProperties(PROPERTY_GRID);
        assert!(props.is_grid());
        assert!(!props.is_private());
        assert_eq!(
            ExperienceInfo::default().properties,
            ExperienceProperties(0)
        );
        Ok(())
    }

    /// The `GetExperienceInfo` URL suffix round-trips through its parser, batching
    /// every requested id back out of the `public_id` query parameters.
    #[test]
    fn experience_info_query_round_trip() -> Result<(), String> {
        let ids = [
            uuid("11111111-1111-1111-1111-111111111111")?,
            uuid("22222222-2222-2222-2222-222222222222")?,
        ];
        let suffix = experience_info_query(&ids);
        assert_eq!(parse_experience_info_query(&suffix), ids);
        Ok(())
    }

    /// The search query round-trips, recovering the percent-decoded text and page.
    #[test]
    fn find_experience_query_round_trip() {
        let suffix = find_experience_query("a b&c", 2);
        assert_eq!(
            parse_find_experience_query(&suffix),
            Some(("a b&c".to_owned(), 2))
        );
    }

    /// The bare-UUID query forms (group, forget) and the `experience_id=` form
    /// each round-trip through their parsers.
    #[test]
    fn uuid_query_round_trips() -> Result<(), String> {
        let id = uuid("11111111-1111-1111-1111-111111111111")?;
        assert_eq!(
            parse_group_experiences_query(&group_experiences_query(id)),
            Some(id)
        );
        assert_eq!(
            parse_forget_experience_query(&forget_experience_query(id)),
            Some(id)
        );
        assert_eq!(
            parse_experience_id_query(&experience_id_query(id)),
            Some(id)
        );
        Ok(())
    }

    /// The `ExperiencePreferences` PUT body round-trips builder → parser, and the
    /// `{ experiences, blocked }` reply round-trips builder → parser.
    #[test]
    fn permission_request_and_reply_round_trip() -> Result<(), String> {
        let id = uuid("11111111-1111-1111-1111-111111111111")?;
        let body = build_set_experience_permission_request(id, ExperiencePermission::Block);
        let parsed =
            parse_set_experience_permission_request(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, Some((id, ExperiencePermission::Block)));

        let allowed = [id];
        let blocked = [uuid("22222222-2222-2222-2222-222222222222")?];
        let reply = build_experience_permissions_response(&allowed, &blocked);
        let parsed = parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?;
        let (allowed_out, blocked_out) = parse_experience_permissions(&parsed);
        assert_eq!(allowed_out, allowed);
        assert_eq!(blocked_out, blocked);
        Ok(())
    }

    /// The `UpdateExperience` POST body round-trips builder → parser.
    #[test]
    fn update_experience_request_round_trip() -> Result<(), String> {
        let update = ExperienceUpdate {
            public_id: uuid("11111111-1111-1111-1111-111111111111")?,
            name: "Renamed".to_owned(),
            description: "desc & more".to_owned(),
            maturity: 13,
            properties: PROPERTY_GRID,
            slurl: "http://maps/y".to_owned(),
            extended_metadata: "<x/>".to_owned(),
        };
        let body = build_update_experience_request(&update);
        let parsed =
            parse_update_experience_request(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, update);
        Ok(())
    }

    /// The `RegionExperiences` POST body and reply each round-trip through their
    /// request parser / response builder.
    #[test]
    fn region_experiences_service_round_trip() -> Result<(), String> {
        let allowed = [uuid("11111111-1111-1111-1111-111111111111")?];
        let trusted = [uuid("22222222-2222-2222-2222-222222222222")?];
        let request = build_region_experiences_request(&allowed, &[], &trusted);
        let (allowed_out, blocked_out, trusted_out) =
            parse_region_experiences_request(&request).map_err(|error| format!("{error:?}"))?;
        assert_eq!(allowed_out, allowed);
        assert!(blocked_out.is_empty());
        assert_eq!(trusted_out, trusted);

        let reply = build_region_experiences_response(&allowed, &[], &trusted);
        let (allowed_out, blocked_out, trusted_out) = parse_region_experiences(
            &parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?,
        );
        assert_eq!(allowed_out, allowed);
        assert!(blocked_out.is_empty());
        assert_eq!(trusted_out, trusted);
        Ok(())
    }

    /// The `experience_ids` reply round-trips builder → parser.
    #[test]
    fn experience_ids_response_round_trip() -> Result<(), String> {
        let ids = [
            uuid("11111111-1111-1111-1111-111111111111")?,
            uuid("22222222-2222-2222-2222-222222222222")?,
        ];
        let reply = build_experience_ids_response(&ids);
        let parsed =
            parse_experience_ids(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed, ids);
        Ok(())
    }

    /// The `GetExperienceInfo` reply round-trips a full record through
    /// `experience_keys` and a missing id through `error_ids`.
    #[test]
    fn experience_infos_response_round_trip() -> Result<(), String> {
        let real = ExperienceInfo {
            public_id: uuid("11111111-1111-1111-1111-111111111111")?,
            name: "My Experience".to_owned(),
            agent_id: uuid("22222222-2222-2222-2222-222222222222")?,
            description: "fun & games".to_owned(),
            properties: ExperienceProperties(PROPERTY_GRID),
            maturity: 13,
            slurl: "http://maps/x".to_owned(),
            ..ExperienceInfo::default()
        };
        let missing = ExperienceInfo {
            public_id: uuid("33333333-3333-3333-3333-333333333333")?,
            properties: ExperienceProperties(PROPERTY_INVALID),
            missing: true,
            ..ExperienceInfo::default()
        };
        let reply = build_experience_infos_response(&[real.clone(), missing.clone()]);
        let infos =
            parse_experience_infos(&parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?);
        let [first, second] = infos.as_slice() else {
            return Err(format!("expected 2 infos, got {}", infos.len()));
        };
        assert_eq!(*first, real);
        assert_eq!(*second, missing);
        Ok(())
    }

    /// A status reply round-trips builder → parser for both truth values.
    #[test]
    fn status_response_round_trip() -> Result<(), String> {
        for value in [true, false] {
            let reply = build_experience_status_response(value);
            let parsed = parse_experience_status(
                &parse_llsd_xml(&reply).map_err(|error| format!("{error:?}"))?,
            );
            assert_eq!(parsed, value);
        }
        Ok(())
    }
}
