//! Decoding an `AT_MATERIAL` asset into a [`GltfMaterial`].
//!
//! The asset is an LLSD envelope (`{ version, type, data }`) whose `data` string
//! is a glTF 2.0 JSON document carrying one material; see the crate `README.md`.
//! The envelope is unwrapped here and the glTF document parsed into the
//! renderer-agnostic material with [`serde_json`].

use serde::Deserialize;
use sl_llsd::{Llsd, parse_llsd_binary, parse_llsd_xml};
use sl_types::key::TextureKey;
use uuid::Uuid;

use crate::error::MaterialError;
use crate::types::{GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform};

/// The glTF material-asset envelope `type` this decoder accepts
/// (`LLGLTFMaterial::ASSET_TYPE`).
const ASSET_TYPE: &str = "GLTF 2.0";

/// The glTF material-asset envelope `version`s this decoder accepts
/// (`LLGLTFMaterial::ACCEPTED_ASSET_VERSIONS`).
const ACCEPTED_VERSIONS: [&str; 2] = ["1.0", "1.1"];

/// Decode an `AT_MATERIAL` asset (the bytes the `ViewerAsset` capability returns
/// for a `material_id`) into a [`GltfMaterial`].
///
/// The bytes are the LLSD envelope Second Life serializes for a material asset
/// (binary LLSD, so they lead with the `<? LLSD/Binary ?>` header the fetch
/// returns verbatim); this unwraps it, checks the `version` / `type`, and parses
/// the inner glTF 2.0 document.
///
/// # Errors
///
/// Returns a [`MaterialError`] if the LLSD envelope or the inner glTF document
/// does not parse, the envelope is missing its expected fields, or its declared
/// version / type is not one this decoder accepts.
pub fn parse_material_asset(bytes: &[u8]) -> Result<GltfMaterial, MaterialError> {
    let envelope = parse_envelope(bytes)?;
    let version = envelope
        .get("version")
        .and_then(Llsd::as_str)
        .ok_or(MaterialError::MalformedEnvelope)?;
    if !ACCEPTED_VERSIONS.contains(&version) {
        return Err(MaterialError::UnsupportedVersion(version.to_owned()));
    }
    let asset_type = envelope
        .get("type")
        .and_then(Llsd::as_str)
        .ok_or(MaterialError::MalformedEnvelope)?;
    if asset_type != ASSET_TYPE {
        return Err(MaterialError::UnsupportedType(asset_type.to_owned()));
    }
    let data = envelope
        .get("data")
        .and_then(Llsd::as_str)
        .ok_or(MaterialError::MalformedEnvelope)?;
    parse_gltf_material_document(data)
}

/// Parse a glTF 2.0 material JSON document (the envelope's `data` string) into a
/// [`GltfMaterial`], reading the first material and resolving each texture slot's
/// asset id (via `textures[].source` → `images[].uri`) and
/// `KHR_texture_transform`.
///
/// # Errors
///
/// Returns [`MaterialError::Json`] if the document does not parse, or
/// [`MaterialError::NoMaterial`] if it carries no material.
pub fn parse_gltf_material_document(json: &str) -> Result<GltfMaterial, MaterialError> {
    let document: GltfDocument = serde_json::from_str(json)?;
    let material = document
        .materials
        .first()
        .ok_or(MaterialError::NoMaterial)?;
    Ok(build_material(material, &document))
}

/// Unwrap the LLSD envelope of a material asset, detecting the serialization
/// (LL's binary / XML headers, a bare XML declaration, or headerless binary) and
/// dispatching to the matching `sl-llsd` parser.
fn parse_envelope(bytes: &[u8]) -> Result<Llsd, MaterialError> {
    let trimmed = trim_leading_ascii_whitespace(bytes);
    // A standard XML declaration: hand the whole document (declaration included)
    // to the XML parser.
    if trimmed.starts_with(b"<?xml") {
        return parse_xml(trimmed);
    }
    // An LL serialization header line (`<? LLSD/Binary ?>` / `<? LLSD/XML ?>`):
    // strip it and dispatch on which format it names.
    if trimmed.starts_with(b"<?") {
        let (header, body) = split_first_line(trimmed);
        let header = String::from_utf8_lossy(header).to_ascii_lowercase();
        if header.contains("binary") {
            return Ok(parse_llsd_binary(body)?);
        }
        if header.contains("xml") {
            return parse_xml(body);
        }
        // An unknown / notation header: best-effort binary decode of the body.
        return Ok(parse_llsd_binary(body)?);
    }
    // A bare XML LLSD document (`<llsd>…`), or headerless binary LLSD.
    if trimmed.starts_with(b"<") {
        return parse_xml(trimmed);
    }
    Ok(parse_llsd_binary(trimmed)?)
}

