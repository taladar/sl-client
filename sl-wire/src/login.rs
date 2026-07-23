//! The XML-RPC `login_to_simulator` request builder and response parser.
//!
//! This module is pure: it turns a [`LoginRequest`] into an XML-RPC request
//! body and parses an XML-RPC response string into a [`LoginResponse`]. The
//! actual HTTP(S) transport is performed by the I/O driver crates.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::str::FromStr;

use crate::geometry::Direction;
use crate::region_handle::RegionHandle;
use sl_types::key::{AgentKey, InventoryFolderKey};
use sl_types::map::RegionCoordinates;
use thiserror::Error;
use uuid::Uuid;

use crate::CircuitCode;

/// Where a login should place the avatar — the `start` member of a
/// [`LoginRequest`].
///
/// The XML-RPC `start` field is a tiny string grammar: `"last"` (resume at the
/// last logout location), `"home"` (the avatar's home), or `"uri:Region&x&y&z"`
/// (a named region plus an in-region position). Modelling it as an enum makes
/// the three forms explicit and an out-of-grammar value unrepresentable, instead
/// of a free-form `String` that any typo silently slips through. Build one
/// directly or [parse](StartLocation::from_str) a wire string into one, and
/// render it back with [`to_wire_string`](StartLocation::to_wire_string).
#[derive(Debug, Clone, PartialEq)]
pub enum StartLocation {
    /// Resume at the avatar's last logout location (`"last"`).
    Last,
    /// Start at the avatar's home location (`"home"`).
    Home,
    /// Start at a named region and position (`"uri:Region&x&y&z"`).
    Region {
        /// The destination region's name.
        region: String,
        /// The position within the region, in metres.
        position: RegionCoordinates,
    },
}

impl StartLocation {
    /// A [`StartLocation::Region`] for the named region at the given in-region
    /// position.
    #[must_use]
    pub fn region(name: impl Into<String>, position: RegionCoordinates) -> Self {
        Self::Region {
            region: name.into(),
            position,
        }
    }

    /// Renders this start location as the `start` wire string a grid expects:
    /// `"last"`, `"home"`, or `"uri:Region&x&y&z"`. The inverse of
    /// [`from_str`](StartLocation::from_str).
    #[must_use]
    pub fn to_wire_string(&self) -> String {
        match self {
            Self::Last => "last".to_owned(),
            Self::Home => "home".to_owned(),
            Self::Region { region, position } => {
                let (x, y, z) = (position.x(), position.y(), position.z());
                format!("uri:{region}&{x}&{y}&{z}")
            }
        }
    }
}

impl FromStr for StartLocation {
    type Err = StartLocationParseError;

    /// Parses a `start` wire string: `"last"`, `"home"`, or
    /// `"uri:Region&x&y&z"` (the three coordinates parsed as `f32`). Any other
    /// form is a [`StartLocationParseError`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "last" => Ok(Self::Last),
            "home" => Ok(Self::Home),
            other => {
                let rest = other
                    .strip_prefix("uri:")
                    .ok_or_else(|| StartLocationParseError::Unrecognized(other.to_owned()))?;
                // Split off the three trailing `&`-separated coordinates from the
                // right, so a (legal) region name is taken as everything before
                // them rather than choking on a stray `&`.
                let mut parts = rest.rsplitn(4, '&');
                let malformed = || StartLocationParseError::MalformedUri(other.to_owned());
                let z = parts.next().ok_or_else(malformed)?;
                let y = parts.next().ok_or_else(malformed)?;
                let x = parts.next().ok_or_else(malformed)?;
                let region = parts
                    .next()
                    .filter(|r| !r.is_empty())
                    .ok_or_else(malformed)?;
                let coord =
                    |value: &str| value.trim().parse::<f32>().map_err(|_ignored| malformed());
                Ok(Self::Region {
                    region: region.to_owned(),
                    position: RegionCoordinates::new(coord(x)?, coord(y)?, coord(z)?),
                })
            }
        }
    }
}

/// An error parsing a [`StartLocation`] from its `start` wire string.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartLocationParseError {
    /// The value matched none of `"last"`, `"home"`, or a `"uri:"` location.
    #[error(
        "unrecognised start location {0:?} (expected \"last\", \"home\", or \"uri:Region&x&y&z\")"
    )]
    Unrecognized(String),
    /// A `"uri:"` value was missing the region name or its three coordinates,
    /// or a coordinate was not a number.
    #[error("malformed start location {0:?} (expected \"uri:Region&x&y&z\")")]
    MalformedUri(String),
}

/// The parameters of an XML-RPC `login_to_simulator` request.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginRequest {
    /// The avatar's first name.
    pub first_name: String,
    /// The avatar's last name.
    pub last_name: String,
    /// The plaintext password (hashed when the request is built).
    pub password: String,
    /// The start location: last location, home, or a region and position.
    pub start: StartLocation,
    /// The viewer channel name.
    pub channel: String,
    /// The viewer version string.
    pub version: String,
    /// The platform string (e.g. `"lin"`, `"win"`, `"mac"`).
    pub platform: String,
    /// A hashed MAC address (any stable token; OpenSim is lenient).
    pub mac: String,
    /// A machine/installation id (may be empty).
    pub id0: String,
    /// The multi-factor authentication token (the one-time code), or empty on
    /// the first attempt before any [`LoginResponse::MfaChallenge`].
    pub token: String,
    /// A remembered multi-factor `mfa_hash` to echo back, or empty. Populated
    /// from a prior [`LoginSuccess::mfa_hash`] or an [`MfaChallenge::mfa_hash`].
    pub mfa_hash: String,
    /// The requested response option flags (e.g. `inventory-root`).
    pub options: Vec<String>,
}

