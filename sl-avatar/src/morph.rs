//! Visual-param morph-target application (P13.3): deform a decoded base-body
//! part into an avatar's real shape by blending its [`MorphTarget`](crate::MorphTarget) deltas,
//! scaled by the per-param weights an `AvatarAppearance.visual_params` vector
//! carries.
//!
//! Second Life shapes the system avatar by *morphing* the base mesh: every
//! shape/face/build slider is a visual param ([`crate::params`]) whose weight
//! scales a named morph target's sparse per-vertex deltas (position / normal /
//! …). The final rest body is the base geometry plus the sum of every morph's
//! `weight * delta`. This module is the pure, I/O-free, Bevy-free math for that
//! blend, mirroring Firestorm `LLPolyMorphTarget::apply`:
//!
//! - [`MorphWeights`] resolves the appearance vector against the visual-param
//!   table into a `morph-target name → weight` lookup, once per avatar (reusable
//!   across every base part).
//! - [`MorphWeights::apply`] applies that
//!   lookup to one base part, returning the morphed [`MorphedMesh`] (positions
//!   and normals) in Second Life Z-up space.
//!
//! Only the direct morph params are applied here: a param's weight comes from
//! the appearance vector (transmitted params) or its default (absent ones).
//! Driver → driven propagation and skeletal-scale params are Phase 13.4; texture
//! (colour / alpha) params never move geometry.

use std::collections::HashMap;

use crate::basemesh::BaseMesh;
use crate::masks::PartMorphMask;
use crate::params::{AppearanceValues, ParamEffect, VisualParams};
use crate::resolve::ResolvedParams;

/// The factor Firestorm scales each morph's normal delta by before
/// re-normalizing (`LLPolyMorphTarget::apply`'s `NORMAL_SOFTEN_FACTOR`), so a
/// morphed surface's shading eases rather than snapping to the raw delta normal.
const NORMAL_SOFTEN_FACTOR: f32 = 0.65;

/// A resolved set of morph-target weights, keyed by morph-target name — the
/// appearance of one avatar reduced to just the values that move base geometry.
///
/// Built once from a [`VisualParams`] table and an appearance vector (or already
/// mapped [`AppearanceValues`]), then applied to each base part with
/// [`MorphWeights::apply`]. Only params whose
/// effect is [`ParamEffect::Morph`] and whose weight
/// is non-zero are retained; a base morph target with no matching entry is left
/// at rest.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MorphWeights` reads clearly"
)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MorphWeights {
    /// Morph-target name → the weight scaling its deltas (only non-zero entries).
    by_name: HashMap<String, f32>,
}

impl MorphWeights {
    /// Resolve the weights from a visual-param table and a raw wire
    /// `AvatarAppearance.visual_params` byte vector.
    ///
    /// Transmitted params take their dequantized weight from the vector; driven
    /// morph params take their driver-propagated weight; every other morph param
    /// falls back to its [`default`](crate::VisualParam::default). Sex gating is
    /// applied throughout ([`ResolvedParams::effective_weight`]).
    #[must_use]
    pub fn from_appearance(params: &VisualParams, visual_params: &[u8]) -> Self {
        Self::from_resolved(
            params,
            &ResolvedParams::from_appearance(params, visual_params),
        )
    }

    /// Resolve the weights from a visual-param table and already-mapped
    /// [`AppearanceValues`] (avoids re-dequantizing when the caller kept them).
    #[must_use]
    pub fn from_values(params: &VisualParams, appearance: &AppearanceValues) -> Self {
        Self::from_resolved(params, &ResolvedParams::from_values(params, appearance))
    }

