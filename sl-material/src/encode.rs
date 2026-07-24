//! Encode a [`MaterialOverride`] as the GLTF-JSON override document the
//! `ModifyMaterialParams` capability carries (`gltf_json`), mirroring the
//! reference viewer's `LLGLTFMaterial::asJSON` / `writeToModel`.
//!
//! The simulator stores the sent override and re-broadcasts it to viewers (as the
//! notation-LLSD [`parse_material_override`](crate::parse_material_override)
//! decodes); the JSON produced here is what the reference sends when a build-tool
//! user edits a PBR material's per-channel transform (or assigns / clears a map).
//! An override field is applied by the simulator only when it differs from the
//! GLTF default, so writing the full set of a material's overridden fields (and
//! omitting the untouched ones) reproduces the reference's delta semantics.

use serde_json::{Map, Value as JsonValue, json};
use uuid::Uuid;

use crate::overrides::{MaterialOverride, TextureOverride, TextureTransformOverride};
use crate::types::{GltfMaterial, GltfTexture};
use crate::{GltfAlphaMode, GltfTextureTransform};

/// The base-colour texture slot index.
const SLOT_BASE_COLOR: usize = 0;
/// The normal texture slot index.
const SLOT_NORMAL: usize = 1;
/// The metallic-roughness texture slot index.
const SLOT_METALLIC_ROUGHNESS: usize = 2;
/// The emissive texture slot index.
const SLOT_EMISSIVE: usize = 3;

/// The sentinel texture id a cleared-slot override carries (`0xffff…ffff`, the
/// reference's `GLTF_OVERRIDE_NULL_UUID`).
const NULL_TEXTURE_UUID: Uuid = Uuid::from_u128(u128::MAX);

/// The default UV offset a texture transform omits.
const DEFAULT_OFFSET: [f32; 2] = [0.0, 0.0];
/// The default UV scale a texture transform omits.
const DEFAULT_SCALE: [f32; 2] = [1.0, 1.0];

/// Encode `over` as a GLTF-2.0 material-override JSON document (the
/// `ModifyMaterialParams` `gltf_json` field). An empty override encodes to a bare
/// material — the reference's "clear all overrides" is instead an empty
/// `gltf_json` string, sent separately.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "the public API reads best as encode_override_gltf_json; the `encode` module groups \
              the crate's one serialiser"
)]
pub fn encode_override_gltf_json(over: &MaterialOverride) -> String {
    let mut images: Vec<JsonValue> = Vec::new();
    let mut textures: Vec<JsonValue> = Vec::new();
    let mut pbr = Map::new();
    let mut material = Map::new();

    if let Some(base_color) = over.base_color {
        let _prev = pbr.insert("baseColorFactor".to_owned(), json!(base_color));
    }
    if let Some(metallic) = over.metallic_factor {
        let _prev = pbr.insert("metallicFactor".to_owned(), json!(metallic));
    }
    if let Some(roughness) = over.roughness_factor {
        let _prev = pbr.insert("roughnessFactor".to_owned(), json!(roughness));
    }
    if let Some(info) = slot_texture_info(over, SLOT_BASE_COLOR, &mut images, &mut textures) {
        let _prev = pbr.insert("baseColorTexture".to_owned(), info);
    }
    if let Some(info) = slot_texture_info(over, SLOT_METALLIC_ROUGHNESS, &mut images, &mut textures)
    {
        let _prev = pbr.insert("metallicRoughnessTexture".to_owned(), info);
    }
    if let Some(info) = slot_texture_info(over, SLOT_NORMAL, &mut images, &mut textures) {
        let _prev = material.insert("normalTexture".to_owned(), info);
    }
    if let Some(info) = slot_texture_info(over, SLOT_EMISSIVE, &mut images, &mut textures) {
        let _prev = material.insert("emissiveTexture".to_owned(), info);
    }

    if !pbr.is_empty() {
        let _prev = material.insert("pbrMetallicRoughness".to_owned(), JsonValue::Object(pbr));
    }
    if let Some(emissive) = over.emissive_factor {
        let _prev = material.insert("emissiveFactor".to_owned(), json!(emissive));
    }
    if let Some(mode) = over.alpha_mode
        && let Some(name) = alpha_mode_name(mode)
    {
        let _prev = material.insert("alphaMode".to_owned(), json!(name));
    }
    if let Some(cutoff) = over.alpha_cutoff {
        let _prev = material.insert("alphaCutoff".to_owned(), json!(cutoff));
    }
    if let Some(double_sided) = over.double_sided {
        let _prev = material.insert("doubleSided".to_owned(), json!(double_sided));
    }

    let mut document = Map::new();
    let _prev = document.insert("asset".to_owned(), json!({ "version": "2.0" }));
    // GLTF forbids empty `images` / `textures` arrays, so only emit them when a
    // slot referenced one.
    if !images.is_empty() {
        let _prev = document.insert("images".to_owned(), JsonValue::Array(images));
    }
    if !textures.is_empty() {
        let _prev = document.insert("textures".to_owned(), JsonValue::Array(textures));
    }
    let _prev = document.insert("materials".to_owned(), json!([JsonValue::Object(material)]));

    serde_json::to_string(&JsonValue::Object(document)).unwrap_or_default()
}