impl LoginRequest {
    /// Builds a request for the given credentials and start location.
    ///
    /// The `channel` and `version` identify your application to the grid: they
    /// are sent as the `channel`/`version` XML-RPC fields and combined into the
    /// HTTP `User-Agent` header (see [`LoginRequest::user_agent`]). There is no
    /// default — every application must supply its own identity. The remaining
    /// viewer-identification fields keep conservative defaults.
    #[must_use]
    pub fn new(
        first_name: impl Into<String>,
        last_name: impl Into<String>,
        password: impl Into<String>,
        start: StartLocation,
        channel: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            first_name: first_name.into(),
            last_name: last_name.into(),
            password: password.into(),
            start,
            channel: channel.into(),
            version: version.into(),
            platform: "lin".to_owned(),
            mac: "00000000000000000000000000000000".to_owned(),
            id0: String::new(),
            token: String::new(),
            mfa_hash: String::new(),
            // Request the inventory root and folder skeleton so the login
            // response carries the agent's full folder tree, the matching
            // Library ("OpenSim Library" / "Library") roots and skeleton so it
            // carries the shared read-only library tree, and the buddy list so
            // it carries the agent's friends and their rights. (`home`,
            // `look_at`, `agent_access[_max]`, and `max-agent-groups` are
            // standard top-level fields and need no option.)
            options: vec![
                "inventory-root".to_owned(),
                "inventory-skeleton".to_owned(),
                "inventory-lib-root".to_owned(),
                "inventory-lib-owner".to_owned(),
                "inventory-skel-lib".to_owned(),
                "buddy-list".to_owned(),
            ],
        }
    }

    /// The HTTP `User-Agent` header value identifying this viewer: the
    /// [`channel`](Self::channel) and [`version`](Self::version) joined by a
    /// space (e.g. `"MyViewer 1.2.3"`), mirroring the XML-RPC `channel`/`version`
    /// login fields.
    #[must_use]
    pub fn user_agent(&self) -> String {
        format!("{} {}", self.channel, self.version)
    }

    /// Returns a copy of this request prepared to answer a multi-factor
    /// challenge: with the one-time `token` set and the challenge's `mfa_hash`
    /// (if any) echoed back.
    #[must_use]
    pub fn with_mfa(mut self, token: impl Into<String>, mfa_hash: Option<String>) -> Self {
        self.token = token.into();
        if let Some(mfa_hash) = mfa_hash {
            self.mfa_hash = mfa_hash;
        }
        self
    }
}

/// The hashed form of a password as sent in the `passwd` field: `$1$` followed
/// by the lowercase hex MD5 of the plaintext.
#[must_use]
pub fn password_hash(password: &str) -> String {
    format!("$1${:x}", md5::compute(password.as_bytes()))
}

/// Builds the XML-RPC request body for a `login_to_simulator` call.
#[must_use]
pub fn build_login_request(request: &LoginRequest) -> String {
    let mut out = String::new();
    out.push_str(
        "<?xml version=\"1.0\"?>\n<methodCall>\n<methodName>login_to_simulator</methodName>\n<params><param><value><struct>\n",
    );
    push_string_member(&mut out, "first", &request.first_name);
    push_string_member(&mut out, "last", &request.last_name);
    push_string_member(&mut out, "passwd", &password_hash(&request.password));
    push_string_member(&mut out, "start", &request.start.to_wire_string());
    push_string_member(&mut out, "channel", &request.channel);
    push_string_member(&mut out, "version", &request.version);
    push_string_member(&mut out, "platform", &request.platform);
    push_string_member(&mut out, "mac", &request.mac);
    push_string_member(&mut out, "id0", &request.id0);
    push_string_member(&mut out, "token", &request.token);
    push_string_member(&mut out, "mfa_hash", &request.mfa_hash);
    push_bool_member(&mut out, "agree_to_tos", true);
    push_bool_member(&mut out, "read_critical", true);
    // Request structured error reasons (e.g. `mfa_challenge`).
    push_bool_member(&mut out, "extended_errors", true);
    push_options_member(&mut out, &request.options);
    out.push_str("</struct></value></param></params>\n</methodCall>\n");
    out
}

/// Appends a `<string>` struct member.
fn push_string_member(out: &mut String, name: &str, value: &str) {
    out.push_str("<member><name>");
    out.push_str(name);
    out.push_str("</name><value><string>");
    push_escaped(out, value);
    out.push_str("</string></value></member>\n");
}

/// Appends a `<boolean>` struct member.
fn push_bool_member(out: &mut String, name: &str, value: bool) {
    out.push_str("<member><name>");
    out.push_str(name);
    out.push_str("</name><value><boolean>");
    out.push_str(if value { "1" } else { "0" });
    out.push_str("</boolean></value></member>\n");
}

/// Appends the `options` array member.
fn push_options_member(out: &mut String, options: &[String]) {
    out.push_str("<member><name>options</name><value><array><data>\n");
    for option in options {
        out.push_str("<value><string>");
        push_escaped(out, option);
        out.push_str("</string></value>\n");
    }
    out.push_str("</data></array></value></member>\n");
}

/// Appends `value` to `out`, escaping the XML metacharacters.
fn push_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
}

/// A parsed login response: success, a multi-factor challenge, or a failure.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginResponse {
    /// The login succeeded.
    Success(Box<LoginSuccess>),
    /// The grid requires a multi-factor one-time code. Retry the login with
    /// [`LoginRequest::with_mfa`], passing the code and this challenge's
    /// [`MfaChallenge::mfa_hash`].
    MfaChallenge(MfaChallenge),
    /// The login was rejected by the grid.
    Failure(LoginFailure),
}

/// A multi-factor authentication challenge returned by the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaChallenge {
    /// An `mfa_hash` the grid wants echoed back on the retry, if it provided
    /// one.
    pub mfa_hash: Option<String>,
    /// The human-readable challenge message.
    pub message: String,
}

