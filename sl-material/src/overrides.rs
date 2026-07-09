//! Per-face GLTF (PBR) material **overrides** (P27.2).
//!
//! An override is a sparse delta layered on top of a face's base
//! [`GltfMaterial`]: the simulator pushes it in a GLTF material-override
//! `GenericStreamingMessage`, one notation-LLSD document (`od[i]`) per affected
//! face. [`parse_material_override`] decodes one such document into a
//! [`MaterialOverride`]; [`MaterialOverride::apply_to`] then folds it onto the
//! base material — set / clear each texture slot, replace any factor the delta
//! carries, and update the per-slot texture transforms.
//!
//! The reference is Firestorm's `LLGLTFMaterial::applyOverrideLLSD`
//! (decode) and `applyOverride` (layering), in
//! `indra/llprimitive/llgltfmaterial.cpp`. The override document uses the
//! shaved-down keys `tex` / `bc` / `ec` / `mf` / `rf` / `am` / `ac` / `ds` / `ti`
//! rather than the full glTF field names of a material asset.

use sl_llsd::{Llsd, parse_llsd_notation};
use sl_types::key::TextureKey;
use uuid::Uuid;

use crate::error::MaterialError;
use crate::types::{GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform};

/// The GLTF override-null texture sentinel (all-`f`, `LLGLTFMaterial::
/// GLTF_OVERRIDE_NULL_UUID`): a texture-slot override carrying this id **clears**
/// the slot (as opposed to the nil id, which leaves the base texture unchanged).
const GLTF_OVERRIDE_NULL_UUID: Uuid = Uuid::from_u128(u128::MAX);

/// The number of GLTF texture-info slots (base colour, normal, metallic-roughness
/// / occlusion, emissive) — `LLGLTFMaterial::GLTF_TEXTURE_INFO_COUNT`.
const TEXTURE_SLOT_COUNT: usize = 4;

/// The base-colour texture slot index (`GLTF_TEXTURE_INFO_BASE_COLOR`).
const SLOT_BASE_COLOR: usize = 0;
/// The normal-map texture slot index (`GLTF_TEXTURE_INFO_NORMAL`).
const SLOT_NORMAL: usize = 1;
/// The metallic-roughness (also occlusion) texture slot index
/// (`GLTF_TEXTURE_INFO_METALLIC_ROUGHNESS`).
const SLOT_METALLIC_ROUGHNESS: usize = 2;
/// The emissive texture slot index (`GLTF_TEXTURE_INFO_EMISSIVE`).
const SLOT_EMISSIVE: usize = 3;

/// What a texture-slot override does to a base material's slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureOverride {
    /// Replace the slot's texture with this asset id.
    Set(TextureKey),
    /// Clear the slot's texture (the `GLTF_OVERRIDE_NULL_UUID` sentinel).
    Clear,
}

/// A per-slot texture-transform override: only the components the delta actually
/// carries are `Some`, mirroring the reference's per-component
/// `mTextureTransform` overwrite.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextureTransformOverride {
    /// The overriding UV offset `(s, t)`, or `None` to keep the base's.
    pub offset: Option<[f32; 2]>,
    /// The overriding UV scale `(s, t)`, or `None` to keep the base's.
    pub scale: Option<[f32; 2]>,
    /// The overriding UV rotation in radians, or `None` to keep the base's.
    pub rotation: Option<f32>,
}

impl TextureTransformOverride {
    /// Whether this override carries no component (so it changes nothing).
    const fn is_empty(&self) -> bool {
        self.offset.is_none() && self.scale.is_none() && self.rotation.is_none()
    }

    /// Fold this override's components onto `transform`.
    const fn apply_to(&self, transform: &mut GltfTextureTransform) {
        if let Some(offset) = self.offset {
            transform.offset = offset;
        }
        if let Some(scale) = self.scale {
            transform.scale = scale;
        }
        if let Some(rotation) = self.rotation {
            transform.rotation = rotation;
        }
    }
}

