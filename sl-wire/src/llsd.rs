//! A minimal LLSD (Linden Lab Structured Data) value type and an LLSD-XML
//! parser, plus the small serializers needed to build capability requests.
//!
//! Second Life / OpenSim deliver capability (CAPS) payloads — notably the
//! `EventQueueGet` long-poll and its `ParcelProperties` events — as LLSD-XML
//! over HTTP. This module parses that format into an [`Llsd`] tree and builds
//! the LLSD-XML request bodies; the higher layers interpret the trees.

use std::collections::HashMap;

use base64::Engine as _;
use uuid::Uuid;

/// A parsed LLSD value.
#[derive(Debug, Clone, PartialEq)]
pub enum Llsd {
    /// The undefined / null value.
    Undef,
    /// A boolean.
    Boolean(bool),
    /// A 32-bit signed integer.
    Integer(i32),
    /// A double-precision real.
    Real(f64),
    /// A string.
    String(String),
    /// A UUID.
    Uuid(Uuid),
    /// An ISO-8601 date string (kept verbatim).
    Date(String),
    /// A URI string.
    Uri(String),
    /// Raw bytes (base64-encoded on the wire).
    Binary(Vec<u8>),
    /// An ordered array of values.
    Array(Vec<Self>),
    /// A string-keyed map of values.
    Map(HashMap<String, Self>),
}

impl Llsd {
    /// Returns the map member for `key`, if this is a map containing it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Map(map) => map.get(key),
            _ => None,
        }
    }

    /// Returns the array element at `index`, if this is an array containing it.
    #[must_use]
    pub fn index(&self, index: usize) -> Option<&Self> {
        match self {
            Self::Array(array) => array.get(index),
            _ => None,
        }
    }

    /// Returns the array elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(array) => Some(array),
            _ => None,
        }
    }

    /// Returns the string, if this is a string (or URI/date).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) | Self::Uri(value) | Self::Date(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the integer value, if this is an integer (or boolean).
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(*value),
            Self::Boolean(value) => Some(i32::from(*value)),
            _ => None,
        }
    }

    /// Returns the value as an `f64`, accepting reals and integers.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Real(value) => Some(*value),
            Self::Integer(value) => Some(f64::from(*value)),
            _ => None,
        }
    }

    /// Returns the value narrowed to an `f32` (LLSD reals are `f64`).
    #[must_use]
    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(narrow_to_f32)
    }

    /// Returns the boolean value, if this is a boolean (or integer).
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            Self::Integer(value) => Some(*value != 0),
            _ => None,
        }
    }

    /// Returns the UUID, if this is a UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> Option<Uuid> {
        match self {
            Self::Uuid(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the bytes, if this is a binary value.
    #[must_use]
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Self::Binary(value) => Some(value),
            _ => None,
        }
    }
}

/// Narrows an LLSD real (`f64`) to the `f32` used for vector components.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "LLSD reals are f64; vector components are stored as f32"
)]
const fn narrow_to_f32(value: f64) -> f32 {
    value as f32
}

/// Parses an LLSD-XML document into an [`Llsd`] value.
///
/// Malformed scalar contents are parsed leniently (defaulting to zero/empty);
/// only a structurally invalid XML document is reported as an error.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_llsd_xml(xml: &str) -> Result<Llsd, roxmltree::Error> {
    let document = roxmltree::Document::parse(xml)?;
    let root = document.root_element();
    // The <llsd> root wraps a single value element.
    let value = root
        .children()
        .find(roxmltree::Node::is_element)
        .map_or(Llsd::Undef, node_to_llsd);
    Ok(value)
}

