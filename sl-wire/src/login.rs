//! The XML-RPC `login_to_simulator` request builder and response parser.
//!
//! This module is pure: it turns a [`LoginRequest`] into an XML-RPC request
//! body and parses an XML-RPC response string into a [`LoginResponse`]. The
//! actual HTTP(S) transport is performed by the I/O driver crates.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use thiserror::Error;
use uuid::Uuid;

/// The parameters of an XML-RPC `login_to_simulator` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRequest {
    /// The avatar's first name.
    pub first_name: String,
    /// The avatar's last name.
    pub last_name: String,
    /// The plaintext password (hashed when the request is built).
    pub password: String,
    /// The start location: `"last"`, `"home"`, or `"uri:Region&x&y&z"`.
    pub start: String,
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
        start: impl Into<String>,
        channel: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            first_name: first_name.into(),
            last_name: last_name.into(),
            password: password.into(),
            start: start.into(),
            channel: channel.into(),
            version: version.into(),
            platform: "lin".to_owned(),
            mac: "00000000000000000000000000000000".to_owned(),
            id0: String::new(),
            token: String::new(),
            mfa_hash: String::new(),
            // Request the inventory root and folder skeleton so the login
            // response carries the agent's full folder tree, and the buddy
            // list so it carries the agent's friends and their rights.
            options: vec![
                "inventory-root".to_owned(),
                "inventory-skeleton".to_owned(),
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
    push_string_member(&mut out, "start", &request.start);
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    /// The avatar/agent id.
    pub agent_id: Uuid,
    /// The session id used in the circuit.
    pub session_id: Uuid,
    /// The secure session id.
    pub secure_session_id: Uuid,
    /// The circuit code for `UseCircuitCode`.
    pub circuit_code: u32,
    /// The destination simulator's IPv4 address.
    pub sim_ip: Ipv4Addr,
    /// The destination simulator's UDP port.
    pub sim_port: u16,
    /// The capabilities seed URL.
    pub seed_capability: String,
    /// The welcome/login message, if any.
    pub message: Option<String>,
    /// A fresh `mfa_hash` to remember and send on future logins to skip the
    /// multi-factor challenge ("remember this device"), if the grid provided
    /// one.
    pub mfa_hash: Option<String>,
    /// The agent's inventory root ("My Inventory") folder id, from the
    /// `inventory-root` response field (if requested and provided).
    pub inventory_root: Option<Uuid>,
    /// The agent's inventory folder skeleton (every folder's id, parent, name,
    /// type, and version), from the `inventory-skeleton` response field. Empty if
    /// not requested/provided.
    pub inventory_skeleton: Vec<SkeletonFolder>,
    /// The agent's friends (the buddy list), each with the rights the agent
    /// grants them and the rights they grant the agent, from the `buddy-list`
    /// response field. Empty if not requested/provided or the agent has no
    /// friends.
    pub buddy_list: Vec<BuddyListEntry>,
}

/// One folder of the inventory skeleton carried in a login response
/// (`inventory-skeleton`): the folder tree without item contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonFolder {
    /// The folder's id.
    pub folder_id: Uuid,
    /// The parent folder's id (nil for the root).
    pub parent_id: Uuid,
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

/// An error encountered while parsing a login response.
#[derive(Debug, Error)]
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
        agent_id: parse_uuid(&members, "agent_id")?,
        session_id: parse_uuid(&members, "session_id")?,
        secure_session_id: parse_uuid(&members, "secure_session_id")?,
        circuit_code: parse_parsed(&members, "circuit_code")?,
        sim_ip: parse_parsed(&members, "sim_ip")?,
        sim_port: parse_parsed(&members, "sim_port")?,
        seed_capability: required(&members, "seed_capability")?.clone(),
        message: members.get("message").cloned(),
        mfa_hash: members.get("mfa_hash").cloned(),
        inventory_root: parse_inventory_root(response_struct),
        inventory_skeleton: parse_inventory_skeleton(response_struct),
        buddy_list: parse_buddy_list(response_struct),
    })))
}

/// Extracts the inventory root folder id from the `inventory-root` member: an
/// array holding one struct with a `folder_id` string.
fn parse_inventory_root(response_struct: roxmltree::Node<'_, '_>) -> Option<Uuid> {
    let value = member_value_node(response_struct, "inventory-root")?;
    let folder_struct = array_structs(value).next()?;
    let members = collect_members(folder_struct);
    members
        .get("folder_id")
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// Extracts the inventory folder skeleton from the `inventory-skeleton` member:
/// an array of structs, one per folder.
fn parse_inventory_skeleton(response_struct: roxmltree::Node<'_, '_>) -> Vec<SkeletonFolder> {
    let Some(value) = member_value_node(response_struct, "inventory-skeleton") else {
        return Vec::new();
    };
    array_structs(value)
        .filter_map(|folder_struct| {
            let members = collect_members(folder_struct);
            Some(SkeletonFolder {
                folder_id: Uuid::parse_str(members.get("folder_id")?).ok()?,
                parent_id: members
                    .get("parent_id")
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .unwrap_or_else(Uuid::nil),
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