/// The fields of a successful login needed to bring up the UDP circuit.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginSuccess {
    /// The avatar/agent id.
    pub agent_id: AgentKey,
    /// The session id used in the circuit.
    pub session_id: Uuid,
    /// The secure session id.
    pub secure_session_id: Uuid,
    /// The circuit code for `UseCircuitCode`.
    pub circuit_code: CircuitCode,
    /// The destination simulator's IPv4 address.
    pub sim_ip: Ipv4Addr,
    /// The destination simulator's UDP port.
    pub sim_port: u16,
    /// The capabilities seed URL.
    pub seed_capability: url::Url,
    /// The welcome/login message, if any.
    pub message: Option<String>,
    /// A fresh `mfa_hash` to remember and send on future logins to skip the
    /// multi-factor challenge ("remember this device"), if the grid provided
    /// one.
    pub mfa_hash: Option<String>,
    /// The agent's inventory root ("My Inventory") folder id, from the
    /// `inventory-root` response field (if requested and provided).
    pub inventory_root: Option<InventoryFolderKey>,
    /// The agent's inventory folder skeleton (every folder's id, parent, name,
    /// type, and version), from the `inventory-skeleton` response field. Empty if
    /// not requested/provided.
    pub inventory_skeleton: Vec<SkeletonFolder>,
    /// The agent's friends (the buddy list), each with the rights the agent
    /// grants them and the rights they grant the agent, from the `buddy-list`
    /// response field. Empty if not requested/provided or the agent has no
    /// friends.
    pub buddy_list: Vec<BuddyListEntry>,
    /// The agent's home location (region handle, position, and look-at), parsed
    /// from the `home` response field, if present and well-formed.
    pub home: Option<HomeLocation>,
    /// The camera look-at direction at the start location, parsed from the
    /// top-level `look_at` response field, if present and well-formed.
    pub look_at: Option<Direction>,
    /// The global X metre coordinate of the start region's south-west corner,
    /// from the top-level `region_x` response field. `None` if the grid did not
    /// provide it. Together with [`region_y`](Self::region_y) this packs into the
    /// start region's handle (`(region_x << 32) | region_y`); divide either by
    /// 256 for the grid coordinate (region index).
    pub region_x: Option<u32>,
    /// The global Y metre coordinate of the start region's south-west corner,
    /// from the top-level `region_y` response field. See
    /// [`region_x`](Self::region_x).
    pub region_y: Option<u32>,
    /// The account's current maturity/content rating (`agent_access`), as the
    /// grid's short code: `"PG"`, `"M"` (mature), or `"A"` (adult). `None` if
    /// the grid did not provide it.
    pub agent_access: Option<String>,
    /// The maximum maturity rating the account is entitled to
    /// (`agent_access_max`), in the same short-code form as
    /// [`agent_access`](Self::agent_access).
    pub agent_access_max: Option<String>,
    /// The maximum number of groups this account may join (`max-agent-groups`).
    /// A client should check this before joining a group. `None` if the grid did
    /// not provide it.
    pub max_agent_groups: Option<u32>,
    /// The shared Library inventory's root folder id, from the
    /// `inventory-lib-root` response field (if requested and provided).
    pub library_root: Option<InventoryFolderKey>,
    /// The agent id that owns the shared Library inventory, from the
    /// `inventory-lib-owner` response field (if requested and provided). The
    /// library's folder contents are fetched as that owner's inventory.
    pub library_owner: Option<AgentKey>,
    /// The shared Library inventory's folder skeleton, from the
    /// `inventory-skel-lib` response field. Empty if not requested/provided.
    pub library_skeleton: Vec<SkeletonFolder>,
    /// The base URL of the agent-appearance (server-side "Sunshine" bake)
    /// service, from the `agent_appearance_service` response field. Server-baked
    /// avatar textures are fetched from here as
    /// `<url>texture/<avatar_id>/<slot>/<baked_uuid>` — **not** by UUID from the
    /// `GetTexture`/`ViewerAsset` CDN (which rejects a baked id, typically with a
    /// `503`). `None` on a grid that does not central-bake (e.g. OpenSim).
    pub agent_appearance_service: Option<url::Url>,
    /// The grid's map-tile server base URL, from the `map-server-url` response
    /// field. World-map tiles are fetched from here as
    /// `<url>map-<zoom>-<x>-<y>-objects.jpg` (zoom 1–8, grid coordinates
    /// snapped to the tile corner). OpenSim announces it when its
    /// `MapTileURL` is configured (the standalone default); a region's
    /// `SimulatorFeatures` `map-server-url` — where present — is fresher and
    /// should win. `None` when the grid does not announce one.
    pub map_server_url: Option<url::Url>,
}

/// An agent's home location, parsed from the `home` login response field (a
/// quasi-LLSD string such as `{'region_handle':[r256000,r256000],
/// 'position':[r128.0,r128.0,r25.0], 'look_at':[r1.0,r0.0,r0.0]}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomeLocation {
    /// The home region's handle (its grid-corner world coordinates in metres,
    /// the two components the wire carries as `region_handle: [x, y]`).
    pub region_handle: RegionHandle,
    /// The home position within the region (`position`).
    pub position: RegionCoordinates,
    /// The camera look-at direction at home (`look_at`).
    pub look_at: Direction,
}

/// One folder of the inventory skeleton carried in a login response
/// (`inventory-skeleton`): the folder tree without item contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonFolder {
    /// The folder's id.
    pub folder_id: InventoryFolderKey,
    /// The parent folder's id (nil for the root).
    pub parent_id: InventoryFolderKey,
    /// The folder name.
    pub name: String,
    /// The default asset/folder type (the `FolderType`; `-1` for none).
    pub type_default: i8,
    /// The folder version (for cache validation).
    pub version: i32,
}

/// One friend carried in a login response (`buddy-list`): a friend's id and the
/// two friendship rights bitfields. The bit values match the `RIGHTS_*` flags
/// used by `GrantUserRights`/`ChangeUserRights` (bit 0 = see online, bit 1 = see
/// on map, bit 2 = modify objects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuddyListEntry {
    /// The friend's agent id.
    pub buddy_id: Uuid,
    /// The rights the agent grants this friend (`buddy_rights_given`).
    pub rights_granted: i32,
    /// The rights this friend grants the agent (`buddy_rights_has`).
    pub rights_has: i32,
}

/// The reason a login was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginFailure {
    /// The machine-readable reason code (e.g. `"key"`, `"presence"`).
    pub reason: String,
    /// The human-readable failure message.
    pub message: String,
}

/// A coarse classification of a [`LoginFailure`], so callers can react to the
/// well-known cases without matching on the raw [`reason`](LoginFailure::reason)
/// string — in particular to recognise the *retryable* "already logged in"
/// rejection and offer the user a retry, while leaving truly fatal rejections
/// alone.
///
/// The grid's `reason` code alone is not enough to tell these apart: Second Life
/// and OpenSim both reuse the `"presence"` code for *several* distinct
/// conditions — a stale/duplicate presence ("you appear to be already logged
/// in", which a retry usually clears once the grid evicts the ghost), but also
/// administratively restricted logins and unverified accounts, which a retry
/// must **not** hammer. Disambiguating the retryable case therefore also inspects
/// the human-readable [`message`](LoginFailure::message); see
/// [`LoginFailure::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoginRejectKind {
    /// The avatar already has a presence registered on the grid ("you appear to
    /// be already logged in"). This is usually transient — a prior session that
    /// did not log out cleanly leaves a ghost the grid evicts on the next login
    /// attempt — so logging in again typically succeeds. A driver may retry,
    /// ideally after consulting the user and mindful that a grid may flag rapid
    /// repeated attempts.
    AlreadyLoggedIn,
    /// Authentication failed: an unknown account or a wrong password (`"key"`).
    /// Retrying with the same credentials cannot succeed.
    BadCredentials,
    /// Any other rejection — including the non-retryable `"presence"` variants
    /// (logins administratively restricted, unverified account) and reasons this
    /// classifier does not model. Inspect the raw
    /// [`reason`](LoginFailure::reason) / [`message`](LoginFailure::message).
    Other,
}