/// Converts a single LLSD-XML value element into an [`Llsd`].
fn node_to_llsd(node: roxmltree::Node<'_, '_>) -> Llsd {
    match node.tag_name().name() {
        "boolean" => Llsd::Boolean(parse_bool(node.text())),
        "integer" => Llsd::Integer(parse_scalar(node.text(), 0)),
        "real" => Llsd::Real(parse_scalar(node.text(), 0.0)),
        "string" => Llsd::String(node.text().unwrap_or("").to_owned()),
        "uuid" => Llsd::Uuid(
            node.text()
                .and_then(|text| Uuid::parse_str(text.trim()).ok())
                .unwrap_or_else(Uuid::nil),
        ),
        "date" => Llsd::Date(node.text().unwrap_or("").to_owned()),
        "uri" => Llsd::Uri(node.text().unwrap_or("").to_owned()),
        "binary" => Llsd::Binary(decode_binary(node.text())),
        "array" => Llsd::Array(
            node.children()
                .filter(roxmltree::Node::is_element)
                .map(node_to_llsd)
                .collect(),
        ),
        "map" => Llsd::Map(parse_map(node)),
        // "undef" and anything unrecognised.
        _ => Llsd::Undef,
    }
}

/// Parses an LLSD-XML `<map>` element into a key/value map.
fn parse_map(node: roxmltree::Node<'_, '_>) -> HashMap<String, Llsd> {
    let mut map = HashMap::new();
    let mut pending_key: Option<String> = None;
    for child in node.children().filter(roxmltree::Node::is_element) {
        if child.tag_name().name() == "key" {
            pending_key = Some(child.text().unwrap_or("").to_owned());
        } else if let Some(key) = pending_key.take() {
            map.insert(key, node_to_llsd(child));
        }
    }
    map
}

/// Parses a trimmed scalar from element text, falling back to `default`.
fn parse_scalar<T: std::str::FromStr>(text: Option<&str>, default: T) -> T {
    text.and_then(|text| text.trim().parse().ok())
        .unwrap_or(default)
}

/// Parses an LLSD boolean: `1`/`true` are true, everything else false.
fn parse_bool(text: Option<&str>) -> bool {
    matches!(text.map(str::trim), Some("1" | "true"))
}

