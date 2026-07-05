//! Clothing-morph alpha masks (P14.5): the `<morph_masks>` table from
//! `avatar_lad.xml` and the per-vertex mask weights it drives from a region's
//! baked texture.
//!
//! A handful of body morphs are *clothing morphs* — the flared sleeve, pant-leg,
//! long-cuff and loose-body displacements (`Shirtsleeve_flair`, `Leg_Pantflair`,
//! `Leg_Longcuffs`, `Displace_Loose_Upper/Lowerbody`, …). Applied unconditionally
//! they would flare the *whole* limb, including the bare skin below a short
//! sleeve. Second Life instead masks each such morph per vertex by the baked
//! texture of the clothing layer it belongs to: where the baked layer is opaque
//! (fabric is present) the morph applies at full strength, and where it is
//! transparent (bare skin) the morph is masked off. The reference viewer builds
//! this in `LLPolyVertexMask::generateMask` / `LLPolyMorphTarget::applyMask`,
//! fed by `LLVOAvatar::onBakedTextureMasksLoaded` sampling the baked image's
//! alpha channel — so it can only run once the baked textures are fetched and
//! decoded (Phase 14).
//!
//! This module is the pure, I/O-free, Bevy-free half:
//!
//! - [`MorphMasks`] parses the `<morph_masks>` table (each `<mask>` associates a
//!   morph name with a `body_region` and clothing `layer`, plus an optional
//!   `invert`).
//! - [`MorphMasks::sample_part`] samples a region's decoded baked
//!   [`MaskTexture`] at each masked morph vertex's UV (through the base mesh's
//!   shared-vertex remap, exactly as the reference viewer does), returning a
//!   [`PartMorphMask`] of per-delta mask weights.
//!
//! The mask weights are then fed to
//! [`MorphWeights::apply_masked`](crate::MorphWeights::apply_masked), which
//! scales each masked morph delta by its per-vertex weight.

use std::collections::HashMap;

use crate::basemesh::BaseMesh;
use crate::params::ParamError;

/// One `<mask>` entry from the `avatar_lad.xml` `<morph_masks>` table: a clothing
/// morph masked by a body region's baked texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MorphMask {
    /// The morph target this mask applies to (matched against the base-mesh morph
    /// names, e.g. `Shirtsleeve_flair`).
    pub morph_name: String,
    /// The body region whose baked texture masks the morph (`head`, `upper_body`,
    /// `lower_body`), matching the region a base part belongs to.
    pub body_region: String,
    /// The clothing layer the mask keys off (`upper_clothes`, `lower_pants`,
    /// `facialhair`), kept for reference; the mask samples the region's composited
    /// bake, so this is informational.
    pub layer: String,
    /// Whether the sampled alpha is inverted (`1 - alpha`) before use, mirroring
    /// the reference viewer's per-mask `invert` flag.
    pub invert: bool,
}

/// The parsed `<morph_masks>` table: the clothing morphs and the region bakes
/// that mask them (P14.5).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MorphMasks` reads clearly"
)]
#[derive(Clone, Debug, Default)]
pub struct MorphMasks {
    /// The masks in document order.
    masks: Vec<MorphMask>,
}

