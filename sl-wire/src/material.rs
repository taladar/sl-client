//! Material protocol support: PBR/GLTF material overrides, the legacy
//! `RenderMaterials` capability, and the `ModifyMaterialParams` set request.
//!
//! Second Life carries two kinds of surface material referenced per face by a
//! `TextureEntry`'s 16-byte material id:
//!
//! - **Legacy materials** (normal/specular maps) exchanged over the
//!   `RenderMaterials` capability, whose payload is a *zlib-compressed binary
//!   LLSD* document — the only path stock OpenSim implements. This module ports
//!   that codec ([`build_render_materials_request`] /
//!   [`parse_render_materials_response`]) including a minimal binary-LLSD
//!   reader/writer.
//! - **Modern GLTF (PBR) materials**, where per-object/per-face *overrides* are
//!   pushed in a `GenericStreamingMessage` (method `0x4175`) as *notation* LLSD,
//!   and set with a `ModifyMaterialParams` capability POST. Per the project's
//!   asset-fetch scope, the GLTF document itself is **not** parsed here: the
//!   override envelope (object local id + affected faces) is decoded and each
//!   per-face override is surfaced as its raw notation bytes
//!   ([`parse_gltf_material_override`]), and the JSON a caller sets via
//!   [`build_modify_material_params_request`] is passed through opaque.
//!
//! The material *assets* themselves (`AT_MATERIAL` / `AT_GLTF`) are fetched and
//! uploaded over the generic asset pipeline (see the runtime asset commands);
//! only the surface-material protocol lives here.

use std::collections::HashMap;

use base64::Engine as _;
use uuid::Uuid;

use crate::endian;
use crate::field::Reader;
use crate::llsd::{Llsd, parse_llsd_xml, push_escaped};

/// The `GenericStreamingMessage` method id for a GLTF material override
/// (`LLGenericStreamingMessage::METHOD_GLTF_MATERIAL_OVERRIDE`).
pub const GLTF_MATERIAL_OVERRIDE_METHOD: u16 = 0x4175;

/// The fixed-point scale the `RenderMaterials` capability applies to a legacy
/// material's normal/specular map offsets, repeats and rotations: the wire
/// carries `round(value * 10000)` as an integer.
const MATERIAL_FIXED_SCALE: f32 = 10000.0;

/// A decoded GLTF (PBR) material override pushed in a `GenericStreamingMessage`.
///
/// The override targets a single object (`local_id`) and a set of its faces;
/// per the asset-fetch scope the per-face GLTF override documents are *not*
/// interpreted — each is surfaced as its raw notation-LLSD bytes, positionally
/// correlated with [`faces`](Self::faces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GltfMaterialOverride {
    /// The region-local id of the object whose material is overridden.
    pub local_id: u32,
    /// The face indices carrying an override, in order.
    pub faces: Vec<u8>,
    /// The raw per-face override LLSD (notation-encoded), one entry per face in
    /// [`faces`](Self::faces); left undecoded (it is GLTF material data).
    pub overrides: Vec<Vec<u8>>,
}

/// A legacy (pre-PBR) surface material: a diffuse-alpha mode plus optional
/// normal and specular maps with their texture transforms, as carried by the
/// `RenderMaterials` capability (`LLMaterial` / OpenSim's `FaceMaterial`).
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyMaterial {
    /// The normal-map texture id (nil for none).
    pub normal_map: Uuid,
    /// The normal-map offset `(s, t)`.
    pub normal_offset: (f32, f32),
    /// The normal-map repeats `(s, t)`.
    pub normal_repeat: (f32, f32),
    /// The normal-map rotation, in radians.
    pub normal_rotation: f32,
    /// The specular-map texture id (nil for none).
    pub specular_map: Uuid,
    /// The specular-map offset `(s, t)`.
    pub specular_offset: (f32, f32),
    /// The specular-map repeats `(s, t)`.
    pub specular_repeat: (f32, f32),
    /// The specular-map rotation, in radians.
    pub specular_rotation: f32,
    /// The specular highlight colour, RGBA.
    pub specular_color: [u8; 4],
    /// The specular-highlight exponent (glossiness).
    pub specular_exponent: u8,
    /// The environment-reflection intensity.
    pub environment_intensity: u8,
    /// The diffuse alpha-blending mode (`0` blend, `1` none, `2` emissive mask,
    /// `3` alpha mask).
    pub diffuse_alpha_mode: u8,
    /// The alpha-mask cutoff (used when [`diffuse_alpha_mode`](Self::diffuse_alpha_mode)
    /// is the alpha-mask mode).
    pub alpha_mask_cutoff: u8,
}