impl LoginFailure {
    /// Classify this rejection into a [`LoginRejectKind`].
    ///
    /// `"key"` maps to [`LoginRejectKind::BadCredentials`]. The `"presence"`
    /// reason maps to [`LoginRejectKind::AlreadyLoggedIn`] *only* when the
    /// message identifies the already-logged-in case (it contains "already
    /// logged in"); the other `"presence"` uses (restricted logins, unverified
    /// account) are deliberately left as [`LoginRejectKind::Other`] so a caller
    /// does not retry them. Everything else is [`LoginRejectKind::Other`].
    #[must_use]
    pub fn kind(&self) -> LoginRejectKind {
        match self.reason.as_str() {
            "key" => LoginRejectKind::BadCredentials,
            "presence"
                if self
                    .message
                    .to_ascii_lowercase()
                    .contains("already logged in") =>
            {
                LoginRejectKind::AlreadyLoggedIn
            }
            _other => LoginRejectKind::Other,
        }
    }
}

/// An error encountered while parsing a login response.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoginParseError {
    /// The response was not well-formed XML.
    #[error("malformed XML in login response: {0}")]
    Xml(#[from] roxmltree::Error),
    /// The response was an XML-RPC fault.
    #[error("login server returned an XML-RPC fault: {message}")]
    Fault {
        /// The fault string.
        message: String,
    },
    /// The response did not contain the expected response struct.
    #[error("login response did not contain a response struct")]
    NoStruct,
    /// A required field was missing from a successful response.
    #[error("login response is missing required field {name:?}")]
    MissingField {
        /// The missing field name.
        name: &'static str,
    },
    /// A field could not be parsed into its expected type.
    #[error("login response field {name:?} has an invalid value {value:?}")]
    InvalidField {
        /// The field name.
        name: &'static str,
        /// The offending value.
        value: String,
    },
}

/// Parses an XML-RPC `login_to_simulator` response body.
///
/// # Errors
///
/// Returns a [`LoginParseError`] if the body is not well-formed, is an XML-RPC
/// fault, lacks the response struct, or is missing/has invalid required fields.
pub fn parse_login_response(xml: &str) -> Result<LoginResponse, LoginParseError> {
    let document = roxmltree::Document::parse(xml)?;

    if let Some(fault) = document.descendants().find(|n| n.has_tag_name("fault")) {
        let members = fault
            .descendants()
            .find(|n| n.has_tag_name("struct"))
            .map(collect_members)
            .unwrap_or_default();
        let message = members
            .get("faultString")
            .cloned()
            .unwrap_or_else(|| "unknown fault".to_owned());
        return Err(LoginParseError::Fault { message });
    }

    let response_struct = document
        .descendants()
        .find(|n| n.has_tag_name("param"))
        .and_then(|param| param.descendants().find(|n| n.has_tag_name("struct")))
        .ok_or(LoginParseError::NoStruct)?;
    let members = collect_members(response_struct);

    if members.get("login").map(String::as_str) != Some("true") {
        let reason = members.get("reason").cloned().unwrap_or_default();
        let message = members.get("message").cloned().unwrap_or_default();
        if reason == "mfa_challenge" {
            return Ok(LoginResponse::MfaChallenge(MfaChallenge {
                mfa_hash: members.get("mfa_hash").cloned(),
                message,
            }));
        }
        return Ok(LoginResponse::Failure(LoginFailure { reason, message }));
    }

    Ok(LoginResponse::Success(Box::new(LoginSuccess {
        agent_id: AgentKey::from(parse_uuid(&members, "agent_id")?),
        session_id: parse_uuid(&members, "session_id")?,
        secure_session_id: parse_uuid(&members, "secure_session_id")?,
        circuit_code: CircuitCode(parse_parsed(&members, "circuit_code")?),
        sim_ip: parse_parsed(&members, "sim_ip")?,
        sim_port: parse_parsed(&members, "sim_port")?,
        seed_capability: parse_parsed(&members, "seed_capability")?,
        message: members.get("message").cloned(),
        mfa_hash: members.get("mfa_hash").cloned(),
        inventory_root: parse_array_struct_uuid(response_struct, "inventory-root", "folder_id")
            .map(InventoryFolderKey::from),
        inventory_skeleton: parse_skeleton(response_struct, "inventory-skeleton"),
        buddy_list: parse_buddy_list(response_struct),
        home: members.get("home").and_then(|h| parse_home(h)),
        look_at: members.get("look_at").and_then(|l| parse_direction(l)),
        region_x: members.get("region_x").and_then(|x| x.trim().parse().ok()),
        region_y: members.get("region_y").and_then(|y| y.trim().parse().ok()),
        agent_access: members.get("agent_access").cloned(),
        agent_access_max: members.get("agent_access_max").cloned(),
        max_agent_groups: members
            .get("max-agent-groups")
            .and_then(|g| g.trim().parse().ok()),
        library_root: parse_array_struct_uuid(response_struct, "inventory-lib-root", "folder_id")
            .map(InventoryFolderKey::from),
        library_owner: parse_array_struct_uuid(response_struct, "inventory-lib-owner", "agent_id")
            .map(AgentKey::from),
        library_skeleton: parse_skeleton(response_struct, "inventory-skel-lib"),
        agent_appearance_service: members
            .get("agent_appearance_service")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        map_server_url: members
            .get("map-server-url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
    })))
}

/// Extracts a UUID from the named member: an array holding one struct with a
/// `field` string (e.g. `inventory-root` → `folder_id`, `inventory-lib-owner` →
/// `agent_id`).
fn parse_array_struct_uuid(
    response_struct: roxmltree::Node<'_, '_>,
    member: &str,
    field: &str,
) -> Option<Uuid> {
    let value = member_value_node(response_struct, member)?;
    let entry = array_structs(value).next()?;
    let members = collect_members(entry);
    members.get(field).and_then(|id| Uuid::parse_str(id).ok())
}

/// Parses the `home` field: a quasi-LLSD string `{'region_handle':[rX,rY],
/// 'position':[rX,rY,rZ], 'look_at':[rX,rY,rZ]}`. The numbers are prefixed with
/// `r` (the LLSD-over-XML-RPC real-number marker). Returns `None` if any of the
/// three sections is missing or malformed.
fn parse_home(value: &str) -> Option<HomeLocation> {
    let handle = r_numbers(section(value, "region_handle")?);
    let position = parse_region_coords(section(value, "position")?)?;
    let look_at = parse_direction(section(value, "look_at")?)?;
    let [x, y, ..] = handle.as_slice() else {
        return None;
    };
    Some(HomeLocation {
        region_handle: RegionHandle::from_global(round_to_u32(*x), round_to_u32(*y)),
        position,
        look_at,
    })
}

/// Parses a three-component vector from a quasi-LLSD `r`-prefixed list (e.g.
/// `[r1.0,r0.0,r0.0]`), tolerating surrounding brackets and whitespace.
fn parse_vector3(value: &str) -> Option<[f32; 3]> {
    let numbers = r_numbers(value);
    let [x, y, z, ..] = numbers.as_slice() else {
        return None;
    };
    Some([f64_to_f32(*x), f64_to_f32(*y), f64_to_f32(*z)])
}

/// Parses a quasi-LLSD `r`-prefixed list as region-local coordinates.
fn parse_region_coords(value: &str) -> Option<RegionCoordinates> {
    let [x, y, z] = parse_vector3(value)?;
    Some(RegionCoordinates::new(x, y, z))
}

/// Parses a quasi-LLSD `r`-prefixed list as a facing direction.
fn parse_direction(value: &str) -> Option<Direction> {
    let [x, y, z] = parse_vector3(value)?;
    Some(Direction::new(x, y, z))
}

/// Returns the contents between the `[` and `]` that follow the first occurrence
/// of `key` in `s` (e.g. `section("…'position':[r1,r2]…", "position")` →
/// `"r1,r2"`).
fn section<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let after = s.get(s.find(key)?.checked_add(key.len())?..)?;
    let open = after.find('[')?;
    let inner = after.get(open.checked_add(1)?..)?;
    let close = inner.find(']')?;
    inner.get(..close)
}