    /// Resolve the weights from a visual-param table and already-resolved
    /// [`ResolvedParams`] (driver propagation + sex already applied) — the shared
    /// path both other constructors funnel through, and the one to use when the
    /// caller already built [`ResolvedParams`] for the skeletal resolver too.
    ///
    /// Only params whose effect is [`ParamEffect::Morph`] with a significant
    /// (non-zero) effective weight are retained.
    #[must_use]
    pub fn from_resolved(params: &VisualParams, resolved: &ResolvedParams) -> Self {
        let mut by_name = HashMap::new();
        for param in params.all() {
            if matches!(param.effect, ParamEffect::Morph) {
                let weight = resolved.effective_weight(param);
                if is_significant(weight) {
                    by_name.insert(param.name.clone(), weight);
                }
            }
        }
        Self { by_name }
    }

    /// The weight scaling the morph target named `name`, or `0.0` if that morph
    /// is not driven (left at rest).
    #[must_use]
    pub fn weight(&self, name: &str) -> f32 {
        self.by_name.get(name).copied().unwrap_or(0.0)
    }

    /// The number of driven (non-zero) morph targets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether no morph target is driven (the body stays at its base rest shape).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Blend `base`'s morph targets by these weights, returning the morphed rest
    /// geometry.
    ///
    /// Each driven morph adds `weight * delta` to the affected vertices'
    /// positions and (softened, then re-normalized) normals, exactly as
    /// Firestorm `LLPolyMorphTarget::apply` accumulates them onto the base mesh.
    /// A morph target with no weight (or a delta whose vertex index is out of
    /// range) contributes nothing.
    #[must_use]
    pub fn apply(&self, base: &BaseMesh) -> MorphedMesh {
        self.apply_inner(base, None)
    }

    /// Blend `base`'s morph targets by these weights with per-vertex clothing-morph
    /// masks (P14.5), returning the morphed rest geometry.
    ///
    /// Identical to [`apply`](Self::apply), except that any morph the `masks`
    /// carry per-delta weights for (a clothing morph like `Shirtsleeve_flair`) is
    /// scaled *additionally* by its per-vertex mask weight — so the flared cuff
    /// applies only where the region's baked clothing layer is opaque, and an
    /// un-clothed limb shows no stray flare. A morph not present in `masks`
    /// applies at full strength exactly as [`apply`](Self::apply) does.
    #[must_use]
    pub fn apply_masked(&self, base: &BaseMesh, masks: &PartMorphMask) -> MorphedMesh {
        self.apply_inner(base, Some(masks))
    }

    /// The shared morph-blend, with an optional per-morph per-vertex mask.
    ///
    /// Each driven morph adds `weight * delta` (times the per-vertex mask weight
    /// when `masks` supplies one for that morph) to the affected vertices'
    /// positions and (softened, then re-normalized) normals, exactly as
    /// Firestorm `LLPolyMorphTarget::apply` accumulates them onto the base mesh.
    /// A morph target with no weight (or a delta whose vertex index is out of
    /// range) contributes nothing.
    fn apply_inner(&self, base: &BaseMesh, masks: Option<&PartMorphMask>) -> MorphedMesh {
        let mut positions = base.positions().to_vec();
        // Accumulate morphed normals into the scaled base normals, then
        // normalize once at the end (the reference viewer's `scaled_normals`).
        let mut scaled_normals = base.normals().to_vec();
        for morph in base.morphs() {
            let weight = self.weight(&morph.name);
            if !is_significant(weight) {
                continue;
            }
            // The clothing-morph mask, if this morph is masked in this part.
            let mask = masks.and_then(|masks| masks.get(&morph.name));
            for (delta_index, delta) in morph.deltas.iter().enumerate() {
                // Per-vertex mask weight (1.0 = unmasked / full strength).
                let mask_weight = mask
                    .and_then(|weights| weights.get(delta_index))
                    .copied()
                    .unwrap_or(1.0);
                let effective = weight * mask_weight;
                if !is_significant(effective) {
                    continue;
                }
                if let Some(position) = positions.get_mut(delta.vertex_index) {
                    add_scaled(position, delta.position, effective);
                }
                if let Some(normal) = scaled_normals.get_mut(delta.vertex_index) {
                    add_scaled(normal, delta.normal, effective * NORMAL_SOFTEN_FACTOR);
                }
            }
        }
        let normals = scaled_normals
            .iter()
            .map(|normal| normalize(*normal))
            .collect();
        MorphedMesh { positions, normals }
    }

