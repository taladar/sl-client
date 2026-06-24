//! Second Life **display names** over the `GetDisplayNames` capability.
//!
//! A *display name* is the mutable, user-chosen name an avatar shows in world,
//! layered over the immutable *legacy* `First Last` identity that the UDP
//! `UUIDNameRequest`/`UUIDNameReply` path resolves (see `sl_proto::AvatarName`).
//! Display names live behind an HTTP capability rather than UDP: the viewer
//! batches a set of agent ids into one `GetDisplayNames` GET and decodes the
//! `{ agents, bad_ids }` LLSD reply.
//!
//! This module builds that request's query string and decodes the reply (client
//! side), and parses the query and builds the reply (server side). Field names,
//! LLSD keys, and the request/reply shapes are cross-checked against the
//! Firestorm viewer's `indra/llmessage/llavatarname.{h,cpp}` /
//! `llavatarnamecache.cpp` and OpenSim's `GetDisplayNames` cap handler.
//!
//! The capability is a single GET:
//!
//! - `GetDisplayNames` — GET `…?ids=<id>&ids=<id>&…`, batch lookup → `{ agents:
//!   [ { id, username, display_name, legacy_first_name, legacy_last_name,
//!   is_display_name_default, display_name_expires, display_name_next_update } ],
//!   bad_ids: [ <id>, … ] }`. Unresolved ids come back in `bad_ids`.

use std::collections::HashMap;

use sl_types::key::AgentKey;
use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

/// A single avatar's display-name record, as carried in a `GetDisplayNames`
/// reply's `agents` array (and decoded by [`DisplayName::from_llsd`]).
///
/// Unresolved ids come back in the reply's `bad_ids` array instead; those decode
/// into records with only [`id`](Self::id) set and [`missing`](Self::missing)
/// `true`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName {
    /// The agent id that was looked up (`id`).
    pub id: AgentKey,
    /// The agent's username / SLID (`username`), e.g. `"james.linden"` or the
    /// single-word `"bobsmith123"`. This is the immutable login identity in
    /// lowercase-dotted form.
    pub username: String,
    /// The agent's chosen display name (`display_name`). Equal to the legacy
    /// name's display form when the agent has not set a custom one (see
    /// [`is_display_name_default`](Self::is_display_name_default)).
    pub display_name: String,
    /// The legacy first name (`legacy_first_name`).
    pub legacy_first_name: String,
    /// The legacy last name (`legacy_last_name`). Modern single-name accounts use
    /// the placeholder `"Resident"`.
    pub legacy_last_name: String,
    /// Whether [`display_name`](Self::display_name) is still the default derived
    /// from the legacy name rather than a custom one (`is_display_name_default`).
    pub is_display_name_default: bool,
    /// When this record's display name expires and should be re-fetched
    /// (`display_name_expires`), as the verbatim LLSD date string.
    pub display_name_expires: String,
    /// The earliest the agent may next change their display name
    /// (`display_name_next_update`), as the verbatim LLSD date string.
    pub display_name_next_update: String,
    /// `true` when this is a placeholder for a `bad_ids` entry — the grid could
    /// not resolve the id.
    pub missing: bool,
}

impl Default for DisplayName {
    /// A placeholder record with a nil [`id`](Self::id) — the base for a
    /// `bad_ids` entry, whose `id` is then overwritten. [`AgentKey`] has no
    /// `Default`, so this is hand-written rather than derived.
    fn default() -> Self {
        Self {
            id: AgentKey::from(Uuid::nil()),
            username: String::new(),
            display_name: String::new(),
            legacy_first_name: String::new(),
            legacy_last_name: String::new(),
            is_display_name_default: false,
            display_name_expires: String::new(),
            display_name_next_update: String::new(),
            missing: false,
        }
    }
}