/// One legacy material returned by the `RenderMaterials` capability: the
/// 16-byte material id keying it and its decoded [`LegacyMaterial`].
#[derive(Debug, Clone, PartialEq)]
pub struct RenderMaterialEntry {
    /// The material id (the per-face `TextureEntry` material id requesting it).
    pub material_id: Uuid,
    /// The decoded material parameters.
    pub material: LegacyMaterial,
}

/// A single per-face material assignment for a `ModifyMaterialParams` request:
/// applies a GLTF override (`gltf_json`, opaque), a stored material `asset_id`,
/// or both to one face of an object. `side` is the face index, or `-1` for all
/// faces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialOverrideUpdate {
    /// The object to modify.
    pub object_id: Uuid,
    /// The target face index, or `-1` for every face.
    pub side: i32,
    /// The GLTF override document to apply (passed through verbatim; `""` clears
    /// the face's override). `None` omits the field.
    pub gltf_json: Option<String>,
    /// The stored material asset to apply (`None` omits the field).
    pub asset_id: Option<Uuid>,
}

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
        local_id,
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

// ---------------------------------------------------------------------------
// RenderMaterials (legacy materials capability — zipped binary LLSD)
// ---------------------------------------------------------------------------

/// Builds the LLSD-XML body for a `RenderMaterials` capability POST that
/// requests the legacy materials for `material_ids`: a `{ "Zipped": <binary> }`
/// map whose binary is the zlib-compressed binary-LLSD array of the 16-byte
/// material ids (the form OpenSim's `MaterialsModule` expects).
#[must_use]
pub fn build_render_materials_request(material_ids: &[Uuid]) -> String {
    let array = Llsd::Array(
        material_ids
            .iter()
            .map(|id| Llsd::Binary(id.as_bytes().to_vec()))
            .collect(),
    );
    let mut binary = Vec::new();
    write_binary_value(&array, &mut binary);
    let zipped = miniz_oxide::deflate::compress_to_vec_zlib(&binary, 6);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&zipped);
    format!("<llsd><map><key>Zipped</key><binary>{encoded}</binary></map></llsd>")
}

/// Parses a `RenderMaterials` capability POST response (a
/// `{ "Zipped": <binary> }` LLSD-XML map whose binary unzips to a binary-LLSD
/// array of `{ "ID": <binary>, "Material": <map> }`) into the decoded entries.
///
/// Best-effort: a malformed or empty response yields an empty vector.
#[must_use]
pub fn parse_render_materials_response(xml: &str) -> Vec<RenderMaterialEntry> {
    let Ok(root) = parse_llsd_xml(xml) else {
        return Vec::new();
    };
    let Some(zipped) = root.get("Zipped").and_then(Llsd::as_binary) else {
        return Vec::new();
    };
    let Ok(raw) = miniz_oxide::inflate::decompress_to_vec_zlib(zipped) else {
        return Vec::new();
    };
    let mut reader = Reader::new(&raw);
    let Some(Llsd::Array(items)) = read_binary_value(&mut reader) else {
        return Vec::new();
    };
    items.iter().filter_map(render_material_entry).collect()
}

/// Decodes one `{ "ID", "Material" }` entry of a `RenderMaterials` response.
fn render_material_entry(item: &Llsd) -> Option<RenderMaterialEntry> {
    let id_bytes = item.get("ID").and_then(Llsd::as_binary)?;
    let material_id = Uuid::from_slice(id_bytes).ok()?;
    let material = legacy_material_from_llsd(item.get("Material")?);
    Some(RenderMaterialEntry {
        material_id,
        material,
    })
}