impl MorphMasks {
    /// Parse the `<morph_masks>` table from an `avatar_lad.xml` document.
    ///
    /// Collects every `<mask>` under a `<morph_masks>` element (the reference
    /// viewer's `LLAvatarXmlInfo` morph-mask list). A document with no
    /// `<morph_masks>` section yields an empty table rather than an error, so a
    /// stripped-down test asset still loads.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if the XML is malformed, the root is not
    /// `<linden_avatar>`, or a `<mask>` lacks a required attribute
    /// (`morph_name` / `body_region` / `layer`).
    pub fn from_xml(xml: &str) -> Result<Self, ParamError> {
        let doc = roxmltree::Document::parse(xml)?;
        let root = doc.root_element();
        if root.tag_name().name() != "linden_avatar" {
            return Err(ParamError::UnexpectedRoot {
                found: root.tag_name().name().to_owned(),
            });
        }

        let mut masks = Vec::new();
        for node in doc
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "mask")
        {
            masks.push(parse_mask(node)?);
        }
        Ok(Self { masks })
    }

    /// Every mask in the table, in document order.
    #[must_use]
    pub fn all(&self) -> &[MorphMask] {
        &self.masks
    }

    /// The number of masks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.masks.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    /// The masks whose `body_region` matches `region` (e.g. every clothing morph
    /// masked by the upper-body bake).
    pub fn for_region<'a>(&'a self, region: &'a str) -> impl Iterator<Item = &'a MorphMask> {
        self.masks
            .iter()
            .filter(move |mask| mask.body_region == region)
    }

    /// Whether any mask targets `region` (a cheap gate before sampling a bake).
    #[must_use]
    pub fn has_region(&self, region: &str) -> bool {
        self.masks.iter().any(|mask| mask.body_region == region)
    }

    /// Sample per-delta mask weights for every masked morph of `region` that
    /// `base` defines, from the region's decoded baked `texture`.
    ///
    /// For each mask whose `body_region` is `region` and whose morph the base
    /// mesh carries, each of that morph's per-vertex deltas is assigned a weight
    /// in `0.0..=1.0`: the baked texture's alpha sampled (nearest, clamped) at the
    /// delta vertex's UV — the *shared* (canonical) vertex's UV when the vertex is
    /// a duplicate, exactly as `LLPolyVertexMask::generateMask` does — inverted
    /// when the mask says so. The returned [`PartMorphMask`] is keyed by morph
    /// name, each weight vector aligned to that morph's `deltas`.
    #[must_use]
    pub fn sample_part(
        &self,
        base: &BaseMesh,
        region: &str,
        texture: &MaskTexture<'_>,
    ) -> PartMorphMask {
        // Duplicate-vertex → canonical-vertex remap, so a seam vertex samples the
        // mask at its shared UV (the reference loader's `getSharedVert`).
        let mut shared: HashMap<usize, usize> = HashMap::new();
        for remap in base.shared_verts() {
            let _prev = shared.insert(remap.source, remap.destination);
        }

        let mut by_morph = HashMap::new();
        for mask in self.for_region(region) {
            if let Some(morph) = base.morph(&mask.morph_name) {
                let weights: Vec<f32> = morph
                    .deltas
                    .iter()
                    .map(|delta| {
                        let uv_index = shared
                            .get(&delta.vertex_index)
                            .copied()
                            .unwrap_or(delta.vertex_index);
                        let uv = base
                            .tex_coords()
                            .get(uv_index)
                            .copied()
                            .unwrap_or([0.0, 0.0]);
                        let alpha = texture.alpha_at(uv);
                        if mask.invert { 1.0 - alpha } else { alpha }
                    })
                    .collect();
                let _prev = by_morph.insert(mask.morph_name.clone(), weights);
            }
        }
        PartMorphMask { by_morph }
    }
}

/// A borrowed decoded baked texture to sample the clothing-morph mask alpha from
/// (P14.5): the region bake's RGBA (or greyscale) pixels plus its dimensions.
///
/// The alpha channel is the composited clothing coverage the reference viewer
/// feeds into the morph mask (its auxiliary single-component image); a source
/// with fewer than the expected components samples its last component, matching
/// `LLPolyVertexMask::generateMask`'s `num_components - 1` index.
#[derive(Clone, Copy, Debug)]
pub struct MaskTexture<'a> {
    /// The raw interleaved pixel bytes, `components` per pixel, row-major.
    pub pixels: &'a [u8],
    /// The image width in pixels.
    pub width: usize,
    /// The image height in pixels.
    pub height: usize,
    /// The number of components per pixel (4 for RGBA); the mask samples the last.
    pub components: usize,
}

impl MaskTexture<'_> {
    /// Sample the mask alpha at normalised UV `[u, v]`, in `0.0..=1.0`.
    ///
    /// Nearest-neighbour with the reference viewer's clamp
    /// (`s = clamp(u * (width - 1), 0, width - 1)`), reading the pixel's last
    /// component (the alpha channel). A degenerate (zero-sized / componentless)
    /// texture, or an out-of-range index, samples `0.0` — matching Firestorm's
    /// null-mask-data fallback.
    #[must_use]
    pub fn alpha_at(&self, uv: [f32; 2]) -> f32 {
        if self.width == 0 || self.height == 0 || self.components == 0 {
            return 0.0;
        }
        let max_x = self.width.saturating_sub(1);
        let max_y = self.height.saturating_sub(1);
        let s = usize_from_f32_floor(uv[0].clamp(0.0, 1.0) * f32_from_usize(max_x)).min(max_x);
        let t = usize_from_f32_floor(uv[1].clamp(0.0, 1.0) * f32_from_usize(max_y)).min(max_y);
        // (t * width + s) * components + (components - 1).
        let pixel = t.saturating_mul(self.width).saturating_add(s);
        let index = pixel
            .saturating_mul(self.components)
            .saturating_add(self.components.saturating_sub(1));
        self.pixels
            .get(index)
            .map_or(0.0, |&byte| f32_from_u8(byte) / 255.0)
    }
}

