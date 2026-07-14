//! Collision-volume displacement resolution (P34.3, P34.4): the per-volume scale
//! and position displacements an avatar's *shape* imposes on its collision volumes.
//!
//! The shape reaches the volumes by **two** independent mechanisms, and this module
//! resolves both into one accumulation.
//!
//! **The morph pass (P34.3).** A `<param_morph>` does not only move base-mesh
//! vertices. Around thirty of them — `Big_Chest`, `Small_Chest`, `Fat_Torso`,
//! `Breast_Gravity`, `Muscular_Torso`, `Squash_Stretch_Head`, `Bowed_Legs`,
//! `Foot_Size`, … — also carry [`VolumeMorph`](crate::VolumeMorph) children, which
//! add `effective_weight * scale` and `effective_weight * pos` to one of the
//! avatar's **collision volumes** (`LEFT_PEC`, `BELLY`, `BUTT`, `HEAD`,
//! `L_UPPER_LEG`, …) — the volume pass of Firestorm's `LLPolyMorphTarget::apply`.
//!
//! **The skeletal-inheritance pass (P34.4).** A `<param_skeleton>` — `Height`,
//! `Thickness`, `Torso Length`, `Leg Length`, `Head Size`, `Hip Width`, … — scales
//! bones, and a collision volume is the *one* kind of joint that inherits its
//! parent bone's scale deformation (Firestorm's `LLAvatarJointCollisionVolume`
//! overrides `inheritScale()` to true, and `LLPolySkeletalDistortion::setInfo` walks
//! each deformed bone's children to record `cv_rest_scale ⊙ bone_scale_deformation`
//! as a further deformation, applied at the param's weight alongside the bone's
//! own). So a body-thickness or leg-length slider fattens or stretches the volumes
//! as well as the bones. This is proportional, not additive: because the recorded
//! delta is the volume's *rest* scale times the bone's deformation, a bone scaled by
//! `+0.3` grows its volume by 30%, whatever size that volume is.
//!
//! Since [[viewer-p17-2]] the volumes are bindable joints, so this — not the mesh
//! morph target — is how a worn **rigged mesh** body or piece of clothing follows
//! the avatar's shape sliders. (The system body does not care: it is skinned to the
//! `m*` bones, not to the volumes, which is why both passes could go missing so long
//! unnoticed.)
//!
//! [`VolumeDeformations`] resolves an appearance vector into that per-volume
//! accumulation. It is the collision-volume counterpart of
//! [`SkeletalDeformations`](crate::SkeletalDeformations) — same
//! [`ResolvedParams`] input, same pure, I/O-free, Bevy-free Second Life Z-up
//! metres — and the Bevy layer folds it into the collision-volume joints of the
//! skeleton instance, on top of their `avatar_skeleton.xml` rest transform. The
//! skeletal pass additionally needs the [`Skeleton`] (for each bone's volumes and
//! their rest scales), so it lives behind
//! [`from_resolved_with_skeleton`](VolumeDeformations::from_resolved_with_skeleton);
//! the morph-only constructors serve a caller with no skeleton at hand.
//!
//! The [runtime morph params](crate::RUNTIME_MORPH_PARAMS) are **excluded**: the
//! body-physics `*_Driven` params carry volume morphs too, but they are driven per
//! frame by the physics simulation, which applies their volume displacement itself
//! ([`BodyPhysicsState::volume_offsets`](crate::BodyPhysicsState::volume_offsets),
//! P34.2). Including them here would double-count the bounce. Their appearance-rest
//! weight is zero anyway (the hidden controller params default to the middle of
//! their range), so nothing is lost by leaving them out.

use std::collections::HashMap;

use crate::morph::is_runtime_morph_param;
use crate::params::{ParamEffect, VisualParams};
use crate::resolve::ResolvedParams;
use crate::skeleton::Skeleton;

/// One collision volume's accumulated displacement: the per-axis delta added to
/// its rest scale and the per-axis delta added to its rest position, in Second
/// Life Z-up metres.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `VolumeDeform` reads clearly"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeDeform {
    /// Additive per-axis scale delta, on top of the volume's
    /// `avatar_skeleton.xml` rest scale.
    pub scale: [f32; 3],
    /// Additive per-axis position delta, in metres.
    pub position: [f32; 3],
}

