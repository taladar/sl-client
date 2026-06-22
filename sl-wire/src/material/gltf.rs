//! GLTF (PBR) material overrides and the `ModifyMaterialParams` set request.

use super::{GltfMaterialOverride, MaterialOverrideUpdate};
use crate::llsd::{Llsd, parse_llsd_xml, push_escaped};

// ---------------------------------------------------------------------------
// GLTF material override (GenericStreamingMessage method 0x4175)
// ---------------------------------------------------------------------------

/// Decodes a GLTF material-override `GenericStreamingMessage` payload (notation
/// LLSD `{ "id": <local id>, "te": [faces…], "od": [overrides…] }`) into a
/// [`GltfMaterialOverride`].
///
/// The per-face override documents (`od`) are returned as their raw notation
/// bytes rather than parsed — only the envelope (object id and affected faces)
/// is interpreted. Returns `None` if the payload is not the expected map.
#[must_use]
pub fn parse_gltf_material_override(data: &[u8]) -> Option<GltfMaterialOverride> {
    let mut scan = Scan::new(data);
    scan.expect(b'{')?;
    let mut local_id: Option<i64> = None;
    let mut faces_raw: Vec<i64> = Vec::new();
    let mut overrides: Vec<Vec<u8>> = Vec::new();
    loop {
        scan.skip_ws_sep();
        match scan.peek()? {
            b'}' => {
                scan.bump();
                break;
            }
            b'\'' | b'"' => {}
            _ => return None,
        }
        let key = scan.read_quoted_string()?;
        scan.skip_ws_sep();
        scan.expect(b':')?;
        match key.as_str() {
            "id" => local_id = Some(scan.read_integer()?),
            "te" => faces_raw = scan.read_integer_array()?,
            "od" => overrides = scan.read_raw_array()?,
            _ => {
                scan.skip_value()?;
            }
        }
    }
    let local_id = u32::try_from(local_id?).ok()?;
    let faces = faces_raw
        .into_iter()
        .filter_map(|value| u8::try_from(value).ok())
        .collect();
    Some(GltfMaterialOverride {
        local_id: crate::RegionLocalObjectId(local_id),
        faces,
        overrides,
    })
}

/// A minimal cursor over a notation-LLSD byte slice, sufficient to walk the
/// override envelope and slice out (without interpreting) nested values.
struct Scan<'a> {
    /// The backing buffer.
    buf: &'a [u8],
    /// The current offset into `buf`.
    pos: usize,
}

impl<'a> Scan<'a> {
    /// Creates a scanner over `buf`, positioned at its start.
    const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Returns the byte at the cursor without advancing.
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    /// Advances the cursor by one byte (saturating at the buffer end).
    const fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    /// Skips ASCII whitespace and element separators (commas).
    fn skip_ws_sep(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
            self.bump();
        }
    }

    /// Skips whitespace, then consumes `byte` if present, returning `None`
    /// otherwise.
    fn expect(&mut self, byte: u8) -> Option<()> {
        self.skip_ws_sep();
        if self.peek()? == byte {
            self.bump();
            Some(())
        } else {
            None
        }
    }

    /// Reads a notation string token (`'…'` or `"…"`), honouring `\` escapes.
    fn read_quoted_string(&mut self) -> Option<String> {
        self.skip_ws_sep();
        let quote = self.peek()?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        self.bump();
        let mut out = Vec::new();
        loop {
            let byte = self.peek()?;
            self.bump();
            match byte {
                b'\\' => {
                    let escaped = self.peek()?;
                    self.bump();
                    out.push(escaped);
                }
                b if b == quote => break,
                b => out.push(b),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    /// Reads a notation integer token (`i<digits>`, optionally signed).
    fn read_integer(&mut self) -> Option<i64> {
        self.expect(b'i')?;
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        let digits = self.buf.get(start..self.pos)?;
        std::str::from_utf8(digits).ok()?.parse().ok()
    }

    /// Reads a notation array of integers (`[ i1, i2, … ]`).
    fn read_integer_array(&mut self) -> Option<Vec<i64>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_sep();
            if self.peek()? == b']' {
                self.bump();
                break;
            }
            out.push(self.read_integer()?);
        }
        Some(out)
    }

    /// Reads a notation array, returning each element's raw bytes verbatim
    /// (used for the per-face GLTF overrides, which are left uninterpreted).
    fn read_raw_array(&mut self) -> Option<Vec<Vec<u8>>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_sep();
            if self.peek()? == b']' {
                self.bump();
                break;
            }
            let (start, end) = self.skip_value()?;
            out.push(self.buf.get(start..end)?.to_vec());
        }
        Some(out)
    }

    /// Advances past one complete notation value, returning its `(start, end)`
    /// byte range. Nested maps/arrays and quoted strings are balanced so that
    /// delimiters inside strings are not mistaken for structure.
    fn skip_value(&mut self) -> Option<(usize, usize)> {
        self.skip_ws_sep();
        let start = self.pos;
        match self.peek()? {
            b'!' => self.bump(),
            b'0' | b'1' | b't' | b'f' | b'T' | b'F' => self.skip_token(),
            b'i' | b'r' => {
                self.bump();
                self.skip_number();
            }
            b'u' => {
                self.bump();
                self.skip_uuid();
            }
            b'\'' | b'"' => {
                self.read_quoted_string()?;
            }
            b'l' | b'd' => {
                self.bump();
                self.read_quoted_string()?;
            }
            b's' | b'b' => self.skip_sized(),
            b'[' => {
                self.bump();
                loop {
                    self.skip_ws_sep();
                    if self.peek()? == b']' {
                        self.bump();
                        break;
                    }
                    self.skip_value()?;
                }
            }
            b'{' => {
                self.bump();
                loop {
                    self.skip_ws_sep();
                    if self.peek()? == b'}' {
                        self.bump();
                        break;
                    }
                    self.read_quoted_string()?;
                    self.expect(b':')?;
                    self.skip_value()?;
                }
            }
            _ => return None,
        }
        Some((start, self.pos))
    }

    /// Consumes a run of ASCII letters/digits (a bare boolean keyword).
    fn skip_token(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z')) {
            self.bump();
        }
    }

    /// Consumes a numeric run (sign, digits, decimal point and exponent).
    fn skip_number(&mut self) {
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.bump();
        }
    }

    /// Consumes a UUID run (hexadecimal digits and dashes).
    fn skip_uuid(&mut self) {
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'-')
        ) {
            self.bump();
        }
    }

    /// Consumes a size-prefixed string or binary token (`s(len)"…"`,
    /// `b(len)"…"`, `b16"…"` or `b64"…"`).
    fn skip_sized(&mut self) {
        self.bump();
        // Optional size or radix marker before the quoted body.
        while matches!(self.peek(), Some(b'0'..=b'9' | b'(' | b')')) {
            self.bump();
        }
        self.read_quoted_string();
    }
}

