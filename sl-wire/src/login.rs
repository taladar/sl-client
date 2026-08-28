//! The XML-RPC `login_to_simulator` request builder and response parser.
//!
//! This module is pure: it turns a [`LoginRequest`] into an XML-RPC request
//! body and parses an XML-RPC response string into a [`LoginResponse`]. The
//! actual HTTP(S) transport is performed by the I/O driver crates.

use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::str::FromStr;

use crate::geometry::Direction;
use crate::llsd::Llsd;
use crate::region_handle::RegionHandle;
use crate::xmlrpc::{array_value_nodes, push_member, push_value, value_to_llsd};
use sl_llsd::{parse_guarded_xml, push_escaped};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, TextureKey};
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The OS version string (`platform_string`, e.g. `"Linux 6.1"`; may be
    /// empty).
    pub platform_string: String,
    /// The OS/platform version number (`platform_version`; may be empty).
    pub platform_version: String,
    /// The process address size in bits (`address_size`, 32 or 64).
    pub address_size: i32,
    /// A stable host identifier (`host_id`; may be empty — the reference
    /// viewer sends its `HostID` setting).
    pub host_id: String,
    /// A hashed MAC address (any stable token; OpenSim is lenient).
    pub mac: String,
    /// A machine/installation id (may be empty).
    pub id0: String,
    /// How the previous session ended (`last_exec_event`, the viewer's
    /// crash-state code), if reported.
    pub last_exec_event: Option<i32>,
    /// The previous session's duration (`last_exec_duration`), if reported.
    pub last_exec_duration: Option<i32>,
    /// The previous session's agent session id (`last_exec_session_id`), if
    /// reported.
    pub last_exec_session_id: Option<Uuid>,
    /// The multi-factor authentication token (the one-time code), or empty on
    /// the first attempt before any [`LoginResponse::MfaChallenge`].
    pub token: String,
    /// A remembered multi-factor `mfa_hash` to echo back, or empty. Populated
    /// from a prior [`LoginSuccess::mfa_hash`] or an [`MfaChallenge::mfa_hash`].
    pub mfa_hash: String,
    /// Whether this request accepts the grid's terms of service
    /// (`agree_to_tos`). Kept `true` by default; a server that gates on a
    /// fresh ToS acceptance rejects with reason `"tos"` until the viewer
    /// re-sends with this set (see [`LoginRejectKind::Tos`]).
    pub agree_to_tos: bool,
    /// Whether this request acknowledges the grid's critical message
    /// (`read_critical`). Kept `true` by default; the `"critical"` gate
    /// mirrors the ToS gate (see [`LoginRejectKind::CriticalMessage`]).
    pub read_critical: bool,
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
            platform_string: String::new(),
            platform_version: String::new(),
            address_size: 64,
            host_id: String::new(),
            mac: "00000000000000000000000000000000".to_owned(),
            id0: String::new(),
            last_exec_event: None,
            last_exec_duration: None,
            last_exec_session_id: None,
            token: String::new(),
            mfa_hash: String::new(),
            agree_to_tos: true,
            read_critical: true,
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
    build_login_request_with_method(request, "login_to_simulator")
}

/// Builds the XML-RPC request body for a login call with an explicit method
/// name — the same struct as [`build_login_request`], but named per a
/// [`LoginRedirect::next_method`] when following a login redirect.
#[must_use]
pub fn build_login_request_with_method(request: &LoginRequest, method_name: &str) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<methodCall>\n<methodName>");
    push_escaped(&mut out, method_name);
    out.push_str("</methodName>\n<params><param><value><struct>\n");
    push_string_member(&mut out, "first", &request.first_name);
    push_string_member(&mut out, "last", &request.last_name);
    push_string_member(&mut out, "passwd", &password_hash(&request.password));
    push_string_member(&mut out, "start", &request.start.to_wire_string());
    push_string_member(&mut out, "channel", &request.channel);
    push_string_member(&mut out, "version", &request.version);
    push_string_member(&mut out, "platform", &request.platform);
    push_string_member(&mut out, "platform_string", &request.platform_string);
    push_string_member(&mut out, "platform_version", &request.platform_version);
    push_int_member(&mut out, "address_size", i64::from(request.address_size));
    push_string_member(&mut out, "host_id", &request.host_id);
    push_string_member(&mut out, "mac", &request.mac);
    push_string_member(&mut out, "id0", &request.id0);
    if let Some(event) = request.last_exec_event {
        push_int_member(&mut out, "last_exec_event", i64::from(event));
    }
    if let Some(duration) = request.last_exec_duration {
        push_int_member(&mut out, "last_exec_duration", i64::from(duration));
    }
    if let Some(session_id) = request.last_exec_session_id {
        push_string_member(&mut out, "last_exec_session_id", &session_id.to_string());
    }
    push_string_member(&mut out, "token", &request.token);
    push_string_member(&mut out, "mfa_hash", &request.mfa_hash);
    push_bool_member(&mut out, "agree_to_tos", request.agree_to_tos);
    push_bool_member(&mut out, "read_critical", request.read_critical);
    // Request structured error reasons (e.g. `mfa_challenge`).
    push_bool_member(&mut out, "extended_errors", true);
    push_string_array_member(&mut out, "options", &request.options);
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

/// Appends an array-of-strings member (the request `options` list, a
/// redirect's `next_options`), the form [`array_strings`] reads.
fn push_string_array_member(out: &mut String, name: &str, values: &[String]) {
    out.push_str("<member><name>");
    out.push_str(name);
    out.push_str("</name><value><array><data>\n");
    for value in values {
        out.push_str("<value><string>");
        push_escaped(out, value);
        out.push_str("</string></value>\n");
    }
    out.push_str("</data></array></value></member>\n");
}

/// A parsed login response: success, a multi-factor challenge, a redirect to
/// another login endpoint, or a failure.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LoginResponse {
    /// The login succeeded.
    Success(Box<LoginSuccess>),
    /// The grid requires a multi-factor one-time code. Retry the login with
    /// [`LoginRequest::with_mfa`], passing the code and this challenge's
    /// [`MfaChallenge::mfa_hash`].
    MfaChallenge(MfaChallenge),
    /// The grid redirected the login (`login == "indeterminate"`): re-POST
    /// the same request to [`LoginRedirect::next_url`].
    Redirect(LoginRedirect),
    /// The login was rejected by the grid.
    Failure(LoginFailure),
}

/// A login redirect (`login == "indeterminate"`): the grid wants the same
/// login re-POSTed to another endpoint — Second Life uses this to hand a
/// login off between servers. The reference viewer re-sends the identical
/// parameter struct to [`next_url`](Self::next_url) as an XML-RPC call named
/// [`next_method`](Self::next_method), looping until a terminal
/// success/failure arrives (our drivers bound the loop).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginRedirect {
    /// The login endpoint to re-POST to (`next_url`).
    pub next_url: url::Url,
    /// The XML-RPC method name for the re-POST (`next_method`, normally
    /// `"login_to_simulator"`).
    pub next_method: String,
    /// A human-readable progress message (`message`), if any — the viewer
    /// shows it on the login progress bar.
    pub message: Option<String>,
    /// The `next_options` list, carried for wire fidelity — the reference
    /// viewer does not consume it.
    pub next_options: Vec<String>,
}

