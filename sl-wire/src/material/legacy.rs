//! Legacy `RenderMaterials` capability: zipped binary-LLSD material codec.

use super::{LegacyMaterial, RenderMaterialEntry};
use crate::endian;
use crate::field::Reader;
use crate::llsd::{Llsd, parse_llsd_xml};
use base64::Engine as _;
use sl_types::key::TextureKey;
use std::collections::HashMap;
use uuid::Uuid;

/// The fixed-point scale the `RenderMaterials` capability applies to a legacy
/// material's normal/specular map offsets, repeats and rotations: the wire
/// carries `round(value * 10000)` as an integer.
const MATERIAL_FIXED_SCALE: f32 = 10000.0;

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
        normal_map: TextureKey::from(
            map.get("NormMap")
                .and_then(Llsd::as_uuid)
                .unwrap_or_default(),
        ),
        normal_offset: (scaled(map, "NormOffsetX"), scaled(map, "NormOffsetY")),
        normal_repeat: (scaled(map, "NormRepeatX"), scaled(map, "NormRepeatY")),
        normal_rotation: scaled(map, "NormRotation"),
        specular_map: TextureKey::from(
            map.get("SpecMap")
                .and_then(Llsd::as_uuid)
                .unwrap_or_default(),
        ),
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
pub(crate) fn read_binary_value(reader: &mut Reader<'_>) -> Option<Llsd> {
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
pub(crate) fn write_binary_value(value: &Llsd, out: &mut Vec<u8>) {
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
    map.insert("NormMap".to_owned(), Llsd::Uuid(material.normal_map.uuid()));
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
    map.insert(
        "SpecMap".to_owned(),
        Llsd::Uuid(material.specular_map.uuid()),
    );
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