impl DisplayName {
    /// The legacy `"First Last"` form, collapsing to just the first name when the
    /// last name is empty or the `"Resident"` placeholder of a modern single-name
    /// account. Mirrors `sl_proto::AvatarName::legacy_name`.
    #[must_use]
    pub fn legacy_name(&self) -> String {
        if self.legacy_last_name.is_empty()
            || self.legacy_last_name.eq_ignore_ascii_case("Resident")
        {
            self.legacy_first_name.clone()
        } else {
            format!("{} {}", self.legacy_first_name, self.legacy_last_name)
        }
    }

    /// Decodes a [`DisplayName`] from one `agents` map element.
    ///
    /// A resolved `agents` entry carries the agent's identity, so the four
    /// identity fields — `id`, `username`, `legacy_first_name`,
    /// `legacy_last_name` — are required: a conforming grid always emits them
    /// (OpenSim `BunchOfCaps.cs` lines 2323-2326) and the Firestorm reader uses
    /// them without a fallback (`llavatarname.cpp` lines 112, 114-115;
    /// `llavatarnamecache.cpp:235` keys the cache by `id`), so their absence
    /// makes the record meaningless. The remaining fields degrade gracefully:
    /// `display_name` falls back to the username when empty/absent in the viewer
    /// (`llavatarname.cpp:123`), and the bool/timestamp fields default to a
    /// stale/immediate-refetch value, so they stay optional.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::MissingField`] if a required identity field is
    /// absent, or [`WireError::MalformedField`] if a present field has the wrong
    /// LLSD kind.
    pub fn from_llsd(map: &Llsd) -> Result<Self, WireError> {
        let optional_string = |key: &'static str| -> Result<String, WireError> {
            Ok(map.field_str(key, key)?.unwrap_or_default().to_owned())
        };
        Ok(Self {
            id: AgentKey::from(map.require_uuid("id", "id")?),
            username: map.require_str("username", "username")?.to_owned(),
            display_name: optional_string("display_name")?,
            legacy_first_name: map
                .require_str("legacy_first_name", "legacy_first_name")?
                .to_owned(),
            legacy_last_name: map
                .require_str("legacy_last_name", "legacy_last_name")?
                .to_owned(),
            is_display_name_default: map
                .field_bool("is_display_name_default", "is_display_name_default")?
                .unwrap_or(false),
            display_name_expires: optional_string("display_name_expires")?,
            display_name_next_update: optional_string("display_name_next_update")?,
            missing: false,
        })
    }

    /// Encodes this record as one `agents` map element — the inverse of
    /// [`from_llsd`](Self::from_llsd). The two timestamp fields are emitted as
    /// LLSD `date` values; the server-side [`build_display_names_response`]
    /// instead routes [`missing`](Self::missing) records to the reply's `bad_ids`
    /// array (which decodes back to the same placeholder).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        Llsd::Map(HashMap::from([
            ("id".to_owned(), Llsd::Uuid(self.id.uuid())),
            ("username".to_owned(), Llsd::String(self.username.clone())),
            (
                "display_name".to_owned(),
                Llsd::String(self.display_name.clone()),
            ),
            (
                "legacy_first_name".to_owned(),
                Llsd::String(self.legacy_first_name.clone()),
            ),
            (
                "legacy_last_name".to_owned(),
                Llsd::String(self.legacy_last_name.clone()),
            ),
            (
                "is_display_name_default".to_owned(),
                Llsd::Boolean(self.is_display_name_default),
            ),
            (
                "display_name_expires".to_owned(),
                Llsd::Date(self.display_name_expires.clone()),
            ),
            (
                "display_name_next_update".to_owned(),
                Llsd::Date(self.display_name_next_update.clone()),
            ),
        ]))
    }
}

// ---------------------------------------------------------------------------
// Client side — the request builder and reply parser.
// ---------------------------------------------------------------------------

/// Builds the URL suffix for a `GetDisplayNames` GET, appended directly to the
/// capability URL (`{cap}{suffix}` → `{cap}?ids=<id>&ids=<id>&…`). Each requested
/// id is added as an `ids` query parameter, batching the lookup into one request
/// as the viewer's avatar-name cache does.
#[must_use]
pub fn display_names_query(ids: &[Uuid]) -> String {
    let mut out = String::from("?ids=");
    let mut first = true;
    for id in ids {
        if first {
            first = false;
        } else {
            out.push_str("&ids=");
        }
        out.push_str(&id.to_string());
    }
    out
}