/// Decodes a [`LegacyMaterial`] from its `RenderMaterials` LLSD map, undoing the
/// fixed-point scaling on the texture transforms.
fn legacy_material_from_llsd(map: &Llsd) -> LegacyMaterial {
    LegacyMaterial {
        normal_map: map
            .get("NormMap")
            .and_then(Llsd::as_uuid)
            .unwrap_or_default(),
        normal_offset: (scaled(map, "NormOffsetX"), scaled(map, "NormOffsetY")),
        normal_repeat: (scaled(map, "NormRepeatX"), scaled(map, "NormRepeatY")),
        normal_rotation: scaled(map, "NormRotation"),
        specular_map: map
            .get("SpecMap")
            .and_then(Llsd::as_uuid)
            .unwrap_or_default(),
        specular_offset: (scaled(map, "SpecOffsetX"), scaled(map, "SpecOffsetY")),
        specular_repeat: (scaled(map, "SpecRepeatX"), scaled(map, "SpecRepeatY")),
        specular_rotation: scaled(map, "SpecRotation"),
        specular_color: color_from_llsd(map.get("SpecColor")),
        specular_exponent: byte_field(map, "SpecExp"),
        environment_intensity: byte_field(map, "EnvIntensity"),
        diffuse_alpha_mode: byte_field(map, "DiffuseAlphaMode"),
        alpha_mask_cutoff: byte_field(map, "AlphaMaskCutoff"),
    }
}

/// Reads an integer map field and undoes the material fixed-point scale.
fn scaled(map: &Llsd, key: &str) -> f32 {
    let raw = map.get(key).and_then(Llsd::as_i32).unwrap_or(0);
    narrow_to_f32(f64::from(raw)) / MATERIAL_FIXED_SCALE
}

/// Reads a small unsigned-byte map field, clamping out-of-range values to `0`.
fn byte_field(map: &Llsd, key: &str) -> u8 {
    map.get(key)
        .and_then(Llsd::as_i32)
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0)
}

/// Decodes a four-element RGBA colour array (each element an integer 0–255).
fn color_from_llsd(value: Option<&Llsd>) -> [u8; 4] {
    let mut color = [255_u8; 4];
    if let Some(array) = value.and_then(Llsd::as_array) {
        for (slot, element) in color.iter_mut().zip(array) {
            if let Some(byte) = element.as_i32().and_then(|raw| u8::try_from(raw).ok()) {
                *slot = byte;
            }
        }
    }
    color
}

/// Narrows an `f64` to `f32` (the material transforms are stored as `f32`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "material texture transforms are f32; the f64 source is a small fixed-point integer"
)]
const fn narrow_to_f32(value: f64) -> f32 {
    value as f32
}

// ---------------------------------------------------------------------------
// Minimal binary LLSD codec (header-less, as OpenSim's MaterialsModule emits)
// ---------------------------------------------------------------------------

/// Reads one header-less binary-LLSD value from `reader`, or `None` on a
/// malformed/truncated stream.
fn read_binary_value(reader: &mut Reader<'_>) -> Option<Llsd> {
    match reader.u8().ok()? {
        b'!' => Some(Llsd::Undef),
        b'1' => Some(Llsd::Boolean(true)),
        b'0' => Some(Llsd::Boolean(false)),
        b'i' => Some(Llsd::Integer(read_be_i32(reader)?)),
        b'r' => Some(Llsd::Real(endian::f64_from_be(
            reader.take_array::<8>().ok()?,
        ))),
        b'u' => Some(Llsd::Uuid(Uuid::from_bytes(
            reader.take_array::<16>().ok()?,
        ))),
        b'b' => {
            let len = read_len(reader)?;
            Some(Llsd::Binary(reader.take(len).ok()?.to_vec()))
        }
        b's' => Some(Llsd::String(read_sized_string(reader)?)),
        b'l' => Some(Llsd::Uri(read_sized_string(reader)?)),
        b'd' => {
            reader.take_array::<8>().ok()?;
            Some(Llsd::Date(String::new()))
        }
        b'[' => read_binary_array(reader),
        b'{' => read_binary_map(reader),
        _ => None,
    }
}