/// The resolved per-collision-volume displacements for one avatar, keyed by the
/// volume name (`LEFT_PEC`, `BELLY`, `HEAD`, …) the `<volume_morph>` elements name.
///
/// Built once from a [`VisualParams`] table and an appearance vector (or already
/// resolved [`ResolvedParams`]); a volume no morph displaces has no entry and stays
/// at its rest transform.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `VolumeDeformations` reads clearly"
)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeDeformations {
    /// Volume name → its accumulated displacement (only volumes some morph moves).
    by_volume: HashMap<String, VolumeDeform>,
}

impl VolumeDeformations {
    /// Resolve the displacements from a visual-param table and a raw wire
    /// `AvatarAppearance.visual_params` byte vector.
    #[must_use]
    pub fn from_appearance(params: &VisualParams, visual_params: &[u8]) -> Self {
        Self::from_resolved(
            params,
            &ResolvedParams::from_appearance(params, visual_params),
        )
    }

    /// Resolve the displacements from a visual-param table and already-resolved
    /// [`ResolvedParams`] (driver propagation + sex already applied), **morph pass
    /// only** — the skeletal params' inherited volume scale needs the skeleton, so
    /// it takes [`from_resolved_with_skeleton`](Self::from_resolved_with_skeleton).
    ///
    /// Each morph param's `<volume_morph>` children contribute
    /// `effective_weight * (scale, pos)` to the volume they name, summed across
    /// params — the net effect of the reference viewer's telescoping volume pass
    /// from a zero baseline. The per-frame
    /// [runtime params](crate::RUNTIME_MORPH_PARAMS) are skipped; see the module
    /// docs.
    #[must_use]
    pub fn from_resolved(params: &VisualParams, resolved: &ResolvedParams) -> Self {
        let mut by_volume: HashMap<String, VolumeDeform> = HashMap::new();
        for param in params.all() {
            let ParamEffect::Morph(volumes) = &param.effect else {
                continue;
            };
            if volumes.is_empty() || is_runtime_morph_param(&param.name) {
                continue;
            }
            let weight = resolved.effective_weight(param);
            if !is_significant(weight) {
                continue;
            }
            for volume in volumes {
                let deform = by_volume.entry(volume.volume.clone()).or_default();
                add_scaled(&mut deform.scale, volume.scale, weight);
                add_scaled(&mut deform.position, volume.position, weight);
            }
        }
        Self { by_volume }
    }

    /// The full resolution: the morph pass of [`from_resolved`](Self::from_resolved)
    /// **plus** the skeletal params' scale inheritance (P34.4), which needs
    /// `skeleton` to know which collision volumes hang off each deformed bone and at
    /// what rest scale.
    ///
    /// For every `param_skeleton` bone with a significant weight, each collision
    /// volume of that bone takes an extra scale delta of
    /// `effective_weight * (volume_rest_scale ⊙ bone_scale_deformation)` — the
    /// deformation Firestorm's `LLPolySkeletalDistortion::setInfo` records for every
    /// child joint whose `inheritScale()` is true, which only a
    /// `LLAvatarJointCollisionVolume` is. A bone the skeleton does not have (or one
    /// with no volumes) contributes nothing. Skeletal params carry no volume
    /// *position* deformation: a volume follows its bone's offset through the
    /// ordinary parent-child transform, so only the scale is inherited.
    #[must_use]
    pub fn from_resolved_with_skeleton(
        params: &VisualParams,
        resolved: &ResolvedParams,
        skeleton: &Skeleton,
    ) -> Self {
        let mut deformations = Self::from_resolved(params, resolved);
        for param in params.all() {
            let ParamEffect::Skeleton(bones) = &param.effect else {
                continue;
            };
            let weight = resolved.effective_weight(param);
            if !is_significant(weight) {
                continue;
            }
            for bone in bones {
                let Some(joint) = skeleton.joint_by_name(&bone.bone) else {
                    continue;
                };
                for volume in &joint.collision_volumes {
                    let deform = deformations
                        .by_volume
                        .entry(volume.name.clone())
                        .or_default();
                    add_scaled(&mut deform.scale, product(volume.scale, bone.scale), weight);
                }
            }
        }
        deformations
    }

    /// The accumulated displacement of the named volume, if any morph moves it.
    #[must_use]
    pub fn get(&self, volume: &str) -> Option<&VolumeDeform> {
        self.by_volume.get(volume)
    }