/// Decodes a `GetDisplayNames` reply (`{ agents, bad_ids }`) into
/// [`DisplayName`] records. Each `agents` map becomes a full record; each
/// `bad_ids` id becomes a [`missing`](DisplayName::missing) placeholder (matching
/// the viewer, which caches an unresolved entry for ids the grid could not
/// resolve).
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if `agents`/`bad_ids` is present with
/// the wrong LLSD kind, or an `agents` element has a wrong-kind field.
pub fn parse_display_names(body: &Llsd) -> Result<Vec<DisplayName>, WireError> {
    let mut names = Vec::new();
    if let Some(agents) = body.field_array("agents", "agents")? {
        names.extend(
            agents
                .iter()
                .map(DisplayName::from_llsd)
                .collect::<Result<Vec<_>, WireError>>()?,
        );
    }
    if let Some(bad) = body.field_array("bad_ids", "bad_ids")? {
        names.extend(bad.iter().filter_map(Llsd::as_uuid).map(|id| DisplayName {
            id: AgentKey::from(id),
            missing: true,
            ..DisplayName::default()
        }));
    }
    Ok(names)
}

// ---------------------------------------------------------------------------
// Server side — the inverse: the request parser and reply builder.
//
// A grid's people service parses the URL a viewer sends (the request parser) and
// serializes the reply the viewer's parser above decodes (the reply builder), so
// a request round-trips builder → parser and a reply round-trips builder →
// parser.
// ---------------------------------------------------------------------------

/// Parses the [`display_names_query`] URL suffix back into the requested ids
/// (every `ids` query parameter). Unparsable ids are skipped; an absent query
/// yields an empty list.
#[must_use]
pub fn parse_display_names_query(suffix: &str) -> Vec<Uuid> {
    let Some((_path, query)) = suffix.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _value)| *key == "ids")
        .filter_map(|(_key, value)| Uuid::parse_str(value).ok())
        .collect()
}