// ---------------------------------------------------------------------------
// ModifyMaterialParams (set GLTF material on object faces)
// ---------------------------------------------------------------------------

/// Builds the LLSD-XML body for a `ModifyMaterialParams` capability POST: an
/// array of per-face material assignments. Each entry carries `object_id` and
/// `side`, plus the supplied `gltf_json` (verbatim) and/or `asset_id`.
#[must_use]
pub fn build_modify_material_params_request(updates: &[MaterialOverrideUpdate]) -> String {
    let mut out = String::from("<llsd><array>");
    for update in updates {
        out.push_str("<map><key>object_id</key><uuid>");
        out.push_str(&update.object_id.to_string());
        out.push_str("</uuid><key>side</key><integer>");
        out.push_str(&update.side.to_string());
        out.push_str("</integer>");
        if let Some(json) = &update.gltf_json {
            out.push_str("<key>gltf_json</key><string>");
            push_escaped(&mut out, json);
            out.push_str("</string>");
        }
        if let Some(asset_id) = update.asset_id {
            out.push_str("<key>asset_id</key><uuid>");
            out.push_str(&asset_id.to_string());
            out.push_str("</uuid>");
        }
        out.push_str("</map>");
    }
    out.push_str("</array></llsd>");
    out
}

/// Builds a GLTF material-override `GenericStreamingMessage` payload — the
/// inverse of `parse_gltf_material_override`.
///
/// Emits the notation-LLSD map
/// `{ 'id': <local id>, 'te': [faces…], 'od': [overrides…] }` the simulator
/// pushes to viewers (the message's method id is
/// [`GLTF_MATERIAL_OVERRIDE_METHOD`](crate::GLTF_MATERIAL_OVERRIDE_METHOD)). The
/// per-face override documents are written verbatim from
/// [`GltfMaterialOverride::overrides`] (they are raw notation-LLSD bytes the
/// caller supplies — this layer does not build GLTF material documents),
/// positionally correlated with the face list.
#[must_use]
pub fn build_gltf_material_override(material_override: &GltfMaterialOverride) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'{');
    out.extend_from_slice(b"'id':i");
    out.extend_from_slice(material_override.local_id.to_string().as_bytes());
    out.extend_from_slice(b",'te':[");
    for (index, face) in material_override.faces.iter().enumerate() {
        if index != 0 {
            out.push(b',');
        }
        out.push(b'i');
        out.extend_from_slice(face.to_string().as_bytes());
    }
    out.extend_from_slice(b"],'od':[");
    for (index, document) in material_override.overrides.iter().enumerate() {
        if index != 0 {
            out.push(b',');
        }
        out.extend_from_slice(document);
    }
    out.extend_from_slice(b"]}");
    out
}

/// Parses a `ModifyMaterialParams` capability request body — the inverse of
/// [`build_modify_material_params_request`].
///
/// Reads the `<llsd><array>` of per-face assignments back into
/// [`MaterialOverrideUpdate`]s. Each entry must carry an `object_id`; `side`
/// defaults to `-1` (all faces) if absent, and the optional `gltf_json` /
/// `asset_id` fields are surfaced only when present. Malformed entries (no
/// `object_id`) are skipped.
///
/// # Errors
///
/// Returns the [`roxmltree`] error if `xml` is not well-formed.
pub fn parse_modify_material_params_request(
    xml: &str,
) -> Result<Vec<MaterialOverrideUpdate>, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let Some(array) = root.as_array() else {
        return Ok(Vec::new());
    };
    Ok(array.iter().filter_map(modify_material_update).collect())
}

/// Decodes one `ModifyMaterialParams` array entry into a
/// [`MaterialOverrideUpdate`], or `None` if it lacks an `object_id`.
fn modify_material_update(item: &Llsd) -> Option<MaterialOverrideUpdate> {
    let object_id = item.get("object_id").and_then(Llsd::as_uuid)?;
    let side = item.get("side").and_then(Llsd::as_i32).unwrap_or(-1);
    let gltf_json = item
        .get("gltf_json")
        .and_then(Llsd::as_str)
        .map(str::to_owned);
    let asset_id = item.get("asset_id").and_then(Llsd::as_uuid);
    Some(MaterialOverrideUpdate {
        object_id,
        side,
        gltf_json,
        asset_id,
    })
}