/// Encode a full [`GltfMaterial`] as an `AT_MATERIAL` asset's bytes: the LLSD-XML
/// envelope `{ version, type: "GLTF 2.0", data: <full glTF-JSON> }` the reference
/// stores (and both grids' `LLSDSerialize` auto-detects). Used to save a
/// build-tool-edited material to inventory.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "the public API reads best as encode_material_asset; the `encode` module groups the \
              crate's serialisers"
)]
pub fn encode_material_asset(material: &GltfMaterial) -> Vec<u8> {
    let json = encode_override_gltf_json(&full_override_of(material));
    let escaped = xml_escape(&json);
    format!(
        "<llsd><map><key>version</key><string>1.1</string><key>type</key><string>GLTF 2.0</string>\
         <key>data</key><string>{escaped}</string></map></llsd>"
    )
    .into_bytes()
}

/// A [`MaterialOverride`] carrying every field of `material` (so
/// [`encode_override_gltf_json`] writes a complete material, not a delta).
fn full_override_of(material: &GltfMaterial) -> MaterialOverride {
    MaterialOverride {
        textures: [
            texture_set(material.base_color_texture),
            texture_set(material.normal_texture),
            texture_set(material.metallic_roughness_texture),
            texture_set(material.emissive_texture),
        ],
        transforms: [
            full_transform(material.base_color_texture),
            full_transform(material.normal_texture),
            full_transform(material.metallic_roughness_texture),
            full_transform(material.emissive_texture),
        ],
        base_color: Some(material.base_color),
        emissive_factor: Some(material.emissive_factor),
        metallic_factor: Some(material.metallic_factor),
        roughness_factor: Some(material.roughness_factor),
        alpha_mode: Some(material.alpha_mode),
        alpha_cutoff: Some(material.alpha_cutoff),
        double_sided: Some(material.double_sided),
    }
}

/// A texture-set override for a present slot (none clears nothing / leaves empty).
fn texture_set(texture: Option<GltfTexture>) -> Option<TextureOverride> {
    texture.map(|texture| TextureOverride::Set(texture.id))
}

/// A fully-specified transform override for a present textured slot.
fn full_transform(texture: Option<GltfTexture>) -> TextureTransformOverride {
    texture.map_or_else(TextureTransformOverride::default, |texture| {
        let GltfTextureTransform {
            offset,
            scale,
            rotation,
        } = texture.transform;
        TextureTransformOverride {
            offset: Some(offset),
            scale: Some(scale),
            rotation: Some(rotation),
        }
    })
}