/// Parse `bytes` (which must be UTF-8) as XML LLSD.
fn parse_xml(bytes: &[u8]) -> Result<Llsd, MaterialError> {
    let text = str::from_utf8(bytes).map_err(|_utf8| MaterialError::MalformedEnvelope)?;
    parse_llsd_xml(text).map_err(|_xml| MaterialError::MalformedEnvelope)
}

/// Split `bytes` at its first newline, returning `(first_line, remainder)` with
/// the newline dropped; the remainder is empty when there is no newline.
fn split_first_line(bytes: &[u8]) -> (&[u8], &[u8]) {
    match bytes.iter().position(|&byte| byte == b'\n') {
        Some(index) => (
            bytes.get(..index).unwrap_or(bytes),
            bytes.get(index.saturating_add(1)..).unwrap_or(&[]),
        ),
        None => (bytes, &[]),
    }
}

/// Drop any leading ASCII whitespace from `bytes`.
const fn trim_leading_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut rest = bytes;
    while let Some((first, tail)) = rest.split_first() {
        if first.is_ascii_whitespace() {
            rest = tail;
        } else {
            break;
        }
    }
    rest
}

/// Assemble a [`GltfMaterial`] from a parsed glTF material and the document it
/// belongs to (needed to resolve each texture slot's asset id).
fn build_material(material: &GltfMaterialJson, document: &GltfDocument) -> GltfMaterial {
    let pbr = &material.pbr_metallic_roughness;
    GltfMaterial {
        base_color: pbr.base_color_factor.unwrap_or([1.0, 1.0, 1.0, 1.0]),
        base_color_texture: pbr
            .base_color_texture
            .as_ref()
            .and_then(|info| resolve_texture(info, document)),
        metallic_factor: pbr.metallic_factor.unwrap_or(1.0).clamp(0.0, 1.0),
        roughness_factor: pbr.roughness_factor.unwrap_or(1.0).clamp(0.0, 1.0),
        metallic_roughness_texture: pbr
            .metallic_roughness_texture
            .as_ref()
            .and_then(|info| resolve_texture(info, document)),
        normal_texture: material
            .normal_texture
            .as_ref()
            .and_then(|info| resolve_texture(info, document)),
        emissive_factor: material.emissive_factor.unwrap_or([0.0, 0.0, 0.0]),
        emissive_texture: material
            .emissive_texture
            .as_ref()
            .and_then(|info| resolve_texture(info, document)),
        alpha_mode: parse_alpha_mode(material.alpha_mode.as_deref()),
        alpha_cutoff: material.alpha_cutoff.unwrap_or(0.5).clamp(0.0, 1.0),
        double_sided: material.double_sided,
    }
}

/// Resolve a glTF texture reference to a [`GltfTexture`]: follow
/// `textures[index].source` → `images[source].uri`, parse the URI as the
/// texture asset UUID, and read its `KHR_texture_transform`. Returns `None` when
/// any link is missing or the URI is not a (non-nil) UUID.
fn resolve_texture(info: &TextureInfoJson, document: &GltfDocument) -> Option<GltfTexture> {
    let texture = document.textures.get(info.index)?;
    let image = document.images.get(texture.source?)?;
    let uri = image.uri.as_deref()?;
    let uuid = Uuid::parse_str(uri).ok()?;
    if uuid.is_nil() {
        return None;
    }
    Some(GltfTexture {
        id: TextureKey::from(uuid),
        transform: texture_transform(info),
    })
}

/// Read a texture reference's `KHR_texture_transform` extension into a
/// [`GltfTextureTransform`], falling back to the identity transform for any
/// component the extension omits (or when the extension is absent).
fn texture_transform(info: &TextureInfoJson) -> GltfTextureTransform {
    let default = GltfTextureTransform::default();
    let Some(transform) = info
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.khr_texture_transform.as_ref())
    else {
        return default;
    };
    GltfTextureTransform {
        offset: transform.offset.unwrap_or(default.offset),
        scale: transform.scale.unwrap_or(default.scale),
        rotation: transform.rotation.unwrap_or(default.rotation),
    }
}

/// Map a glTF `alphaMode` string to a [`GltfAlphaMode`], defaulting to
/// [`Opaque`](GltfAlphaMode::Opaque) for the absent / unrecognised case.
fn parse_alpha_mode(mode: Option<&str>) -> GltfAlphaMode {
    match mode {
        Some("MASK") => GltfAlphaMode::Mask,
        Some("BLEND") => GltfAlphaMode::Blend,
        _ => GltfAlphaMode::Opaque,
    }
}

