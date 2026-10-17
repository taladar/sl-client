//! Experience value types: properties, permissions, info, and update params.

use super::llsd_uuid;
use crate::WireError;
use crate::llsd::{Llsd, LlsdError};
use sl_types::key::{AgentKey, ExperienceKey, GroupKey};
use std::collections::HashMap;
use uuid::Uuid;

/// Reads a required UUID field that, like [`super::llsd_uuid`], accepts either a
/// `uuid` or a UUID-valued `string`. An absent (or `Undef`) field is a
/// [`LlsdError::MissingField`]; a present value that is neither a `uuid` nor a
/// parseable UUID string is a [`LlsdError::MalformedField`]. (We cannot use
/// [`Llsd::require_uuid`](crate::llsd::Llsd::require_uuid), which rejects the
/// string form the lenient path historically accepted here.)
fn require_uuid_lenient(map: &Llsd, field: &'static str) -> Result<Uuid, WireError> {
    match map.get(field) {
        None | Some(Llsd::Undef) => Err(LlsdError::MissingField { field }),
        Some(value) => llsd_uuid(value).ok_or_else(|| LlsdError::MalformedField {
            field,
            value: value.kind().to_owned(),
        }),
    }
    .map_err(WireError::from)
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

/// One page of a `FindExperienceByName` reply: the matching records, plus
/// whether the grid offered a page on either side of this one.
///
/// The reply states the two neighbours as **URLs** (`next_page_url` /
/// `previous_page_url`), but the reference viewer never fetches either — it
/// re-queries by page number and reads only whether the key is *there*
/// (`LLPanelExperiencePicker::processResponse` enables its `right_btn` /
/// `left_btn` from `content.has(…)`). So the two markers are carried as
/// booleans: that is the whole of what the URLs are used for, and a URL nothing
/// dereferences would be a field kept only to be ignored.
///
/// A grid that omits both keys therefore offers no paging at all, which is
/// exactly what the reference does with such a reply.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExperienceSearchPage {
    /// The experiences on this page, in reply order.
    pub infos: Vec<ExperienceInfo>,
    /// Whether the reply carried a `next_page_url` — there is a page after
    /// this one.
    pub has_next_page: bool,
    /// Whether the reply carried a `previous_page_url` — there is a page
    /// before this one.
    pub has_previous_page: bool,
}

/// The estate's experience lists, as the `RegionExperiences` capability states
/// them: the three id arrays, plus the estate's **default experience**.
///
/// The `default` key is what the reference reads out of the reply beside the
/// arrays (`LLPanelRegionExperiences::processResponse` guards it with
/// `content.has("default")`), and it names an experience the estate cannot be
/// without: the panel appends it to the **trusted** list, refuses to remove it
/// there, and filters it out of the Allowed and Blocked pickers. A grid that
/// has no default simply omits the key, so it is decoded as an [`Option`]
/// rather than as a null id.
///
/// The POST body shares this shape but carries no `default` — the reference's
/// `sendUpdate` writes only the three arrays, and the default rides back inside
/// `trusted`, where `processResponse` put it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegionExperienceLists {
    /// The experiences the region allows.
    pub allowed: Vec<ExperienceKey>,
    /// The experiences the region blocks.
    pub blocked: Vec<ExperienceKey>,
    /// The experiences the region trusts (privileged, key-grid scope).
    pub trusted: Vec<ExperienceKey>,
    /// The estate's default experience, when the reply named one.
    pub default_experience: Option<ExperienceKey>,
}

/// The per-experience preference an agent can set over `ExperiencePreferences`.
/// `Allow`/`Block` are sent as a PUT body; `Forget` clears any prior preference
/// (sent as a DELETE — see [`build_set_experience_permission_request`](crate::build_set_experience_permission_request)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// An experience's home location, as its record's `slurl` states it.
///
/// Second Life writes three kinds of value there, measured on aditi
/// (2026-10-16, an `sl-repl` probe over `FindExperienceByName`): a
/// `secondlife://` location URL, the empty string (no home location — decoded
/// as no [`ExperienceLocation`] at all), and the bare word **`last`**, which
/// about one record in five carries. `last` and `home` are the two location
/// words the reference's SLURL type knows (`LLSLURL::SIM_LOCATION_LAST` /
/// `SIM_LOCATION_HOME`, `indra/newview/llslurl.cpp`), and its profile window
/// shows either verbatim. A decoder that read the field as a URL refused
/// `last` — and with it the whole reply, so any search page holding one such
/// record came back as nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExperienceLocation {
    /// The word `last`: the resident's last location.
    Last,
    /// The word `home`: the resident's home.
    Home,
    /// A location SLURL.
    Slurl(url::Url),
}

impl ExperienceLocation {
    /// The wire word for [`Self::Last`].
    const LAST: &'static str = "last";

    /// The wire word for [`Self::Home`].
    const HOME: &'static str = "home";