    /// Multiply every displacement by `gain` — a debug affordance, in the spirit of
    /// the reference viewer's `physics_test` switch.
    ///
    /// A real shape's volume displacements are centimetres, and they move *only* a
    /// worn rigged mesh (the system body is not skinned to the volumes), so an
    /// exaggerated gain is the way to *see* that the accumulation reaches a mesh
    /// body at all — at `1.0` this is the identity.
    pub fn amplify(&mut self, gain: f32) {
        for deform in self.by_volume.values_mut() {
            for axis in 0..3 {
                if let Some(scale) = deform.scale.get_mut(axis) {
                    *scale *= gain;
                }
                if let Some(position) = deform.position.get_mut(axis) {
                    *position *= gain;
                }
            }
        }
    }

    /// The named volume's accumulated scale delta, or zero if no morph moves it.
    #[must_use]
    pub fn scale(&self, volume: &str) -> [f32; 3] {
        self.by_volume
            .get(volume)
            .map_or([0.0; 3], |deform| deform.scale)
    }

    /// The named volume's accumulated position delta (metres), or zero if no morph
    /// moves it.
    #[must_use]
    pub fn position(&self, volume: &str) -> [f32; 3] {
        self.by_volume
            .get(volume)
            .map_or([0.0; 3], |deform| deform.position)
    }

    /// Every displaced volume and its accumulated displacement, in no particular
    /// order — for a caller that wants to report the whole resolved set rather than
    /// query one volume (the viewer's shape diagnostic).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &VolumeDeform)> {
        self.by_volume
            .iter()
            .map(|(name, deform)| (name.as_str(), deform))
    }

    /// The number of displaced volumes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_volume.len()
    }

    /// Whether no volume is displaced (they all stay at their rest transform).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_volume.is_empty()
    }
}