    /// Test-only setter so a morph blend can be exercised without a param table
    /// (the base-mesh fixture's morph names differ from the param fixture's).
    #[cfg(test)]
    fn set_for_test(&mut self, name: &str, weight: f32) {
        self.by_name.insert(name.to_owned(), weight);
    }
}

/// The morphed rest geometry of one base part: per-vertex positions and normals
/// after blending the driven morph targets, parallel to the source
/// [`BaseMesh`]'s vertex arrays (Second Life Z-up metres).
///
/// Only positions and normals are produced — the shape and its shading. UV and
/// binormal morph deltas (normal-map tangents, texture-seam nudges) do not move
/// the silhouette and are left to the base values, matching what the un-textured
/// Phase-13.3 body needs.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphedMesh {
    /// Morphed per-vertex positions (Z-up metres).
    positions: Vec<[f32; 3]>,
    /// Morphed, re-normalized per-vertex normals.
    normals: Vec<[f32; 3]>,
}

impl MorphedMesh {
    /// The morphed per-vertex positions (Z-up metres).
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The morphed, re-normalized per-vertex normals.
    #[must_use]
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }
}

/// Add `scale * delta` into `target` component-wise.
fn add_scaled(target: &mut [f32; 3], delta: [f32; 3], scale: f32) {
    let [tx, ty, tz] = target;
    let [dx, dy, dz] = delta;
    *tx += dx * scale;
    *ty += dy * scale;
    *tz += dz * scale;
}

/// Normalize a 3-vector, leaving a degenerate (near-zero) vector unchanged so no
/// NaN is produced.
fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = vector;
    let length = (x * x + y * y + z * z).sqrt();
    if length > f32::EPSILON {
        [x / length, y / length, z / length]
    } else {
        vector
    }
}