/// Parses a comma-separated list of `r`-prefixed real numbers, ignoring any
/// stray brackets and whitespace and skipping unparsable tokens.
fn r_numbers(list: &str) -> Vec<f64> {
    list.split(',')
        .filter_map(|token| {
            token
                .trim()
                .trim_matches(|c| c == '[' || c == ']')
                .trim()
                .trim_start_matches('r')
                .trim()
                .parse::<f64>()
                .ok()
        })
        .collect()
}

/// Narrows an `f64` to an `f32` (login coordinates are well within `f32` range).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "login position/look-at values are within f32 range"
)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

/// Rounds a non-negative `f64` world coordinate to a `u32` (region handle
/// components are integer-valued metres).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "region-handle world coordinates are non-negative integers within u32"
)]
const fn round_to_u32(value: f64) -> u32 {
    value.round() as u32
}

/// Extracts an inventory folder skeleton from the named member (e.g.
/// `inventory-skeleton` or `inventory-skel-lib`): an array of structs, one per
/// folder.
fn parse_skeleton(response_struct: roxmltree::Node<'_, '_>, member: &str) -> Vec<SkeletonFolder> {
    let Some(value) = member_value_node(response_struct, member) else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|folder_struct| {
            let members = collect_members(folder_struct);
            Some(SkeletonFolder {
                folder_id: InventoryFolderKey::from(
                    Uuid::parse_str(members.get("folder_id")?).ok()?,
                ),
                parent_id: InventoryFolderKey::from(
                    members
                        .get("parent_id")
                        .and_then(|id| Uuid::parse_str(id).ok())
                        .unwrap_or_else(Uuid::nil),
                ),
                name: members.get("name").cloned().unwrap_or_default(),
                type_default: members
                    .get("type_default")
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(-1),
                version: members
                    .get("version")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Extracts the friend/buddy list from the `buddy-list` member: an array of
/// structs, one per friend, each with a `buddy_id` and the two rights ints.
fn parse_buddy_list(response_struct: roxmltree::Node<'_, '_>) -> Vec<BuddyListEntry> {
    let Some(value) = member_value_node(response_struct, "buddy-list") else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|buddy_struct| {
            let members = collect_members(buddy_struct);
            Some(BuddyListEntry {
                buddy_id: Uuid::parse_str(members.get("buddy_id")?).ok()?,
                rights_granted: members
                    .get("buddy_rights_given")
                    .and_then(|r| r.trim().parse().ok())
                    .unwrap_or(0),
                rights_has: members
                    .get("buddy_rights_has")
                    .and_then(|r| r.trim().parse().ok())
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Finds the `<value>` node of the named `<member>` directly under a `<struct>`.
fn member_value_node<'a>(
    struct_node: roxmltree::Node<'a, '_>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'a>> {
    struct_node
        .children()
        .filter(|n| n.has_tag_name("member"))
        .find(|member| {
            member
                .children()
                .find(|n| n.has_tag_name("name"))
                .and_then(|n| n.text())
                == Some(name)
        })
        .and_then(|member| member.children().find(|n| n.has_tag_name("value")))
}

/// Iterates the `<struct>` nodes inside an array `<value>` (`value → array →
/// data → value → struct`).
fn array_structs<'a>(
    value_node: roxmltree::Node<'a, 'a>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    value_node
        .children()
        .find(|n| n.has_tag_name("array"))
        .and_then(|array| array.children().find(|n| n.has_tag_name("data")))
        .into_iter()
        .flat_map(|data| data.children().filter(|n| n.has_tag_name("value")))
        .filter_map(|value| value.children().find(|n| n.has_tag_name("struct")))
}

/// Collects the direct `<member>` children of a `<struct>` node into a map of
/// member name to scalar text value.
fn collect_members(struct_node: roxmltree::Node<'_, '_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for member in struct_node.children().filter(|n| n.has_tag_name("member")) {
        let name = member
            .children()
            .find(|n| n.has_tag_name("name"))
            .and_then(|n| n.text());
        let value = member
            .children()
            .find(|n| n.has_tag_name("value"))
            .map(scalar_text);
        if let (Some(name), Some(value)) = (name, value) {
            map.insert(name.to_owned(), value);
        }
    }
    map
}

/// Extracts the scalar text of a `<value>` node (its typed child's text, or its
/// own text for an untyped value).
fn scalar_text(value_node: roxmltree::Node<'_, '_>) -> String {
    if let Some(element) = value_node.children().find(roxmltree::Node::is_element) {
        element.text().unwrap_or_default().to_owned()
    } else {
        value_node.text().unwrap_or_default().to_owned()
    }
}

/// Returns a required member or a [`LoginParseError::MissingField`].
fn required<'a>(
    members: &'a HashMap<String, String>,
    name: &'static str,
) -> Result<&'a String, LoginParseError> {
    members
        .get(name)
        .ok_or(LoginParseError::MissingField { name })
}

/// Parses a required member as a UUID.
fn parse_uuid(
    members: &HashMap<String, String>,
    name: &'static str,
) -> Result<Uuid, LoginParseError> {
    let value = required(members, name)?;
    Uuid::parse_str(value).map_err(|_ignored| LoginParseError::InvalidField {
        name,
        value: value.clone(),
    })
}

/// Parses a required member via its [`std::str::FromStr`] implementation.
fn parse_parsed<T>(
    members: &HashMap<String, String>,
    name: &'static str,
) -> Result<T, LoginParseError>
where
    T: std::str::FromStr,
{
    let value = required(members, name)?;
    value
        .trim()
        .parse::<T>()
        .map_err(|_ignored| LoginParseError::InvalidField {
            name,
            value: value.clone(),
        })
}

// ---------------------------------------------------------------------------
// Server (login-endpoint) direction — the inverse of the client request
// builder and response parser above. `parse_login_request` reads what a viewer
// sent, `build_login_response` writes what a grid returns, and `LoginServer`
// maps a parsed request plus account/sim facts to the response to send.
// ---------------------------------------------------------------------------

/// A parsed XML-RPC `login_to_simulator` request, as a login server sees it.
///
/// The server-side counterpart to [`LoginRequest`]: the same fields, but the
/// password is the already-hashed `passwd` token the client sent (the server
/// never sees the plaintext) and the three boolean acknowledgement flags
/// (`agree_to_tos`/`read_critical`/`extended_errors`) are surfaced so the
/// endpoint can enforce them. Produced by [`parse_login_request`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLoginRequest {
    /// The avatar's first name (`first`).
    pub first_name: String,
    /// The avatar's last name (`last`).
    pub last_name: String,
    /// The hashed password as sent in `passwd` (`$1$<md5>`; see
    /// [`password_hash`]). Compared against the stored hash, never reversed.
    pub password_hash: String,
    /// The start location (`start`) the client requested. Parsed into a typed
    /// [`StartLocation`] when it matches the grammar (`Ok`); otherwise the raw
    /// string the client sent is preserved verbatim (`Err`), since this is
    /// untrusted input that need not be well-formed — so no value is ever lost
    /// and a malformed `start` cannot masquerade as a valid location.
    pub start: Result<StartLocation, String>,
    /// The viewer channel name (`channel`).
    pub channel: String,
    /// The viewer version string (`version`).
    pub version: String,
    /// The platform string (`platform`).
    pub platform: String,
    /// The hashed MAC address (`mac`).
    pub mac: String,
    /// The machine/installation id (`id0`).
    pub id0: String,
    /// The multi-factor one-time code (`token`), empty when not answering a
    /// challenge.
    pub token: String,
    /// A remembered `mfa_hash` echoed back to skip the challenge, empty when none.
    pub mfa_hash: String,
    /// Whether the request accepted the terms of service (`agree_to_tos`).
    pub agree_to_tos: bool,
    /// Whether the request acknowledged critical messages (`read_critical`).
    pub read_critical: bool,
    /// Whether the client asked for structured error reasons (`extended_errors`).
    pub extended_errors: bool,
    /// The requested response option flags (`options`, e.g. `inventory-root`).
    pub options: Vec<String>,
}

/// Parses an XML-RPC `login_to_simulator` request body into its fields.
///
/// The inverse of [`build_login_request`]: it reads the request struct a viewer
/// POSTs to the login endpoint. Missing scalar members default to empty strings
/// (the booleans to `false`), so a partial request still parses.
///
/// # Errors
///
/// Returns a [`LoginParseError`] if the body is not well-formed XML or does not
/// contain the request struct.
pub fn parse_login_request(xml: &str) -> Result<ParsedLoginRequest, LoginParseError> {
    let document = roxmltree::Document::parse(xml)?;
    let request_struct = document
        .descendants()
        .find(|n| n.has_tag_name("param"))
        .and_then(|param| param.descendants().find(|n| n.has_tag_name("struct")))
        .ok_or(LoginParseError::NoStruct)?;
    let members = collect_members(request_struct);
    let options = member_value_node(request_struct, "options")
        .map(array_strings)
        .unwrap_or_default();
    Ok(ParsedLoginRequest {
        first_name: member_string(&members, "first"),
        last_name: member_string(&members, "last"),
        password_hash: member_string(&members, "passwd"),
        start: parse_start_member(member_string(&members, "start")),
        channel: member_string(&members, "channel"),
        version: member_string(&members, "version"),
        platform: member_string(&members, "platform"),
        mac: member_string(&members, "mac"),
        id0: member_string(&members, "id0"),
        token: member_string(&members, "token"),
        mfa_hash: member_string(&members, "mfa_hash"),
        agree_to_tos: parse_bool_member(&members, "agree_to_tos"),
        read_critical: parse_bool_member(&members, "read_critical"),
        extended_errors: parse_bool_member(&members, "extended_errors"),
        options,
    })
}

/// Returns the named scalar member, or the empty string if absent.
fn member_string(members: &HashMap<String, String>, name: &str) -> String {
    members.get(name).cloned().unwrap_or_default()
}

/// Parses the request's raw `start` member into a typed [`StartLocation`],
/// preserving the original string (`Err`) when it does not match the grammar —
/// the client could send anything, and nothing is discarded.
fn parse_start_member(raw: String) -> Result<StartLocation, String> {
    raw.parse::<StartLocation>().map_err(|_ignored| raw)
}

/// Reads a boolean struct member, accepting the XML-RPC `1`/`0` and the textual
/// `true`/`false` forms; an absent or unrecognised member reads as `false`.
fn parse_bool_member(members: &HashMap<String, String>, name: &str) -> bool {
    matches!(members.get(name).map(String::as_str), Some("1" | "true"))
}

/// Iterates the string values inside an array `<value>` (`value → array → data
/// → value → string`), used for the request `options` list.
fn array_strings(value_node: roxmltree::Node<'_, '_>) -> Vec<String> {
    value_node
        .children()
        .find(|n| n.has_tag_name("array"))
        .and_then(|array| array.children().find(|n| n.has_tag_name("data")))
        .into_iter()
        .flat_map(|data| data.children().filter(|n| n.has_tag_name("value")))
        .map(scalar_text)
        .collect()
}

/// Builds the XML-RPC `login_to_simulator` response body for a [`LoginResponse`].
///
/// The inverse of [`parse_login_response`]: it emits the `<methodResponse>`
/// struct a grid returns — `login` plus the success payload (ids, sim placement,
/// seed cap, and any inventory/buddy/home/access/library fields that are
/// present), or the `reason`/`message` of a failure, or an `mfa_challenge`.
/// Optional fields are emitted only when set, so the result re-parses to an
/// equal [`LoginResponse`].
#[must_use]
pub fn build_login_response(response: &LoginResponse) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<methodResponse>\n<params><param><value><struct>\n");
    match response {
        LoginResponse::Success(success) => push_success_members(&mut out, success),
        LoginResponse::MfaChallenge(challenge) => {
            push_string_member(&mut out, "login", "false");
            push_string_member(&mut out, "reason", "mfa_challenge");
            push_string_member(&mut out, "message", &challenge.message);
            if let Some(mfa_hash) = &challenge.mfa_hash {
                push_string_member(&mut out, "mfa_hash", mfa_hash);
            }
        }
        LoginResponse::Failure(failure) => {
            push_string_member(&mut out, "login", "false");
            push_string_member(&mut out, "reason", &failure.reason);
            push_string_member(&mut out, "message", &failure.message);
        }
    }
    out.push_str("</struct></value></param></params>\n</methodResponse>\n");
    out
}

/// Appends the members of a successful login, in the order
/// [`parse_login_response`] reads them.
fn push_success_members(out: &mut String, success: &LoginSuccess) {
    push_string_member(out, "login", "true");
    push_string_member(out, "agent_id", &success.agent_id.to_string());
    push_string_member(out, "session_id", &success.session_id.to_string());
    push_string_member(
        out,
        "secure_session_id",
        &success.secure_session_id.to_string(),
    );
    push_int_member(out, "circuit_code", i64::from(success.circuit_code.get()));
    push_string_member(out, "sim_ip", &success.sim_ip.to_string());
    push_int_member(out, "sim_port", i64::from(success.sim_port));
    push_string_member(out, "seed_capability", success.seed_capability.as_str());
    push_opt_string_member(out, "message", success.message.as_deref());
    push_opt_string_member(out, "mfa_hash", success.mfa_hash.as_deref());
    if let Some(root) = success.inventory_root {
        push_id_array_member(out, "inventory-root", "folder_id", root.uuid());
    }
    push_skeleton_member(out, "inventory-skeleton", &success.inventory_skeleton);
    push_buddy_list_member(out, &success.buddy_list);
    if let Some(home) = &success.home {
        push_string_member(out, "home", &home_to_string(home));
    }
    if let Some(look_at) = success.look_at {
        push_string_member(
            out,
            "look_at",
            &vector3_to_string([look_at.x(), look_at.y(), look_at.z()]),
        );
    }
    if let Some(region_x) = success.region_x {
        push_int_member(out, "region_x", i64::from(region_x));
    }
    if let Some(region_y) = success.region_y {
        push_int_member(out, "region_y", i64::from(region_y));
    }
    push_opt_string_member(out, "agent_access", success.agent_access.as_deref());
    push_opt_string_member(out, "agent_access_max", success.agent_access_max.as_deref());
    if let Some(groups) = success.max_agent_groups {
        push_int_member(out, "max-agent-groups", i64::from(groups));
    }
    if let Some(root) = success.library_root {
        push_id_array_member(out, "inventory-lib-root", "folder_id", root.uuid());
    }
    if let Some(owner) = success.library_owner {
        push_id_array_member(out, "inventory-lib-owner", "agent_id", owner.uuid());
    }
    push_skeleton_member(out, "inventory-skel-lib", &success.library_skeleton);
    push_opt_string_member(
        out,
        "map-server-url",
        success.map_server_url.as_ref().map(url::Url::as_str),
    );
}

/// Appends an `<i4>` struct member.
fn push_int_member(out: &mut String, name: &str, value: i64) {
    out.push_str("<member><name>");
    out.push_str(name);
    out.push_str("</name><value><i4>");
    out.push_str(&value.to_string());
    out.push_str("</i4></value></member>\n");
}

/// Appends a `<string>` struct member only when the value is present.
fn push_opt_string_member(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        push_string_member(out, name, value);
    }
}

/// Appends an array member holding a single struct with one id field, the form
/// [`parse_array_struct_uuid`] reads (e.g. `inventory-root` → `folder_id`,
/// `inventory-lib-owner` → `agent_id`).
fn push_id_array_member(out: &mut String, member: &str, field: &str, id: Uuid) {
    out.push_str("<member><name>");
    out.push_str(member);
    out.push_str("</name><value><array><data>\n<value><struct>");
    push_string_member(out, field, &id.to_string());
    out.push_str("</struct></value>\n</data></array></value></member>\n");
}

/// Appends an inventory folder skeleton member (an array of folder structs), the
/// form [`parse_skeleton`] reads. Nothing is emitted for an empty skeleton, so
/// it re-parses as "not provided".
fn push_skeleton_member(out: &mut String, member: &str, folders: &[SkeletonFolder]) {
    if folders.is_empty() {
        return;
    }
    out.push_str("<member><name>");
    out.push_str(member);
    out.push_str("</name><value><array><data>\n");
    for folder in folders {
        out.push_str("<value><struct>");
        push_string_member(out, "folder_id", &folder.folder_id.to_string());
        push_string_member(out, "parent_id", &folder.parent_id.to_string());
        push_string_member(out, "name", &folder.name);
        push_int_member(out, "type_default", i64::from(folder.type_default));
        push_int_member(out, "version", i64::from(folder.version));
        out.push_str("</struct></value>\n");
    }
    out.push_str("</data></array></value></member>\n");
}

/// Appends the `buddy-list` member (an array of friend structs), the form
/// [`parse_buddy_list`] reads. Nothing is emitted for an empty list.
fn push_buddy_list_member(out: &mut String, buddies: &[BuddyListEntry]) {
    if buddies.is_empty() {
        return;
    }
    out.push_str("<member><name>buddy-list</name><value><array><data>\n");
    for buddy in buddies {
        out.push_str("<value><struct>");
        push_string_member(out, "buddy_id", &buddy.buddy_id.to_string());
        push_int_member(out, "buddy_rights_given", i64::from(buddy.rights_granted));
        push_int_member(out, "buddy_rights_has", i64::from(buddy.rights_has));
        out.push_str("</struct></value>\n");
    }
    out.push_str("</data></array></value></member>\n");
}

/// Formats a [`HomeLocation`] as the quasi-LLSD `home` string [`parse_home`]
/// reads: `{'region_handle':[rX,rY], 'position':[rX,rY,rZ], 'look_at':[rX,rY,rZ]}`
/// with the `r` real-number markers.
fn home_to_string(home: &HomeLocation) -> String {
    let (rx, ry) = home.region_handle.global_coordinates();
    let (px, py, pz) = (home.position.x(), home.position.y(), home.position.z());
    let (lx, ly, lz) = (home.look_at.x(), home.look_at.y(), home.look_at.z());
    format!(
        "{{'region_handle':[r{rx},r{ry}], 'position':[r{px},r{py},r{pz}], 'look_at':[r{lx},r{ly},r{lz}]}}"
    )
}

/// Formats a three-component vector as the quasi-LLSD `[rX,rY,rZ]` string
/// [`parse_vector3`] reads (used for the top-level `look_at` field).
fn vector3_to_string(vector: [f32; 3]) -> String {
    let [x, y, z] = vector;
    format!("[r{x},r{y},r{z}]")
}

/// A grid's server-side multi-factor policy for an account, used by
/// [`LoginServer::respond`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaPolicy {
    /// The one-time token the request's `token` must equal to authenticate.
    pub expected_token: String,
    /// The `mfa_hash` that, when echoed in the request's `mfa_hash`, skips the
    /// challenge ("remember this device") — and that a fresh challenge hands out.
    pub mfa_hash: String,
    /// The human-readable challenge message returned when MFA is required.
    pub challenge_message: String,
}

