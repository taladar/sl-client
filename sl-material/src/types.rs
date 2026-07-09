//! The renderer-agnostic decoded material and its texture references.

use sl_types::key::TextureKey;

/// The alpha-blending mode of a GLTF material (glTF 2.0 `material.alphaMode`),
/// mirroring `LLGLTFMaterial::AlphaMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GltfAlphaMode {
    /// Fully opaque; the base-colour alpha is ignored (the glTF default).
    #[default]
    Opaque,
    /// A hard cutout: a texel is opaque where its alpha is at or above
    /// [`GltfMaterial::alpha_cutoff`], and fully transparent below it.
    Mask,
    /// Ordinary alpha blending by the base-colour alpha.
    Blend,
}

/// A GLTF texture reference: the texture asset it names plus the
/// `KHR_texture_transform` placement applied to its UVs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GltfTexture {
    /// The texture asset id (from the glTF `images[].uri`, which carries the
    /// UUID rather than a path/data-URI for a Second Life material asset).
    pub id: TextureKey,
    /// The per-texture UV transform (`KHR_texture_transform`), identity when the
    /// extension is absent.
    pub transform: GltfTextureTransform,
}

/// A glTF `KHR_texture_transform`: an offset, scale, and rotation applied to a
/// texture's UV coordinates, mirroring `LLGLTFMaterial::TextureTransform`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GltfTextureTransform {
    /// The UV offset `(s, t)` (glTF `offset`, default `[0, 0]`).
    pub offset: [f32; 2],
    /// The UV scale / repeats `(s, t)` (glTF `scale`, default `[1, 1]`).
    pub scale: [f32; 2],
    /// The UV rotation in radians (glTF `rotation`, default `0`).
    pub rotation: f32,
}

impl Default for GltfTextureTransform {
    fn default() -> Self {
        Self {
            offset: [0.0, 0.0],
            scale: [1.0, 1.0],
            rotation: 0.0,
        }
    }
}

/// A decoded Second Life / OpenSim GLTF 2.0 (PBR) render material, mirroring the
/// fields of the reference viewer's `LLGLTFMaterial`. Colour factors are in
/// **linear** space (as glTF stores them), unlike a legacy per-face tint.
///
/// Produced by [`parse_material_asset`](crate::parse_material_asset) from an
/// `AT_MATERIAL` asset. The viewer maps it onto its renderer material (a Bevy
/// `StandardMaterial`), fetching each referenced texture through the ordinary
/// texture pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GltfMaterial {
    /// The base-colour factor, linear RGBA (glTF `baseColorFactor`, default
    /// `[1, 1, 1, 1]`); multiplied by the base-colour texture.
    pub base_color: [f32; 4],
    /// The base-colour (albedo) texture, sRGB-encoded, or `None`.
    pub base_color_texture: Option<GltfTexture>,
    /// The metallic factor `0..=1` (glTF `metallicFactor`, default `1`).
    pub metallic_factor: f32,
    /// The roughness factor `0..=1` (glTF `roughnessFactor`, default `1`).
    pub roughness_factor: f32,
    /// The packed metallic-roughness texture (glTF `metallicRoughnessTexture`),
    /// linear: green = roughness, blue = metallic. Second Life also reads its
    /// red channel as ambient occlusion (the ORM convention), so a separate
    /// occlusion texture is not carried.
    pub metallic_roughness_texture: Option<GltfTexture>,
    /// The tangent-space normal map (glTF `normalTexture`), linear, or `None`.
    pub normal_texture: Option<GltfTexture>,
    /// The emissive factor, linear RGB (glTF `emissiveFactor`, default
    /// `[0, 0, 0]`).
    pub emissive_factor: [f32; 3],
    /// The emissive texture (glTF `emissiveTexture`), sRGB-encoded, or `None`.
    pub emissive_texture: Option<GltfTexture>,
    /// The alpha-blending mode (glTF `alphaMode`, default
    /// [`Opaque`](GltfAlphaMode::Opaque)).
    pub alpha_mode: GltfAlphaMode,
    /// The alpha cutoff for [`Mask`](GltfAlphaMode::Mask) mode (glTF
    /// `alphaCutoff`, default `0.5`).
    pub alpha_cutoff: f32,
    /// Whether both faces are rendered (glTF `doubleSided`, default `false`).
    pub double_sided: bool,
}

impl Default for GltfMaterial {
    fn default() -> Self {
        Self {
            base_color: [1.0, 1.0, 1.0, 1.0],
            base_color_texture: None,
            metallic_factor: 1.0,
            roughness_factor: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_factor: [0.0, 0.0, 0.0],
            emissive_texture: None,
            alpha_mode: GltfAlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        }
    }
}