/// Builds a `GetDisplayNames` reply (`{ agents, bad_ids }`) from a list of
/// records — the inverse of [`parse_display_names`]. Records flagged
/// [`missing`](DisplayName::missing) are emitted as bare ids in `bad_ids` (the
/// grid's "could not resolve" form), the rest as full `agents` maps; `bad_ids` is
/// omitted when empty. Built on [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_llsd_xml`](crate::parse_llsd_xml).
#[must_use]
pub fn build_display_names_response(names: &[DisplayName]) -> String {
    let mut agents = Vec::new();
    let mut bad = Vec::new();
    for name in names {
        if name.missing {
            bad.push(Llsd::Uuid(name.id.uuid()));
        } else {
            agents.push(name.to_llsd());
        }
    }
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert("agents".to_owned(), Llsd::Array(agents));
    if !bad.is_empty() {
        let _previous = map.insert("bad_ids".to_owned(), Llsd::Array(bad));
    }
    Llsd::Map(map).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use uuid::Uuid;

    use super::{
        DisplayName, build_display_names_response, display_names_query, parse_display_names,
        parse_display_names_query,
    };
    use crate::WireError;
    use crate::llsd::parse_llsd_xml;

    /// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
    fn uuid(text: &str) -> Result<Uuid, String> {
        Uuid::parse_str(text).map_err(|error| error.to_string())
    }

    /// `GetDisplayNames` batches every id as an `ids` query parameter, and its
    /// `agents` decode into full records while `bad_ids` become `missing`
    /// placeholders.
    #[test]
    fn display_names_query_and_decode() -> Result<(), String> {
        let id = uuid("11111111-1111-1111-1111-111111111111")?;
        let other = uuid("22222222-2222-2222-2222-222222222222")?;
        let suffix = display_names_query(&[id, other]);
        assert_eq!(
            suffix,
            "?ids=11111111-1111-1111-1111-111111111111&ids=22222222-2222-2222-2222-222222222222"
        );

        let reply = parse_llsd_xml(concat!(
            "<llsd><map><key>agents</key><array><map>",
            "<key>id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
            "<key>username</key><string>james.linden</string>",
            "<key>display_name</key><string>James the Great</string>",
            "<key>legacy_first_name</key><string>James</string>",
            "<key>legacy_last_name</key><string>Linden</string>",
            "<key>is_display_name_default</key><boolean>false</boolean>",
            "<key>display_name_expires</key><date>2010-04-16T21:32:26Z</date>",
            "<key>display_name_next_update</key><date>2010-04-16T21:34:02Z</date>",
            "</map></array>",
            "<key>bad_ids</key><array>",
            "<uuid>33333333-3333-3333-3333-333333333333</uuid></array>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let names = parse_display_names(&reply).map_err(|error| format!("{error:?}"))?;
        let [first, second] = names.as_slice() else {
            return Err(format!("expected 2 names, got {}", names.len()));
        };
        assert_eq!(first.id.uuid(), id);
        assert_eq!(first.username, "james.linden");
        assert_eq!(first.display_name, "James the Great");
        assert_eq!(first.legacy_name(), "James Linden");
        assert!(!first.is_display_name_default);
        assert_eq!(first.display_name_expires, "2010-04-16T21:32:26Z");
        assert!(!first.missing);
        assert_eq!(
            second.id.uuid(),
            uuid("33333333-3333-3333-3333-333333333333")?
        );
        assert!(second.missing);
        Ok(())
    }

    /// An `agents` entry that omits a mandatory identity field (here the
    /// `username`) is rejected as [`WireError::MissingField`] rather than
    /// decoding a half-populated record — a resolved entry's identity fields are
    /// always emitted by a conforming grid.
    #[test]
    fn display_names_missing_identity_field_is_error() -> Result<(), String> {
        let reply = parse_llsd_xml(concat!(
            "<llsd><map><key>agents</key><array><map>",
            "<key>id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>",
            "<key>legacy_first_name</key><string>James</string>",
            "<key>legacy_last_name</key><string>Linden</string>",
            "</map></array></map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        match parse_display_names(&reply) {
            Err(WireError::MissingField { field: "username" }) => Ok(()),
            other => Err(format!("expected MissingField username, got {other:?}")),
        }
    }

    /// The server query parser is the inverse of [`display_names_query`], and the
    /// reply builder round-trips through [`parse_display_names`].
    #[test]
    fn display_names_response_round_trip() -> Result<(), String> {
        let id = uuid("44444444-4444-4444-4444-444444444444")?;
        let bad = uuid("55555555-5555-5555-5555-555555555555")?;
        let suffix = display_names_query(&[id, bad]);
        assert_eq!(parse_display_names_query(&suffix), vec![id, bad]);

        let name = DisplayName {
            id: AgentKey::from(id),
            username: "bobsmith123".to_owned(),
            display_name: "bobsmith123".to_owned(),
            legacy_first_name: "Bobsmith123".to_owned(),
            legacy_last_name: "Resident".to_owned(),
            is_display_name_default: true,
            display_name_expires: "2024-01-02T03:04:05Z".to_owned(),
            display_name_next_update: "2024-01-09T03:04:05Z".to_owned(),
            missing: false,
        };
        let missing = DisplayName {
            id: AgentKey::from(bad),
            missing: true,
            ..DisplayName::default()
        };
        let xml = build_display_names_response(&[name.clone(), missing]);
        let parsed =
            parse_display_names(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        let [first, second] = parsed.as_slice() else {
            return Err(format!("expected 2 names, got {}", parsed.len()));
        };
        // Single-name account collapses its legacy form to just the first name.
        assert_eq!(first.legacy_name(), "Bobsmith123");
        assert!(first.is_display_name_default);
        assert_eq!(first, &name);
        assert_eq!(second.id.uuid(), bad);
        assert!(second.missing);
        Ok(())
    }
}