/// Base64-decodes binary element text, yielding empty bytes on failure.
fn decode_binary(text: Option<&str>) -> Vec<u8> {
    let Some(text) = text else {
        return Vec::new();
    };
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .unwrap_or_default()
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

/// Builds the LLSD-XML body for a capability-seed request: an array of the
/// requested capability names.
#[must_use]
pub fn build_seed_request(capability_names: &[&str]) -> String {
    let mut out = String::from("<llsd><array>");
    for name in capability_names {
        out.push_str("<string>");
        push_escaped(&mut out, name);
        out.push_str("</string>");
    }
    out.push_str("</array></llsd>");
    out
}

/// Builds the LLSD-XML body for an `EventQueueGet` poll: `{ ack, done }`.
#[must_use]
pub fn build_event_queue_request(ack: Option<i32>, done: bool) -> String {
    let ack_xml = match ack {
        Some(id) => format!("<integer>{id}</integer>"),
        None => "<undef />".to_owned(),
    };
    let done_xml = i32::from(done);
    format!(
        "<llsd><map><key>ack</key>{ack_xml}<key>done</key><boolean>{done_xml}</boolean></map></llsd>"
    )
}

/// Builds the LLSD-XML body for a `FetchInventoryDescendents2` request: a
/// `folders` array, one entry per folder to fetch, each requesting its
/// sub-folders and items sorted by name.
#[must_use]
pub fn build_fetch_inventory_request(owner_id: Uuid, folder_ids: &[Uuid]) -> String {
    let mut out = String::from("<llsd><map><key>folders</key><array>");
    for folder in folder_ids {
        out.push_str("<map><key>folder_id</key><uuid>");
        out.push_str(&folder.to_string());
        out.push_str("</uuid><key>owner_id</key><uuid>");
        out.push_str(&owner_id.to_string());
        out.push_str(concat!(
            "</uuid>",
            "<key>fetch_folders</key><boolean>1</boolean>",
            "<key>fetch_items</key><boolean>1</boolean>",
            "<key>sort_order</key><integer>0</integer></map>",
        ));
    }
    out.push_str("</array></map></llsd>");
    out
}

/// Builds the LLSD-XML body for a `GroupMemberData` capability request: a map
/// with the `group_id` to fetch the full member roster for (no paging, so the
/// simulator returns every member).
#[must_use]
pub fn build_group_member_data_request(group_id: Uuid) -> String {
    format!("<llsd><map><key>group_id</key><uuid>{group_id}</uuid></map></llsd>")
}

/// Builds the LLSD-XML body for an `UpdateAvatarAppearance` capability request
/// (the modern Second Life server-side bake): a map carrying the Current Outfit
/// Folder version the grid should bake. The grid replies with `{ success,
/// error?, expected? }` and broadcasts the baked result over UDP
/// `AvatarAppearance`.
#[must_use]
pub fn build_update_avatar_appearance_request(cof_version: i32) -> String {
    format!("<llsd><map><key>cof_version</key><integer>{cof_version}</integer></map></llsd>")
}

/// Builds the LLSD-XML metadata body for the first step of a
/// `NewFileAgentInventory` capability upload (the modern path that stores a new
/// asset *and* creates an inventory item). The simulator replies with an
/// `uploader` URL to which the raw asset bytes are then POSTed (see
/// [`parse_asset_upload_response`]).
///
/// `asset_type` and `inventory_type` are LL's short type names (e.g.
/// `"texture"` / `"texture"`, `"animatn"` / `"animation"`, `"mesh"` /
/// `"mesh"`); the `*_mask` values are the permission bitfields granted to the
/// next owner / group / everyone; `expected_upload_cost` is the L$ price the
/// client expects (the grid rejects a mismatch).
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the flat NewFileAgentInventory LLSD request fields"
)]
pub fn build_new_file_agent_inventory_request(
    folder_id: Uuid,
    asset_type: &str,
    inventory_type: &str,
    name: &str,
    description: &str,
    next_owner_mask: u32,
    group_mask: u32,
    everyone_mask: u32,
    expected_upload_cost: i32,
) -> String {
    let mut out = String::from("<llsd><map>");
    out.push_str("<key>folder_id</key><uuid>");
    out.push_str(&folder_id.to_string());
    out.push_str("</uuid><key>asset_type</key><string>");
    push_escaped(&mut out, asset_type);
    out.push_str("</string><key>inventory_type</key><string>");
    push_escaped(&mut out, inventory_type);
    out.push_str("</string><key>name</key><string>");
    push_escaped(&mut out, name);
    out.push_str("</string><key>description</key><string>");
    push_escaped(&mut out, description);
    out.push_str("</string>");
    out.push_str(&format!(
        concat!(
            "<key>next_owner_mask</key><integer>{}</integer>",
            "<key>group_mask</key><integer>{}</integer>",
            "<key>everyone_mask</key><integer>{}</integer>",
            "<key>expected_upload_cost</key><integer>{}</integer>",
        ),
        next_owner_mask, group_mask, everyone_mask, expected_upload_cost,
    ));
    out.push_str("</map></llsd>");
    out
}

/// Builds the LLSD-XML metadata body for the first step of an
/// `Update*AgentInventory` capability upload (replacing the asset of an
/// existing inventory item — gesture, notecard, script, or settings): a map
/// carrying the `item_id` to update. The simulator replies with an `uploader`
/// URL (see [`parse_asset_upload_response`]).
#[must_use]
pub fn build_update_item_asset_request(item_id: Uuid) -> String {
    format!("<llsd><map><key>item_id</key><uuid>{item_id}</uuid></map></llsd>")
}

/// Builds the LLSD-XML metadata body for the first step of an
/// `UploadBakedTexture` capability upload (a temporary avatar bake, which
/// creates no inventory item): an empty map, as the viewer sends.
#[must_use]
pub fn build_upload_baked_texture_request() -> String {
    String::from("<llsd><map /></llsd>")
}

/// A parsed response from either step of a CAPS asset upload (the
/// `NewFileAgentInventory` / `UploadBakedTexture` / `Update*AgentInventory`
/// two-step uploader). The first POST yields a `state` of `"upload"` with an
/// [`uploader`](Self::uploader) URL; the second (the raw-bytes POST) yields a
/// `state` of `"complete"` with [`new_asset`](Self::new_asset) and, when an
/// inventory item was created/updated,
/// [`new_inventory_item`](Self::new_inventory_item). A failure yields some other
/// state and, usually, an [`error`](Self::error) message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetUploadResponse {
    /// The uploader state (`"upload"`, `"complete"`, or an error state).
    pub state: String,
    /// The URL to POST the raw asset bytes to (present on the first step).
    pub uploader: Option<String>,
    /// The newly stored asset's UUID (present on completion).
    pub new_asset: Option<Uuid>,
    /// The created/updated inventory item's UUID (present on completion when the
    /// upload produced an inventory item; nil/absent for a baked texture).
    pub new_inventory_item: Option<Uuid>,
    /// The grid's error message, if the response reported a failure.
    pub error: Option<String>,
}