/// The component-wise product of two vectors (Firestorm's `LLVector3::scaleVec`).
fn product(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax * bx, ay * by, az * bz]
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
/// morph params that quantize or default to exactly zero).
fn is_significant(weight: f32) -> bool {
    weight.abs() > f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::VolumeDeformations;
    use crate::params::VisualParams;
    use crate::resolve::ResolvedParams;
    use crate::skeleton::Skeleton;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The committed 4-bone skeleton fixture: `PELVIS` (rest scale `0.12 0.16 0.17`)
    /// hangs off `mPelvis`, `BELLY` (`0.09 0.13 0.15`) off `mTorso`; `mChest` and
    /// `mHipRight` carry none.
    const MINI_SKELETON: &str = include_str!("../tests/fixtures/mini_skeleton.xml");

    /// A table with two transmitted shape morphs carrying volume morphs — `Big_Chest`
    /// (id 1, scaling and lifting both pecs) and `Fat_Torso` (id 2, also widening the
    /// left pec, so the two accumulate) — a female-only `Breast_Gravity` (id 3), a
    /// morph with no volume morph at all (id 4), and the body-physics driven param
    /// `Breast_Physics_UpDown_Driven` (id 1200, a runtime param the physics
    /// simulation drives, so its volume morph must be skipped here). Wire order is by
    /// ascending id: [1, 2, 3, 4, 80, 1200].
    const VOLUME_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="upperBodyMesh" lod="0" file_name="avatar_upper_body.llm">
    <param id="1" group="0" name="Big_Chest" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="LEFT_PEC" scale="0.02 0.02 0.02" pos="0.0 0.0 0.01"/>
        <volume_morph name="RIGHT_PEC" scale="0.02 0.02 0.02" pos="0.0 0.0 0.01"/>
      </param_morph>
    </param>
    <param id="2" group="0" name="Fat_Torso" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="LEFT_PEC" scale="0.01 0.0 0.0"/>
      </param_morph>
    </param>
    <param id="3" group="0" name="Breast_Gravity" sex="female" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="LEFT_PEC" pos="0.0 0.0 -0.01"/>
      </param_morph>
    </param>
    <param id="4" group="0" name="Plain" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
    <param id="1200" group="1" name="Breast_Physics_UpDown_Driven" value_min="-3" value_max="3" value_default="3">
      <param_morph>
        <volume_morph name="LEFT_PEC" pos="0.0 0.0 -0.01"/>
      </param_morph>
    </param>
  </mesh>
  <driver_parameters>
    <param id="80" group="0" name="male" value_min="0" value_max="1" value_default="0">
      <param_driver/>
    </param>
  </driver_parameters>
</linden_avatar>"#;

    /// Compare two float vectors within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn close<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn a_shape_morph_displaces_its_volumes_at_its_weight() -> Result<(), TestError> {
        let params = VisualParams::from_xml(VOLUME_LAD)?;
        // Big_Chest (slot 0) to full, everything else off.
        let deform = VolumeDeformations::from_appearance(&params, &[255, 0, 0, 0, 0, 0]);
        assert!(close(deform.scale("LEFT_PEC"), [0.02, 0.02, 0.02]));
        assert!(close(deform.position("RIGHT_PEC"), [0.0, 0.0, 0.01]));
        // Half weight -> half the displacement.
        let half = VolumeDeformations::from_appearance(&params, &[128, 0, 0, 0, 0, 0]);
        assert!(close(half.position("RIGHT_PEC"), [0.0, 0.0, 0.005]));
        // A volume no morph names has no entry.
        assert!(deform.get("BELLY").is_none());
        assert!(close(deform.scale("BELLY"), [0.0, 0.0, 0.0]));
        Ok(())
    }

    #[test]
    fn several_morphs_accumulate_onto_one_volume() -> Result<(), TestError> {
        let params = VisualParams::from_xml(VOLUME_LAD)?;
        // Big_Chest and Fat_Torso both scale LEFT_PEC's X: 0.02 + 0.01.
        let deform = VolumeDeformations::from_appearance(&params, &[255, 255, 0, 0, 0, 0]);
        assert!(close(deform.scale("LEFT_PEC"), [0.03, 0.02, 0.02]));
        // …but only Big_Chest touches RIGHT_PEC.
        assert!(close(deform.scale("RIGHT_PEC"), [0.02, 0.02, 0.02]));
        Ok(())
    }

    #[test]
    fn sex_gating_applies_to_the_volume_pass() -> Result<(), TestError> {
        let params = VisualParams::from_xml(VOLUME_LAD)?;
        // Female avatar: the female-only Breast_Gravity drops the left pec.
        let female = VolumeDeformations::from_appearance(&params, &[0, 0, 255, 0, 0, 0]);
        assert!(close(female.position("LEFT_PEC"), [0.0, 0.0, -0.01]));
        // Male avatar (the `male` driver, slot 4, at full): gated back to its
        // default, so the volume stays at rest.
        let male = VolumeDeformations::from_appearance(&params, &[0, 0, 255, 0, 255, 0]);
        assert!(male.is_empty());
        Ok(())
    }

    #[test]
    fn the_physics_driven_params_are_left_to_the_simulation() -> Result<(), TestError> {
        let params = VisualParams::from_xml(VOLUME_LAD)?;
        // The driven param is non-transmitted and defaults to full weight, so only
        // the runtime-param filter can keep it out of the accumulation — and it must,
        // or the bounce would be counted twice (P34.2 applies it per frame).
        let deform = VolumeDeformations::from_appearance(&params, &[]);
        assert!(deform.is_empty());
        assert_eq!(deform.len(), 0);
        Ok(())
    }

    #[test]
    fn a_volume_morph_declared_on_two_parts_applies_once_per_part() -> Result<(), TestError> {
        // The reference builds one `LLPolyMorphTarget` per `<mesh>` declaration of a
        // param, each running the volume pass, so a param declared on two parts with
        // a volume morph on each displaces the volume twice — and one declared with
        // the volume morph on only one part (the real `Squash_Stretch_Head`, whose
        // last, eyelash declaration carries none) still displaces it once.
        let lad = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="headMesh" lod="0" file_name="avatar_head.llm">
    <param id="187" group="0" name="Squash_Stretch_Head" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="HEAD" scale="0.0 0.0 0.015"/>
      </param_morph>
    </param>
    <param id="188" group="0" name="Twice" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="HEAD" pos="0.0 0.0 0.01"/>
      </param_morph>
    </param>
  </mesh>
  <mesh type="eyelashMesh" lod="0" file_name="avatar_eyelash.llm">
    <param id="187" group="0" shared="1" name="Squash_Stretch_Head" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
    <param id="188" group="0" shared="1" name="Twice" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="HEAD" pos="0.0 0.0 0.01"/>
      </param_morph>
    </param>
  </mesh>
</linden_avatar>"#;
        let params = VisualParams::from_xml(lad)?;
        let deform = VolumeDeformations::from_appearance(&params, &[255, 255]);
        // The head param survives its volume-less eyelash re-declaration…
        assert!(close(deform.scale("HEAD"), [0.0, 0.0, 0.015]));
        // …and the one declared with a volume morph on both parts applies twice.
        assert!(close(deform.position("HEAD"), [0.0, 0.0, 0.02]));
        Ok(())
    }

    /// A table exercising the P34.4 skeletal-inheritance pass against
    /// [`MINI_SKELETON`]: a morph `Fat_Torso` (id 1) that widens `BELLY` directly,
    /// a skeletal `Height` (id 33) that stretches `mTorso` (whose volume is
    /// `BELLY`), and a skeletal `Thickness` (id 34) that fattens `mPelvis` (whose
    /// volume is `PELVIS`) and also names a bone the skeleton does not have. Wire
    /// order is by ascending id: [1, 33, 34].
    const SKEL_VOLUME_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="upperBodyMesh" lod="0" file_name="avatar_upper_body.llm">
    <param id="1" group="0" name="Fat_Torso" value_min="0" value_max="1" value_default="0">
      <param_morph>
        <volume_morph name="BELLY" scale="0.01 0.0 0.0" pos="0.0 0.0 0.002"/>
      </param_morph>
    </param>
  </mesh>
  <skeleton file_name="avatar_skeleton.xml">
    <param id="33" group="0" name="Height" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mTorso" scale="0 0 0.1" offset="0 0 0.02"/>
      </param_skeleton>
    </param>
    <param id="34" group="0" name="Thickness" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mPelvis" scale="0.3 0.3 0"/>
        <bone name="mNotInTheSkeleton" scale="0.5 0.5 0.5"/>
      </param_skeleton>
    </param>
  </skeleton>
