//! Fixture-based tests for the `.anim` (Linden keyframe-motion) decoder.
//!
//! The committed fixture `fixtures/minimal.anim` is a hand-built keyframe motion
//! (one `mPelvis` joint with two rotation and one position key, plus one
//! ground-plane constraint). The decode tests assert the exact values it should
//! produce, and the error-path tests mutate a copy of it to drive each
//! rejection branch.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_anim::decode::{
        AnimDecodeError, ConstraintTargetType, ConstraintType, HandPose, JointPriority, Motion,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The committed binary fixture (modern `1.0` encoding).
    const MINIMAL: &[u8] = include_bytes!("fixtures/minimal.anim");

    /// The committed legacy `0.1` fixture: one `mHead` joint with an `f32`-time,
    /// Euler-angle rotation key and an `f32` position key whose Z is clamped.
    const MINIMAL_OLD: &[u8] = include_bytes!("fixtures/minimal_old.anim");

    /// Assert two `f32`s agree within the keyframe-dequantisation tolerance.
    #[track_caller]
    fn approx(actual: f32, expected: f32) {
        let diff = (actual - expected).abs();
        assert!(
            diff < 1.0e-4,
            "expected ~{expected}, got {actual} (|diff| = {diff})"
        );
    }

    /// Assert two 3-vectors agree component-wise within the tolerance.
    #[track_caller]
    fn approx_vec3(actual: [f32; 3], expected: [f32; 3]) {
        approx(actual[0], expected[0]);
        approx(actual[1], expected[1]);
        approx(actual[2], expected[2]);
    }

    #[test]
    fn decodes_minimal_motion_header() -> Result<(), TestError> {
        let motion = Motion::from_bytes(MINIMAL)?;

        assert_eq!(motion.base_priority, JointPriority::HIGHER);
        approx(motion.duration, 2.0);
        assert_eq!(motion.emote_name, "");
        approx(motion.loop_in_point, 0.0);
        approx(motion.loop_out_point, 2.0);
        assert!(motion.loops);
        approx(motion.ease_in_duration, 0.8);
        approx(motion.ease_out_duration, 0.5);
        assert_eq!(motion.hand_pose, HandPose::RELAXED);
        assert_eq!(motion.joints.len(), 1);
        assert_eq!(motion.constraints.len(), 1);
        Ok(())
    }

    #[test]
    fn decodes_minimal_motion_joint_tracks() -> Result<(), TestError> {
        let motion = Motion::from_bytes(MINIMAL)?;
        let joint = motion.joints.first().ok_or("expected one joint")?;

        assert_eq!(joint.name, "mPelvis");
        assert_eq!(joint.priority, JointPriority::HIGHEST);

        assert_eq!(joint.rotation_keys.len(), 2);
        let rot0 = joint.rotation_keys.first().ok_or("rot key 0")?;
        approx(rot0.time, 0.0);
        // u16 65535/0/32768 over [-1, 1] → 1.0, -1.0, ~0.0 (near-zero snapped).
        approx(rot0.rotation[0], 1.0);
        approx(rot0.rotation[1], -1.0);
        approx(rot0.rotation[2], 0.0);
        // t = 1 - (1 + 1 + 0) < 0 → real part clamped to 0.
        approx(rot0.rotation[3], 0.0);

        let rot1 = joint.rotation_keys.get(1).ok_or("rot key 1")?;
        approx(rot1.time, 2.0);
        // all components snap to 0 → identity quaternion.
        approx(rot1.rotation[0], 0.0);
        approx(rot1.rotation[1], 0.0);
        approx(rot1.rotation[2], 0.0);
        approx(rot1.rotation[3], 1.0);

        assert_eq!(joint.position_keys.len(), 1);
        let pos0 = joint.position_keys.first().ok_or("pos key 0")?;
        approx(pos0.time, 0.0);
        // u16 65535/0/32768 over [-5, 5] → 5.0, -5.0, ~0.0.
        approx(pos0.position[0], 5.0);
        approx(pos0.position[1], -5.0);
        approx(pos0.position[2], 0.0);
        Ok(())
    }

    #[test]
    fn decodes_minimal_motion_constraint() -> Result<(), TestError> {
        let motion = Motion::from_bytes(MINIMAL)?;
        let constraint = motion.constraints.first().ok_or("one constraint")?;

        assert_eq!(constraint.chain_length, 1);
        assert_eq!(constraint.constraint_type, ConstraintType::Plane);
        assert_eq!(constraint.source_volume, "mSourceVol");
        // f32 offsets are packed as raw bits, so they round-trip exactly.
        approx_vec3(constraint.source_offset, [0.1, 0.2, 0.3]);
        assert_eq!(constraint.target_type, ConstraintTargetType::Ground);
        assert_eq!(constraint.target_volume, "");
        approx_vec3(constraint.target_offset, [1.0, 2.0, 3.0]);
        approx_vec3(constraint.target_dir, [0.0, 0.0, 0.0]);
        approx(constraint.ease_in_start_time, 0.0);
        approx(constraint.ease_in_stop_time, 0.1);
        approx(constraint.ease_out_start_time, 0.9);
        approx(constraint.ease_out_stop_time, 1.0);
        Ok(())
    }

    #[test]
    fn decodes_legacy_0_1_motion() -> Result<(), TestError> {
        let motion = Motion::from_bytes(MINIMAL_OLD)?;

        assert_eq!(motion.base_priority, JointPriority::HIGH);
        approx(motion.duration, 2.0);
        assert!(!motion.loops);
        assert_eq!(motion.hand_pose, HandPose::SPREAD);
        assert_eq!(motion.joints.len(), 1);
        assert!(motion.constraints.is_empty());

        let joint = motion.joints.first().ok_or("expected one joint")?;
        assert_eq!(joint.name, "mHead");
        assert_eq!(joint.priority, JointPriority::USE_MOTION);

        // Legacy rotation key: f32 time and Euler angles (90, 0, 0) degrees in
        // ZYX order → a +90° rotation about X.
        let rot = joint.rotation_keys.first().ok_or("rot key")?;
        approx(rot.time, 0.5);
        approx(rot.rotation[0], 0.707_106_77);
        approx(rot.rotation[1], 0.0);
        approx(rot.rotation[2], 0.0);
        approx(rot.rotation[3], 0.707_106_77);

        // Legacy position key: f32 time and metre components, Z clamped to 5.0.
        let pos = joint.position_keys.first().ok_or("pos key")?;
        approx(pos.time, 1.0);
        approx(pos.position[0], 1.5);
        approx(pos.position[1], -2.0);
        approx(pos.position[2], 5.0);
        Ok(())
    }

    #[test]
    fn rejects_truncated_data() -> Result<(), TestError> {
        let head = MINIMAL.get(..3).ok_or("fixture shorter than 3 bytes")?;
        assert!(matches!(
            Motion::from_bytes(head),
            Err(AnimDecodeError::UnexpectedEof { .. })
        ));
        Ok(())
    }

    #[test]
    fn rejects_empty_data() {
        assert_eq!(
            Motion::from_bytes(&[]).err(),
            Some(AnimDecodeError::UnexpectedEof { field: "version" })
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        // Flip the major version (first u16) to 2.
        let mut bytes = MINIMAL.to_vec();
        bytes.splice(0..2, [2, 0]);
        assert_eq!(
            Motion::from_bytes(&bytes).err(),
            Some(AnimDecodeError::UnsupportedVersion {
                version: 2,
                sub_version: 0
            })
        );
    }

    #[test]
    fn rejects_zero_joints() {
        // num_joints is the u32 at offset 37 (2+2+4+4+1+4+4+4+4+4+4 header
        // bytes: an empty emote name is a single NUL).
        let mut bytes = MINIMAL.to_vec();
        bytes.splice(37..41, [0, 0, 0, 0]);
        assert_eq!(
            Motion::from_bytes(&bytes).err(),
            Some(AnimDecodeError::NoJoints)
        );
    }
}