/// A decoded per-face GLTF material override: the sparse set of fields a
/// simulator-pushed override document changes, layered onto a base
/// [`GltfMaterial`] by [`apply_to`](Self::apply_to). Every field is `None` /
/// [`TextureOverride`]-absent for a component the delta leaves untouched, so an
/// empty override is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaterialOverride {
    /// Per-slot texture-id overrides (base colour, normal, metallic-roughness,
    /// emissive); `None` leaves the base slot's texture in place.
    pub textures: [Option<TextureOverride>; TEXTURE_SLOT_COUNT],
    /// Per-slot texture-transform overrides, positionally matching
    /// [`textures`](Self::textures).
    pub transforms: [TextureTransformOverride; TEXTURE_SLOT_COUNT],
    /// The overriding base-colour factor (linear RGBA), or `None`.
    pub base_color: Option<[f32; 4]>,
    /// The overriding emissive factor (linear RGB), or `None`.
    pub emissive_factor: Option<[f32; 3]>,
    /// The overriding metallic factor, or `None`.
    pub metallic_factor: Option<f32>,
    /// The overriding roughness factor, or `None`.
    pub roughness_factor: Option<f32>,
    /// The overriding alpha mode, or `None`.
    pub alpha_mode: Option<GltfAlphaMode>,
    /// The overriding alpha cutoff, or `None`.
    pub alpha_cutoff: Option<f32>,
    /// The overriding double-sided flag, or `None`.
    pub double_sided: Option<bool>,
}

impl MaterialOverride {
    /// Whether this override changes nothing (every field absent) — the shape a
    /// "revert to base" override document (`od[i]` = `!` / `{}`) decodes to.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textures.iter().all(Option::is_none)
            && self
                .transforms
                .iter()
                .all(TextureTransformOverride::is_empty)
            && self.base_color.is_none()
            && self.emissive_factor.is_none()
            && self.metallic_factor.is_none()
            && self.roughness_factor.is_none()
            && self.alpha_mode.is_none()
            && self.alpha_cutoff.is_none()
            && self.double_sided.is_none()
    }

    /// Fold this override onto `base`, mutating it in place — the P27.2 layering
    /// step, mirroring `LLGLTFMaterial::applyOverride`.
    ///
    /// Each texture slot is set / cleared / left per its [`TextureOverride`], its
    /// transform folded on, and each scalar factor the delta carries replaces the
    /// base's. A slot the override clears (or that has no texture after the
    /// override) leaves the base slot empty, dropping any transform there — the
    /// same single-`uv_transform` approximation the base pipeline documents.
    pub fn apply_to(&self, base: &mut GltfMaterial) {
        base.base_color_texture = self.apply_slot(base.base_color_texture, SLOT_BASE_COLOR);
        base.normal_texture = self.apply_slot(base.normal_texture, SLOT_NORMAL);
        base.metallic_roughness_texture =
            self.apply_slot(base.metallic_roughness_texture, SLOT_METALLIC_ROUGHNESS);
        base.emissive_texture = self.apply_slot(base.emissive_texture, SLOT_EMISSIVE);

        if let Some(base_color) = self.base_color {
            base.base_color = base_color;
        }
        if let Some(emissive) = self.emissive_factor {
            base.emissive_factor = emissive;
        }
        if let Some(metallic) = self.metallic_factor {
            base.metallic_factor = metallic;
        }
        if let Some(roughness) = self.roughness_factor {
            base.roughness_factor = roughness;
        }
        if let Some(alpha_mode) = self.alpha_mode {
            base.alpha_mode = alpha_mode;
        }
        if let Some(cutoff) = self.alpha_cutoff {
            base.alpha_cutoff = cutoff;
        }
        if let Some(double_sided) = self.double_sided {
            base.double_sided = double_sided;
        }
    }

    /// Resolve one texture slot: apply the slot's id override (set / clear /
    /// leave) and fold its transform override onto the resulting texture.
    fn apply_slot(&self, current: Option<GltfTexture>, slot: usize) -> Option<GltfTexture> {
        let mut id = current.map(|texture| texture.id);
        match self.textures.get(slot).copied().flatten() {
            Some(TextureOverride::Set(key)) => id = Some(key),
            Some(TextureOverride::Clear) => id = None,
            None => {}
        }
        let mut transform =
            current.map_or_else(GltfTextureTransform::default, |texture| texture.transform);
        if let Some(over) = self.transforms.get(slot) {
            over.apply_to(&mut transform);
        }
        id.map(|id| GltfTexture { id, transform })
    }
}