impl MfaPolicy {
    /// Whether `request` satisfies this policy: it carries the matching one-time
    /// token, or echoes the remembered [`mfa_hash`](Self::mfa_hash).
    #[must_use]
    pub fn is_satisfied_by(&self, request: &ParsedLoginRequest) -> bool {
        (!request.token.is_empty() && request.token == self.expected_token)
            || (!request.mfa_hash.is_empty() && request.mfa_hash == self.mfa_hash)
    }
}

/// The stored credentials a [`LoginServer`] checks a login request against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// The stored password hash (`$1$<md5>`; see [`password_hash`]), compared to
    /// the request's `passwd` field.
    pub password_hash: String,
    /// The multi-factor policy, if this account/grid requires one.
    pub mfa: Option<MfaPolicy>,
}

impl Credential {
    /// Whether the request's hashed password matches the stored hash.
    #[must_use]
    pub fn password_matches(&self, request: &ParsedLoginRequest) -> bool {
        self.password_hash == request.password_hash
    }
}

/// The server side of the XML-RPC `login_to_simulator` endpoint: the inverse of
/// the viewer's [`build_login_request`]/[`parse_login_response`] pair.
///
/// [`LoginServer::respond`] maps a parsed [`ParsedLoginRequest`] plus the
/// supplied account/simulator facts to the [`LoginResponse`] to return — a
/// success, a multi-factor challenge, or a failure. Sans-I/O: the caller looks
/// the account up, mints the session (the [`LoginSuccess`] it hands in), and
/// performs the HTTP transport; [`LoginServer`] enforces the password/MFA checks
/// and selects the response variant, which [`build_login_response`] then
/// serializes.
#[derive(Debug, Clone, Copy)]
pub struct LoginServer;

