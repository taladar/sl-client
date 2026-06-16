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
