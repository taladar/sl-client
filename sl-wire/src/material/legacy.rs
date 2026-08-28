//! Legacy `RenderMaterials` capability: zipped binary-LLSD material codec.

use super::{FaceMaterialPut, LegacyMaterial, RenderMaterialEntry};
use crate::WireError;
use crate::llsd::{Llsd, parse_llsd_binary, parse_llsd_xml};
use base64::Engine as _;
use sl_types::key::TextureKey;
use std::collections::HashMap;
use uuid::Uuid;

/// The fixed-point scale the `RenderMaterials` capability applies to a legacy
/// material's normal/specular map offsets, repeats and rotations: the wire
/// carries `round(value * 10000)` as an integer.
const MATERIAL_FIXED_SCALE: f32 = 10000.0;

/// The largest a `{ "Zipped": … }` body may inflate to.
///
/// Both `RenderMaterials` paths hand a base64 blob straight to zlib, and zlib
/// compresses a run of zeros about a thousand to one — so an uncapped inflate
/// turns a kilobyte of capability response into a gigabyte of resident memory,
/// sized entirely by whatever answered the request. A legacy material is a
/// couple of hundred bytes of binary LLSD, so even a region-wide "fetch all"
/// reply is orders of magnitude under this; the margin is deliberate so no real
/// grid is refused.
const MAX_INFLATED_MATERIALS_BYTES: usize = 64 << 20;

// ---------------------------------------------------------------------------
// RenderMaterials (legacy materials capability — zipped binary LLSD)
// ---------------------------------------------------------------------------

/// Wraps a binary-LLSD value in the `{ "Zipped": <binary> }` LLSD-XML envelope
/// every `RenderMaterials` body — request, PUT and response alike — is carried
/// in: zlib-compress the header-less binary LLSD, base64 it, and emit the
/// one-key map (`MaterialsModule::ZCompressOSD(osd, useHeader: false)`).
fn zipped_body(value: &Llsd) -> String {
    let zipped = miniz_oxide::deflate::compress_to_vec_zlib(&value.to_llsd_binary(), 6);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&zipped);
    format!("<llsd><map><key>Zipped</key><binary>{encoded}</binary></map></llsd>")
}

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
    zipped_body(&array)
}

/// Builds the LLSD-XML body for a `RenderMaterials` capability **PUT** that sets
/// (or clears) legacy materials on object faces: a `{ "Zipped": <binary> }` map
/// whose binary is the zlib-compressed binary-LLSD map
/// `{ "FullMaterialsPerFace": [ { "Face": <te>, "ID": <local id>, "Material":
/// <map> } ] }` (the reference `LLMaterialMgr::processPutQueue` body). A cleared
/// face omits the `Material` field, matching `LLMaterial::null`. The simulator
/// assigns the material id and echoes it on the face's `TextureEntry`.
#[must_use]
pub fn build_render_materials_put_request(updates: &[FaceMaterialPut]) -> String {
    let faces = Llsd::Array(updates.iter().map(face_material_put_to_llsd).collect());
    let mut map = HashMap::new();
    map.insert("FullMaterialsPerFace".to_owned(), faces);
    zipped_body(&Llsd::Map(map))
}

/// Encodes one `{ "Face", "ID", "Material" }` PUT entry; a cleared face omits
/// `Material` (the reference sends a null material for a removal).
fn face_material_put_to_llsd(update: &FaceMaterialPut) -> Llsd {
    let mut map = HashMap::new();
    map.insert("Face".to_owned(), Llsd::Integer(i32::from(update.face)));
    map.insert(
        "ID".to_owned(),
        Llsd::Integer(local_id_as_i32(update.local_id)),
    );
    if let Some(material) = &update.material {
        map.insert("Material".to_owned(), legacy_material_to_llsd(material));
    }
    Llsd::Map(map)
}