impl LoginServer {
    /// The failure reason code returned for a bad name/password, matching
    /// OpenSim/Second Life's `"key"`.
    pub const BAD_CREDENTIALS_REASON: &'static str = "key";

    /// Authenticates `request` against `credential` and selects the response to
    /// send: [`LoginResponse::Success`] wrapping the supplied `success` facts
    /// when the password matches and any MFA policy is satisfied;
    /// [`LoginResponse::MfaChallenge`] (with the policy's remembered hash and
    /// message) when MFA is required but unmet; or [`LoginResponse::Failure`]
    /// (reason [`LoginServer::BAD_CREDENTIALS_REASON`]) on a password mismatch.
    #[must_use]
    pub fn respond(
        request: &ParsedLoginRequest,
        credential: &Credential,
        success: Box<LoginSuccess>,
    ) -> LoginResponse {
        if !credential.password_matches(request) {
            return LoginResponse::Failure(LoginFailure {
                reason: Self::BAD_CREDENTIALS_REASON.to_owned(),
                message: "Could not authenticate your avatar. Check your user name and password."
                    .to_owned(),
            });
        }
        if let Some(mfa) = &credential.mfa
            && !mfa.is_satisfied_by(request)
        {
            return LoginResponse::MfaChallenge(MfaChallenge {
                mfa_hash: Some(mfa.mfa_hash.clone()),
                message: mfa.challenge_message.clone(),
            });
        }
        LoginResponse::Success(success)
    }
}

