//! Material value types: GLTF override, legacy material, render-material entry.

use uuid::Uuid;

/// The `GenericStreamingMessage` method id for a GLTF material override
/// (`LLGenericStreamingMessage::METHOD_GLTF_MATERIAL_OVERRIDE`).
pub const GLTF_MATERIAL_OVERRIDE_METHOD: u16 = 0x4175;

/// A decoded GLTF (PBR) material override pushed in a `GenericStreamingMessage`.
///
/// The override targets a single object (`local_id`) and a set of its faces;
/// per the asset-fetch scope the per-face GLTF override documents are *not*
/// interpreted — each is surfaced as its raw notation-LLSD bytes, positionally
/// correlated with [`faces`](Self::faces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GltfMaterialOverride {
    /// The region-local id of the object whose material is overridden.
    pub local_id: crate::RegionLocalObjectId,
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