/// Reads a big-endian binary-LLSD `i32` (integers and size prefixes).
fn read_be_i32(reader: &mut Reader<'_>) -> Option<i32> {
    Some(endian::i32_from_be(reader.take_array::<4>().ok()?))
}

/// Reads a binary-LLSD size prefix as a `usize`, rejecting negatives.
fn read_len(reader: &mut Reader<'_>) -> Option<usize> {
    usize::try_from(read_be_i32(reader)?).ok()
}

/// Reads a size-prefixed binary-LLSD string body (UTF-8, lossily decoded).
fn read_sized_string(reader: &mut Reader<'_>) -> Option<String> {
    let len = read_len(reader)?;
    Some(String::from_utf8_lossy(reader.take(len).ok()?).into_owned())
}

/// Reads a binary-LLSD array body (count, elements, then the trailing `]`).
fn read_binary_array(reader: &mut Reader<'_>) -> Option<Llsd> {
    let count = read_len(reader)?;
    let mut items = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        items.push(read_binary_value(reader)?);
    }
    reader.u8().ok();
    Some(Llsd::Array(items))
}

/// Reads a binary-LLSD map body (count, `k`-prefixed entries, then `}`).
fn read_binary_map(reader: &mut Reader<'_>) -> Option<Llsd> {
    let count = read_len(reader)?;
    let mut map = HashMap::with_capacity(count.min(4096));
    for _ in 0..count {
        // The key tag (`k`) precedes a size-prefixed name; consume it.
        reader.u8().ok()?;
        let key = read_sized_string(reader)?;
        let value = read_binary_value(reader)?;
        map.insert(key, value);
    }
    reader.u8().ok();
    Some(Llsd::Map(map))
}

/// Writes one header-less binary-LLSD value to `out`.
fn write_binary_value(value: &Llsd, out: &mut Vec<u8>) {
    match value {
        Llsd::Undef => out.push(b'!'),
        Llsd::Boolean(flag) => out.push(if *flag { b'1' } else { b'0' }),
        Llsd::Integer(number) => {
            out.push(b'i');
            out.extend_from_slice(&endian::i32_to_be(*number));
        }
        Llsd::Real(number) => {
            out.push(b'r');
            out.extend_from_slice(&endian::f64_to_be(*number));
        }
        Llsd::Uuid(id) => {
            out.push(b'u');
            out.extend_from_slice(id.as_bytes());
        }
        Llsd::Binary(bytes) => {
            out.push(b'b');
            out.extend_from_slice(&endian::i32_to_be(len_as_i32(bytes.len())));
            out.extend_from_slice(bytes);
        }
        Llsd::String(text) => write_binary_string(b's', text, out),
        Llsd::Uri(text) => write_binary_string(b'l', text, out),
        Llsd::Date(text) => write_binary_string(b'd', text, out),
        Llsd::Array(items) => {
            out.push(b'[');
            out.extend_from_slice(&endian::i32_to_be(len_as_i32(items.len())));
            for item in items {
                write_binary_value(item, out);
            }
            out.push(b']');
        }
        Llsd::Map(map) => {
            out.push(b'{');
            out.extend_from_slice(&endian::i32_to_be(len_as_i32(map.len())));
            for (key, item) in map {
                out.push(b'k');
                out.extend_from_slice(&endian::i32_to_be(len_as_i32(key.len())));
                out.extend_from_slice(key.as_bytes());
                write_binary_value(item, out);
            }
            out.push(b'}');
        }
    }
}

/// Writes a tagged, size-prefixed binary-LLSD string body.
fn write_binary_string(tag: u8, text: &str, out: &mut Vec<u8>) {
    out.push(tag);
    out.extend_from_slice(&endian::i32_to_be(len_as_i32(text.len())));
    out.extend_from_slice(text.as_bytes());
}