/// The glTF 2.0 document fields this decoder reads: the materials plus the
/// `textures` / `images` indirection its texture slots resolve through.
#[derive(Debug, Default, Deserialize)]
struct GltfDocument {
    /// The document's materials; only the first is decoded.
    #[serde(default)]
    materials: Vec<GltfMaterialJson>,
    /// The document's textures, each pointing at an image by `source` index.
    #[serde(default)]
    textures: Vec<GltfTextureJson>,
    /// The document's images, whose `uri` carries the texture asset UUID.
    #[serde(default)]
    images: Vec<GltfImageJson>,
}

/// A glTF 2.0 material's fields (the subset Second Life uses).
#[derive(Debug, Default, Deserialize)]
struct GltfMaterialJson {
    /// The metallic-roughness workflow parameters.
    #[serde(default, rename = "pbrMetallicRoughness")]
    pbr_metallic_roughness: PbrMetallicRoughnessJson,
    /// The tangent-space normal-map texture reference.
    #[serde(default, rename = "normalTexture")]
    normal_texture: Option<TextureInfoJson>,
    /// The emissive texture reference.
    #[serde(default, rename = "emissiveTexture")]
    emissive_texture: Option<TextureInfoJson>,
    /// The emissive factor (linear RGB).
    #[serde(default, rename = "emissiveFactor")]
    emissive_factor: Option<[f32; 3]>,
    /// The alpha mode string (`OPAQUE` / `MASK` / `BLEND`).
    #[serde(default, rename = "alphaMode")]
    alpha_mode: Option<String>,
    /// The alpha cutoff for `MASK` mode.
    #[serde(default, rename = "alphaCutoff")]
    alpha_cutoff: Option<f32>,
    /// Whether both faces are rendered.
    #[serde(default, rename = "doubleSided")]
    double_sided: bool,
}

/// A glTF 2.0 `pbrMetallicRoughness` block.
#[derive(Debug, Default, Deserialize)]
struct PbrMetallicRoughnessJson {
    /// The base-colour factor (linear RGBA).
    #[serde(default, rename = "baseColorFactor")]
    base_color_factor: Option<[f32; 4]>,
    /// The base-colour texture reference.
    #[serde(default, rename = "baseColorTexture")]
    base_color_texture: Option<TextureInfoJson>,
    /// The metallic factor.
    #[serde(default, rename = "metallicFactor")]
    metallic_factor: Option<f32>,
    /// The roughness factor.
    #[serde(default, rename = "roughnessFactor")]
    roughness_factor: Option<f32>,
    /// The packed metallic-roughness (ORM) texture reference.
    #[serde(default, rename = "metallicRoughnessTexture")]
    metallic_roughness_texture: Option<TextureInfoJson>,
}

/// A glTF `textureInfo`: an index into `textures[]` plus optional extensions
/// (only `KHR_texture_transform` is read).
#[derive(Debug, Deserialize)]
struct TextureInfoJson {
    /// The index into the document's `textures` array.
    index: usize,
    /// The texture reference's extensions.
    #[serde(default)]
    extensions: Option<TextureExtensionsJson>,
}

/// The `extensions` object of a glTF `textureInfo`.
#[derive(Debug, Deserialize)]
struct TextureExtensionsJson {
    /// The `KHR_texture_transform` extension, if present.
    #[serde(rename = "KHR_texture_transform")]
    khr_texture_transform: Option<KhrTextureTransformJson>,
}

/// A glTF `KHR_texture_transform` extension.
#[derive(Debug, Deserialize)]
struct KhrTextureTransformJson {
    /// The UV offset `(s, t)`.
    #[serde(default)]
    offset: Option<[f32; 2]>,
    /// The UV scale `(s, t)`.
    #[serde(default)]
    scale: Option<[f32; 2]>,
    /// The UV rotation in radians.
    #[serde(default)]
    rotation: Option<f32>,
}

/// A glTF `texture`: a pointer to an image by `source` index.
#[derive(Debug, Deserialize)]
struct GltfTextureJson {
    /// The index into the document's `images` array.
    #[serde(default)]
    source: Option<usize>,
}

/// A glTF `image`: its `uri` holds the texture asset UUID for a Second Life
/// material asset.
#[derive(Debug, Deserialize)]
struct GltfImageJson {
    /// The image URI (the texture asset UUID string).
    #[serde(default)]
    uri: Option<String>,
}
