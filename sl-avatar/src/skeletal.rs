//! Skeletal-distortion (`param_skeleton`) resolution (P13.4): the per-bone scale
//! and position deformations an avatar's visual params impose on its skeleton.
//!
//! Second Life shapes an avatar's *proportions* — height, limb and head scale,
//! hip width, torso length — by deforming skeleton bones rather than morphing
//! mesh vertices. Each `<param_skeleton>` param ([`ParamEffect::Skeleton`])
//! lists bones with a per-axis `scale` (and optional `offset`) deformation; the
//! reference viewer accumulates `effective_weight * deformation` onto each
//! bone's rest scale / position (Firestorm `LLPolySkeletalDistortion::apply`,
//! telescoping from a zero baseline so a param at any weight contributes
//! `weight * deformation`).
//!
//! [`SkeletalDeformations`] resolves an appearance vector into that per-bone
//! accumulation. It is the skeletal counterpart of
//! [`MorphWeights`](crate::MorphWeights): pure, I/O-free, Bevy-free, and in
//! Second Life Z-up metres. The Bevy layer turns these deformations into the
//! skeleton instance's deformed joint transforms; the scale deltas stretch a
//! bone's own bound geometry, while their effect on child bone *positions* (the
//! `scaleChildOffset` mechanism that makes height / limb-length work) is applied
//! there by the world-transform recurrence.
//!
//! Collision-volume scale inheritance (Firestorm's `inheritScale()` child pass)
//! is deliberately omitted: only `LLAvatarJointCollisionVolume` inherits, and
//! collision volumes are not part of the rendered / skinned skeleton.

use std::collections::HashMap;

use crate::params::{ParamEffect, VisualParams};
use crate::resolve::ResolvedParams;

/// One bone's accumulated skeletal deformation: the per-axis delta added to its
/// rest local scale and the per-axis delta added to its rest local position, in
/// Second Life Z-up metres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoneDeform {
    /// Additive per-axis scale delta (applied on top of the bone's rest scale,
    /// which is `1` for the standard skeleton).
    pub scale: [f32; 3],
    /// Additive per-axis position delta, in metres.
    pub offset: [f32; 3],
}

/// The resolved per-bone skeletal deformations for one avatar, keyed by the
/// skeleton bone name (`mNeck`, `mChest`, …) the params target.
///
/// Built once from a [`VisualParams`] table and an appearance vector (or already
/// resolved [`ResolvedParams`]); a bone with no deforming param has no entry and
/// stays at its rest transform.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `SkeletalDeformations` reads clearly"
)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkeletalDeformations {
    /// Bone name → its accumulated deformation (only bones some param moves).
    by_bone: HashMap<String, BoneDeform>,
}

impl SkeletalDeformations {
    /// Resolve the deformations from a visual-param table and a raw wire
    /// `AvatarAppearance.visual_params` byte vector.
    #[must_use]
    pub fn from_appearance(params: &VisualParams, visual_params: &[u8]) -> Self {
        Self::from_resolved(
            params,
            &ResolvedParams::from_appearance(params, visual_params),
        )
    }

    /// Resolve the deformations from a visual-param table and already-resolved
    /// [`ResolvedParams`] (driver propagation + sex already applied).
    ///
    /// Each `param_skeleton` param contributes `effective_weight * deformation`
    /// to every bone it lists, summed across params — exactly the net effect of
    /// the reference viewer's telescoping `apply` from a zero baseline.
    #[must_use]
    pub fn from_resolved(params: &VisualParams, resolved: &ResolvedParams) -> Self {
        let mut by_bone: HashMap<String, BoneDeform> = HashMap::new();
        for param in params.all() {
            let ParamEffect::Skeleton(bones) = &param.effect else {
                continue;
            };
            let weight = resolved.effective_weight(param);
            if !is_significant(weight) {
                continue;
            }
            for bone in bones {
                let deform = by_bone.entry(bone.bone.clone()).or_default();
                add_scaled(&mut deform.scale, bone.scale, weight);
                if let Some(offset) = bone.offset {
                    add_scaled(&mut deform.offset, offset, weight);
                }
            }
        }
        Self { by_bone }
    }

