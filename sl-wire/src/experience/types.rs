//! Experience value types: properties, permissions, info, and update params.

use super::llsd_uuid;
use crate::WireError;
use crate::llsd::Llsd;
use sl_types::key::{AgentKey, ExperienceKey, GroupKey, OwnerKey};
use std::collections::HashMap;
use uuid::Uuid;

/// Reads a required UUID field that, like [`super::llsd_uuid`], accepts either a
/// `uuid` or a UUID-valued `string`. An absent (or `Undef`) field is a
/// [`WireError::MissingField`]; a present value that is neither a `uuid` nor a
/// parseable UUID string is a [`WireError::MalformedField`]. (We cannot use
/// [`Llsd::require_uuid`](crate::llsd::Llsd::require_uuid), which rejects the
/// string form the lenient path historically accepted here.)
fn require_uuid_lenient(map: &Llsd, field: &'static str) -> Result<Uuid, WireError> {
    match map.get(field) {
        None | Some(Llsd::Undef) => Err(WireError::MissingField { field }),
        Some(value) => llsd_uuid(value).ok_or_else(|| WireError::MalformedField {
            field,
            value: value.kind().to_owned(),
        }),
    }
}

// The experience property bitfield and its `PROPERTY_*` flag constants now live
// in the shared `sl-types` crate; re-exported here (and onward through the
// `experience` module / `sl-wire` / `sl-proto`) so the existing paths are
// unchanged.
pub use sl_types::experience::{
    ExperienceProperties, PROPERTY_DISABLED, PROPERTY_GRID, PROPERTY_INVALID, PROPERTY_PRIVATE,
    PROPERTY_PRIVILEGED, PROPERTY_SUSPENDED,
};

/// The `FindExperienceByName` results-per-page count the reference viewer sends.
/// (A client request constant, kept local — not a general SL concept.)
pub const SEARCH_PAGE_SIZE: i32 = 30;

/// The per-experience preference an agent can set over `ExperiencePreferences`.
/// `Allow`/`Block` are sent as a PUT body; `Forget` clears any prior preference
/// (sent as a DELETE — see [`build_set_experience_permission_request`](crate::build_set_experience_permission_request)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

/// Resolves an experience's owner from the wire `(agent_id, group_id)` pair: a
/// non-nil `agent_id` is an agent owner, else a non-nil `group_id` is a group
/// owner, else `None` (e.g. a placeholder record carries neither).
fn experience_owner(agent_id: Uuid, group_id: Uuid) -> Option<OwnerKey> {
    if !agent_id.is_nil() {
        Some(OwnerKey::Agent(AgentKey::from(agent_id)))
    } else if !group_id.is_nil() {
        Some(OwnerKey::Group(GroupKey::from(group_id)))
    } else {
        None
    }
}

/// A single experience's metadata record, as carried in a cap reply's
/// `experience_keys` array (and decoded by [`ExperienceInfo::from_llsd`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ExperienceInfo {
    /// The experience's public id (`public_id`) — the key used everywhere else.
    pub public_id: ExperienceKey,
    /// The experience's display name.
    pub name: String,
    /// The experience's owner — an agent or a group — decoded from the wire
    /// `(agent_id, group_id)` pair (exactly one is set), or `None` when neither
    /// is (e.g. a [`missing`](Self::missing) placeholder).
    pub owner: Option<OwnerKey>,
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
    /// A SLURL to the experience's home location (`slurl`). The empty wire value
    /// (no home location set) decodes to [`None`].
    pub slurl: Option<url::Url>,
    /// Opaque extended metadata XML (`extended_metadata`).
    pub extended_metadata: String,
    /// `true` when this is a placeholder for an `error_ids` entry — the grid could
    /// not resolve the id (also flagged via [`PROPERTY_INVALID`]).
    pub missing: bool,
}

impl Default for ExperienceInfo {
    fn default() -> Self {
        Self {
            public_id: ExperienceKey::from(Uuid::default()),
            name: String::default(),
            owner: None,
            description: String::default(),
            properties: ExperienceProperties::default(),
            quota: i32::default(),
            expiration: f64::default(),
            maturity: i32::default(),
            slurl: None,
            extended_metadata: String::default(),
            missing: bool::default(),
        }
    }
}

