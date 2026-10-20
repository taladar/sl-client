//! The reference viewer's body size (`LLAvatarAppearance::computeBodySize`,
//! `llavatarappearance.cpp`): how tall an avatar of a given shape stands, and
//! how far its pelvis sits above its soles.
//!
//! Both sides of the protocol need it. A viewer plants the avatar root from the
//! wire position (the physics-capsule centre) and advertises its own height in
//! `AgentSetAppearance`; a central-baking grid states the height in its bake
//! answer's `avatar_scale` (`book/src/gridspec/appearance.md` § Baking). The
//! formula is here once, over whatever resolves a joint's current local
//! position and scale: a viewer's posed skeleton with its rig overrides, or a
//! [`Skeleton`](crate::Skeleton)'s rest pose deformed by the shape
//! ([`Skeleton::body_size`](crate::Skeleton::body_size)).

/// The reference viewer's `computeBodySize` quantities for one avatar shape
/// (Second Life Z-up metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySizeMetrics {
    /// The reference's `mPelvisToFoot`: the vertical distance from the pelvis
    /// joint down to the sole (it includes the foot-below-ankle segment).
    /// ≈ 0.979 m at rest.
    pub pelvis_to_foot: f32,
    /// The reference's `mBodySize.z`: sole to (approximate) top of head —
    /// the avatar height the simulator's physics capsule is sized by.
    /// ≈ 1.707 m at rest.
    pub body_size_z: f32,
    /// The pelvis joint's current local (root-relative) Z — where the skeleton
    /// instance's `mPelvis` sits above the body-root entity (1.067 m at rest).
    /// The reference zeroes its pelvis under `mRoot` instead
    /// (`llavatarappearance.cpp` `buildSkeleton`), so a consumer placing a root
    /// *below* an un-zeroed pelvis must subtract this.
    pub pelvis_local_z: f32,
}

/// The reference's depth of an avatar's bounding box, in metres
/// (`DEFAULT_AGENT_DEPTH`): `computeBodySize` never measures it ("TODO --
/// measure the real depth and width"), and Second Life's bake answers state
/// the same.
pub const AVATAR_DEPTH: f32 = 0.45;

/// The reference's width of an avatar's bounding box, in metres
/// (`DEFAULT_AGENT_WIDTH`), unmeasured like [`AVATAR_DEPTH`].
pub const AVATAR_WIDTH: f32 = 0.6;

/// The body size from `joint`, which resolves a joint by name to its current
/// local position Z and scale Z, or `None` when it is missing.
///
/// The reference formula verbatim, including its sign quirk on the hip term
/// (the hip offset is *added* where the rest of the leg chain is subtracted)
/// and the √2 skull approximation "to get to the top of the head". Each
/// segment is scaled by its *parent's* local scale — the `scaleChildOffset`
/// semantics of the reference's recurrence. `None` when any joint of the chain
/// is missing (a malformed skeleton).
#[must_use]
pub fn body_size_from_joints(
    joint: impl Fn(&str) -> Option<(f32, f32)>,
) -> Option<BodySizeMetrics> {
    let (pelvis_z, pelvis_scale) = joint("mPelvis")?;
    let (torso_z, torso_scale) = joint("mTorso")?;
    let (chest_z, chest_scale) = joint("mChest")?;
    let (neck_z, neck_scale) = joint("mNeck")?;
    let (head_z, head_scale) = joint("mHead")?;
    let (skull_z, _skull_scale) = joint("mSkull")?;
    let (hip_z, hip_scale) = joint("mHipLeft")?;
    let (knee_z, knee_scale) = joint("mKneeLeft")?;
    let (ankle_z, ankle_scale) = joint("mAnkleLeft")?;
    let (foot_z, _foot_scale) = joint("mFootLeft")?;
    let pelvis_to_foot =
        hip_z * pelvis_scale - knee_z * hip_scale - ankle_z * knee_scale - foot_z * ankle_scale;
    let body_size_z = pelvis_to_foot
        + std::f32::consts::SQRT_2 * skull_z * head_scale
        + head_z * neck_scale
        + neck_z * chest_scale
        + chest_z * torso_scale
        + torso_z * pelvis_scale;
    Some(BodySizeMetrics {
        pelvis_to_foot,
        body_size_z,
        pelvis_local_z: pelvis_z,
    })
}

#[cfg(test)]
mod tests {
    use super::body_size_from_joints;
    use pretty_assertions::assert_eq;

    /// The formula over a chain of unit scales: the leg terms with the hip's
    /// sign quirk, the √2 skull, and the spine — and nothing without a joint.
    #[test]
    fn the_body_size_is_the_reference_formula() {
        let chain = |name: &str| -> Option<(f32, f32)> {
            let z = match name {
                "mPelvis" => 1.0,
                "mTorso" => 0.1,
                "mChest" => 0.2,
                "mNeck" => 0.3,
                "mHead" => 0.05,
                "mSkull" => 0.12,
                "mHipLeft" => -0.1,
                "mKneeLeft" => -0.5,
                "mAnkleLeft" => -0.4,
                "mFootLeft" => -0.05,
                _ => return None,
            };
            Some((z, 1.0))
        };
        let metrics = body_size_from_joints(chain);
        let pelvis_to_foot = -0.1_f32 + 0.5 + 0.4 + 0.05;
        assert_eq!(
            metrics.map(|metrics| (metrics.pelvis_to_foot, metrics.pelvis_local_z)),
            Some((pelvis_to_foot, 1.0))
        );
        let expected = pelvis_to_foot + std::f32::consts::SQRT_2 * 0.12 + 0.05 + 0.3 + 0.2 + 0.1;
        assert!(
            metrics.is_some_and(|metrics| (metrics.body_size_z - expected).abs() < 1e-6),
            "body size {metrics:?}, expected {expected}"
        );
        assert_eq!(body_size_from_joints(|_name| None), None);
    }
}