    /// Decodes a record's `slurl` field: the empty string is no location, the
    /// two location words are themselves, and anything else must be a URL.
    ///
    /// # Errors
    ///
    /// Returns a [`WireError`] when the value is none of the three.
    pub fn from_wire(field: &'static str, raw: &str) -> Result<Option<Self>, WireError> {
        Ok(match raw.trim() {
            "" => None,
            Self::LAST => Some(Self::Last),
            Self::HOME => Some(Self::Home),
            _other => Some(Self::Slurl(crate::url_from_wire(field, raw)?)),
        })
    }

    /// Encodes an optional location as the `slurl` field's string — the inverse
    /// of [`from_wire`](Self::from_wire).
    #[must_use]
    pub fn to_wire(location: Option<&Self>) -> String {
        match location {
            None => String::new(),
            Some(Self::Last) => Self::LAST.to_owned(),
            Some(Self::Home) => Self::HOME.to_owned(),
            Some(Self::Slurl(url)) => crate::url_to_wire(url),
        }
    }
}

/// Reads one of a record's two identity ids (`agent_id` / `group_id`), where
/// the nil id — which Second Life writes for an experience with no group —
/// means there is none.
fn non_nil(map: &Llsd, field: &str) -> Option<Uuid> {
    map.get(field).and_then(llsd_uuid).filter(|id| !id.is_nil())
}

/// A single experience's metadata record, as carried in a cap reply's
/// `experience_keys` array (and decoded by [`ExperienceInfo::from_llsd`]).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExperienceInfo {
    /// The experience's public id (`public_id`) — the key used everywhere else.
    pub public_id: ExperienceKey,
    /// The experience's display name.
    pub name: String,
    /// The resident who owns the experience (`agent_id`), or `None` for the
    /// nil id (e.g. a [`missing`](Self::missing) placeholder). Every record
    /// aditi served carried one, group experiences included.
    pub owner: Option<AgentKey>,
    /// The group the experience is associated with (`group_id`), or `None`
    /// for the nil id. It sits **beside** the owner rather than replacing it:
    /// Second Life sends both for a group's experience (measured on aditi,
    /// 2026-10-16), and the reference's profile shows the owner and, when set,
    /// the group (`LLFloaterExperienceProfile::refreshExperience`).
    pub group: Option<GroupKey>,
    /// The free-text description.
    pub description: String,
    /// The [`ExperienceProperties`] bitfield.
    pub properties: ExperienceProperties,
    /// The script-memory quota in megabytes (`quota`).
    pub quota: i32,
    /// The cache expiration, in seconds (`expiration`).
    pub expiration: f64,
    /// The content rating (`maturity`; `sim_access` codes: PG 13 / Mature 21 /
    /// Adult 42). Second Life also serves values outside the three — 0 and 254
    /// were both measured on aditi — which the reference shows no rating for.
    pub maturity: i32,
    /// The experience's home location (`slurl`). The empty wire value (no home
    /// location set) decodes to [`None`].
    pub slurl: Option<ExperienceLocation>,
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
            group: None,
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
    /// Returns a [`LlsdError::MissingField`] if `public_id` is absent, or a
    /// [`LlsdError::MalformedField`] if a present field has the wrong LLSD kind.
    pub fn from_llsd(map: &Llsd) -> Result<Self, WireError> {
        let string = |key: &'static str| -> Result<String, WireError> {
            Ok(map.field_str(key, key)?.unwrap_or_default().to_owned())
        };
        Ok(Self {
            public_id: ExperienceKey::from(require_uuid_lenient(map, "public_id")?),
            name: string("name")?,
            owner: non_nil(map, "agent_id").map(AgentKey::from),
            group: non_nil(map, "group_id").map(GroupKey::from),
            description: string("description")?,
            properties: ExperienceProperties(
                map.field_i32("properties", "properties")?.unwrap_or(0),
            ),
            quota: map.field_i32("quota", "quota")?.unwrap_or(0),
            expiration: map.field_f64("expiration", "expiration")?.unwrap_or(0.0),
            maturity: map.field_i32("maturity", "maturity")?.unwrap_or(0),
            slurl: ExperienceLocation::from_wire("slurl", &string("slurl")?)?,
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
        let agent_id = self.owner.map_or_else(Uuid::nil, |owner| owner.uuid());
        let group_id = self.group.map_or_else(Uuid::nil, |group| group.uuid());
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
                Llsd::String(ExperienceLocation::to_wire(self.slurl.as_ref())),
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
/// [`build_update_experience_request`](crate::build_update_experience_request)).
/// The reference posts the whole record less `quota`, `expiration` and
/// `agent_id` (`LLExperienceCache::updateExperienceCoro`), so this carries the
/// rest — the group included, which its profile edits through `Group_btn` and
/// otherwise sends back as it read it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The new home location ([`None`] clears it / leaves it unset).
    pub slurl: Option<ExperienceLocation>,
    /// The new extended-metadata XML.
    pub extended_metadata: String,
    /// The experience's group (`group_id`), [`None`] for the nil id.
    pub group: Option<GroupKey>,
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
            group: None,
        }
    }
}
