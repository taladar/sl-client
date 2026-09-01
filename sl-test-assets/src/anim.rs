//! A procedural Second Life **animation asset**: the Linden keyframe-motion
//! (`.anim`) binary a viewer plays to pose an avatar's skeleton, which is what
//! the `ViewerAsset` capability serves and what
//! [`sl_anim`](https://docs.rs/sl-anim) decodes.
//!
//! There is no keyframe-motion *encoder* in the workspace — nothing but a test
//! needs to produce an asset — so this module is the one place that writes the
//! format, exactly as [`mesh`](crate::mesh) is for LLMesh. Only the modern
//! `1.0` encoding is written; the decoder also reads the legacy `0.1` Euler
//! form, which no fixture has any reason to emit.
//!
//! What it writes is deliberately the smallest motion the decoder accepts and a
//! camera can still see: the fixed header, **one** animated joint, a rotation
//! track, an empty position track and no constraints.

use crate::{push_u16, quantize_u16};

/// The `version` a modern `.anim` file declares
/// (`KEYFRAME_MOTION_VERSION`).
const VERSION: u16 = 1;

/// The `sub_version` a modern `.anim` file declares
/// (`KEYFRAME_MOTION_SUBVERSION`).
const SUB_VERSION: u16 = 0;

/// The motion's base priority: `HIGHEST_PRIORITY`, so the fixture's pose wins
/// over whatever idle a viewer falls back to when nothing is playing.
const BASE_PRIORITY: i32 = 4;

/// The per-joint priority written for the animated joint:
/// `USE_MOTION_PRIORITY`, which defers to [`BASE_PRIORITY`].
const JOINT_PRIORITY: i32 = -1;

/// The resting hand pose the motion selects for the joints it does not animate:
/// `HAND_POSE_RELAXED`.
const HAND_POSE: u32 = 1;

/// The joint [`chest_twist_animation_asset`] animates. The chest is one joint
/// every skeleton has (no optional Bento bone is involved) and it carries the
/// head and both arms with it, so twisting it moves a large patch of screen.
const TWIST_JOINT: &str = "mChest";

/// How long the twist takes, in seconds: out and back, so the pose at `t` and
/// at `t + 1 s` are the two extremes.
const TWIST_DURATION_S: f32 = 2.0;

/// The identity rotation, as the quaternion `[x, y, z, w]` the keyframes carry.
const NO_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// A sixth of a turn about the joint's local Z axis — `[0, 0, sin(30°),
/// cos(30°)]`. Big enough that no capture can mistake the two extremes for
/// each other, small enough to stay a plausible torso twist rather than a
/// broken pose.
const TWIST: [f32; 4] = [0.0, 0.0, 0.5, 0.866_025_4];

/// One rotation keyframe: when it happens and the orientation it holds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RotationKey {
    /// The keyframe time, in seconds from the start of the motion
    /// (`0..=duration`).
    time: f32,
    /// The orientation as a quaternion `[x, y, z, w]` whose real part is
    /// **non-negative**: the format stores only the three imaginary components
    /// and the decoder recovers `w = sqrt(1 - x² - y² - z²)`, so a negative-`w`
    /// quaternion would decode as its own conjugate.
    rotation: [f32; 4],
}

/// A **chest-twist animation asset**: two seconds of the avatar twisting its
/// torso a sixth of a turn and back, looping, with no ease in or out.
///
/// This is the motion a fake grid serves for an NPC that is playing something.
/// It exists so an avatar in a fixture actually *moves*: the render oracle for
/// "this avatar is animated" is that two captures a second apart differ, and a
/// second apart is exactly the gap between this motion's two extremes.
///
/// Easing is zero on both ends so the pose at a given time is the same however
/// long the motion has been running — a capture taken at an arbitrary moment
/// after arrival is still comparable.
#[must_use]
pub fn chest_twist_animation_asset() -> Vec<u8> {
    keyframe_motion(
        TWIST_JOINT,
        &[
            RotationKey {
                time: 0.0,
                rotation: NO_ROTATION,
            },
            RotationKey {
                time: TWIST_DURATION_S / 2.0,
                rotation: TWIST,
            },
            RotationKey {
                time: TWIST_DURATION_S,
                rotation: NO_ROTATION,
            },
        ],
    )
}

/// The whole `.anim` body: the fixed header, one animated `joint` carrying
/// `rotations`, an empty position track, and no constraints.
///
/// The motion's duration is the last keyframe's time, because a rotation key
/// past the duration is what the decoder rejects as corrupt.
fn keyframe_motion(joint: &str, rotations: &[RotationKey]) -> Vec<u8> {
    let duration = rotations
        .iter()
        .fold(0.0_f32, |longest, key| longest.max(key.time));

    let mut out = Vec::new();
    push_u16(&mut out, VERSION);
    push_u16(&mut out, SUB_VERSION);
    push_i32(&mut out, BASE_PRIORITY);
    push_f32(&mut out, duration);
    // No emote: the motion poses the body, not the face.
    push_cstring(&mut out, "");
    push_f32(&mut out, 0.0);
    push_f32(&mut out, duration);
    // Looping, so an NPC standing in a fixture keeps moving indefinitely.
    push_i32(&mut out, 1);
    // No ease in, no ease out — see `chest_twist_animation_asset`.
    push_f32(&mut out, 0.0);
    push_f32(&mut out, 0.0);
    push_u32(&mut out, HAND_POSE);
    push_u32(&mut out, 1);

    push_cstring(&mut out, joint);
    push_i32(&mut out, JOINT_PRIORITY);
    push_i32(&mut out, i32::try_from(rotations.len()).unwrap_or(0));
    for key in rotations {
        push_u16(&mut out, quantize_u16(key.time, 0.0, duration));
        let [x, y, z, _w] = key.rotation;
        for component in [x, y, z] {
            push_u16(&mut out, quantize_u16(component, -1.0, 1.0));
        }
    }
    // No position keys: the motion rotates a joint, it does not move one.
    push_i32(&mut out, 0);
    // No collision-volume constraints.
    push_i32(&mut out, 0);
    out
}

