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
pub(crate) fn push_escaped(out: &mut String, value: &str) {
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

/// Media-permission bit: no one (a `perms_interact` / `perms_control` value).
pub const MEDIA_PERM_NONE: u8 = 0;
/// Media-permission bit: the object owner.
pub const MEDIA_PERM_OWNER: u8 = 1;
/// Media-permission bit: the object's group.
pub const MEDIA_PERM_GROUP: u8 = 2;
/// Media-permission bit: anyone.
pub const MEDIA_PERM_ANYONE: u8 = 4;
/// All media permissions (owner | group | anyone) — the viewer's default.
pub const MEDIA_PERM_ALL: u8 = 7;

/// Per-face media settings for the media-on-a-prim system, as carried by the
/// `ObjectMedia` / `ObjectMediaNavigate` capabilities. Mirrors the viewer's
/// `LLMediaEntry`: a prim that has media enabled carries one of these per media
/// face (faces without media are absent / `None` in the per-face list).
///
/// The field defaults match the viewer's `LLMediaEntry` constructor, so a
/// [`Default`]ed entry with [`home_url`](Self::home_url) /
/// [`current_url`](Self::current_url) set is a valid "media here" record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the viewer's LLMediaEntry: these are independent on/off media flags, not a state enum"
)]
pub struct MediaEntry {
    /// Whether the alternate (non-interactive) image is shown until the media is
    /// interacted with (`alt_image_enable`).
    pub alt_image_enable: bool,
    /// The control style: `0` = standard browser controls, `1` = mini
    /// (`controls`).
    pub controls: i32,
    /// The URL currently shown (`current_url`); a navigation changes this.
    pub current_url: String,
    /// The home URL loaded when the media (re)starts (`home_url`).
    pub home_url: String,
    /// Whether playback loops (`auto_loop`).
    pub auto_loop: bool,
    /// Whether the media plays automatically (`auto_play`).
    pub auto_play: bool,
    /// Whether the media is scaled to fit the face (`auto_scale`).
    pub auto_scale: bool,
    /// Whether the camera zooms to the media on click (`auto_zoom`).
    pub auto_zoom: bool,
    /// Whether the first click interacts rather than zooming/selecting
    /// (`first_click_interact`).
    pub first_click_interact: bool,
    /// The media surface width in pixels (`width_pixels`).
    pub width_pixels: i32,
    /// The media surface height in pixels (`height_pixels`).
    pub height_pixels: i32,
    /// Whether the navigation white-list is enforced (`whitelist_enable`).
    pub whitelist_enable: bool,
    /// The navigation white-list URL patterns (`whitelist`).
    pub whitelist: Vec<String>,
    /// Who may interact with the media (`perms_interact`; a media-perms bitfield
    /// of [`MEDIA_PERM_OWNER`] / [`MEDIA_PERM_GROUP`] / [`MEDIA_PERM_ANYONE`]).
    pub perms_interact: u8,
    /// Who may use the media controls (`perms_control`; same bitfield).
    pub perms_control: u8,
}

impl Default for MediaEntry {
    fn default() -> Self {
        Self {
            alt_image_enable: false,
            controls: 0,
            current_url: String::new(),
            home_url: String::new(),
            auto_loop: false,
            auto_play: false,
            auto_scale: false,
            auto_zoom: false,
            first_click_interact: false,
            width_pixels: 0,
            height_pixels: 0,
            whitelist_enable: false,
            whitelist: Vec::new(),
            perms_interact: MEDIA_PERM_ALL,
            perms_control: MEDIA_PERM_ALL,
        }
    }
}