/// Whether a weight is far enough from zero to bother applying (skips the many
/// params that quantize or default to exactly zero).
fn is_significant(weight: f32) -> bool {
    weight.abs() > f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::MorphWeights;
    use crate::basemesh::BaseMesh;
    use crate::params::VisualParams;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The committed minimal base-mesh fixture (4 vertices, one `Fatten` morph
    /// with deltas on vertices 0 and 3).
    const MINI_BASEMESH: &[u8] = include_bytes!("../tests/fixtures/mini_basemesh.llm");
    /// The committed minimal visual-param fixture (param id 1 is a `Morph`).
    const MINI_PARAMS: &str = include_str!("../tests/fixtures/mini_params.xml");

    /// Compare two float vectors within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn close<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    /// A morph weights map built by name, for applying without a param table.
    fn weights(pairs: &[(&str, f32)]) -> MorphWeights {
        let mut map = MorphWeights::default();
        for &(name, weight) in pairs {
            map.set_for_test(name, weight);
        }
        map
    }

    #[test]
    fn zero_weights_leave_the_base_at_rest() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let morphed = MorphWeights::default().apply(&mesh);
        assert_eq!(morphed.positions(), mesh.positions());
        Ok(())
    }

    #[test]
    fn full_weight_adds_the_delta() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        // The fixture's `Fatten` morph moves vertex 0 by (0.1, 0, 0) at weight 1.
        let morphed = weights(&[("Fatten", 1.0)]).apply(&mesh);
        let base0 = *mesh.positions().first().ok_or("base vertex 0")?;
        let morphed0 = *morphed.positions().first().ok_or("morphed vertex 0")?;
        assert!(close(morphed0, [base0[0] + 0.1, base0[1], base0[2]]));
        // A vertex the morph does not touch (vertex 1) is unchanged.
        assert_eq!(morphed.positions().get(1), mesh.positions().get(1));
        Ok(())
    }

    #[test]
    fn half_weight_adds_half_the_delta() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let morphed = weights(&[("Fatten", 0.5)]).apply(&mesh);
        let base0 = *mesh.positions().first().ok_or("base vertex 0")?;
        let morphed0 = *morphed.positions().first().ok_or("morphed vertex 0")?;
        assert!(close(morphed0, [base0[0] + 0.05, base0[1], base0[2]]));
        Ok(())
    }

    #[test]
    fn normals_stay_unit_length() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let morphed = weights(&[("Fatten", 1.0)]).apply(&mesh);
        for normal in morphed.normals() {
            let [x, y, z] = *normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!((length - 1.0).abs() < 1.0e-3 || length < f32::EPSILON);
        }
        Ok(())
    }

    #[test]
    fn resolves_weights_from_param_table() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Param id 1 (`Big_Brow`) is a morph in the fixture; wire order is
        // [1, 4, 32, 111, 112], so byte 0 drives it. Byte 255 over [-0.3, 2] is 2.
        let resolved = MorphWeights::from_appearance(&params, &[255, 0, 0, 0, 0]);
        assert!((resolved.weight("Big_Brow") - 2.0).abs() < 1.0e-4);
        // A name with no morph param is not driven.
        assert!(resolved.weight("Nonexistent").abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn mask_scales_the_morph_per_vertex() -> Result<(), TestError> {
        use crate::masks::PartMorphMask;
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        // The fixture's `Fatten` morph has two deltas (vertices 0 and 3); mask the
        // first fully off (0.0) and leave the second at full (1.0).
        let mask = PartMorphMask::from_pairs_for_test(&[("Fatten", vec![0.0, 1.0])]);
        let morphed = weights(&[("Fatten", 1.0)]).apply_masked(&mesh, &mask);
        // Vertex 0 (first delta, masked off) stays at rest.
        assert_eq!(morphed.positions().first(), mesh.positions().first());
        // Vertex 3 (second delta, full) moves by the delta.
        let base3 = *mesh.positions().get(3).ok_or("base vertex 3")?;
        let morphed3 = *morphed.positions().get(3).ok_or("morphed vertex 3")?;
        assert!(!close(morphed3, base3));
        Ok(())
    }

    #[test]
    fn half_mask_scales_the_delta_by_half() -> Result<(), TestError> {
        use crate::masks::PartMorphMask;
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let base0 = *mesh.positions().first().ok_or("base vertex 0")?;
        // Full weight, but the first delta is half-masked → half the delta (0.05).
        let mask = PartMorphMask::from_pairs_for_test(&[("Fatten", vec![0.5, 0.0])]);
        let morphed = weights(&[("Fatten", 1.0)]).apply_masked(&mesh, &mask);
        let morphed0 = *morphed.positions().first().ok_or("morphed vertex 0")?;
        assert!(close(morphed0, [base0[0] + 0.05, base0[1], base0[2]]));
        Ok(())
    }

    #[test]
    fn unmasked_morph_applies_at_full_strength() -> Result<(), TestError> {
        use crate::masks::PartMorphMask;
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        // A mask that names a different morph leaves `Fatten` at full strength, so
        // the result matches the plain `apply`.
        let mask = PartMorphMask::from_pairs_for_test(&[("Other", vec![0.0])]);
        let masked = weights(&[("Fatten", 1.0)]).apply_masked(&mesh, &mask);
        let plain = weights(&[("Fatten", 1.0)]).apply(&mesh);
        assert_eq!(masked.positions(), plain.positions());
        Ok(())
    }

    #[test]
    fn non_morph_params_are_excluded() -> Result<(), TestError> {
        let params = VisualParams::from_xml(MINI_PARAMS)?;
        // Param 32 is skeletal, 111 colour, 112 alpha — none drive a morph even
        // with a full weight byte.
        let resolved = MorphWeights::from_appearance(&params, &[0, 0, 255, 255, 255]);
        assert!(resolved.weight("Male_Skeleton").abs() < f32::EPSILON);
        Ok(())
    }
}