/// A multi-factor authentication challenge returned by the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MfaChallenge {
    /// An `mfa_hash` the grid wants echoed back on the retry, if it provided
    /// one.
    pub mfa_hash: Option<String>,
    /// The human-readable challenge message.
    pub message: String,
}

/// The fields of a successful login needed to bring up the UDP circuit.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The OpenID endpoint the viewer POSTs [`openid_token`](Self::openid_token)
    /// to at login (`openid_url` response field) to mint the grid's web-session
    /// cookie. The POST reply's `Set-Cookie` is injected into the embedded
    /// browser's cookie store so the in-viewer web surfaces (web profiles,
    /// search, marketplace) open already logged in. Only Second Life grids send
    /// this; OpenSim omits it, so `None` there.
    pub openid_url: Option<url::Url>,
    /// The one-time token POSTed to [`openid_url`](Self::openid_url) as the raw
    /// request body (`Content-Type: application/x-www-form-urlencoded`), from
    /// the `openid_token` response field. `None` on grids that do not
    /// central-authenticate their websites (OpenSim).
    pub openid_token: Option<String>,
    /// The avatar's first name as the grid returned it (`first_name`), kept
    /// verbatim (Second Life has historically quoted it).
    pub first_name: Option<String>,
    /// The avatar's last name (`last_name`).
    pub last_name: Option<String>,
    /// The avatar's display name (`display_name`).
    pub display_name: Option<String>,
    /// The real/owning agent id behind this login (`real_id`), used by grids
    /// that support aliased logins. Nil/absent on most grids.
    pub real_id: Option<AgentKey>,
    /// The maturity rating of the *region* the avatar starts in
    /// (`agent_region_access`), in the same `"PG"`/`"M"`/`"A"` short-code form
    /// as [`agent_access`](Self::agent_access).
    pub agent_region_access: Option<String>,
    /// The start location the grid actually granted (`start_location`):
    /// `"last"`, `"home"`, or `"url"` — the *granted* category, not the
    /// request's `uri:` grammar.
    pub start_location: Option<String>,
    /// The server's current UNIX time (`seconds_since_epoch`), used by the
    /// viewer to compute its offset from grid time.
    pub seconds_since_epoch: Option<i64>,
    /// LLUDP message names the client must not send (`udp_blacklist`,
    /// comma-separated on the wire). Empty when the grid sent none.
    pub udp_blacklist: Vec<String>,
    /// The simulator's HTTP port (`http_port`). OpenSim sends it in the
    /// XML-RPC response only; `None` elsewhere.
    pub http_port: Option<u16>,
    /// The start region's X extent in metres (`region_size_x`), sent by
    /// OpenSim for variable-size regions (256 when absent).
    pub region_size_x: Option<u32>,
    /// The start region's Y extent in metres (`region_size_y`). See
    /// [`region_size_x`](Self::region_size_x).
    pub region_size_y: Option<u32>,
    /// The `login-flags` section (first-login flag, daylight savings, …), if
    /// provided.
    pub login_flags: Option<LoginFlags>,
    /// The `global-textures` section (grid default sun/cloud/moon texture
    /// ids), if provided.
    pub global_textures: Option<GlobalTextures>,
    /// The `ui-config` section, if provided.
    pub ui_config: Option<UiConfig>,
    /// The `initial-outfit` section (first-login library outfit), if
    /// provided.
    pub initial_outfit: Option<InitialOutfit>,
    /// The `newuser-config` section (default new-user avatars), if provided.
    pub newuser_config: Option<NewUserConfig>,
    /// The `voice-config` section (the grid's voice backend), if provided.
    pub voice_config: Option<VoiceConfig>,
    /// The avatar's active gestures (`gestures`). Empty if not
    /// requested/provided.
    pub gestures: Vec<GestureEntry>,
    /// The grid's event directory categories (`event_categories`). Empty if
    /// not requested/provided.
    pub event_categories: Vec<LoginCategory>,
    /// The grid's classified-ad categories (`classified_categories`). Empty
    /// if not requested/provided.
    pub classified_categories: Vec<LoginCategory>,
    /// The `event_notifications` entries, kept as opaque [`Llsd`] values —
    /// OpenSim always sends an empty list and Second Life's shape is not
    /// pinned by the reference viewer's parser, so nothing is discarded.
    pub event_notifications: Vec<Llsd>,
    /// The `tutorial_setting` entries (tutorial web page URLs). Empty if not
    /// requested/provided.
    pub tutorial_settings: Vec<TutorialSetting>,
    /// The grid's help-page URL *template* (`help_url_format`, with
    /// substitution placeholders — deliberately not parsed as a URL).
    pub help_url_format: Option<String>,
    /// The web-profile base URL (`web_profile_url`).
    pub web_profile_url: Option<url::Url>,
    /// The profile server base URL (`profile-server-url`, OpenSim).
    pub profile_server_url: Option<url::Url>,
    /// The search server URL (`search`, OpenSim).
    pub search_url: Option<url::Url>,
    /// The destination-guide URL (`destination_guide_url`).
    pub destination_guide_url: Option<url::Url>,
    /// The avatar-picker URL (`avatar_picker_url`).
    pub avatar_picker_url: Option<url::Url>,
    /// The grid's currency symbol (`currency`, e.g. `"L$"`; OpenSim helper).
    pub currency: Option<String>,
    /// The fee for placing a classified ad (`classified_fee`).
    pub classified_fee: Option<i32>,
    /// The fee for a directory listing (`directory_fee`).
    pub directory_fee: Option<i32>,
    /// The account's subscription level name (`account_type`, e.g. `"Base"`
    /// or `"Premium"`; Second Life).
    pub account_type: Option<String>,
    /// The account's benefit limits (`account_level_benefits`), kept as an
    /// opaque [`Llsd`] map — the set of keys is grid-defined and grows
    /// without notice (Second Life; absent on OpenSim).
    pub account_level_benefits: Option<Llsd>,
    /// The grid's subscription packages (`premium_packages`), kept as an
    /// opaque [`Llsd`] map. The reference viewer requires the `Base` and
    /// `Premium` keys to exist on grids that send this at all — a fidelity
    /// obligation on the *caller* filling this in, not on the codec.
    pub premium_packages: Option<Llsd>,
}

impl LoginSuccess {
    /// A success carrying only the fields required to bring up the UDP
    /// circuit — the seven the reference viewer refuses to proceed without —
    /// with every optional field empty. The starting point for tests and for
    /// servers that fill in optional sections one by one.
    #[must_use]
    pub const fn minimal(
        agent_id: AgentKey,
        session_id: Uuid,
        secure_session_id: Uuid,
        circuit_code: CircuitCode,
        sim_ip: Ipv4Addr,
        sim_port: u16,
        seed_capability: url::Url,
    ) -> Self {
        Self {
            agent_id,
            session_id,
            secure_session_id,
            circuit_code,
            sim_ip,
            sim_port,
            seed_capability,
            message: None,
            mfa_hash: None,
            inventory_root: None,
            inventory_skeleton: Vec::new(),
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
            library_skeleton: Vec::new(),
            agent_appearance_service: None,
            map_server_url: None,
            openid_url: None,
            openid_token: None,
            first_name: None,
            last_name: None,
            display_name: None,
            real_id: None,
            agent_region_access: None,
            start_location: None,
            seconds_since_epoch: None,
            udp_blacklist: Vec::new(),
            http_port: None,
            region_size_x: None,
            region_size_y: None,
            login_flags: None,
            global_textures: None,
            ui_config: None,
            initial_outfit: None,
            newuser_config: None,
            voice_config: None,
            gestures: Vec::new(),
            event_categories: Vec::new(),
            classified_categories: Vec::new(),
            event_notifications: Vec::new(),
            tutorial_settings: Vec::new(),
            help_url_format: None,
            web_profile_url: None,
            profile_server_url: None,
            search_url: None,
            destination_guide_url: None,
            avatar_picker_url: None,
            currency: None,
            classified_fee: None,
            directory_fee: None,
            account_type: None,
            account_level_benefits: None,
            premium_packages: None,
        }
    }