impl MediaEntry {
    /// Decodes a [`MediaEntry`] from its LLSD map form (one per-face entry of an
    /// `ObjectMedia` response's `object_media_data` array). Missing fields fall
    /// back to the viewer's [`Default`] values.
    #[must_use]
    pub fn from_llsd(value: &Llsd) -> Self {
        let default = Self::default();
        Self {
            alt_image_enable: llsd_bool(value, "alt_image_enable", default.alt_image_enable),
            controls: llsd_int(value, "controls", default.controls),
            current_url: llsd_string(value, "current_url"),
            home_url: llsd_string(value, "home_url"),
            auto_loop: llsd_bool(value, "auto_loop", default.auto_loop),
            auto_play: llsd_bool(value, "auto_play", default.auto_play),
            auto_scale: llsd_bool(value, "auto_scale", default.auto_scale),
            auto_zoom: llsd_bool(value, "auto_zoom", default.auto_zoom),
            first_click_interact: llsd_bool(
                value,
                "first_click_interact",
                default.first_click_interact,
            ),
            width_pixels: llsd_int(value, "width_pixels", default.width_pixels),
            height_pixels: llsd_int(value, "height_pixels", default.height_pixels),
            whitelist_enable: llsd_bool(value, "whitelist_enable", default.whitelist_enable),
            whitelist: value
                .get("whitelist")
                .and_then(Llsd::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(|entry| entry.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            perms_interact: llsd_perm(value, "perms_interact", default.perms_interact),
            perms_control: llsd_perm(value, "perms_control", default.perms_control),
        }
    }

    /// Serializes this entry as the LLSD-XML `<map>…</map>` body the viewer
    /// sends for one face of an `ObjectMedia` UPDATE (the field order matches
    /// the viewer's `LLMediaEntry::asLLSD`).
    fn push_llsd_map(&self, out: &mut String) {
        out.push_str("<map>");
        push_bool_field(out, "alt_image_enable", self.alt_image_enable);
        push_int_field(out, "controls", self.controls);
        push_string_field(out, "current_url", &self.current_url);
        push_string_field(out, "home_url", &self.home_url);
        push_bool_field(out, "auto_loop", self.auto_loop);
        push_bool_field(out, "auto_play", self.auto_play);
        push_bool_field(out, "auto_scale", self.auto_scale);
        push_bool_field(out, "auto_zoom", self.auto_zoom);
        push_bool_field(out, "first_click_interact", self.first_click_interact);
        push_int_field(out, "width_pixels", self.width_pixels);
        push_int_field(out, "height_pixels", self.height_pixels);
        push_bool_field(out, "whitelist_enable", self.whitelist_enable);
        out.push_str("<key>whitelist</key><array>");
        for pattern in &self.whitelist {
            out.push_str("<string>");
            push_escaped(out, pattern);
            out.push_str("</string>");
        }
        out.push_str("</array>");
        push_int_field(out, "perms_interact", i32::from(self.perms_interact));
        push_int_field(out, "perms_control", i32::from(self.perms_control));
        out.push_str("</map>");
    }
}

/// Reads a boolean LLSD map field, falling back to `default` when absent.
fn llsd_bool(value: &Llsd, key: &str, default: bool) -> bool {
    value.get(key).and_then(Llsd::as_bool).unwrap_or(default)
}

/// Reads an integer LLSD map field, falling back to `default` when absent.
fn llsd_int(value: &Llsd, key: &str, default: i32) -> i32 {
    value.get(key).and_then(Llsd::as_i32).unwrap_or(default)
}

/// Reads a string LLSD map field, defaulting to the empty string when absent.
fn llsd_string(value: &Llsd, key: &str) -> String {
    value
        .get(key)
        .and_then(Llsd::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Reads a media-permission byte field, falling back to `default` when absent or
/// out of a byte's range.
fn llsd_perm(value: &Llsd, key: &str, default: u8) -> u8 {
    value
        .get(key)
        .and_then(Llsd::as_i32)
        .and_then(|raw| u8::try_from(raw).ok())
        .unwrap_or(default)
}

/// Reads a UUID-valued LLSD value, accepting either a `uuid` or a `string`.
fn llsd_uuid(value: &Llsd) -> Option<Uuid> {
    value.as_uuid().or_else(|| {
        value
            .as_str()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
    })
}

/// Appends `<key>{key}</key><boolean>{0|1}</boolean>` to `out`.
fn push_bool_field(out: &mut String, key: &str, value: bool) {
    out.push_str("<key>");
    out.push_str(key);
    out.push_str("</key><boolean>");
    out.push(if value { '1' } else { '0' });
    out.push_str("</boolean>");
}

/// Appends `<key>{key}</key><integer>{value}</integer>` to `out`.
fn push_int_field(out: &mut String, key: &str, value: i32) {
    out.push_str("<key>");
    out.push_str(key);
    out.push_str("</key><integer>");
    out.push_str(&value.to_string());
    out.push_str("</integer>");
}

/// Appends `<key>{key}</key><string>{escaped value}</string>` to `out`.
fn push_string_field(out: &mut String, key: &str, value: &str) {
    out.push_str("<key>");
    out.push_str(key);
    out.push_str("</key><string>");
    push_escaped(out, value);
    out.push_str("</string>");
}

/// Builds the LLSD-XML body for an `ObjectMedia` capability **GET** request: a
/// `{ verb: "GET", object_id }` map asking for an object's current per-face
/// media. The simulator replies with an [`ObjectMediaResponse`].
#[must_use]
pub fn build_object_media_get_request(object_id: Uuid) -> String {
    format!(
        "<llsd><map><key>verb</key><string>GET</string><key>object_id</key><uuid>{object_id}</uuid></map></llsd>"
    )
}

/// Builds the LLSD-XML body for an `ObjectMedia` capability **UPDATE** request:
/// a `{ verb: "UPDATE", object_id, object_media_data: [...] }` map that sets the
/// object's per-face media. `faces` is one entry per prim face in order; a face
/// with no media is `None` (serialized as an LLSD `undef`, as the viewer does).
#[must_use]
pub fn build_object_media_update_request(object_id: Uuid, faces: &[Option<MediaEntry>]) -> String {
    let mut out =
        String::from("<llsd><map><key>verb</key><string>UPDATE</string><key>object_id</key><uuid>");
    out.push_str(&object_id.to_string());
    out.push_str("</uuid><key>object_media_data</key><array>");
    for face in faces {
        match face {
            Some(entry) => entry.push_llsd_map(&mut out),
            None => out.push_str("<undef />"),
        }
    }
    out.push_str("</array></map></llsd>");
    out
}

/// Builds the LLSD-XML body for an `ObjectMediaNavigate` capability request: a
/// `{ object_id, current_url, texture_index }` map navigating the media on a
/// single face (`face`) to `url`.
#[must_use]
pub fn build_object_media_navigate_request(object_id: Uuid, face: u8, url: &str) -> String {
    let mut out = String::from("<llsd><map><key>object_id</key><uuid>");
    out.push_str(&object_id.to_string());
    out.push_str("</uuid><key>current_url</key><string>");
    push_escaped(&mut out, url);
    out.push_str("</string><key>texture_index</key><integer>");
    out.push_str(&u32::from(face).to_string());
    out.push_str("</integer></map></llsd>");
    out
}

/// A decoded `ObjectMedia` capability GET response: the object's per-face media
/// (one slot per prim face, `None` for a face without media) and the media
/// version string the simulator advances on every media change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMediaResponse {
    /// The object the media belongs to.
    pub object_id: Uuid,
    /// The media version string (`x-mv:<serial>/<uuid>`); the same value the
    /// object's `MediaURL` field carries, advanced on every media change.
    pub version: String,
    /// Per-face media, one slot per prim face in order; `None` for a face that
    /// has no media.
    pub faces: Vec<Option<MediaEntry>>,
}

impl ObjectMediaResponse {
    /// Decodes an [`ObjectMediaResponse`] from the LLSD body of an `ObjectMedia`
    /// GET reply (`{ object_id, object_media_version, object_media_data }`).
    /// Returns `None` if the body carries no `object_id`.
    #[must_use]
    pub fn from_llsd(body: &Llsd) -> Option<Self> {
        let object_id = body.get("object_id").and_then(llsd_uuid)?;
        let version = body
            .get("object_media_version")
            .and_then(Llsd::as_str)
            .unwrap_or_default()
            .to_owned();
        let faces = body
            .get("object_media_data")
            .and_then(Llsd::as_array)
            .map(|array| {
                array
                    .iter()
                    .map(|entry| match entry {
                        Llsd::Map(_) => Some(MediaEntry::from_llsd(entry)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            object_id,
            version,
            faces,
        })
    }
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