/// Narrows a byte length to the `i32` a binary-LLSD size prefix uses.
fn len_as_i32(len: usize) -> i32 {
    i32::try_from(len).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Server-side inverses (Tier F): the encoders/parsers the *grid* uses
// ---------------------------------------------------------------------------

/// Builds a GLTF material-override `GenericStreamingMessage` payload — the
/// inverse of `parse_gltf_material_override`.
///
/// Emits the notation-LLSD map
/// `{ 'id': <local id>, 'te': [faces…], 'od': [overrides…] }` the simulator
/// pushes to viewers (the message's method id is
/// [`GLTF_MATERIAL_OVERRIDE_METHOD`]). The per-face override documents are
/// written verbatim from [`GltfMaterialOverride::overrides`] (they are raw
/// notation-LLSD bytes the caller supplies — this layer does not build GLTF
/// material documents), positionally correlated with the face list.
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

/// Builds a `RenderMaterials` capability response — the inverse of
/// [`parse_render_materials_response`].
///
/// Emits the `{ "Zipped": <binary> }` LLSD-XML map whose binary is the
/// zlib-compressed binary-LLSD array of `{ "ID": <binary>, "Material": <map> }`
/// entries the OpenSim `MaterialsModule` returns, re-applying the fixed-point
/// scaling the parser undoes.
#[must_use]
pub fn build_render_materials_response(entries: &[RenderMaterialEntry]) -> String {
    let array = Llsd::Array(entries.iter().map(render_material_entry_to_llsd).collect());
    let mut binary = Vec::new();
    write_binary_value(&array, &mut binary);
    let zipped = miniz_oxide::deflate::compress_to_vec_zlib(&binary, 6);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&zipped);
    format!("<llsd><map><key>Zipped</key><binary>{encoded}</binary></map></llsd>")
}

/// Encodes one `{ "ID", "Material" }` entry of a `RenderMaterials` response.
fn render_material_entry_to_llsd(entry: &RenderMaterialEntry) -> Llsd {
    let mut map = HashMap::new();
    map.insert(
        "ID".to_owned(),
        Llsd::Binary(entry.material_id.as_bytes().to_vec()),
    );
    map.insert(
        "Material".to_owned(),
        legacy_material_to_llsd(&entry.material),
    );
    Llsd::Map(map)
}

/// Encodes a [`LegacyMaterial`] as its `RenderMaterials` LLSD map, re-applying
/// the fixed-point scaling on the texture transforms (the inverse of
/// `legacy_material_from_llsd`).
fn legacy_material_to_llsd(material: &LegacyMaterial) -> Llsd {
    let mut map = HashMap::new();
    map.insert("NormMap".to_owned(), Llsd::Uuid(material.normal_map));
    map.insert(
        "NormOffsetX".to_owned(),
        fixed_llsd(material.normal_offset.0),
    );
    map.insert(
        "NormOffsetY".to_owned(),
        fixed_llsd(material.normal_offset.1),
    );
    map.insert(
        "NormRepeatX".to_owned(),
        fixed_llsd(material.normal_repeat.0),
    );
    map.insert(
        "NormRepeatY".to_owned(),
        fixed_llsd(material.normal_repeat.1),
    );
    map.insert(
        "NormRotation".to_owned(),
        fixed_llsd(material.normal_rotation),
    );
    map.insert("SpecMap".to_owned(), Llsd::Uuid(material.specular_map));
    map.insert(
        "SpecOffsetX".to_owned(),
        fixed_llsd(material.specular_offset.0),
    );
    map.insert(
        "SpecOffsetY".to_owned(),
        fixed_llsd(material.specular_offset.1),
    );
    map.insert(
        "SpecRepeatX".to_owned(),
        fixed_llsd(material.specular_repeat.0),
    );
    map.insert(
        "SpecRepeatY".to_owned(),
        fixed_llsd(material.specular_repeat.1),
    );
    map.insert(
        "SpecRotation".to_owned(),
        fixed_llsd(material.specular_rotation),
    );
    map.insert(
        "SpecColor".to_owned(),
        color_to_llsd(material.specular_color),
    );
    map.insert(
        "SpecExp".to_owned(),
        Llsd::Integer(i32::from(material.specular_exponent)),
    );
    map.insert(
        "EnvIntensity".to_owned(),
        Llsd::Integer(i32::from(material.environment_intensity)),
    );
    map.insert(
        "DiffuseAlphaMode".to_owned(),
        Llsd::Integer(i32::from(material.diffuse_alpha_mode)),
    );
    map.insert(
        "AlphaMaskCutoff".to_owned(),
        Llsd::Integer(i32::from(material.alpha_mask_cutoff)),
    );
    Llsd::Map(map)
}