    /// Clears every response section whose *request option* name is not in
    /// `options`, leaving the always-sent fields untouched — the behaviour of
    /// a grid that honours the request's `options` list (Second Life does;
    /// OpenSim ignores the list and sends everything).
    ///
    /// [`LoginServer::respond`] deliberately does **not** call this: whether
    /// to honour the options is the serving grid's policy, so a fake grid
    /// picks by calling (or not calling) this on the success it hands in.
    pub fn filter_options(&mut self, options: &[String]) {
        /// Whether `options` contains `name`.
        fn wants(options: &[String], name: &str) -> bool {
            options.iter().any(|option| option == name)
        }
        if !wants(options, "inventory-root") {
            self.inventory_root = None;
        }
        if !wants(options, "inventory-skeleton") {
            self.inventory_skeleton.clear();
        }
        if !wants(options, "inventory-lib-root") {
            self.library_root = None;
        }
        if !wants(options, "inventory-lib-owner") {
            self.library_owner = None;
        }
        if !wants(options, "inventory-skel-lib") {
            self.library_skeleton.clear();
        }
        if !wants(options, "buddy-list") {
            self.buddy_list.clear();
        }
        if !wants(options, "gestures") {
            self.gestures.clear();
        }
        if !wants(options, "login-flags") {
            self.login_flags = None;
        }
        if !wants(options, "global-textures") {
            self.global_textures = None;
        }
        if !wants(options, "ui-config") {
            self.ui_config = None;
        }
        if !wants(options, "event_categories") {
            self.event_categories.clear();
        }
        if !wants(options, "event_notifications") {
            self.event_notifications.clear();
        }
        if !wants(options, "classified_categories") {
            self.classified_categories.clear();
        }
        if !wants(options, "initial-outfit") {
            self.initial_outfit = None;
        }
        if !wants(options, "newuser-config") {
            self.newuser_config = None;
        }
        if !wants(options, "tutorial_setting") {
            self.tutorial_settings.clear();
        }
        if !wants(options, "voice-config") {
            self.voice_config = None;
        }
        if !wants(options, "map-server-url") {
            self.map_server_url = None;
        }
        if !wants(options, "max-agent-groups") {
            self.max_agent_groups = None;
        }
    }
}

/// An agent's home location, parsed from the `home` login response field (a
/// quasi-LLSD string such as `{'region_handle':[r256000,r256000],
/// 'position':[r128.0,r128.0,r25.0], 'look_at':[r1.0,r0.0,r0.0]}`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuddyListEntry {
    /// The friend's agent id.
    pub buddy_id: Uuid,
    /// The rights the agent grants this friend (`buddy_rights_given`).
    pub rights_granted: i32,
    /// The rights this friend grants the agent (`buddy_rights_has`).
    pub rights_has: i32,
}

/// One active gesture carried in a login response (`gestures`): the gesture
/// inventory item and the gesture asset it plays.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GestureEntry {
    /// The gesture's inventory item id (`item_id`).
    pub item_id: InventoryKey,
    /// The gesture's asset id (`asset_id`).
    pub asset_id: Uuid,
}

/// The `login-flags` section of a login response: account-state flags the
/// viewer reads at startup. On the wire this is an array holding one struct of
/// `"Y"`/`"N"` strings; the three yes/no flags are surfaced as `bool`s and
/// re-emitted as `"Y"`/`"N"`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginFlags {
    /// Whether the account has logged in before (`ever_logged_in`). The viewer
    /// treats `false` as a first login (welcome flows, initial outfit).
    pub ever_logged_in: bool,
    /// Whether grid time (US Pacific) is currently on daylight savings
    /// (`daylight_savings`), used by the viewer's clock display.
    pub daylight_savings: bool,
    /// Whether the account's avatar has a gender set (`gendered`).
    pub gendered: bool,
    /// The `stipend_since_login` value, kept verbatim (OpenSim sends `"N"`;
    /// the field predates modelling and has no consumer in modern viewers).
    pub stipend_since_login: String,
}

/// The `global-textures` section of a login response: the grid-wide default
/// environment texture ids. Array-of-one-struct on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlobalTextures {
    /// The sun texture id (`sun_texture_id`).
    pub sun_texture_id: TextureKey,
    /// The cloud texture id (`cloud_texture_id`).
    pub cloud_texture_id: TextureKey,
    /// The moon texture id (`moon_texture_id`).
    pub moon_texture_id: TextureKey,
}

/// The `ui-config` section of a login response. Array-of-one-struct on the
/// wire, with `"Y"`/`"N"` string values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UiConfig {
    /// Whether the viewer may show "first life" profile UI
    /// (`allow_first_life`).
    pub allow_first_life: bool,
}

/// The `initial-outfit` section of a login response: the library outfit a
/// first-time avatar is dressed in. Array-of-one-struct on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InitialOutfit {
    /// The library clothing folder name (`folder_name`).
    pub folder_name: String,
    /// The outfit's gender (`gender`, e.g. `"female"`).
    pub gender: String,
}

/// The `newuser-config` section of a login response: the default avatars a
/// brand-new account may be offered. Array-of-one-struct on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NewUserConfig {
    /// The default female avatar name (`DefaultFemaleAvatar`), if provided.
    pub default_female_avatar: Option<String>,
    /// The default male avatar name (`DefaultMaleAvatar`), if provided.
    pub default_male_avatar: Option<String>,
}

/// The `voice-config` section of a login response. Array-of-one-struct on the
/// wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VoiceConfig {
    /// The voice backend the grid uses (`VoiceServerType`, e.g. `"webrtc"`).
    pub voice_server_type: String,
}

/// One category entry of the `event_categories` / `classified_categories`
/// login response arrays (both share this wire shape).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginCategory {
    /// The category id (`category_id`).
    pub category_id: i32,
    /// The human-readable category name (`category_name`).
    pub category_name: String,
}

/// One entry of the `tutorial_setting` login response array.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TutorialSetting {
    /// The tutorial web page URL (`tutorial_url`), kept verbatim (grids have
    /// sent both full URLs and URL fragments here).
    pub tutorial_url: String,
}