/// Escape the five XML metacharacters in a string embedded in LLSD-XML.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Build the GLTF `textureInfo` for slot `slot`, appending its backing image +
/// texture to `images` / `textures`. Returns `None` when the slot has neither a
/// texture-id override nor a transform (nothing to write). The `KHR_texture_transform`
/// extension is written only when the slot carries a transform override, filling
/// each absent component with its GLTF default (the reference writes the whole
/// transform once any component is set).
fn slot_texture_info(
    over: &MaterialOverride,
    slot: usize,
    images: &mut Vec<JsonValue>,
    textures: &mut Vec<JsonValue>,
) -> Option<JsonValue> {
    let texture_override = over.textures.get(slot).copied().flatten();
    let transform_override = over
        .transforms
        .get(slot)
        .copied()
        .unwrap_or_else(TextureTransformOverride::default);
    let has_transform = !transform_is_empty(&transform_override);
    if texture_override.is_none() && !has_transform {
        return None;
    }

    let uuid = match texture_override {
        Some(TextureOverride::Set(key)) => key.uuid(),
        Some(TextureOverride::Clear) => NULL_TEXTURE_UUID,
        None => Uuid::nil(),
    };
    let image_index = images.len();
    images.push(json!({ "uri": uuid.to_string() }));
    let texture_index = textures.len();
    textures.push(json!({ "source": image_index }));

    let mut info = Map::new();
    let _prev = info.insert("index".to_owned(), json!(texture_index));
    if has_transform {
        let offset = transform_override.offset.unwrap_or(DEFAULT_OFFSET);
        let scale = transform_override.scale.unwrap_or(DEFAULT_SCALE);
        let rotation = transform_override.rotation.unwrap_or(0.0);
        let transform = json!({
            "offset": offset,
            "scale": scale,
            "rotation": rotation,
        });
        let _prev = info.insert(
            "extensions".to_owned(),
            json!({ "KHR_texture_transform": transform }),
        );
    }
    Some(JsonValue::Object(info))
}

/// Whether a transform override carries no component (so it need not be written).
const fn transform_is_empty(over: &TextureTransformOverride) -> bool {
    over.offset.is_none() && over.scale.is_none() && over.rotation.is_none()
}

/// The GLTF `alphaMode` name for a non-default (non-opaque) mode; `None` for
/// opaque (the GLTF default, which the reference omits).
const fn alpha_mode_name(mode: GltfAlphaMode) -> Option<&'static str> {
    match mode {
        GltfAlphaMode::Opaque => None,
        GltfAlphaMode::Mask => Some("MASK"),
        GltfAlphaMode::Blend => Some("BLEND"),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::encode_override_gltf_json;
    use crate::overrides::{MaterialOverride, TextureTransformOverride};

    /// A base-colour transform override encodes a `KHR_texture_transform` on the
    /// base-colour texture info, and parses back as valid JSON.
    #[test]
    fn base_color_transform_encodes() -> Result<(), String> {
        let mut over = MaterialOverride::default();
        if let Some(slot) = over.transforms.get_mut(0) {
            *slot = TextureTransformOverride {
                offset: Some([0.25, 0.5]),
                scale: Some([2.0, 3.0]),
                rotation: Some(0.5),
            };
        }
        let json = encode_override_gltf_json(&over);
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        let base =
            "/materials/0/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform";
        let transform = value.pointer(base).ok_or("no transform")?;
        assert_eq!(transform.pointer("/scale/0"), Some(&json!(2.0)));
        assert_eq!(transform.pointer("/scale/1"), Some(&json!(3.0)));
        assert_eq!(transform.pointer("/offset/0"), Some(&json!(0.25)));
        assert_eq!(value.pointer("/asset/version"), Some(&json!("2.0")));
        Ok(())
    }

    /// A scalar-only override omits the `images` / `textures` arrays.
    #[test]
    fn scalar_override_omits_textures() -> Result<(), String> {
        let over = MaterialOverride {
            metallic_factor: Some(0.75),
            ..MaterialOverride::default()
        };
        let json = encode_override_gltf_json(&over);
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        assert!(value.get("images").is_none());
        assert_eq!(
            value.pointer("/materials/0/pbrMetallicRoughness/metallicFactor"),
            Some(&json!(0.75))
        );
        Ok(())
    }
}