/// Per-part clothing-morph mask weights (P14.5): for each masked morph the base
/// part defines, one weight in `0.0..=1.0` per morph delta (aligned to the
/// morph's `deltas`), sampled from the region's baked texture by
/// [`MorphMasks::sample_part`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartMorphMask {
    /// Morph-target name → per-delta mask weight (aligned to the morph's deltas).
    by_morph: HashMap<String, Vec<f32>>,
}

impl PartMorphMask {
    /// The per-delta mask weights for morph `name`, or `None` when the morph is
    /// not masked (it then applies at full strength).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[f32]> {
        self.by_morph.get(name).map(Vec::as_slice)
    }

    /// The number of masked morphs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_morph.len()
    }

    /// Whether no morph is masked (every morph applies at full strength).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_morph.is_empty()
    }

    /// Test-only builder so the morph blend can be exercised with a hand-made mask
    /// (the mask normally comes from [`MorphMasks::sample_part`]).
    #[cfg(test)]
    pub(crate) fn from_pairs_for_test(pairs: &[(&str, Vec<f32>)]) -> Self {
        let by_morph = pairs
            .iter()
            .map(|(name, weights)| ((*name).to_owned(), weights.clone()))
            .collect();
        Self { by_morph }
    }
}

/// Parse one `<mask>` element into a [`MorphMask`].
fn parse_mask(node: roxmltree::Node<'_, '_>) -> Result<MorphMask, ParamError> {
    let morph_name = req_attr(node, "morph_name")?.to_owned();
    let body_region = req_attr(node, "body_region")?.to_owned();
    let layer = req_attr(node, "layer")?.to_owned();
    let invert = node.attribute("invert") == Some("true");
    Ok(MorphMask {
        morph_name,
        body_region,
        layer,
        invert,
    })
}

/// Return a required `<mask>` attribute or a [`ParamError::MissingAttribute`].
fn req_attr<'a>(
    node: roxmltree::Node<'a, '_>,
    attribute: &'static str,
) -> Result<&'a str, ParamError> {
    node.attribute(attribute)
        .ok_or(ParamError::MissingAttribute {
            element: "mask",
            attribute,
        })
}

/// Widen a `usize` pixel dimension to `f32` for the sampling arithmetic (pixel
/// dimensions are far below `f32`'s exact-integer range).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a pixel dimension, well within f32's exact-integer range"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Widen a `u8` sample to `f32`.
fn f32_from_u8(value: u8) -> f32 {
    f32::from(value)
}