/// The reason a login was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginFailure {
    /// The machine-readable reason code (e.g. `"key"`, `"presence"`).
    pub reason: String,
    /// The human-readable failure message.
    pub message: String,
    /// A localization key for the message (`message_id`, e.g.
    /// `"LoginFailedAccountSuspended"`), sent when the request asked for
    /// `extended_errors`. The viewer looks it up in its string table and
    /// substitutes [`message_args`](Self::message_args).
    pub message_id: Option<String>,
    /// Substitution arguments for [`message_id`](Self::message_id)
    /// (`message_args`, e.g. `TIME` for a suspension end, `VERSION` for a
    /// required update). Empty when the grid sent none.
    pub message_args: BTreeMap<String, String>,
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The grid requires the terms of service to be accepted (`"tos"`). The
    /// reference viewer shows the ToS text from the failure message and, on
    /// acceptance, re-sends the same login with `agree_to_tos` set — retryable
    /// once the user agrees.
    Tos,
    /// The grid requires a critical message to be acknowledged (`"critical"`).
    /// The reference viewer shows the message and re-sends the same login
    /// with `read_critical` set — retryable once acknowledged.
    CriticalMessage,
    /// The grid requires a viewer update (`"update"` or `"optional"`). The
    /// required version, when provided, is in
    /// [`message_args`](LoginFailure::message_args) under `VERSION`.
    UpdateRequired,
    /// Any other rejection — including the non-retryable `"presence"` variants
    /// (logins administratively restricted, unverified account) and reasons this
    /// classifier does not model. Inspect the raw
    /// [`reason`](LoginFailure::reason) / [`message`](LoginFailure::message).
    Other,
}

