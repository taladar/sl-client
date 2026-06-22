//! `ImprovedTerseObjectUpdate` motion (`Data`) and wrapped `TextureEntry` codec.

use super::{f32_to_u16, read_quantized_vector, u16_to_f32, write_quantized_vector};
use crate::types::ObjectMotion;
use sl_types::lsl::Rotation;
use sl_wire::{Reader, RegionLocalObjectId, Writer};

// ---------------------------------------------------------------------------
// `ImprovedTerseObjectUpdate` motion (`Data`) + wrapped `TextureEntry`
// ---------------------------------------------------------------------------

/// A decoded `ImprovedTerseObjectUpdate` entry: the object's local id, its state
/// byte, and its new motion.
#[derive(Debug, Clone, PartialEq)]
pub struct TerseUpdate {
    /// The object's region-local id.
    pub local_id: RegionLocalObjectId,
    /// The object/attachment state byte.
    pub state: u8,
    /// The object's new kinematic state (position full precision; velocity,
    /// acceleration, rotation, and angular velocity 16-bit quantized).
    pub motion: ObjectMotion,
}

/// Decodes the `Data` blob of an `ImprovedTerseObjectUpdate` entry. Returns
/// `None` on a short/garbled blob.
pub(crate) fn terse_update(blob: &[u8]) -> Option<TerseUpdate> {
    let mut reader = Reader::new(blob);
    let local_id = reader.u32().ok()?;
    let state = reader.u8().ok()?;
    let has_collision_plane = reader.u8().ok()? != 0;
    // Avatar updates carry a collision plane (LLVector4); other objects do not.
    let collision_plane = if has_collision_plane {
        Some(reader.vector4().ok()?)
    } else {
        None
    };
    let position = reader.vector3().ok()?;
    let velocity = read_quantized_vector(&mut reader, 128.0).ok()?;
    let acceleration = read_quantized_vector(&mut reader, 64.0).ok()?;
    // Rotation: four explicit 16-bit components (x, y, z, w) — not packed.
    let rot_x = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_y = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_z = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rot_s = u16_to_f32(reader.u16().ok()?, -1.0, 1.0);
    let rotation = Rotation {
        x: rot_x,
        y: rot_y,
        z: rot_z,
        s: rot_s,
    };
    let angular_velocity = read_quantized_vector(&mut reader, 64.0).ok()?;
    Some(TerseUpdate {
        local_id: RegionLocalObjectId(local_id),
        state,
        motion: ObjectMotion {
            position,
            velocity,
            acceleration,
            rotation,
            angular_velocity,
            collision_plane,
        },
    })
}

/// Encodes a [`TerseUpdate`] into the `Data` blob of an
/// `ImprovedTerseObjectUpdate` entry — the inverse of `terse_update` and a port
/// of OpenSim's `CreateImprovedTerseBlock`: the local id, state, a
/// collision-plane-present byte (and the 16-byte `LLVector4` plane for an
/// avatar), the full-precision position, the velocity (16-bit quantized over
/// `±128`), the acceleration (`±64`), the four explicit 16-bit rotation
/// components (`x, y, z, s` over `±1`), and the angular velocity (`±64`). The
/// 16-bit fields use the round-tripping `f32_to_u16` quantization (LL's
/// `F32_to_U16_ROUND`).
#[must_use]
pub fn encode_terse_object_data(update: &TerseUpdate) -> Vec<u8> {
    let mut writer = Writer::new();
    let motion = &update.motion;
    writer.put_u32(update.local_id.0);
    writer.put_u8(update.state);
    writer.put_u8(u8::from(motion.collision_plane.is_some()));
    if let Some(plane) = motion.collision_plane {
        writer.put_vector4(plane);
    }
    writer.put_vector3(&motion.position);
    write_quantized_vector(&mut writer, &motion.velocity, 128.0);
    write_quantized_vector(&mut writer, &motion.acceleration, 64.0);
    writer.put_u16(f32_to_u16(motion.rotation.x, -1.0, 1.0));
    writer.put_u16(f32_to_u16(motion.rotation.y, -1.0, 1.0));
    writer.put_u16(f32_to_u16(motion.rotation.z, -1.0, 1.0));
    writer.put_u16(f32_to_u16(motion.rotation.s, -1.0, 1.0));
    write_quantized_vector(&mut writer, &motion.angular_velocity, 64.0);
    writer.into_bytes()
}

/// Extracts the raw `TextureEntry` blob from the trailing `TextureEntry` field
/// of an `ImprovedTerseObjectUpdate` block, or `None` when the simulator sent no
/// texture change (the common case — the field is empty unless the update is
/// flagged `Textures`).
///
/// Unlike a full `ObjectUpdate`, whose `TextureEntry` field is the bare blob, the
/// terse field is wrapped: a 2-byte inner length, two zero bytes, then the
/// `TextureEntry` (OpenSim `CreateImprovedTerseBlock`; the codec has already
/// stripped the outer 2-byte field length). Skip the four-byte wrapper to recover
/// the blob, which decodes with
/// [`decode_texture_entry`](crate::decode_texture_entry).
pub(crate) fn terse_texture_entry(field: &[u8]) -> Option<Vec<u8>> {
    let blob = field.get(4..)?;
    if blob.is_empty() {
        return None;
    }
    Some(blob.to_vec())
}

/// Wraps a raw `TextureEntry` blob into the `TextureEntry` field content of an
/// `ImprovedTerseObjectUpdate` block — the inverse of `terse_texture_entry` and
/// a port of OpenSim's `CreateImprovedTerseBlock`: the inner blob length (a
/// little-endian `u16`, masked to 15 bits) followed by two zero bytes and then
/// the blob. (The codec adds the outer 2-byte field length.)
#[must_use]
pub fn encode_terse_texture_entry(texture_entry: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    // The inner length OpenSim writes is the blob length masked to 15 bits.
    let len = u16::try_from(texture_entry.len()).unwrap_or(0x7fff) & 0x7fff;
    writer.put_u16(len);
    writer.put_u16(0);
    writer.bytes(texture_entry);
    writer.into_bytes()
}