/// Parses a CAPS asset-upload response (either step of the two-step uploader)
/// into its [`state`](AssetUploadResponse::state), `uploader` URL, and
/// `new_asset` / `new_inventory_item` ids.
///
/// A nil `new_inventory_item` (as `UploadBakedTexture` returns) is normalised to
/// `None`.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_asset_upload_response(xml: &str) -> Result<AssetUploadResponse, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let state = root
        .get("state")
        .and_then(Llsd::as_str)
        .unwrap_or_default()
        .to_owned();
    let uploader = root
        .get("uploader")
        .and_then(Llsd::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned);
    let new_asset = upload_uuid(&root, "new_asset");
    let new_inventory_item = upload_uuid(&root, "new_inventory_item").filter(|id| !id.is_nil());
    let error = root
        .get("error")
        .and_then(upload_error_message)
        .filter(|message| !message.is_empty());
    Ok(AssetUploadResponse {
        state,
        uploader,
        new_asset,
        new_inventory_item,
        error,
    })
}

/// Extracts a UUID-valued field from an upload response, accepting it as either
/// an LLSD `uuid` or a `string` (the viewer encodes `new_asset` as a string).
fn upload_uuid(root: &Llsd, key: &str) -> Option<Uuid> {
    let value = root.get(key)?;
    value.as_uuid().or_else(|| {
        value
            .as_str()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
    })
}

/// Extracts a human-readable message from an upload response's `error` field,
/// which may be a plain string or a map carrying a `message` key.
fn upload_error_message(error: &Llsd) -> Option<String> {
    if let Some(text) = error.as_str() {
        return Some(text.to_owned());
    }
    error
        .get("message")
        .and_then(Llsd::as_str)
        .map(str::to_owned)
}

/// A single event from an [`EventQueueResponse`].
#[derive(Debug, Clone, PartialEq)]
pub struct EventQueueEvent {
    /// The event message name (e.g. `"ParcelProperties"`).
    pub message: String,
    /// The event body (an LLSD value, usually a map).
    pub body: Llsd,
}

/// A parsed `EventQueueGet` response.
#[derive(Debug, Clone, PartialEq)]
pub struct EventQueueResponse {
    /// The response id, echoed back as the next request's `ack`.
    pub id: i32,
    /// The events delivered in this response.
    pub events: Vec<EventQueueEvent>,
}

/// Parses a capability-seed response: an LLSD map of capability name to URL.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_seed_response(xml: &str) -> Result<HashMap<String, String>, roxmltree::Error> {
    let mut capabilities = HashMap::new();
    if let Llsd::Map(map) = parse_llsd_xml(xml)? {
        for (name, value) in map {
            if let Some(url) = value.as_str() {
                capabilities.insert(name, url.to_owned());
            }
        }
    }
    Ok(capabilities)
}

/// Parses an `EventQueueGet` response into its id and events.
///
/// # Errors
///
/// Returns a [`roxmltree::Error`] if the body is not well-formed XML.
pub fn parse_event_queue_response(xml: &str) -> Result<EventQueueResponse, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let id = root.get("id").and_then(Llsd::as_i32).unwrap_or(0);
    let mut events = Vec::new();
    if let Some(array) = root.get("events").and_then(Llsd::as_array) {
        for event in array {
            let message = event
                .get("message")
                .and_then(Llsd::as_str)
                .unwrap_or("")
                .to_owned();
            if message.is_empty() {
                continue;
            }
            let body = event.get("body").cloned().unwrap_or(Llsd::Undef);
            events.push(EventQueueEvent { message, body });
        }
    }
    Ok(EventQueueResponse { id, events })
}