impl LoginFailure {
    /// A failure with the given reason code and message and no extended
    /// error fields.
    #[must_use]
    pub fn new(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            message: message.into(),
            message_id: None,
            message_args: BTreeMap::new(),
        }
    }

    /// Classify this rejection into a [`LoginRejectKind`].
    ///
    /// `"key"` maps to [`LoginRejectKind::BadCredentials`], `"tos"` to
    /// [`LoginRejectKind::Tos`], `"critical"` to
    /// [`LoginRejectKind::CriticalMessage`], and `"update"`/`"optional"` to
    /// [`LoginRejectKind::UpdateRequired`]. The `"presence"`
    /// reason maps to [`LoginRejectKind::AlreadyLoggedIn`] *only* when the
    /// message identifies the already-logged-in case (it contains "already
    /// logged in"); the other `"presence"` uses (restricted logins, unverified
    /// account) are deliberately left as [`LoginRejectKind::Other`] so a caller
    /// does not retry them. Everything else is [`LoginRejectKind::Other`].
    #[must_use]
    pub fn kind(&self) -> LoginRejectKind {
        match self.reason.as_str() {
            "key" => LoginRejectKind::BadCredentials,
            "tos" => LoginRejectKind::Tos,
            "critical" => LoginRejectKind::CriticalMessage,
            "update" | "optional" => LoginRejectKind::UpdateRequired,
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
/// Returns a [`LoginParseError`] if the body is not well-formed or is nested
/// past [`sl_llsd::MAX_NESTING_DEPTH`], is an XML-RPC fault, lacks the response
/// struct, or is missing/has invalid required fields.
pub fn parse_login_response(xml: &str) -> Result<LoginResponse, LoginParseError> {
    let document = parse_guarded_xml(xml)?;

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

    let login = members.get("login").map(String::as_str);
    if login == Some("indeterminate") {
        // A login redirect: re-POST the same request to `next_url`. A
        // redirect without a usable `next_url` cannot be followed, so it
        // degrades to a failure rather than losing the response.
        let message = members.get("message").cloned();
        if let Some(next_url) = members
            .get("next_url")
            .and_then(|u| url::Url::parse(u.trim()).ok())
        {
            return Ok(LoginResponse::Redirect(LoginRedirect {
                next_url,
                next_method: members
                    .get("next_method")
                    .cloned()
                    .unwrap_or_else(|| "login_to_simulator".to_owned()),
                message,
                next_options: member_value_node(response_struct, "next_options")
                    .map(array_strings)
                    .unwrap_or_default(),
            }));
        }
        return Ok(LoginResponse::Failure(LoginFailure::new(
            "indeterminate",
            message.unwrap_or_default(),
        )));
    }
    if login != Some("true") {
        let reason = members.get("reason").cloned().unwrap_or_default();
        let message = members.get("message").cloned().unwrap_or_default();
        if reason == "mfa_challenge" {
            return Ok(LoginResponse::MfaChallenge(MfaChallenge {
                mfa_hash: members.get("mfa_hash").cloned(),
                message,
            }));
        }
        return Ok(LoginResponse::Failure(LoginFailure {
            reason,
            message,
            message_id: members.get("message_id").cloned(),
            message_args: parse_message_args(response_struct),
        }));
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
        // OpenSim also answers the viewer's OpenSim-specific `max_groups`
        // option under that key; prefer the canonical name when both exist.
        max_agent_groups: members
            .get("max-agent-groups")
            .or_else(|| members.get("max_groups"))
            .and_then(|g| g.trim().parse().ok()),
        library_root: parse_array_struct_uuid(response_struct, "inventory-lib-root", "folder_id")
            .map(InventoryFolderKey::from),
        library_owner: parse_array_struct_uuid(response_struct, "inventory-lib-owner", "agent_id")
            .map(AgentKey::from),
        library_skeleton: parse_skeleton(response_struct, "inventory-skel-lib"),
        agent_appearance_service: parse_appearance_service(&members),
        map_server_url: members
            .get("map-server-url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        openid_url: members
            .get("openid_url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        openid_token: members.get("openid_token").cloned(),
        first_name: members.get("first_name").cloned(),
        last_name: members.get("last_name").cloned(),
        display_name: members.get("display_name").cloned(),
        real_id: members
            .get("real_id")
            .and_then(|id| Uuid::parse_str(id.trim()).ok())
            .map(AgentKey::from),
        agent_region_access: members.get("agent_region_access").cloned(),
        start_location: members.get("start_location").cloned(),
        seconds_since_epoch: members
            .get("seconds_since_epoch")
            .and_then(|s| s.trim().parse().ok()),
        udp_blacklist: members
            .get("udp_blacklist")
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        http_port: members.get("http_port").and_then(|p| p.trim().parse().ok()),
        region_size_x: members
            .get("region_size_x")
            .and_then(|s| s.trim().parse().ok()),
        region_size_y: members
            .get("region_size_y")
            .and_then(|s| s.trim().parse().ok()),
        login_flags: parse_login_flags(response_struct),
        global_textures: parse_global_textures(response_struct),
        ui_config: parse_ui_config(response_struct),
        initial_outfit: parse_initial_outfit(response_struct),
        newuser_config: parse_newuser_config(response_struct),
        voice_config: parse_voice_config(response_struct),
        gestures: parse_gestures(response_struct),
        event_categories: parse_categories(response_struct, "event_categories"),
        classified_categories: parse_categories(response_struct, "classified_categories"),
        event_notifications: parse_event_notifications(response_struct),
        tutorial_settings: parse_tutorial_settings(response_struct),
        help_url_format: members.get("help_url_format").cloned(),
        web_profile_url: members
            .get("web_profile_url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        profile_server_url: members
            .get("profile-server-url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        search_url: members
            .get("search")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        destination_guide_url: members
            .get("destination_guide_url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        avatar_picker_url: members
            .get("avatar_picker_url")
            .and_then(|s| url::Url::parse(s.trim()).ok()),
        currency: members.get("currency").cloned(),
        classified_fee: members
            .get("classified_fee")
            .and_then(|f| f.trim().parse().ok()),
        directory_fee: members
            .get("directory_fee")
            .and_then(|f| f.trim().parse().ok()),
        account_type: members.get("account_type").cloned(),
        account_level_benefits: member_value_node(response_struct, "account_level_benefits")
            .map(value_to_llsd),
        premium_packages: member_value_node(response_struct, "premium_packages").map(value_to_llsd),
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
pub(crate) fn parse_home(value: &str) -> Option<HomeLocation> {
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
pub(crate) fn parse_direction(value: &str) -> Option<Direction> {
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

/// Extracts the `message_args` substitution map of a failure response (an
/// XML-RPC struct of scalar values, e.g. `TIME`/`VERSION`). Empty if absent.
fn parse_message_args(response_struct: roxmltree::Node<'_, '_>) -> BTreeMap<String, String> {
    member_value_node(response_struct, "message_args")
        .and_then(|value| value.children().find(|n| n.has_tag_name("struct")))
        .map(|args_struct| collect_members(args_struct).into_iter().collect())
        .unwrap_or_default()
}

/// Returns the member map of the *first* struct inside the named array member
/// — the "array holding one struct" shape the login response uses for its
/// config-like sections (`login-flags`, `global-textures`, `ui-config`, …).
fn first_array_struct_members(
    response_struct: roxmltree::Node<'_, '_>,
    member: &str,
) -> Option<HashMap<String, String>> {
    let value = member_value_node(response_struct, member)?;
    Some(collect_members(array_structs(value).next()?))
}

/// Parses a grid yes/no flag value: `"Y"`, `"true"`, or `"1"` (ASCII
/// case-insensitive) read as yes; anything else as no.
pub(crate) fn yn_wire_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "y" | "true" | "1"
    )
}

/// Reads a yes/no flag member: absent reads as no.
fn yn_flag(members: &HashMap<String, String>, key: &str) -> bool {
    members.get(key).is_some_and(|value| yn_wire_flag(value))
}

/// Renders a yes/no flag in the canonical `"Y"`/`"N"` grid form (the form
/// [`yn_wire_flag`] parses back, so flags round-trip).
pub(crate) const fn yn_str(value: bool) -> &'static str {
    if value { "Y" } else { "N" }
}

/// Extracts the `login-flags` section, if present.
fn parse_login_flags(response_struct: roxmltree::Node<'_, '_>) -> Option<LoginFlags> {
    let members = first_array_struct_members(response_struct, "login-flags")?;
    Some(LoginFlags {
        ever_logged_in: yn_flag(&members, "ever_logged_in"),
        daylight_savings: yn_flag(&members, "daylight_savings"),
        gendered: yn_flag(&members, "gendered"),
        stipend_since_login: members
            .get("stipend_since_login")
            .cloned()
            .unwrap_or_default(),
    })
}

/// Extracts the `global-textures` section, if present with all three ids.
fn parse_global_textures(response_struct: roxmltree::Node<'_, '_>) -> Option<GlobalTextures> {
    /// Parses one of the section's texture-id members.
    fn texture(members: &HashMap<String, String>, key: &str) -> Option<TextureKey> {
        Uuid::parse_str(members.get(key)?.trim())
            .ok()
            .map(TextureKey::from)
    }
    let members = first_array_struct_members(response_struct, "global-textures")?;
    Some(GlobalTextures {
        sun_texture_id: texture(&members, "sun_texture_id")?,
        cloud_texture_id: texture(&members, "cloud_texture_id")?,
        moon_texture_id: texture(&members, "moon_texture_id")?,
    })
}

/// Extracts the `ui-config` section, if present.
fn parse_ui_config(response_struct: roxmltree::Node<'_, '_>) -> Option<UiConfig> {
    let members = first_array_struct_members(response_struct, "ui-config")?;
    Some(UiConfig {
        allow_first_life: yn_flag(&members, "allow_first_life"),
    })
}

/// Extracts the `initial-outfit` section, if present.
fn parse_initial_outfit(response_struct: roxmltree::Node<'_, '_>) -> Option<InitialOutfit> {
    let members = first_array_struct_members(response_struct, "initial-outfit")?;
    Some(InitialOutfit {
        folder_name: members.get("folder_name").cloned().unwrap_or_default(),
        gender: members.get("gender").cloned().unwrap_or_default(),
    })
}

/// Extracts the `newuser-config` section, if present.
fn parse_newuser_config(response_struct: roxmltree::Node<'_, '_>) -> Option<NewUserConfig> {
    let members = first_array_struct_members(response_struct, "newuser-config")?;
    Some(NewUserConfig {
        default_female_avatar: members.get("DefaultFemaleAvatar").cloned(),
        default_male_avatar: members.get("DefaultMaleAvatar").cloned(),
    })
}

/// Extracts the `voice-config` section, if present with its server type.
fn parse_voice_config(response_struct: roxmltree::Node<'_, '_>) -> Option<VoiceConfig> {
    let members = first_array_struct_members(response_struct, "voice-config")?;
    Some(VoiceConfig {
        voice_server_type: members.get("VoiceServerType")?.clone(),
    })
}

/// Extracts the active-gesture list from the `gestures` member, skipping
/// entries without a parseable item and asset id.
fn parse_gestures(response_struct: roxmltree::Node<'_, '_>) -> Vec<GestureEntry> {
    let Some(value) = member_value_node(response_struct, "gestures") else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|gesture_struct| {
            let members = collect_members(gesture_struct);
            Some(GestureEntry {
                item_id: InventoryKey::from(Uuid::parse_str(members.get("item_id")?.trim()).ok()?),
                asset_id: Uuid::parse_str(members.get("asset_id")?.trim()).ok()?,
            })
        })
        .collect()
}

/// Extracts an event/classified category list (both share the
/// `category_id` + `category_name` wire shape), skipping malformed entries.
fn parse_categories(response_struct: roxmltree::Node<'_, '_>, member: &str) -> Vec<LoginCategory> {
    let Some(value) = member_value_node(response_struct, member) else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|category_struct| {
            let members = collect_members(category_struct);
            Some(LoginCategory {
                category_id: members.get("category_id")?.trim().parse().ok()?,
                category_name: members.get("category_name")?.clone(),
            })
        })
        .collect()
}

/// Extracts the `event_notifications` entries as opaque [`Llsd`] values.
fn parse_event_notifications(response_struct: roxmltree::Node<'_, '_>) -> Vec<Llsd> {
    let Some(value) = member_value_node(response_struct, "event_notifications") else {
        return Vec::new();
    };
    array_value_nodes(value).map(value_to_llsd).collect()
}

/// Extracts the `tutorial_setting` entries, skipping ones without a URL.
fn parse_tutorial_settings(response_struct: roxmltree::Node<'_, '_>) -> Vec<TutorialSetting> {
    let Some(value) = member_value_node(response_struct, "tutorial_setting") else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|setting_struct| {
            let members = collect_members(setting_struct);
            Some(TutorialSetting {
                tutorial_url: members.get("tutorial_url")?.clone(),
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
    array_value_nodes(value_node)
        .filter_map(|value| value.children().find(|n| n.has_tag_name("struct")))
}

/// Collects the direct `<member>` children of a `<struct>` node into a map of
/// member name to scalar text value.
/// Parse the `agent_appearance_service` login field (the server-side bake service
/// base URL), logging the outcome so a grid that returns baked-avatar textures
/// grey — because the field is absent, named differently, or not a parseable URL,
/// forcing the viewer onto the by-UUID CDN fallback the CDN rejects — is
/// diagnosable. On an absent field, logs the members that *do* look service-ish
/// (any key or value mentioning "appearance"/"bake") so a rename is spotted.
fn parse_appearance_service(members: &HashMap<String, String>) -> Option<url::Url> {
    match members.get("agent_appearance_service") {
        Some(raw) => match url::Url::parse(raw.trim()) {
            Ok(url) => {
                tracing::debug!("login agent_appearance_service = {url}");
                Some(url)
            }
            Err(error) => {
                tracing::warn!(
                    "login agent_appearance_service present but not a URL ({error}): {raw:?}"
                );
                None
            }
        },
        None => {
            let service_ish: Vec<&str> = members
                .iter()
                .filter(|(key, value)| {
                    let hay = format!("{key} {value}").to_lowercase();
                    hay.contains("appearance") || hay.contains("bake")
                })
                .map(|(key, _value)| key.as_str())
                .collect();
            tracing::warn!(
                "login response has no agent_appearance_service field (baked avatars will \
                 fall back to the by-UUID CDN fetch); appearance/bake-ish keys present: {service_ish:?}"
            );
            None
        }
    }
}

/// Collect an XML-RPC `<struct>` node's `<member>` entries into a name → scalar
/// value map (the typed child's text via [`scalar_text`]), the flat form the
/// login-response parsers read their fields out of.
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The OS version string (`platform_string`), empty when not sent.
    pub platform_string: String,
    /// The OS/platform version number (`platform_version`), empty when not
    /// sent.
    pub platform_version: String,
    /// The client's process address size in bits (`address_size`), if sent.
    pub address_size: Option<i32>,
    /// The client's stable host identifier (`host_id`), empty when not sent.
    pub host_id: String,
    /// The hashed MAC address (`mac`).
    pub mac: String,
    /// The machine/installation id (`id0`).
    pub id0: String,
    /// How the client's previous session ended (`last_exec_event`), if sent.
    pub last_exec_event: Option<i32>,
    /// The client's previous session duration (`last_exec_duration`), if sent.
    pub last_exec_duration: Option<i32>,
    /// The client's previous agent session id (`last_exec_session_id`), if
    /// sent and well-formed.
    pub last_exec_session_id: Option<Uuid>,
    /// The grid scope id (`scope_id`), if sent — OpenSim's LLSD login carries
    /// it; the XML-RPC form normally does not.
    pub scope_id: Option<Uuid>,
    /// A one-time web login key (`web_login_key`), if sent — OpenSim's
    /// alternative to `passwd` for web-initiated logins. Parse-only: checking
    /// it is the account service's job, not [`Credential`]'s.
    pub web_login_key: Option<Uuid>,
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
/// Returns a [`LoginParseError`] if the body is not well-formed XML, is nested
/// past [`sl_llsd::MAX_NESTING_DEPTH`], or does not contain the request struct.
pub fn parse_login_request(xml: &str) -> Result<ParsedLoginRequest, LoginParseError> {
    let document = parse_guarded_xml(xml)?;
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
        platform_string: member_string(&members, "platform_string"),
        platform_version: member_string(&members, "platform_version"),
        address_size: member_int(&members, "address_size"),
        host_id: member_string(&members, "host_id"),
        mac: member_string(&members, "mac"),
        id0: member_string(&members, "id0"),
        last_exec_event: member_int(&members, "last_exec_event"),
        last_exec_duration: member_int(&members, "last_exec_duration"),
        last_exec_session_id: member_uuid(&members, "last_exec_session_id"),
        scope_id: member_uuid(&members, "scope_id"),
        web_login_key: member_uuid(&members, "web_login_key"),
        token: member_string(&members, "token"),
        mfa_hash: member_string(&members, "mfa_hash"),
        agree_to_tos: parse_bool_member(&members, "agree_to_tos"),
        read_critical: parse_bool_member(&members, "read_critical"),
        extended_errors: parse_bool_member(&members, "extended_errors"),
        options,
    })
}

/// Returns the named member parsed as an integer, or `None` when absent or
/// malformed (untrusted client input).
fn member_int(members: &HashMap<String, String>, name: &str) -> Option<i32> {
    members
        .get(name)
        .and_then(|value| value.trim().parse().ok())
}

/// Returns the named member parsed as a UUID, or `None` when absent or
/// malformed (untrusted client input).
fn member_uuid(members: &HashMap<String, String>, name: &str) -> Option<Uuid> {
    members
        .get(name)
        .and_then(|value| Uuid::parse_str(value.trim()).ok())
}

/// Returns the named scalar member, or the empty string if absent.
fn member_string(members: &HashMap<String, String>, name: &str) -> String {
    members.get(name).cloned().unwrap_or_default()
}

/// Parses the request's raw `start` member into a typed [`StartLocation`],
/// preserving the original string (`Err`) when it does not match the grammar —
/// the client could send anything, and nothing is discarded.
pub(crate) fn parse_start_member(raw: String) -> Result<StartLocation, String> {
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
/// present), or the `reason`/`message` (plus any extended-error fields) of a
/// failure, or an `mfa_challenge`, or an `indeterminate` redirect.
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
        LoginResponse::Redirect(redirect) => {
            push_string_member(&mut out, "login", "indeterminate");
            push_string_member(&mut out, "next_url", redirect.next_url.as_str());
            push_string_member(&mut out, "next_method", &redirect.next_method);
            push_opt_string_member(&mut out, "message", redirect.message.as_deref());
            if !redirect.next_options.is_empty() {
                push_string_array_member(&mut out, "next_options", &redirect.next_options);
            }
        }
        LoginResponse::Failure(failure) => {
            push_string_member(&mut out, "login", "false");
            push_string_member(&mut out, "reason", &failure.reason);
            push_string_member(&mut out, "message", &failure.message);
            push_opt_string_member(&mut out, "message_id", failure.message_id.as_deref());
            if !failure.message_args.is_empty() {
                out.push_str("<member><name>message_args</name><value><struct>");
                for (key, value) in &failure.message_args {
                    push_string_member(&mut out, key, value);
                }
                out.push_str("</struct></value></member>\n");
            }
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
    push_opt_string_member(
        out,
        "agent_appearance_service",
        success
            .agent_appearance_service
            .as_ref()
            .map(url::Url::as_str),
    );
    push_opt_string_member(
        out,
        "openid_url",
        success.openid_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(out, "openid_token", success.openid_token.as_deref());
    push_opt_string_member(out, "first_name", success.first_name.as_deref());
    push_opt_string_member(out, "last_name", success.last_name.as_deref());
    push_opt_string_member(out, "display_name", success.display_name.as_deref());
    if let Some(real_id) = success.real_id {
        push_string_member(out, "real_id", &real_id.to_string());
    }
    push_opt_string_member(
        out,
        "agent_region_access",
        success.agent_region_access.as_deref(),
    );
    push_opt_string_member(out, "start_location", success.start_location.as_deref());
    if let Some(seconds) = success.seconds_since_epoch {
        push_int_member(out, "seconds_since_epoch", seconds);
    }
    if !success.udp_blacklist.is_empty() {
        push_string_member(out, "udp_blacklist", &success.udp_blacklist.join(","));
    }
    if let Some(port) = success.http_port {
        push_int_member(out, "http_port", i64::from(port));
    }
    if let Some(size) = success.region_size_x {
        push_int_member(out, "region_size_x", i64::from(size));
    }
    if let Some(size) = success.region_size_y {
        push_int_member(out, "region_size_y", i64::from(size));
    }
    if let Some(flags) = &success.login_flags {
        push_single_struct_member(out, "login-flags", |body| {
            push_string_member(body, "ever_logged_in", yn_str(flags.ever_logged_in));
            push_string_member(body, "daylight_savings", yn_str(flags.daylight_savings));
            push_string_member(body, "gendered", yn_str(flags.gendered));
            push_string_member(body, "stipend_since_login", &flags.stipend_since_login);
        });
    }
    if let Some(textures) = &success.global_textures {
        push_single_struct_member(out, "global-textures", |body| {
            push_string_member(body, "sun_texture_id", &textures.sun_texture_id.to_string());
            push_string_member(
                body,
                "cloud_texture_id",
                &textures.cloud_texture_id.to_string(),
            );
            push_string_member(
                body,
                "moon_texture_id",
                &textures.moon_texture_id.to_string(),
            );
        });
    }
    if let Some(ui_config) = &success.ui_config {
        push_single_struct_member(out, "ui-config", |body| {
            push_string_member(body, "allow_first_life", yn_str(ui_config.allow_first_life));
        });
    }
    if let Some(outfit) = &success.initial_outfit {
        push_single_struct_member(out, "initial-outfit", |body| {
            push_string_member(body, "folder_name", &outfit.folder_name);
            push_string_member(body, "gender", &outfit.gender);
        });
    }
    if let Some(config) = &success.newuser_config {
        push_single_struct_member(out, "newuser-config", |body| {
            push_opt_string_member(
                body,
                "DefaultFemaleAvatar",
                config.default_female_avatar.as_deref(),
            );
            push_opt_string_member(
                body,
                "DefaultMaleAvatar",
                config.default_male_avatar.as_deref(),
            );
        });
    }
    if let Some(voice) = &success.voice_config {
        push_single_struct_member(out, "voice-config", |body| {
            push_string_member(body, "VoiceServerType", &voice.voice_server_type);
        });
    }
    push_struct_array_member(out, "gestures", &success.gestures, |body, gesture| {
        push_string_member(body, "item_id", &gesture.item_id.to_string());
        push_string_member(body, "asset_id", &gesture.asset_id.to_string());
    });
    push_struct_array_member(
        out,
        "event_categories",
        &success.event_categories,
        push_category_members,
    );
    push_struct_array_member(
        out,
        "classified_categories",
        &success.classified_categories,
        push_category_members,
    );
    if !success.event_notifications.is_empty() {
        out.push_str("<member><name>event_notifications</name><value><array><data>\n");
        for entry in &success.event_notifications {
            push_value(out, entry);
            out.push('\n');
        }
        out.push_str("</data></array></value></member>\n");
    }
    push_struct_array_member(
        out,
        "tutorial_setting",
        &success.tutorial_settings,
        |body, setting| {
            push_string_member(body, "tutorial_url", &setting.tutorial_url);
        },
    );
    push_opt_string_member(out, "help_url_format", success.help_url_format.as_deref());
    push_opt_string_member(
        out,
        "web_profile_url",
        success.web_profile_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(
        out,
        "profile-server-url",
        success.profile_server_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(
        out,
        "search",
        success.search_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(
        out,
        "destination_guide_url",
        success.destination_guide_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(
        out,
        "avatar_picker_url",
        success.avatar_picker_url.as_ref().map(url::Url::as_str),
    );
    push_opt_string_member(out, "currency", success.currency.as_deref());
    if let Some(fee) = success.classified_fee {
        push_int_member(out, "classified_fee", i64::from(fee));
    }
    if let Some(fee) = success.directory_fee {
        push_int_member(out, "directory_fee", i64::from(fee));
    }
    push_opt_string_member(out, "account_type", success.account_type.as_deref());
    if let Some(benefits) = &success.account_level_benefits {
        push_member(out, "account_level_benefits", benefits);
    }
    if let Some(packages) = &success.premium_packages {
        push_member(out, "premium_packages", packages);
    }
}

/// Appends the two members of a [`LoginCategory`] struct (shared by the
/// `event_categories` and `classified_categories` arrays).
fn push_category_members(out: &mut String, category: &LoginCategory) {
    push_string_member(out, "category_name", &category.category_name);
    push_int_member(out, "category_id", i64::from(category.category_id));
}

/// Appends an array member holding a single struct whose members `emit_fields`
/// writes — the "array of one struct" shape of the config-like login response
/// sections, the form [`first_array_struct_members`] reads.
fn push_single_struct_member(
    out: &mut String,
    member: &str,
    emit_fields: impl FnOnce(&mut String),
) {
    out.push_str("<member><name>");
    out.push_str(member);
    out.push_str("</name><value><array><data>\n<value><struct>");
    emit_fields(out);
    out.push_str("</struct></value>\n</data></array></value></member>\n");
}

/// Appends an array member with one struct per entry, each written by
/// `emit_entry` — the shape [`array_structs`] reads. Nothing is emitted for an
/// empty list, so it re-parses as "not provided".
fn push_struct_array_member<T>(
    out: &mut String,
    member: &str,
    entries: &[T],
    mut emit_entry: impl FnMut(&mut String, &T),
) {
    if entries.is_empty() {
        return;
    }
    out.push_str("<member><name>");
    out.push_str(member);
    out.push_str("</name><value><array><data>\n");
    for entry in entries {
        out.push_str("<value><struct>");
        emit_entry(out, entry);
        out.push_str("</struct></value>\n");
    }
    out.push_str("</data></array></value></member>\n");
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
pub(crate) fn home_to_string(home: &HomeLocation) -> String {
    let (rx, ry) = home.region_handle.global_coordinates();
    let (px, py, pz) = (home.position.x(), home.position.y(), home.position.z());
    let (lx, ly, lz) = (home.look_at.x(), home.look_at.y(), home.look_at.z());
    format!(
        "{{'region_handle':[r{rx},r{ry}], 'position':[r{px},r{py},r{pz}], 'look_at':[r{lx},r{ly},r{lz}]}}"
    )
}

/// Formats a three-component vector as the quasi-LLSD `[rX,rY,rZ]` string
/// [`parse_vector3`] reads (used for the top-level `look_at` field).
pub(crate) fn vector3_to_string(vector: [f32; 3]) -> String {
    let [x, y, z] = vector;
    format!("[r{x},r{y},r{z}]")
}

/// A grid's server-side multi-factor policy for an account, used by
/// [`LoginServer::respond`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// The per-request policy gates a [`LoginServer`] enforces before letting a
/// correctly-authenticated login through — everything a grid can put between
/// a valid password and a session. All off by default, so
/// `&LoginGates::default()` is the plain password(+MFA)-only server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginGates {
    /// Redirect the login to another endpoint (`login = "indeterminate"`),
    /// served before any other check — the authoritative endpoint performs
    /// them after the client re-POSTs there.
    pub redirect: Option<LoginRedirect>,
    /// The terms-of-service text to require acceptance of: rejected with
    /// reason [`LoginServer::TOS_REASON`] until the request carries
    /// `agree_to_tos` (the viewer shows this message and re-sends).
    pub tos_message: Option<String>,
    /// The critical message to require acknowledgement of: rejected with
    /// reason [`LoginServer::CRITICAL_REASON`] until the request carries
    /// `read_critical`.
    pub critical_message: Option<String>,
    /// Whether the account already has a live presence on the grid: rejected
    /// with reason [`LoginServer::PRESENCE_REASON`] and the
    /// already-logged-in message (the retryable `"presence"` case).
    pub already_logged_in: bool,
}

/// The server side of the XML-RPC `login_to_simulator` endpoint: the inverse of
/// the viewer's [`build_login_request`]/[`parse_login_response`] pair.
///
/// [`LoginServer::respond`] maps a parsed [`ParsedLoginRequest`] plus the
/// supplied account/simulator facts to the [`LoginResponse`] to return — a
/// success, a multi-factor challenge, a redirect, or a failure. Sans-I/O: the
/// caller looks the account up, mints the session (the [`LoginSuccess`] it
/// hands in, pre-filtered with [`LoginSuccess::filter_options`] if the grid
/// honours the request's `options`), and performs the HTTP transport;
/// [`LoginServer`] enforces the password/gate/MFA checks and selects the
/// response variant, which [`build_login_response`] then serializes.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginServer;

impl LoginServer {
    /// The failure reason code returned for a bad name/password, matching
    /// OpenSim/Second Life's `"key"`.
    pub const BAD_CREDENTIALS_REASON: &'static str = "key";
    /// The failure reason code for the terms-of-service gate (`"tos"`).
    pub const TOS_REASON: &'static str = "tos";
    /// The failure reason code for the critical-message gate (`"critical"`).
    pub const CRITICAL_REASON: &'static str = "critical";
    /// The failure reason code for presence conflicts (`"presence"`).
    pub const PRESENCE_REASON: &'static str = "presence";
    /// The message accompanying an already-logged-in `"presence"` rejection,
    /// phrased (as on the real grids) so [`LoginFailure::kind`] classifies it
    /// as the retryable [`LoginRejectKind::AlreadyLoggedIn`].
    pub const ALREADY_LOGGED_IN_MESSAGE: &'static str = "You appear to be already logged in. If this is not the case, please wait a minute and \
         try again.";

    /// Authenticates `request` against `credential`, enforces `gates`, and
    /// selects the response to send. The checks run in the order the real
    /// grids exhibit — **redirect → password → ToS → critical message → MFA →
    /// presence → success** — so, for example, a redirect is served without
    /// leaking whether the password was right, and a wrong password is
    /// reported before any ToS/critical gate:
    ///
    /// - [`LoginResponse::Redirect`] when [`LoginGates::redirect`] is set;
    /// - [`LoginResponse::Failure`] (reason
    ///   [`LoginServer::BAD_CREDENTIALS_REASON`]) on a password mismatch;
    /// - [`LoginResponse::Failure`] (reason [`LoginServer::TOS_REASON`],
    ///   message = the ToS text) when [`LoginGates::tos_message`] is set and
    ///   the request does not carry `agree_to_tos`;
    /// - [`LoginResponse::Failure`] (reason [`LoginServer::CRITICAL_REASON`])
    ///   likewise for [`LoginGates::critical_message`] vs `read_critical`;
    /// - [`LoginResponse::MfaChallenge`] (with the policy's remembered hash
    ///   and message) when MFA is required but unmet;
    /// - [`LoginResponse::Failure`] (reason [`LoginServer::PRESENCE_REASON`],
    ///   the retryable already-logged-in message) when
    ///   [`LoginGates::already_logged_in`] is set;
    /// - otherwise [`LoginResponse::Success`] wrapping the supplied `success`
    ///   facts.
    ///
    /// The checks are [`LoginServer::rejection`], which a caller that has not
    /// built its `success` facts yet can run on its own.
    #[must_use]
    pub fn respond(
        request: &ParsedLoginRequest,
        credential: &Credential,
        gates: &LoginGates,
        success: Box<LoginSuccess>,
    ) -> LoginResponse {
        Self::rejection(request, credential, gates).unwrap_or(LoginResponse::Success(success))
    }

    /// The checks half of [`LoginServer::respond`]: the response to send when
    /// this login must **not** succeed (in the same order), or `None` when it
    /// may.
    ///
    /// A grid that mints an expensive session — sockets, a region state
    /// machine, a copy of the world — calls this first, so a wrong password
    /// or an ungated request costs nothing but the check.
    #[must_use]
    pub fn rejection(
        request: &ParsedLoginRequest,
        credential: &Credential,
        gates: &LoginGates,
    ) -> Option<LoginResponse> {
        if let Some(redirect) = &gates.redirect {
            return Some(LoginResponse::Redirect(redirect.clone()));
        }
        if !credential.password_matches(request) {
            return Some(LoginResponse::Failure(LoginFailure::new(
                Self::BAD_CREDENTIALS_REASON,
                "Could not authenticate your avatar. Check your user name and password.",
            )));
        }
        if let Some(tos_message) = &gates.tos_message
            && !request.agree_to_tos
        {
            return Some(LoginResponse::Failure(LoginFailure::new(
                Self::TOS_REASON,
                tos_message,
            )));
        }
        if let Some(critical_message) = &gates.critical_message
            && !request.read_critical
        {
            return Some(LoginResponse::Failure(LoginFailure::new(
                Self::CRITICAL_REASON,
                critical_message,
            )));
        }
        if let Some(mfa) = &credential.mfa
            && !mfa.is_satisfied_by(request)
        {
            return Some(LoginResponse::MfaChallenge(MfaChallenge {
                mfa_hash: Some(mfa.mfa_hash.clone()),
                message: mfa.challenge_message.clone(),
            }));
        }
        if gates.already_logged_in {
            return Some(LoginResponse::Failure(LoginFailure::new(
                Self::PRESENCE_REASON,
                Self::ALREADY_LOGGED_IN_MESSAGE,
            )));
        }
        None
    }
}

#[cfg(test)]
mod kind_tests {
    use super::{LoginFailure, LoginRejectKind};
    use pretty_assertions::assert_eq;

    /// Builds a failure with the given reason and message.
    fn failure(reason: &str, message: &str) -> LoginFailure {
        LoginFailure::new(reason, message)
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

    /// The gate reasons map to their retry-guiding kinds.
    #[test]
    fn gate_reasons_map_to_their_kinds() {
        assert_eq!(
            failure("tos", "You must accept the ToS.").kind(),
            LoginRejectKind::Tos
        );
        assert_eq!(
            failure("critical", "Grid maintenance tonight.").kind(),
            LoginRejectKind::CriticalMessage
        );
        assert_eq!(
            failure("update", "Please update your viewer.").kind(),
            LoginRejectKind::UpdateRequired
        );
        assert_eq!(
            failure("optional", "A newer viewer is available.").kind(),
            LoginRejectKind::UpdateRequired
        );
    }

    /// An unmodelled reason code falls through to `Other`.
    #[test]
    fn unknown_reason_is_other() {
        assert_eq!(
            failure("connect", "Could not connect.").kind(),
            LoginRejectKind::Other
        );
    }
}
