//! Experience value types: properties, permissions, info, and update params.

use super::llsd_uuid;
use crate::llsd::Llsd;
use sl_types::key::AgentKey;
use std::collections::HashMap;
use uuid::Uuid;

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

/// A single experience's metadata record, as carried in a cap reply's
/// `experience_keys` array (and decoded by [`ExperienceInfo::from_llsd`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ExperienceInfo {
    /// The experience's public id (`public_id`) — the key used everywhere else.
    pub public_id: Uuid,
    /// The experience's display name.
    pub name: String,
    /// The owning agent (`agent_id`); nil when group-owned.
    pub agent_id: AgentKey,
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

impl Default for ExperienceInfo {
    fn default() -> Self {
        Self {
            public_id: Uuid::default(),
            name: String::default(),
            agent_id: AgentKey::from(Uuid::default()),
            group_id: Uuid::default(),
            description: String::default(),
            properties: ExperienceProperties::default(),
            quota: i32::default(),
            expiration: f64::default(),
            maturity: i32::default(),
            slurl: String::default(),
            extended_metadata: String::default(),
            missing: bool::default(),
        }
    }
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
            agent_id: AgentKey::from(map.get("agent_id").and_then(llsd_uuid).unwrap_or_default()),
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
    /// [`build_experience_infos_response`](crate::build_experience_infos_response) instead routes missing ids to the
    /// reply's `error_ids` array (which decodes to the same placeholder).
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let mut map: HashMap<String, Llsd> = HashMap::from([
            ("public_id".to_owned(), Llsd::Uuid(self.public_id)),
            ("name".to_owned(), Llsd::String(self.name.clone())),
            ("agent_id".to_owned(), Llsd::Uuid(self.agent_id.uuid())),
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
/// [`build_update_experience_request`](crate::build_update_experience_request)). The viewer omits `quota`, `expiration`
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