#[cfg(test)]
mod kind_tests {
    use super::{LoginFailure, LoginRejectKind};
    use pretty_assertions::assert_eq;

    /// Builds a failure with the given reason and message.
    fn failure(reason: &str, message: &str) -> LoginFailure {
        LoginFailure {
            reason: reason.to_owned(),
            message: message.to_owned(),
        }
    }

    /// `"key"` is bad credentials.
    #[test]
    fn key_is_bad_credentials() {
        assert_eq!(
            failure("key", "Could not authenticate your avatar.").kind(),
            LoginRejectKind::BadCredentials
        );
    }

    /// A `"presence"` rejection whose message says "already logged in" is the
    /// retryable case (matched case-insensitively).
    #[test]
    fn presence_already_logged_in() {
        assert_eq!(
            failure(
                "presence",
                "You appear to be already logged in.\n\nPlease wait a minute or two and retry.",
            )
            .kind(),
            LoginRejectKind::AlreadyLoggedIn
        );
        assert_eq!(
            failure("presence", "You appear to be ALREADY LOGGED IN.").kind(),
            LoginRejectKind::AlreadyLoggedIn
        );
    }

    /// The other `"presence"` uses (restricted logins, unverified account) are
    /// deliberately *not* classified as retryable.
    #[test]
    fn presence_non_retryable_is_other() {
        assert_eq!(
            failure(
                "presence",
                "Logins are currently restricted. Please try again later."
            )
            .kind(),
            LoginRejectKind::Other
        );
        assert_eq!(
            failure("presence", "Your account has not yet been verified.").kind(),
            LoginRejectKind::Other
        );
    }

    /// An unmodelled reason code falls through to `Other`.
    #[test]
    fn unknown_reason_is_other() {
        assert_eq!(
            failure("tos", "You must accept the ToS.").kind(),
            LoginRejectKind::Other
        );
    }
}