/// Narrows a region-local object id to the `LLSD::Integer` the cap carries,
/// wrapping through the two's-complement bit pattern (the reference stores the
/// same `U32` in a signed `LLSD::Integer`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "the local id is a u32 stored verbatim in a signed LLSD integer, matching the \
              reference's static_cast<LLSD::Integer>(getLocalID())"
)]
const fn local_id_as_i32(local_id: u32) -> i32 {
    local_id as i32
}

/// Unzips the `{ "Zipped": <binary> }` envelope every `RenderMaterials` body
/// shares into its inner binary-LLSD value — the inverse of [`zipped_body`].
/// `None` when the body is not the expected map, the binary does not inflate,
/// or it is not well-formed binary-LLSD. An empty body (the "fetch all region
/// materials" GET) has no `Zipped` and yields `None` too.
fn parse_zipped_body(xml: &str) -> Option<Llsd> {
    let root = parse_llsd_xml(xml).ok()?;
    let zipped = root.get("Zipped").and_then(Llsd::as_binary)?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
        zipped,
        MAX_INFLATED_MATERIALS_BYTES,
    )
    .ok()?;
    parse_llsd_binary(&raw).ok()
}

/// Parses a `RenderMaterials` capability POST **request** — the inverse of
/// [`build_render_materials_request`]. Unzips the `{ "Zipped": … }` binary-LLSD
/// array of 16-byte material ids into the queried [`Uuid`]s.
///
/// Best-effort: a malformed body (or an empty "fetch all" body with no
/// `Zipped`) yields an empty vector, which the handler treats as "return every
/// known material".
#[must_use]
pub fn parse_render_materials_request(xml: &str) -> Vec<Uuid> {
    let Some(Llsd::Array(items)) = parse_zipped_body(xml) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            item.as_binary()
                .and_then(|bytes| Uuid::from_slice(bytes).ok())
        })
        .collect()
}

/// Parses a `RenderMaterials` capability **PUT** request — the inverse of
/// [`build_render_materials_put_request`]. Unzips the
/// `{ "FullMaterialsPerFace": [ { "Face", "ID", "Material"? } ] }` binary-LLSD
/// map into the per-face assignments; a face without a `Material` is a clear
/// (`material: None`).
///
/// Best-effort: a malformed body yields an empty vector.
#[must_use]
pub fn parse_render_materials_put_request(xml: &str) -> Vec<FaceMaterialPut> {
    let Some(root) = parse_zipped_body(xml) else {
        return Vec::new();
    };
    let Some(faces) = root.get("FullMaterialsPerFace").and_then(Llsd::as_array) else {
        return Vec::new();
    };
    faces
        .iter()
        .filter_map(face_material_put_from_llsd)
        .collect()
}

/// Decodes one `{ "Face", "ID", "Material"? }` PUT entry — the inverse of
/// [`face_material_put_to_llsd`]. A missing `Face`/`ID` drops the entry; an
/// absent `Material` is a face clear.
fn face_material_put_from_llsd(item: &Llsd) -> Option<FaceMaterialPut> {
    let face = item
        .get("Face")
        .and_then(Llsd::as_i32)
        .and_then(|value| u8::try_from(value).ok())?;
    let local_id = item
        .get("ID")
        .and_then(Llsd::as_i32)
        .map(local_id_from_i32)?;
    let material = match item.get("Material") {
        Some(value @ Llsd::Map(_)) => legacy_material_from_llsd(value).ok(),
        _ => None,
    };
    Some(FaceMaterialPut {
        local_id,
        face,
        material,
    })
}

/// Widens a signed `LLSD::Integer` object id back to the region-local `u32` —
/// the inverse of [`local_id_as_i32`], recovering the same bit pattern the
/// reference stores round-trip.
const fn local_id_from_i32(id: i32) -> u32 {
    u32::from_ne_bytes(id.to_ne_bytes())
}