/// Floor a non-negative `f32` to `usize`; a negative or non-finite value (which
/// the clamped sampling coordinates cannot produce) maps to `0`. Mirrors the
/// same helper in `sl-sculpt`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a clamped, non-negative pixel coordinate; its floor fits usize"
)]
fn usize_from_f32_floor(value: f32) -> usize {
    if value.is_finite() && value >= 0.0 {
        value.floor() as usize
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{MaskTexture, MorphMasks};
    use crate::basemesh::BaseMesh;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The committed minimal base-mesh fixture (4 vertices, one `Fatten` morph
    /// with deltas on vertices 0 and 3, one shared-vertex remap 2 → 0).
    const MINI_BASEMESH: &[u8] = include_bytes!("../tests/fixtures/mini_basemesh.llm");
    /// The committed minimal visual-param fixture, which also carries a
    /// `<morph_masks>` table.
    const MINI_PARAMS: &str = include_str!("../tests/fixtures/mini_params.xml");

    /// Compare two floats within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn parses_the_morph_mask_table() -> Result<(), TestError> {
        let masks = MorphMasks::from_xml(MINI_PARAMS)?;
        assert_eq!(masks.len(), 2);
        assert!(!masks.is_empty());
        let fatten = masks
            .all()
            .iter()
            .find(|mask| mask.morph_name == "Fatten")
            .ok_or("Fatten mask")?;
        assert_eq!(fatten.body_region, "upper_body");
        assert_eq!(fatten.layer, "upper_clothes");
        assert!(!fatten.invert);
        // The second mask is inverted and targets a different region.
        let inverted = masks
            .all()
            .iter()
            .find(|mask| mask.invert)
            .ok_or("inverted mask")?;
        assert_eq!(inverted.body_region, "lower_body");
        Ok(())
    }

    #[test]
    fn for_region_and_has_region_filter() -> Result<(), TestError> {
        let masks = MorphMasks::from_xml(MINI_PARAMS)?;
        assert!(masks.has_region("upper_body"));
        assert!(!masks.has_region("head"));
        let upper: Vec<&str> = masks
            .for_region("upper_body")
            .map(|mask| mask.morph_name.as_str())
            .collect();
        assert_eq!(upper, ["Fatten"]);
        Ok(())
    }

    #[test]
    fn absent_morph_masks_section_is_empty() -> Result<(), TestError> {
        let masks = MorphMasks::from_xml("<linden_avatar/>")?;
        assert!(masks.is_empty());
        assert_eq!(masks.len(), 0);
        Ok(())
    }

    #[test]
    fn rejects_wrong_root() {
        let result = MorphMasks::from_xml("<linden_skeleton/>");
        assert!(matches!(
            result,
            Err(crate::params::ParamError::UnexpectedRoot { .. })
        ));
    }

    #[test]
    fn alpha_at_is_nearest_neighbour_and_reads_the_last_component() {
        // A 2×1 RGBA image: left pixel alpha 0, right pixel alpha 255.
        let pixels = [10, 20, 30, 0, 40, 50, 60, 255];
        let tex = MaskTexture {
            pixels: &pixels,
            width: 2,
            height: 1,
            components: 4,
        };
        // u = 0 → left pixel (alpha 0); u = 1 → right pixel (alpha 255).
        assert!(approx(tex.alpha_at([0.0, 0.0]), 0.0));
        assert!(approx(tex.alpha_at([1.0, 0.0]), 1.0));
        // Out-of-range UV is clamped, not indexed out of bounds.
        assert!(approx(tex.alpha_at([2.0, 5.0]), 1.0));
    }

    #[test]
    fn alpha_at_degenerate_texture_is_zero() {
        let tex = MaskTexture {
            pixels: &[],
            width: 0,
            height: 0,
            components: 0,
        };
        assert!(approx(tex.alpha_at([0.5, 0.5]), 0.0));
    }

    #[test]
    fn sample_part_masks_the_morph_from_the_bake_alpha() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let masks = MorphMasks::from_xml(MINI_PARAMS)?;
        // A 2×2 RGBA bake where every pixel's alpha is 255 (fully covered): the
        // Fatten mask should hand back a full-strength weight for each delta.
        let opaque = vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let tex = MaskTexture {
            pixels: &opaque,
            width: 2,
            height: 2,
            components: 4,
        };
        let part = masks.sample_part(&mesh, "upper_body", &tex);
        assert_eq!(part.len(), 1);
        assert!(!part.is_empty());
        let fatten = part.get("Fatten").ok_or("Fatten weights")?;
        // The fixture's Fatten morph has two deltas, both fully covered.
        assert_eq!(fatten.len(), 2);
        assert!(fatten.iter().all(|&weight| approx(weight, 1.0)));
        // A region with no matching mask yields nothing.
        let head = masks.sample_part(&mesh, "head", &tex);
        assert!(head.is_empty());
        assert!(head.get("Fatten").is_none());
        Ok(())
    }

    #[test]
    fn sample_part_zero_alpha_masks_the_morph_off() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let masks = MorphMasks::from_xml(MINI_PARAMS)?;
        // Every pixel alpha 0 (no fabric): each delta weight is 0 — the morph is
        // masked off, so an un-clothed body shows no flare.
        let empty = vec![0_u8; 16];
        let tex = MaskTexture {
            pixels: &empty,
            width: 2,
            height: 2,
            components: 4,
        };
        let part = masks.sample_part(&mesh, "upper_body", &tex);
        let fatten = part.get("Fatten").ok_or("Fatten weights")?;
        assert!(fatten.iter().all(|&weight| approx(weight, 0.0)));
        Ok(())
    }
}