/// Appends a little-endian `u32`.
#[expect(
    clippy::little_endian_bytes,
    reason = "every multi-byte field of a keyframe motion is wire-defined little-endian"
)]
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a little-endian signed `i32`, through the two's-complement bit
/// pattern the decoder reads back (avoiding the lint-forbidden `as` cast).
fn push_i32(out: &mut Vec<u8>, value: i32) {
    push_u32(out, value.cast_unsigned());
}

/// Appends a little-endian IEEE-754 `f32`, through its bit pattern.
fn push_f32(out: &mut Vec<u8>, value: f32) {
    push_u32(out, value.to_bits());
}

/// Appends a NUL-terminated ASCII string — how the header's emote name and each
/// joint's name are stored.
fn push_cstring(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.as_bytes());
    out.push(0);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_anim::{HandPose, JointPriority, Motion};

    use super::{TWIST, TWIST_DURATION_S, TWIST_JOINT, chest_twist_animation_asset};

    type TestError = Box<dyn core::error::Error>;

    /// The quantisation step of a rotation component: the `[-1, 1]` range over
    /// the `u16` scale, which is what a decoded key may differ by.
    const ROTATION_EPSILON: f32 = 2.0 / 65_535.0;

    /// The asset the fixtures serve decodes back through the viewer's own
    /// animation decoder into the motion it was written as. The decoder is the
    /// contract, exactly as `sl-mesh` is for the mesh asset.
    #[expect(
        clippy::float_cmp,
        reason = "the header's floats travel verbatim through the file, so \
                  exact equality is the test"
    )]
    #[test]
    fn the_chest_twist_decodes_back_into_its_motion() -> Result<(), TestError> {
        let motion = Motion::from_bytes(&chest_twist_animation_asset())?;
        assert_eq!(motion.base_priority, JointPriority::HIGHEST);
        assert_eq!(motion.duration, TWIST_DURATION_S);
        assert!(motion.emote_name.is_empty());
        assert!(motion.loops);
        assert_eq!(motion.loop_in_point, 0.0);
        assert_eq!(motion.loop_out_point, TWIST_DURATION_S);
        assert_eq!(motion.ease_in_duration, 0.0);
        assert_eq!(motion.ease_out_duration, 0.0);
        assert_eq!(motion.hand_pose, HandPose::RELAXED);
        assert!(motion.constraints.is_empty());

        assert_eq!(motion.joints.len(), 1, "the motion animates one joint");
        let joint = motion.joints.first().ok_or("no joint")?;
        assert_eq!(joint.name, TWIST_JOINT);
        assert_eq!(joint.priority, JointPriority::USE_MOTION);
        assert!(
            joint.position_keys.is_empty(),
            "the motion writes no position track"
        );
        Ok(())
    }

    /// The rotation track comes back as the three keys it was written as, at
    /// the times it was written at — and the middle one really is the twist,
    /// far enough from the ends that a capture can tell them apart.
    #[test]
    fn the_rotation_track_holds_its_keys_and_its_extremes() -> Result<(), TestError> {
        let motion = Motion::from_bytes(&chest_twist_animation_asset())?;
        let joint = motion.joints.first().ok_or("no joint")?;
        let times: Vec<f32> = joint.rotation_keys.iter().map(|key| key.time).collect();
        assert_eq!(times.len(), 3);
        for (got, want) in times.iter().zip([0.0, 1.0, TWIST_DURATION_S]) {
            assert!(
                (got - want).abs() <= TWIST_DURATION_S * ROTATION_EPSILON,
                "keyframe at {got} s, wanted {want} s"
            );
        }

        let middle = joint.rotation_keys.get(1).ok_or("no middle key")?;
        for (got, want) in middle.rotation.iter().zip(TWIST) {
            assert!(
                (got - want).abs() <= ROTATION_EPSILON,
                "the twist decoded to {:?}, wanted {TWIST:?}",
                middle.rotation
            );
        }

        // The ends are the identity, so the pose a second either side of the
        // twist is a different pose: the dot product of two unit quaternions
        // is the cosine of half the angle between them, and a sixth of a turn
        // apart is well under one.
        for end in [
            joint.rotation_keys.first().ok_or("no first key")?,
            joint.rotation_keys.last().ok_or("no last key")?,
        ] {
            let dot: f32 = end
                .rotation
                .iter()
                .zip(middle.rotation)
                .map(|(a, b)| a * b)
                .sum();
            assert!(
                dot.abs() < 0.98,
                "the extremes are only {dot} apart — a capture could not tell them apart"
            );
        }
        Ok(())
    }
}