/// Decode one per-face GLTF material-override document (`od[i]`, the notation
/// LLSD the simulator pushes) into a [`MaterialOverride`].
///
/// The document is a map with the shaved keys `tex` (per-slot texture ids), `bc`
/// / `ec` (base-colour / emissive factors), `mf` / `rf` (metallic / roughness),
/// `am` / `ac` (alpha mode / cutoff), `ds` (double-sided), and `ti` (per-slot
/// texture transforms). Any absent key leaves that field untouched. A `!` (undef)
/// or empty-map document decodes to an empty override (revert to base).
///
/// # Errors
///
/// Returns a [`MaterialError`] if the bytes are not parseable notation LLSD.
pub fn parse_material_override(bytes: &[u8]) -> Result<MaterialOverride, MaterialError> {
    let value = parse_llsd_notation(bytes)?;
    // An undef / non-map document is a well-formed "no override" (revert-to-base).
    if value.as_map().is_none() {
        return Ok(MaterialOverride::default());
    }
    Ok(decode_override(&value))
}

/// Assemble a [`MaterialOverride`] from a parsed override map.
fn decode_override(value: &Llsd) -> MaterialOverride {
    let mut over = MaterialOverride::default();

    if let Some(tex) = value.get("tex").and_then(Llsd::as_array) {
        for (slot, entry) in tex.iter().enumerate().take(TEXTURE_SLOT_COUNT) {
            if let (Some(texture_override), Some(slot_ref)) =
                (texture_override(entry), over.textures.get_mut(slot))
            {
                *slot_ref = Some(texture_override);
            }
        }
    }

    if let Some(transforms) = value.get("ti").and_then(Llsd::as_array) {
        for (slot, entry) in transforms.iter().enumerate().take(TEXTURE_SLOT_COUNT) {
            if let Some(slot_ref) = over.transforms.get_mut(slot) {
                *slot_ref = transform_override(entry);
            }
        }
    }

    over.base_color = float_array(value.get("bc"));
    over.emissive_factor = float_array(value.get("ec"));
    over.metallic_factor = value.get("mf").and_then(Llsd::as_f32);
    over.roughness_factor = value.get("rf").and_then(Llsd::as_f32);
    over.alpha_mode = value.get("am").and_then(Llsd::as_i32).map(alpha_mode);
    over.alpha_cutoff = value.get("ac").and_then(Llsd::as_f32);
    over.double_sided = value.get("ds").and_then(Llsd::as_bool);
    over
}

/// Interpret one `tex[i]` entry: the null-sentinel clears the slot, the nil (or a
/// non-uuid) entry leaves it untouched, any other uuid sets it.
fn texture_override(entry: &Llsd) -> Option<TextureOverride> {
    let uuid = entry.as_uuid()?;
    if uuid == GLTF_OVERRIDE_NULL_UUID {
        Some(TextureOverride::Clear)
    } else if uuid.is_nil() {
        None
    } else {
        Some(TextureOverride::Set(TextureKey::from(uuid)))
    }
}

/// Decode one `ti[i]` texture-transform entry (`{ 'o':[s,t], 's':[s,t], 'r':n }`),
/// reading only the components it carries.
fn transform_override(entry: &Llsd) -> TextureTransformOverride {
    TextureTransformOverride {
        offset: float_array(entry.get("o")),
        scale: float_array(entry.get("s")),
        rotation: entry.get("r").and_then(Llsd::as_f32),
    }
}

/// Read an LLSD real array (`bc` / `ec` / a transform `o` / `s`) into a fixed
/// `[f32; N]`, returning `None` unless every element is present and numeric.
fn float_array<const N: usize>(value: Option<&Llsd>) -> Option<[f32; N]> {
    let array = value?.as_array()?;
    let mut out = [0.0_f32; N];
    for (slot, element) in out.iter_mut().zip(array.iter()) {
        *slot = element.as_f32()?;
    }
    // Reject a short array: an incomplete factor is malformed, not a partial
    // override (the reference always serializes the full colour / vector).
    if array.len() < N {
        return None;
    }
    Some(out)
}