impl ExperienceInfo {
    /// Decodes an [`ExperienceInfo`] from one `experience_keys` map element.
    ///
    /// `public_id` is required: it is the experience key every other record is
    /// filed under, so a record without it is meaningless (the viewer's cache
    /// key and the only field it guards with `has()` — see
    /// `LLExperienceCache::importFile`/`insert` in Firestorm's
    /// `indra/llmessage/llexperiencecache.cpp`). Every other field is optional
    /// and takes its default when absent, mirroring the viewer's unconditional
    /// `.asString()/.asInteger()/.asUUID()` reads. The `missing`/`PROPERTY_INVALID`
    /// placeholder is built directly (not via this fn), so requiring `public_id`
    /// does not affect it.
    ///
    /// # Errors
    ///
    /// Returns a [`WireError::MissingField`] if `public_id` is absent, or a
    /// [`WireError::MalformedField`] if a present field has the wrong LLSD kind.
    pub fn from_llsd(map: &Llsd) -> Result<Self, WireError> {
        let string = |key: &'static str| -> Result<String, WireError> {
            Ok(map.field_str(key, key)?.unwrap_or_default().to_owned())
        };
        Ok(Self {
            public_id: ExperienceKey::from(require_uuid_lenient(map, "public_id")?),
            name: string("name")?,
            owner: experience_owner(
                map.get("agent_id").and_then(llsd_uuid).unwrap_or_default(),
                map.get("group_id").and_then(llsd_uuid).unwrap_or_default(),
            ),
            description: string("description")?,
            properties: ExperienceProperties(
                map.field_i32("properties", "properties")?.unwrap_or(0),
            ),
            quota: map.field_i32("quota", "quota")?.unwrap_or(0),
            expiration: map.field_f64("expiration", "expiration")?.unwrap_or(0.0),
            maturity: map.field_i32("maturity", "maturity")?.unwrap_or(0),
            slurl: crate::optional_url_from_wire("slurl", &string("slurl")?)?,
            extended_metadata: string("extended_metadata")?,
            missing: map
                .field_bool("DoesNotExist", "DoesNotExist")?
                .unwrap_or(false),
        })
    }

    /// Encodes this record as one `experience_keys` map element — the inverse of
    /// [`from_llsd`](Self::from_llsd). A `missing` record carries the
    /// `DoesNotExist` marker so it decodes back as a placeholder; the server-side
    /// [`build_experience_infos_response`](crate::build_experience_infos_response) instead routes missing ids to the
    /// reply's `error_ids` array (which decodes to the same placeholder).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let (agent_id, group_id) = match self.owner {
            Some(OwnerKey::Agent(agent)) => (agent.uuid(), Uuid::nil()),
            Some(OwnerKey::Group(group)) => (Uuid::nil(), group.uuid()),
            None => (Uuid::nil(), Uuid::nil()),
        };
        let mut map: HashMap<String, Llsd> = HashMap::from([
            ("public_id".to_owned(), Llsd::Uuid(self.public_id.uuid())),
            ("name".to_owned(), Llsd::String(self.name.clone())),
            ("agent_id".to_owned(), Llsd::Uuid(agent_id)),
            ("group_id".to_owned(), Llsd::Uuid(group_id)),
            (
                "description".to_owned(),
                Llsd::String(self.description.clone()),
            ),
            ("properties".to_owned(), Llsd::Integer(self.properties.0)),
            ("quota".to_owned(), Llsd::Integer(self.quota)),
            ("expiration".to_owned(), Llsd::Real(self.expiration)),
            ("maturity".to_owned(), Llsd::Integer(self.maturity)),
            (
                "slurl".to_owned(),
                Llsd::String(crate::optional_url_to_wire(self.slurl.as_ref())),
            ),
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
/// [`build_update_experience_request`](crate::build_update_experience_request)). The viewer omits `quota`, `expiration`
/// and `agent_id` (server-controlled), so this carries only the editable fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceUpdate {
    /// The experience to update (`public_id`).
    pub public_id: ExperienceKey,
    /// The new display name.
    pub name: String,
    /// The new description.
    pub description: String,
    /// The new content rating (`maturity`).
    pub maturity: i32,
    /// The new [`ExperienceProperties`] bits (only admins may change them).
    pub properties: i32,
    /// The new home-location SLURL ([`None`] clears it / leaves it unset).
    pub slurl: Option<url::Url>,
    /// The new extended-metadata XML.
    pub extended_metadata: String,
}

impl Default for ExperienceUpdate {
    fn default() -> Self {
        Self {
            public_id: ExperienceKey::from(Uuid::default()),
            name: String::default(),
            description: String::default(),
            maturity: i32::default(),
            properties: i32::default(),
            slurl: None,
            extended_metadata: String::default(),
        }
    }
}