</linden_avatar>"#;

    #[test]
    fn a_skeletal_param_scales_its_bones_collision_volumes() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_VOLUME_LAD)?;
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        // Height (slot 1) to full: mTorso's +0.1 Z stretches BELLY by its own rest
        // scale times that deformation — 0.15 * 0.1 — not by the raw 0.1.
        let resolved = ResolvedParams::from_appearance(&params, &[0, 255, 0]);
        let deform = VolumeDeformations::from_resolved_with_skeleton(&params, &resolved, &skeleton);
        assert!(close(deform.scale("BELLY"), [0.0, 0.0, 0.015]));
        // The bone's *offset* is not inherited — a volume rides its bone's position
        // through the ordinary parent-child transform.
        assert!(close(deform.position("BELLY"), [0.0, 0.0, 0.0]));
        // A bone with no deforming param leaves its volume at rest.
        assert!(deform.get("PELVIS").is_none());

        // Half the weight, half the inherited delta.
        let half = ResolvedParams::from_appearance(&params, &[0, 128, 0]);
        let half = VolumeDeformations::from_resolved_with_skeleton(&params, &half, &skeleton);
        assert!(close(half.scale("BELLY"), [0.0, 0.0, 0.007_53]));
        Ok(())
    }

    #[test]
    fn the_inherited_scale_is_proportional_to_the_volumes_rest_scale() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_VOLUME_LAD)?;
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        // Thickness (slot 2) to full: mPelvis's +0.3 X/Y fattens PELVIS (rest scale
        // 0.12 0.16 0.17) by 30% of each axis — 0.036, 0.048 — and leaves Z alone.
        let resolved = ResolvedParams::from_appearance(&params, &[0, 0, 255]);
        let deform = VolumeDeformations::from_resolved_with_skeleton(&params, &resolved, &skeleton);
        assert!(close(deform.scale("PELVIS"), [0.036, 0.048, 0.0]));
        // The bone the skeleton does not have is simply skipped: it neither panics
        // nor invents a volume.
        assert_eq!(deform.len(), 1);
        Ok(())
    }

    #[test]
    fn the_two_passes_accumulate_onto_one_volume() -> Result<(), TestError> {
        let params = VisualParams::from_xml(SKEL_VOLUME_LAD)?;
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        // Fat_Torso (the morph pass) and Height (the skeletal pass) both reach BELLY.
        let resolved = ResolvedParams::from_appearance(&params, &[255, 255, 0]);
        let both = VolumeDeformations::from_resolved_with_skeleton(&params, &resolved, &skeleton);
        assert!(close(both.scale("BELLY"), [0.01, 0.0, 0.015]));
        assert!(close(both.position("BELLY"), [0.0, 0.0, 0.002]));

        // Without the skeleton only the morph pass lands — which is precisely what
        // P34.4 adds to the resolution.
        let morphs_only = VolumeDeformations::from_resolved(&params, &resolved);
        assert!(close(morphs_only.scale("BELLY"), [0.01, 0.0, 0.0]));
        assert!(close(morphs_only.position("BELLY"), [0.0, 0.0, 0.002]));
        Ok(())
    }
}