/// Parses a `RenderMaterials` capability POST response (a
/// `{ "Zipped": <binary> }` LLSD-XML map whose binary unzips to a binary-LLSD
/// array of `{ "ID": <binary>, "Material": <map> }`) into the decoded entries.
///
/// Best-effort: a malformed or empty response yields an empty vector.
#[must_use]
pub fn parse_render_materials_response(xml: &str) -> Vec<RenderMaterialEntry> {
    let Some(Llsd::Array(items)) = parse_zipped_body(xml) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| render_material_entry(item).ok().flatten())
        .collect()
}

/// Decodes one `{ "ID", "Material" }` entry of a `RenderMaterials` response.
fn render_material_entry(item: &Llsd) -> Result<Option<RenderMaterialEntry>, WireError> {
    let Some(id_bytes) = item.field_binary("ID", "ID")? else {
        return Ok(None);
    };
    let Ok(material_id) = Uuid::from_slice(id_bytes) else {
        return Ok(None);
    };
    // Validate that "Material", when present, is a map; absent stays a default
    // (empty) material. The borrow below reuses the same value.
    if item.field_map("Material", "Material")?.is_none() {
        return Ok(None);
    }
    let Some(material_value) = item.get("Material") else {
        return Ok(None);
    };
    let material = legacy_material_from_llsd(material_value)?;
    Ok(Some(RenderMaterialEntry {
        material_id,
        material,
    }))
}

/// Decodes a [`LegacyMaterial`] from its `RenderMaterials` LLSD map, undoing the
/// fixed-point scaling on the texture transforms.
fn legacy_material_from_llsd(map: &Llsd) -> Result<LegacyMaterial, WireError> {
    Ok(LegacyMaterial {
        normal_map: TextureKey::from(map.field_uuid("NormMap", "NormMap")?.unwrap_or_default()),
        normal_offset: (scaled(map, "NormOffsetX")?, scaled(map, "NormOffsetY")?),
        normal_repeat: (scaled(map, "NormRepeatX")?, scaled(map, "NormRepeatY")?),
        normal_rotation: scaled(map, "NormRotation")?,
        specular_map: TextureKey::from(map.field_uuid("SpecMap", "SpecMap")?.unwrap_or_default()),
        specular_offset: (scaled(map, "SpecOffsetX")?, scaled(map, "SpecOffsetY")?),
        specular_repeat: (scaled(map, "SpecRepeatX")?, scaled(map, "SpecRepeatY")?),
        specular_rotation: scaled(map, "SpecRotation")?,
        specular_color: color_from_llsd(map)?,
        specular_exponent: byte_field(map, "SpecExp")?,
        environment_intensity: byte_field(map, "EnvIntensity")?,
        diffuse_alpha_mode: byte_field(map, "DiffuseAlphaMode")?,
        alpha_mask_cutoff: byte_field(map, "AlphaMaskCutoff")?,
    })
}

/// Reads an integer map field and undoes the material fixed-point scale.
fn scaled(map: &Llsd, key: &'static str) -> Result<f32, WireError> {
    let raw = map.field_i32(key, key)?.unwrap_or(0);
    Ok(narrow_to_f32(f64::from(raw)) / MATERIAL_FIXED_SCALE)
}

/// Reads a small unsigned-byte map field, clamping out-of-range values to `0`.
fn byte_field(map: &Llsd, key: &'static str) -> Result<u8, WireError> {
    Ok(map
        .field_i32(key, key)?
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0))
}

/// Decodes a four-element RGBA colour array (each element an integer 0–255).
fn color_from_llsd(map: &Llsd) -> Result<[u8; 4], WireError> {
    let mut color = [255_u8; 4];
    if let Some(array) = map.field_array("SpecColor", "SpecColor")? {
        for (slot, element) in color.iter_mut().zip(array) {
            if let Some(byte) = element.as_i32().and_then(|raw| u8::try_from(raw).ok()) {
                *slot = byte;
            }
        }
    }
    Ok(color)
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
    zipped_body(&array)
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
