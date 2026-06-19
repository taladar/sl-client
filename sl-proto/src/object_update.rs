//! Packed `ObjectData` / `Data` blob codecs for the object/scene-graph updates
//! (`ObjectUpdate`, `ImprovedTerseObjectUpdate`, `ObjectUpdateCompressed`).
//!
//! The generated LLUDP message codec frames each message and its per-object
//! blocks; the motion and compressed-object payloads inside those blocks are
//! hand-packed binary sub-codecs that live here, split into one module per wire
//! format ([`full`], [`terse`], [`compressed`]) over the shared fixed-point
//! quantization and NUL-string helpers below. Each decoder (the client
//! direction, populating an [`Object`](crate::Object) /
//! [`ObjectMotion`](crate::ObjectMotion)) is paired with the inverse encoder
//! (the simulator direction, assembling the same wire blob), so a server built
//! on `sl-proto` can *send* the updates a viewer decodes.
//!
//! The encoders are the literal inverse of the decoders: they re-quantize with
//! the same factors the decoders divide by and re-pack the `Object`'s raw
//! `texture_entry` / `texture_anim` / `particle_system` / `extra_params` byte
//! fields (the wire bytes a server assembles via the
//! [`encode_texture_entry`](crate::encode_texture_entry) /
//! [`encode_extra_params`](crate::encode_extra_params) /
//! [`encode_particle_system`](crate::encode_particle_system) /
//! [`encode_texture_anim`](crate::encode_texture_anim) sub-codecs), giving a
//! lossless decode → encode → decode round trip.

use sl_types::lsl::Vector;
use sl_wire::{Reader, WireError, Writer};

mod compressed;
mod full;
mod terse;
#[cfg(test)]
mod tests;

pub use compressed::encode_compressed_object;
pub use full::encode_object_motion;
pub use terse::{TerseUpdate, encode_terse_object_data, encode_terse_texture_entry};

pub(crate) use compressed::compressed_object;
pub(crate) use full::full_object_motion;
pub(crate) use terse::{terse_texture_entry, terse_update};

// ---------------------------------------------------------------------------
// Fixed-point quantization helpers
// ---------------------------------------------------------------------------

/// Dequantizes a 16-bit fixed-point value spanning `[lower, upper]` back to an
/// `f32`, matching LL's `U16_to_F32` (including its snap-to-zero of values
/// within one quantum of zero).
fn u16_to_f32(value: u16, lower: f32, upper: f32) -> f32 {
    let range = upper - lower;
    let result = f32::from(value) / f32::from(u16::MAX) * range + lower;
    let max_error = range / f32::from(u16::MAX);
    if result.abs() < max_error {
        0.0
    } else {
        result
    }
}

/// Re-quantizes an `f32` spanning `[lower, upper]` to its 16-bit fixed-point
/// form — the round-tripping inverse of [`u16_to_f32`], matching the viewer's
/// `F32_to_U16_ROUND` (`llquantize.h`): clamp into range, normalize to `[0, 1]`,
/// scale by `u16::MAX`, and round to the nearest integer. (LL's plain
/// `F32_to_U16` *floors* instead, which can round-trip one quantum short of the
/// value our decoder produced; rounding is the exact inverse.)
fn f32_to_u16(value: f32, lower: f32, upper: f32) -> u16 {
    let range = upper - lower;
    let clamped = value.clamp(lower, upper);
    let scaled = (clamped - lower) / range * f32::from(u16::MAX);
    round_to_u16(scaled)
}

/// Rounds a pre-clamped `f32` (already in `0..=65535`) to the nearest `u16`.
/// The cast lints are expected: the caller normalizes the value into the `u16`
/// range before rounding, matching LL's `ll_round`-then-cast.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value pre-clamped to 0..=65535; round-then-cast matches LL's ll_round"
)]
const fn round_to_u16(value: f32) -> u16 {
    value.round() as u16
}

/// Reads three consecutive 16-bit-quantized floats (each spanning
/// `[-range, range]`) as a [`Vector`].
fn read_quantized_vector(reader: &mut Reader<'_>, range: f32) -> Result<Vector, WireError> {
    let x = u16_to_f32(reader.u16()?, -range, range);
    let y = u16_to_f32(reader.u16()?, -range, range);
    let z = u16_to_f32(reader.u16()?, -range, range);
    Ok(Vector { x, y, z })
}

/// Writes a [`Vector`] as three 16-bit-quantized floats (each spanning
/// `[-range, range]`) — the inverse of [`read_quantized_vector`].
fn write_quantized_vector(writer: &mut Writer, vector: &Vector, range: f32) {
    writer.put_u16(f32_to_u16(vector.x, -range, range));
    writer.put_u16(f32_to_u16(vector.y, -range, range));
    writer.put_u16(f32_to_u16(vector.z, -range, range));
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

/// Reads a NUL-terminated UTF-8 string from `reader` (consuming the terminator).
fn read_nul_string(reader: &mut Reader<'_>) -> Option<String> {
    let mut bytes = Vec::new();
    loop {
        let byte = reader.u8().ok()?;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes a UTF-8 string and its NUL terminator — the inverse of
/// [`read_nul_string`]. The string must not contain an interior NUL (the wire
/// form is NUL-terminated, as `llSetText` / media URLs always are).
fn write_nul_string(writer: &mut Writer, value: &str) {
    writer.bytes(value.as_bytes());
    writer.put_u8(0);
}
