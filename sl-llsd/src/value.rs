//! The [`Llsd`] value model, its accessors, and the LLSD-XML codec.

use std::collections::HashMap;

use base64::Engine as _;
use uuid::Uuid;

use crate::error::LlsdError;

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

    /// Returns the map entries, if this is a map. Useful for iterating the
    /// uuid-keyed `_embedded` maps of an AIS3 inventory response.
    #[must_use]
    pub const fn as_map(&self) -> Option<&HashMap<String, Self>> {
        match self {
            Self::Map(map) => Some(map),
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

    /// The short LLSD kind name (`"integer"`, `"string"`, …) of this value, used
    /// to label a wrong-type field in a [`LlsdError::MalformedField`] diagnostic.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Undef => "undef",
            Self::Boolean(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Real(_) => "real",
            Self::String(_) => "string",
            Self::Uuid(_) => "uuid",
            Self::Binary(_) => "binary",
            Self::Date(_) => "date",
            Self::Uri(_) => "uri",
            Self::Array(_) => "array",
            Self::Map(_) => "map",
        }
    }

    /// Looks up an optional map member `key`, distinguishing *absent* from
    /// *present but wrong type*. Returns `Ok(None)` when the key is absent or
    /// explicitly `Undef` (a lenient optional field), `Ok(Some(value))` when the
    /// member is present and `accessor` accepts its LLSD kind, and
    /// `Err(LlsdError::MalformedField)` when the member is present but of the
    /// wrong kind — so a malformed body is rejected rather than silently coerced
    /// to a default.
    ///
    /// `field` is a short static label naming the field for the diagnostic.
    fn field_with<T>(
        &self,
        key: &str,
        field: &'static str,
        accessor: impl Fn(&Self) -> Option<T>,
    ) -> Result<Option<T>, LlsdError> {
        match self.get(key) {
            None | Some(Self::Undef) => Ok(None),
            Some(value) => accessor(value)
                .map(Some)
                .ok_or_else(|| LlsdError::MalformedField {
                    field,
                    value: value.kind().to_owned(),
                }),
        }
    }

    /// Reads an optional integer (or boolean) map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_i32(&self, key: &str, field: &'static str) -> Result<Option<i32>, LlsdError> {
        self.field_with(key, field, Self::as_i32)
    }

    /// Reads an optional real (or integer) map field as `f64`; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_f64(&self, key: &str, field: &'static str) -> Result<Option<f64>, LlsdError> {
        self.field_with(key, field, Self::as_f64)
    }

    /// Reads an optional real (or integer) map field narrowed to `f32`; see
    /// `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_f32(&self, key: &str, field: &'static str) -> Result<Option<f32>, LlsdError> {
        self.field_with(key, field, Self::as_f32)
    }

    /// Reads an optional boolean (or integer) map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_bool(&self, key: &str, field: &'static str) -> Result<Option<bool>, LlsdError> {
        self.field_with(key, field, Self::as_bool)
    }

    /// Reads an optional UUID map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_uuid(&self, key: &str, field: &'static str) -> Result<Option<Uuid>, LlsdError> {
        self.field_with(key, field, Self::as_uuid)
    }

    /// Reads an optional string (or URI/date) map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_str(&self, key: &str, field: &'static str) -> Result<Option<&str>, LlsdError> {
        match self.get(key) {
            None | Some(Self::Undef) => Ok(None),
            Some(value) => value
                .as_str()
                .map(Some)
                .ok_or_else(|| LlsdError::MalformedField {
                    field,
                    value: value.kind().to_owned(),
                }),
        }
    }

    /// Reads an optional binary map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_binary(&self, key: &str, field: &'static str) -> Result<Option<&[u8]>, LlsdError> {
        match self.get(key) {
            None | Some(Self::Undef) => Ok(None),
            Some(value) => value
                .as_binary()
                .map(Some)
                .ok_or_else(|| LlsdError::MalformedField {
                    field,
                    value: value.kind().to_owned(),
                }),
        }
    }

    /// Reads an optional array map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_array(
        &self,
        key: &str,
        field: &'static str,
    ) -> Result<Option<&[Self]>, LlsdError> {
        match self.get(key) {
            None | Some(Self::Undef) => Ok(None),
            Some(value) => value
                .as_array()
                .map(Some)
                .ok_or_else(|| LlsdError::MalformedField {
                    field,
                    value: value.kind().to_owned(),
                }),
        }
    }

    /// Reads an optional map map field; see `field_with`.
    ///
    /// # Errors
    /// Returns [`LlsdError::MalformedField`] if `key` is present but of the wrong LLSD kind.
    pub fn field_map(
        &self,
        key: &str,
        field: &'static str,
    ) -> Result<Option<&HashMap<String, Self>>, LlsdError> {
        match self.get(key) {
            None | Some(Self::Undef) => Ok(None),
            Some(value) => value
                .as_map()
                .map(Some)
                .ok_or_else(|| LlsdError::MalformedField {
                    field,
                    value: value.kind().to_owned(),
                }),
        }
    }

    /// Reads a *required* integer (or boolean) map field: like
    /// [`field_i32`](Self::field_i32) but a [`LlsdError::MissingField`] when the
    /// key is absent.
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_i32(&self, key: &str, field: &'static str) -> Result<i32, LlsdError> {
        self.field_i32(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* real (or integer) map field as `f64`; absent is a
    /// [`LlsdError::MissingField`]. See [`field_f64`](Self::field_f64).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_f64(&self, key: &str, field: &'static str) -> Result<f64, LlsdError> {
        self.field_f64(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* real (or integer) map field narrowed to `f32`; absent
    /// is a [`LlsdError::MissingField`]. See [`field_f32`](Self::field_f32).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_f32(&self, key: &str, field: &'static str) -> Result<f32, LlsdError> {
        self.field_f32(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* boolean (or integer) map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_bool`](Self::field_bool).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_bool(&self, key: &str, field: &'static str) -> Result<bool, LlsdError> {
        self.field_bool(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* UUID map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_uuid`](Self::field_uuid).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_uuid(&self, key: &str, field: &'static str) -> Result<Uuid, LlsdError> {
        self.field_uuid(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* string (or URI/date) map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_str`](Self::field_str).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_str(&self, key: &str, field: &'static str) -> Result<&str, LlsdError> {
        self.field_str(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* binary map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_binary`](Self::field_binary).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_binary(&self, key: &str, field: &'static str) -> Result<&[u8], LlsdError> {
        self.field_binary(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* array map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_array`](Self::field_array).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_array(&self, key: &str, field: &'static str) -> Result<&[Self], LlsdError> {
        self.field_array(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Reads a *required* map map field; absent is a
    /// [`LlsdError::MissingField`]. See [`field_map`](Self::field_map).
    ///
    /// # Errors
    /// Returns [`LlsdError::MissingField`] if `key` is absent, or
    /// [`LlsdError::MalformedField`] if present but of the wrong LLSD kind.
    pub fn require_map(
        &self,
        key: &str,
        field: &'static str,
    ) -> Result<&HashMap<String, Self>, LlsdError> {
        self.field_map(key, field)?
            .ok_or(LlsdError::MissingField { field })
    }

    /// Serializes this value as a complete LLSD-XML document
    /// (`<llsd>…</llsd>`) — the inverse of [`parse_llsd_xml`].
    ///
    /// Map keys are emitted in sorted order so two equal [`Llsd`] trees always
    /// serialize byte-for-byte identically (LLSD maps are unordered, so the
    /// order is a free choice; sorting makes it deterministic). Re-parsing the
    /// output with [`parse_llsd_xml`] yields an equal tree for every value kind:
    /// booleans round-trip via `true`/`false`, binary via standard base64, and
    /// `Date`/`Uri` as their verbatim strings. This is the foundation every
    /// CAPS- and login-side LLSD producer builds on rather than concatenating
    /// XML by hand.
    #[must_use]
    pub fn to_llsd_xml(&self) -> String {
        let mut out = String::from("<llsd>");
        self.push_llsd_xml(&mut out);
        out.push_str("</llsd>");
        out
    }

    /// Appends this value's LLSD-XML element(s) to `out` without the `<llsd>`
    /// document wrapper, recursing into arrays and maps. The element-by-element
    /// inverse of [`node_to_llsd`].
    fn push_llsd_xml(&self, out: &mut String) {
        match self {
            Self::Undef => out.push_str("<undef />"),
            Self::Boolean(value) => out.push_str(if *value {
                "<boolean>true</boolean>"
            } else {
                "<boolean>false</boolean>"
            }),
            Self::Integer(value) => {
                out.push_str("<integer>");
                out.push_str(&value.to_string());
                out.push_str("</integer>");
            }
            Self::Real(value) => {
                out.push_str("<real>");
                // Rust's shortest float formatting round-trips for finite reals;
                // LLSD trees from the wire never carry NaN/infinity.
                out.push_str(&value.to_string());
                out.push_str("</real>");
            }
            Self::String(value) => {
                out.push_str("<string>");
                push_escaped(out, value);
                out.push_str("</string>");
            }
            Self::Uuid(value) => {
                out.push_str("<uuid>");
                out.push_str(&value.to_string());
                out.push_str("</uuid>");
            }
            Self::Date(value) => {
                out.push_str("<date>");
                push_escaped(out, value);
                out.push_str("</date>");
            }
            Self::Uri(value) => {
                out.push_str("<uri>");
                push_escaped(out, value);
                out.push_str("</uri>");
            }
            Self::Binary(value) => {
                out.push_str("<binary>");
                out.push_str(&base64::engine::general_purpose::STANDARD.encode(value));
                out.push_str("</binary>");
            }
            Self::Array(values) => {
                out.push_str("<array>");
                for value in values {
                    value.push_llsd_xml(out);
                }
                out.push_str("</array>");
            }
            Self::Map(map) => {
                out.push_str("<map>");
                let mut entries: Vec<(&String, &Self)> = map.iter().collect();
                entries.sort_by(|left, right| left.0.cmp(right.0));
                for (key, value) in entries {
                    out.push_str("<key>");
                    push_escaped(out, key);
                    out.push_str("</key>");
                    value.push_llsd_xml(out);
                }
                out.push_str("</map>");
            }
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
pub fn push_escaped(out: &mut String, value: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// An absent key is lenient (`Ok(None)`), a present value of the right kind
    /// reads, and a present value of the *wrong* kind is a hard
    /// [`LlsdError::MalformedField`] rather than a silently coerced default.
    #[test]
    fn field_accessors_reject_wrong_kind_but_tolerate_absent() -> Result<(), LlsdError> {
        let map = Llsd::Map(HashMap::from([
            ("count".to_owned(), Llsd::Integer(7)),
            ("name".to_owned(), Llsd::String("region".to_owned())),
            ("missing".to_owned(), Llsd::Undef),
        ]));

        // Present, right kind.
        assert_eq!(map.field_i32("count", "count")?, Some(7));
        assert_eq!(map.field_str("name", "name")?, Some("region"));

        // Absent, or an explicit Undef, is lenient.
        assert_eq!(map.field_i32("absent", "absent")?, None);
        assert_eq!(map.field_str("missing", "missing")?, None);

        // Present but the wrong LLSD kind is a hard error carrying the field
        // label and the offending kind.
        assert_eq!(
            map.field_i32("name", "name"),
            Err(LlsdError::MalformedField {
                field: "name",
                value: "string".to_owned(),
            })
        );
        assert_eq!(
            map.field_str("count", "count"),
            Err(LlsdError::MalformedField {
                field: "count",
                value: "integer".to_owned(),
            })
        );
        Ok(())
    }
}