/// Encodes a texture transform as the fixed-point integer the wire carries:
/// `round(value * 10000)` (the inverse of `scaled`).
fn fixed_llsd(value: f32) -> Llsd {
    Llsd::Integer(fixed_from_f32(value))
}

/// Re-applies the material fixed-point scale, clamping to the `i32` range.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the scaled material transform is a small value that fits an i32"
)]
fn fixed_from_f32(value: f32) -> i32 {
    (value * MATERIAL_FIXED_SCALE).round() as i32
}

/// Encodes a four-element RGBA colour as the integer array the wire carries.
fn color_to_llsd(color: [u8; 4]) -> Llsd {
    Llsd::Array(
        color
            .iter()
            .map(|&byte| Llsd::Integer(i32::from(byte)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use pretty_assertions::assert_eq;

    use super::{
        GltfMaterialOverride, LegacyMaterial, Llsd, MaterialOverrideUpdate, RenderMaterialEntry,
        build_gltf_material_override, build_modify_material_params_request,
        build_render_materials_request, build_render_materials_response,
        parse_gltf_material_override, parse_modify_material_params_request,
        parse_render_materials_response, read_binary_value, write_binary_value,
    };
    use crate::field::Reader;
    use uuid::Uuid;

    /// A binary-LLSD value round-trips through the writer and reader.
    #[test]
    fn binary_llsd_round_trip() -> Result<(), String> {
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
        let mut map = std::collections::HashMap::new();
        map.insert("ID".to_owned(), Llsd::Binary(id.as_bytes().to_vec()));
        map.insert("n".to_owned(), Llsd::Integer(-7));
        map.insert("r".to_owned(), Llsd::Real(1.5));
        let value = Llsd::Array(vec![Llsd::Map(map)]);

        let mut bytes = Vec::new();
        write_binary_value(&value, &mut bytes);
        let mut reader = Reader::new(&bytes);
        let decoded = read_binary_value(&mut reader).ok_or("decode failed")?;
        assert_eq!(decoded, value);
        Ok(())
    }

    /// A `RenderMaterials` request round-trips through the response parser
    /// (build → would-be server echo of the same zipped form → parse).
    #[test]
    fn render_materials_zip_round_trip() -> Result<(), String> {
        // Build the request body for one material id and confirm it is the
        // zipped `{Zipped}` envelope; then feed an equivalent response (an
        // array of {ID, Material}) through the parser.
        let id = Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb);
        let request = build_render_materials_request(&[id]);
        assert!(request.contains("<key>Zipped</key>"));

        // Construct a response: zip a binary-LLSD array of one entry.
        let mut material = std::collections::HashMap::new();
        material.insert("NormOffsetX".to_owned(), Llsd::Integer(5000));
        material.insert("SpecExp".to_owned(), Llsd::Integer(51));
        material.insert(
            "SpecColor".to_owned(),
            Llsd::Array(vec![
                Llsd::Integer(10),
                Llsd::Integer(20),
                Llsd::Integer(30),
                Llsd::Integer(255),
            ]),
        );
        let mut entry = std::collections::HashMap::new();
        entry.insert("ID".to_owned(), Llsd::Binary(id.as_bytes().to_vec()));
        entry.insert("Material".to_owned(), Llsd::Map(material));
        let array = Llsd::Array(vec![Llsd::Map(entry)]);
        let mut binary = Vec::new();
        write_binary_value(&array, &mut binary);
        let zipped = miniz_oxide::deflate::compress_to_vec_zlib(&binary, 6);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&zipped);
        let response =
            format!("<llsd><map><key>Zipped</key><binary>{encoded}</binary></map></llsd>");

        let entries = parse_render_materials_response(&response);
        assert_eq!(entries.len(), 1);
        let decoded = entries.first().ok_or("no entry")?;
        assert_eq!(decoded.material_id, id);
        assert!((decoded.material.normal_offset.0 - 0.5).abs() < f32::EPSILON);
        assert_eq!(decoded.material.specular_exponent, 51);
        assert_eq!(decoded.material.specular_color, [10, 20, 30, 255]);
        Ok(())
    }

    /// The GLTF override envelope decodes its id and faces, leaving the per-face
    /// override documents as raw notation bytes.
    #[test]
    fn gltf_override_envelope() -> Result<(), String> {
        let payload = b"{'id':i42,'te':[i0,i3],'od':[{'bc':[r1,r1,r1,r1]},{'mf':r0.5}]}";
        let decoded = parse_gltf_material_override(payload).ok_or("decode failed")?;
        assert_eq!(
            decoded,
            GltfMaterialOverride {
                local_id: 42,
                faces: vec![0, 3],
                overrides: vec![b"{'bc':[r1,r1,r1,r1]}".to_vec(), b"{'mf':r0.5}".to_vec(),],
            }
        );
        Ok(())
    }

    /// `ModifyMaterialParams` serializes object id, side and the opaque JSON.
    #[test]
    fn modify_material_params_body() {
        let object_id = Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);
        let asset_id = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
        let body = build_modify_material_params_request(&[MaterialOverrideUpdate {
            object_id,
            side: -1,
            gltf_json: Some("{\"a\":1}".to_owned()),
            asset_id: Some(asset_id),
        }]);
        assert!(body.contains(&format!("<uuid>{object_id}</uuid>")));
        assert!(body.contains("<key>side</key><integer>-1</integer>"));
        assert!(body.contains("<key>gltf_json</key><string>{&quot;a&quot;:1}</string>"));
        assert!(body.contains(&format!("<key>asset_id</key><uuid>{asset_id}</uuid>")));
    }

    /// The GLTF override builder produces a payload the parser reads back equal
    /// (the server-side inverse of `parse_gltf_material_override`).
    #[test]
    fn gltf_override_round_trip() -> Result<(), String> {
        let original = GltfMaterialOverride {
            local_id: 42,
            faces: vec![0, 3],
            overrides: vec![b"{'bc':[r1,r1,r1,r1]}".to_vec(), b"{'mf':r0.5}".to_vec()],
        };
        let payload = build_gltf_material_override(&original);
        let decoded = parse_gltf_material_override(&payload).ok_or("decode failed")?;
        assert_eq!(decoded, original);
        Ok(())
    }

    /// A `ModifyMaterialParams` body built by the client parses back to the same
    /// updates on the server side.
    #[test]
    fn modify_material_params_round_trip() -> Result<(), String> {
        let updates = vec![
            MaterialOverrideUpdate {
                object_id: Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10),
                side: -1,
                gltf_json: Some("{\"a\":1}".to_owned()),
                asset_id: Some(Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888)),
            },
            MaterialOverrideUpdate {
                object_id: Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb),
                side: 2,
                gltf_json: None,
                asset_id: None,
            },
        ];
        let body = build_modify_material_params_request(&updates);
        let parsed =
            parse_modify_material_params_request(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, updates);
        Ok(())
    }

    /// A `RenderMaterials` response built from entries parses back to the same
    /// entries (the server-side inverse of `parse_render_materials_response`).
    #[test]
    fn render_materials_response_round_trip() -> Result<(), String> {
        let entry = RenderMaterialEntry {
            material_id: Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb),
            material: LegacyMaterial {
                normal_map: Uuid::from_u128(0x1234),
                normal_offset: (0.5, -0.25),
                normal_repeat: (2.0, 4.0),
                normal_rotation: 1.5,
                specular_map: Uuid::from_u128(0x5678),
                specular_offset: (0.1, 0.2),
                specular_repeat: (1.0, 1.0),
                specular_rotation: 0.0,
                specular_color: [10, 20, 30, 255],
                specular_exponent: 51,
                environment_intensity: 7,
                diffuse_alpha_mode: 1,
                alpha_mask_cutoff: 128,
            },
        };
        let response = build_render_materials_response(std::slice::from_ref(&entry));
        let entries = parse_render_materials_response(&response);
        assert_eq!(entries.len(), 1);
        let decoded = entries.first().ok_or("no entry")?;
        assert_eq!(decoded, &entry);
        Ok(())
    }
}
