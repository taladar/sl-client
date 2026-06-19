//! Full-precision `ObjectData` motion codec for `ObjectUpdate`.

use crate::session::{IDENTITY_ROTATION, ZERO_VECTOR};
use crate::types::ObjectMotion;
use sl_wire::{Reader, WireError, Writer};

// ---------------------------------------------------------------------------
// Full `ObjectUpdate` motion (`ObjectData`)
// ---------------------------------------------------------------------------

/// A zero/identity [`ObjectMotion`], used when a motion blob is malformed.
const fn zero_motion() -> ObjectMotion {
    ObjectMotion {
        position: ZERO_VECTOR,
        velocity: ZERO_VECTOR,
        acceleration: ZERO_VECTOR,
        rotation: IDENTITY_ROTATION,
        angular_velocity: ZERO_VECTOR,
        collision_plane: None,
    }
}

/// Decodes the full-precision `ObjectData` blob of an `ObjectUpdate` into an
/// [`ObjectMotion`]. Avatar variants (length 76/140) carry a 16-byte collision
/// plane prefix, which is skipped. Returns a zero motion on a short/garbled
/// blob rather than erroring (best-effort, no panic).
pub(crate) fn full_object_motion(blob: &[u8]) -> ObjectMotion {
    full_object_motion_inner(blob).unwrap_or_else(|_ignored| zero_motion())
}

/// The fallible inner of [`full_object_motion`].
fn full_object_motion_inner(blob: &[u8]) -> Result<ObjectMotion, WireError> {
    let mut reader = Reader::new(blob);
    // Avatar variants carry a collision-plane (LLVector4) prefix; ordinary
    // objects do not.
    let collision_plane = if matches!(blob.len(), 76 | 140) {
        Some(reader.vector4()?)
    } else {
        None
    };
    let position = reader.vector3()?;
    let velocity = reader.vector3()?;
    let acceleration = reader.vector3()?;
    // Rotation is a packed quaternion (three floats, w reconstructed).
    let rotation = reader.quaternion()?;
    let angular_velocity = reader.vector3()?;
    Ok(ObjectMotion {
        position,
        velocity,
        acceleration,
        rotation,
        angular_velocity,
        collision_plane,
    })
}

/// Encodes an [`ObjectMotion`] into the full-precision `ObjectData` blob of an
/// `ObjectUpdate` — the inverse of `full_object_motion`. Every field is written
/// at full `f32` precision (unlike the terse update's 16-bit quantization). An
/// avatar motion (one carrying a [`collision_plane`](ObjectMotion::collision_plane))
/// is prefixed with its 16-byte `LLVector4` plane, yielding the 76-byte avatar
/// form; an ordinary object yields the 60-byte form. The rotation is written as
/// three floats (its `s` component is dropped and the decoder reconstructs it
/// non-negative), matching the packed-quaternion wire form.
#[must_use]
pub fn encode_object_motion(motion: &ObjectMotion) -> Vec<u8> {
    let mut writer = Writer::new();
    if let Some(plane) = motion.collision_plane {
        writer.put_vector4(plane);
    }
    writer.put_vector3(&motion.position);
    writer.put_vector3(&motion.velocity);
    writer.put_vector3(&motion.acceleration);
    writer.put_quaternion(&motion.rotation);
    writer.put_vector3(&motion.angular_velocity);
    writer.into_bytes()
}