/// Map an override's `am` integer to a [`GltfAlphaMode`], defaulting unknown
/// values to opaque.
///
/// The reference enum order (`LLGLTFMaterial::AlphaMode`) is
/// `ALPHA_MODE_OPAQUE = 0`, `ALPHA_MODE_BLEND = 1`, `ALPHA_MODE_MASK = 2`.
const fn alpha_mode(raw: i32) -> GltfAlphaMode {
    match raw {
        1 => GltfAlphaMode::Blend,
        2 => GltfAlphaMode::Mask,
        _ => GltfAlphaMode::Opaque,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{MaterialOverride, TextureOverride, parse_material_override};
    use crate::types::{GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform};

    /// A boxed error so tests can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// A texture id the override sets the base-colour slot to.
    const NEW_BASE: Uuid = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
    /// The base material's original base-colour texture.
    const OLD_BASE: Uuid = Uuid::from_u128(0xaaaa_bbbb_cccc_dddd_eeee_ffff_0000_1111);
    /// The base material's original normal texture (the override clears it).
    const OLD_NORMAL: Uuid = Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222);

    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} != {expected} (within 1e-5)"
        );
    }

    /// A base material with a base-colour and normal texture, so the override's
    /// set / clear / leave semantics are all observable.
    fn base_material() -> GltfMaterial {
        GltfMaterial {
            base_color_texture: Some(GltfTexture {
                id: OLD_BASE.into(),
                transform: GltfTextureTransform::default(),
            }),
            normal_texture: Some(GltfTexture {
                id: OLD_NORMAL.into(),
                transform: GltfTextureTransform::default(),
            }),
            ..GltfMaterial::default()
        }
    }

    /// A full override document decodes every field and layers onto the base:
    /// the base-colour texture is replaced, the normal texture cleared (via the
    /// null sentinel), the scalar factors and alpha mode replaced, and the base
    /// colour texture's transform updated.
    #[test]
    fn decodes_and_applies_full_override() -> Result<(), TestError> {
        let null = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        let document = format!(
            "{{'tex':[u{NEW_BASE},u{null}],\
             'bc':[r0.1,r0.2,r0.3,r0.4],\
             'ec':[r0.5,r0.6,r0.7],\
             'mf':r0.7,'rf':r0.2,'am':i2,'ac':r0.25,'ds':1,\
             'ti':[{{'o':[r0.5,r0.6],'s':[r2.0,r3.0],'r':r1.0}}]}}"
        );
        let over = parse_material_override(document.as_bytes())?;

        assert_eq!(
            over.textures[0],
            Some(TextureOverride::Set(NEW_BASE.into()))
        );
        assert_eq!(over.textures[1], Some(TextureOverride::Clear));
        assert_eq!(over.alpha_mode, Some(GltfAlphaMode::Mask));
        assert_eq!(over.double_sided, Some(true));

        let mut material = base_material();
        over.apply_to(&mut material);

        let base_tex = material.base_color_texture.ok_or("base texture cleared")?;
        assert_eq!(base_tex.id, NEW_BASE.into());
        approx_slice(&base_tex.transform.offset, &[0.5, 0.6]);
        approx_slice(&base_tex.transform.scale, &[2.0, 3.0]);
        approx(base_tex.transform.rotation, 1.0);

        // The null-sentinel clears the normal slot.
        assert!(material.normal_texture.is_none());

        approx_slice(&material.base_color, &[0.1, 0.2, 0.3, 0.4]);
        approx_slice(&material.emissive_factor, &[0.5, 0.6, 0.7]);
        approx(material.metallic_factor, 0.7);
        approx(material.roughness_factor, 0.2);
        approx(material.alpha_cutoff, 0.25);
        assert_eq!(material.alpha_mode, GltfAlphaMode::Mask);
        assert!(material.double_sided);
        Ok(())
    }

    /// An override that touches only the metallic factor leaves every other field
    /// of the base material untouched.
    #[test]
    fn sparse_override_leaves_other_fields() -> Result<(), TestError> {
        let over = parse_material_override(b"{'mf':r0.25}")?;
        assert!(!over.is_empty());

        let mut material = base_material();
        over.apply_to(&mut material);

        approx(material.metallic_factor, 0.25);
        // The base's textures and colours are untouched.
        assert_eq!(
            material.base_color_texture.map(|texture| texture.id),
            Some(OLD_BASE.into())
        );
        assert_eq!(
            material.normal_texture.map(|texture| texture.id),
            Some(OLD_NORMAL.into())
        );
        approx_slice(&material.base_color, &[1.0, 1.0, 1.0, 1.0]);
        Ok(())
    }

    /// An `!` (undef) or empty-map document decodes to an empty override that
    /// leaves the base material identical — the revert-to-base case.
    #[test]
    fn empty_override_is_noop() -> Result<(), TestError> {
        for document in [b"!".as_slice(), b"{}".as_slice()] {
            let over = parse_material_override(document)?;
            assert!(over.is_empty(), "{over:?} should be empty");
            let mut material = base_material();
            over.apply_to(&mut material);
            assert_eq!(material, base_material());
        }
        Ok(())
    }

    /// The default (`Default`) override changes nothing when applied.
    #[test]
    fn default_override_is_empty() {
        assert!(MaterialOverride::default().is_empty());
    }

    fn approx_slice(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len());
        for (got, want) in actual.iter().zip(expected) {
            approx(*got, *want);
        }
    }
}