    /// The accumulated deformation of the named bone, if any param moves it.
    #[must_use]
    pub fn get(&self, bone: &str) -> Option<&BoneDeform> {
        self.by_bone.get(bone)
    }

    /// The named bone's accumulated scale delta, or zero if no param moves it.
    #[must_use]
    pub fn scale(&self, bone: &str) -> [f32; 3] {
        self.by_bone
            .get(bone)
            .map_or([0.0; 3], |deform| deform.scale)
    }

    /// The named bone's accumulated position delta (metres), or zero if no param
    /// moves it.
    #[must_use]
    pub fn offset(&self, bone: &str) -> [f32; 3] {
        self.by_bone
            .get(bone)
            .map_or([0.0; 3], |deform| deform.offset)
    }

    /// The number of bones with a deformation.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_bone.len()
    }

    /// Whether no bone is deformed (the skeleton stays at its rest proportions).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_bone.is_empty()
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

/// Whether a weight is far enough from zero to bother applying (skips the many
/// skeletal params that default to exactly zero).
fn is_significant(weight: f32) -> bool {
    weight.abs() > f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::SkeletalDeformations;
    use crate::params::VisualParams;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A table with one transmitted skeletal `Height` param (id 33) scaling two
    /// bones plus a `male` driver (id 80) driving a non-transmitted skeletal
    /// `Male_Skeleton` (id 32) that offsets `mHead`. Wire order [33, 80].
    const SKEL_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <skeleton file_name="avatar_skeleton.xml">
    <param id="33" group="0" name="Height" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mTorso" scale="0 0 0.1"/>
        <bone name="mChest" scale="0 0 0.1"/>
      </param_skeleton>
    </param>
    <param id="32" group="1" name="Male_Skeleton" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mHead" scale="0.1 0.1 0" offset="0 0 0.05"/>
      </param_skeleton>
    </param>
  </skeleton>
  <driver_parameters>
    <param id="80" group="0" name="male" value_min="0" value_max="1" value_default="0">
      <param_driver>
        <driven id="32"/>
      </param_driver>
    </param>
  </driver_parameters>
</linden_avatar>"#;

    /// Compare two float vectors within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn close<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn transmitted_skeletal_param_scales_at_its_weight() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_LAD)?;
        // Height (slot 0) to full, male off.
        let deform = SkeletalDeformations::from_appearance(&params, &[255, 0]);
        assert!(close(deform.scale("mTorso"), [0.0, 0.0, 0.1]));
        assert!(close(deform.scale("mChest"), [0.0, 0.0, 0.1]));
        // Half weight -> half the scale delta.
        let half = SkeletalDeformations::from_appearance(&params, &[128, 0]);
        assert!(half.scale("mTorso")[2] > 0.049 && half.scale("mTorso")[2] < 0.051);
        // A bone no param touches has no deformation.
        assert!(close(deform.scale("mPelvis"), [0.0, 0.0, 0.0]));
        assert!(deform.get("mPelvis").is_none());
        Ok(())
    }

    #[test]
    fn male_driver_engages_the_driven_skeletal_param() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_LAD)?;
        // Female (male off): the driven Male_Skeleton stays at zero -> no mHead
        // deformation.
        let female = SkeletalDeformations::from_appearance(&params, &[0, 0]);
        assert!(female.get("mHead").is_none());

        // Male (male on): the driver engages Male_Skeleton, deforming mHead.
        let male = SkeletalDeformations::from_appearance(&params, &[0, 255]);
        assert!(close(male.scale("mHead"), [0.1, 0.1, 0.0]));
        assert!(close(male.offset("mHead"), [0.0, 0.0, 0.05]));
        Ok(())
    }

    #[test]
    fn empty_appearance_deforms_nothing() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_LAD)?;
        let deform = SkeletalDeformations::from_appearance(&params, &[]);
        assert!(deform.is_empty());
        assert_eq!(deform.len(), 0);
        Ok(())
    }
}
