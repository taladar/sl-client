//! Structured decoders for a prim's texture-animation (`TextureAnim`) and
//! particle-system (`PSBlock`) blobs.
//!
//! Both are carried on an `ObjectUpdate` as raw byte fields and retained verbatim
//! on [`Object`](crate::Object); this module decodes them into the typed
//! [`TextureAnimation`] and [`ParticleSystem`] value types, mirroring the
//! reference viewer's `LLTextureAnim::unpackTAMessage` (`lltextureanim.cpp`) and
//! `LLPartSysData::unpackBlock` (`llpartdata.cpp`).

use sl_types::key::ObjectKey;
use sl_types::lsl::Vector;
use sl_wire::{Reader, Writer};

use crate::types::{ParticleSystem, TextureAnimation};

/// The fixed wire size of a `TextureAnim` block (`TA_BLOCK_SIZE`).
const TA_BLOCK_SIZE: usize = 16;

/// The `SMOOTH` mode bit (`LLTextureAnim::SMOOTH`); when clear, the viewer treats
/// a zero frame-grid dimension as 1.
const TA_SMOOTH: u8 = 0x10;

/// The size of the particle source block (`PS_SYS_DATA_BLOCK_SIZE`).
const PS_SYS_DATA_BLOCK_SIZE: usize = 68;
/// The size of the legacy particle (`LLPartData`) block
/// (`PS_LEGACY_PART_DATA_BLOCK_SIZE`).
const PS_LEGACY_PART_DATA_BLOCK_SIZE: usize = 18;
/// The size of a full legacy particle system (`PS_LEGACY_DATA_BLOCK_SIZE`): the
/// source block immediately followed by the legacy particle block, with no size
/// prefixes.
const PS_LEGACY_DATA_BLOCK_SIZE: usize = PS_SYS_DATA_BLOCK_SIZE + PS_LEGACY_PART_DATA_BLOCK_SIZE;

/// The per-particle "has glow data" flag (`LL_PART_DATA_GLOW`).
const LL_PART_DATA_GLOW: u32 = 0x1_0000;
/// The per-particle "has blend-func data" flag (`LL_PART_DATA_BLEND`).
const LL_PART_DATA_BLEND: u32 = 0x2_0000;

/// The default source blend function (`LL_PART_BF_SOURCE_ALPHA`).
const LL_PART_BF_SOURCE_ALPHA: u8 = 7;
/// The default destination blend function (`LL_PART_BF_ONE_MINUS_SOURCE_ALPHA`).
const LL_PART_BF_ONE_MINUS_SOURCE_ALPHA: u8 = 9;

/// Decodes a raw `TextureAnim` blob into a [`TextureAnimation`]. Returns `None`
/// for a blob that is not exactly the 16-byte block (an empty blob means the
/// object has no texture animation), matching the viewer's size check in
/// `unpackTAMessage`.
#[must_use]
pub fn decode_texture_anim(blob: &[u8]) -> Option<TextureAnimation> {
    if blob.len() != TA_BLOCK_SIZE {
        return None;
    }
    let mut reader = Reader::new(blob);
    let mode = reader.u8().ok()?;
    let face = reader.i8().ok()?;
    let raw_size_x = reader.u8().ok()?;
    let raw_size_y = reader.u8().ok()?;
    let start = reader.f32().ok()?;
    let length = reader.f32().ok()?;
    let rate = reader.f32().ok()?;
    // For a non-SMOOTH animation the viewer floors the frame-grid dimensions at 1
    // (a 0×0 grid would page no frames); SMOOTH leaves them as sent.
    let (size_x, size_y) = if mode & TA_SMOOTH != 0 {
        (raw_size_x, raw_size_y)
    } else {
        (raw_size_x.max(1), raw_size_y.max(1))
    };
    Some(TextureAnimation {
        mode,
        face,
        size_x,
        size_y,
        start,
        length,
        rate,
    })
}

/// Decodes a raw `PSBlock` particle-system blob into a [`ParticleSystem`].
///
/// Handles both wire forms the viewer's `LLPartSysData::unpackBlock` does: the
/// legacy fixed 86-byte block, and the modern form that prefixes the source and
/// particle sub-blocks with `S32` sizes and may carry trailing glow / blend-func
/// data gated by the per-particle flags. Returns `None` for an empty blob (no
/// system) or one that does not decode.
#[must_use]
pub fn decode_particle_system(blob: &[u8]) -> Option<ParticleSystem> {
    if blob.is_empty() {
        return None;
    }
    let mut reader = Reader::new(blob);
    if blob.len() == PS_LEGACY_DATA_BLOCK_SIZE {
        decode_legacy(&mut reader)
    } else {
        decode_modern(&mut reader)
    }
}

/// Decodes the legacy 86-byte form: the source block then the legacy particle
/// block, neither size-prefixed (`LLPartSysData::unpackLegacy`).
fn decode_legacy(reader: &mut Reader<'_>) -> Option<ParticleSystem> {
    let mut system = decode_system(reader)?;
    decode_legacy_part(reader, &mut system)?;
    Some(system)
}

/// Decodes the modern form: an `S32` source-block size (which must be the known
/// 68), the source block, an `S32` particle-block size, the legacy particle
/// block, then optional glow / blend-func data (`LLPartSysData::unpack` →
/// `LLPartData::unpack`).
fn decode_modern(reader: &mut Reader<'_>) -> Option<ParticleSystem> {
    let sys_size = usize::try_from(reader.u32().ok()?).ok()?;
    if sys_size != PS_SYS_DATA_BLOCK_SIZE {
        // A source block of an unexpected size is from a newer protocol this
        // decoder does not understand; treat the whole system as undecodable.
        return None;
    }
    let mut system = decode_system(reader)?;
    // The particle block carries its own size; the legacy fields are always
    // present and the glow/blend fields follow only when their flags are set.
    let part_size = usize::try_from(reader.u32().ok()?).ok()?;
    decode_legacy_part(reader, &mut system)?;
    let mut remaining = part_size.checked_sub(PS_LEGACY_PART_DATA_BLOCK_SIZE)?;
    if system.part_flags & LL_PART_DATA_GLOW != 0 {
        remaining = remaining.checked_sub(2)?;
        system.part_start_glow = f32::from(reader.u8().ok()?) / 255.0;
        system.part_end_glow = f32::from(reader.u8().ok()?) / 255.0;
    }
    if system.part_flags & LL_PART_DATA_BLEND != 0 {
        remaining = remaining.checked_sub(2)?;
        system.part_blend_func_source = reader.u8().ok()?;
        system.part_blend_func_dest = reader.u8().ok()?;
    }
    // Any further bytes are unrecognised parameters; the viewer rejects such a
    // system as undisplayable, but for a decode-fidelity surface we keep what we
    // parsed and simply ignore the tail.
    let _ignored = remaining;
    Some(system)
}

/// Decodes the 68-byte particle source block (`LLPartSysData::unpackSystem`),
/// returning a [`ParticleSystem`] whose particle-template fields are left at
/// their defaults until [`decode_legacy_part`] fills them.
fn decode_system(reader: &mut Reader<'_>) -> Option<ParticleSystem> {
    let crc = reader.u32().ok()?;
    let flags = reader.u32().ok()?;
    let pattern = reader.u8().ok()?;
    let max_age = unpack_fixed_u16(reader, 8)?;
    let start_age = unpack_fixed_u16(reader, 8)?;
    let inner_angle = unpack_fixed_u8(reader, 5)?;
    let outer_angle = unpack_fixed_u8(reader, 5)?;
    let burst_rate = unpack_fixed_u16(reader, 8)?.max(0.01);
    let burst_radius = unpack_fixed_u16(reader, 8)?;
    let burst_speed_min = unpack_fixed_u16(reader, 8)?;
    let burst_speed_max = unpack_fixed_u16(reader, 8)?;
    let burst_part_count = reader.u8().ok()?;
    let angular_velocity = unpack_fixed_vector_signed(reader, 8, 7)?;
    let acceleration = unpack_fixed_vector_signed(reader, 8, 7)?;
    let texture_id = reader.uuid().ok()?;
    let target_id = reader.uuid().ok()?;
    Some(ParticleSystem {
        crc,
        flags,
        pattern,
        max_age,
        start_age,
        inner_angle,
        outer_angle,
        burst_rate,
        burst_radius,
        burst_speed_min,
        burst_speed_max,
        burst_part_count,
        angular_velocity,
        acceleration,
        texture_id,
        target_id: ObjectKey::from(target_id),
        part_flags: 0,
        part_max_age: 0.0,
        part_start_color: [255; 4],
        part_end_color: [255; 4],
        part_start_scale: [1.0, 1.0],
        part_end_scale: [1.0, 1.0],
        part_start_glow: 0.0,
        part_end_glow: 0.0,
        part_blend_func_source: LL_PART_BF_SOURCE_ALPHA,
        part_blend_func_dest: LL_PART_BF_ONE_MINUS_SOURCE_ALPHA,
    })
}

/// Decodes the 18-byte legacy particle-template block
/// (`LLPartData::unpackLegacy`) into the `part_*` fields of `system`.
fn decode_legacy_part(reader: &mut Reader<'_>, system: &mut ParticleSystem) -> Option<()> {
    system.part_flags = reader.u32().ok()?;
    system.part_max_age = unpack_fixed_u16(reader, 8)?;
    system.part_start_color = reader.take_array::<4>().ok()?;
    system.part_end_color = reader.take_array::<4>().ok()?;
    let start_scale_x = unpack_fixed_u8(reader, 5)?;
    let start_scale_y = unpack_fixed_u8(reader, 5)?;
    let end_scale_x = unpack_fixed_u8(reader, 5)?;
    let end_scale_y = unpack_fixed_u8(reader, 5)?;
    system.part_start_scale = [start_scale_x, start_scale_y];
    system.part_end_scale = [end_scale_x, end_scale_y];
    Some(())
}

/// Reads an unsigned fixed-point value packed in a `u16` (the viewer's
/// `unpackFixed(false, int_bits, frac_bits)` with `int_bits + frac_bits == 16`):
/// the raw integer divided by `2^frac_bits`.
fn unpack_fixed_u16(reader: &mut Reader<'_>, frac_bits: u32) -> Option<f32> {
    let raw = reader.u16().ok()?;
    Some(f32::from(raw) / pow2(frac_bits))
}

/// Reads an unsigned fixed-point value packed in a `u8` (the viewer's
/// `unpackFixed(false, int_bits, frac_bits)` with `int_bits + frac_bits == 8`).
fn unpack_fixed_u8(reader: &mut Reader<'_>, frac_bits: u32) -> Option<f32> {
    let raw = reader.u8().ok()?;
    Some(f32::from(raw) / pow2(frac_bits))
}

/// Reads three signed fixed-point components packed in `u16`s (the viewer's
/// `unpackFixed(true, int_bits, frac_bits)`): the raw integer divided by
/// `2^frac_bits`, then offset by `-2^int_bits` to recover the sign.
fn unpack_fixed_vector_signed(
    reader: &mut Reader<'_>,
    int_bits: u32,
    frac_bits: u32,
) -> Option<Vector> {
    let bias = pow2(int_bits);
    let scale = pow2(frac_bits);
    let mut read = || -> Option<f32> {
        let raw = reader.u16().ok()?;
        Some(f32::from(raw) / scale - bias)
    };
    let x = read()?;
    let y = read()?;
    let z = read()?;
    Some(Vector { x, y, z })
}

/// `2^exp` as an `f32`, for the fixed-point divisors/biases (small exponents, so
/// the value is exact). Avoids the crate's lint against `1 << n as f32` casts.
fn pow2(exp: u32) -> f32 {
    let mut value = 1.0_f32;
    let mut count = 0_u32;
    while count < exp {
        value *= 2.0;
        count = count.saturating_add(1);
    }
    value
}

/// Encodes a [`TextureAnimation`] into its raw 16-byte `TextureAnim` blob — the
/// inverse of [`decode_texture_anim`] and a port of the reference viewer's
/// `LLTextureAnim::packTAMessage` (`lltextureanim.cpp`): the four header bytes
/// (mode, face, grid x, grid y) followed by the three little-endian `F32`s
/// (start, length, rate). The grid dimensions are written verbatim — the
/// decoder, not the encoder, applies the non-`SMOOTH` floor-to-1, so a value
/// re-read after encoding matches the viewer.
#[must_use]
pub fn encode_texture_anim(anim: &TextureAnimation) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.put_u8(anim.mode);
    writer.put_i8(anim.face);
    writer.put_u8(anim.size_x);
    writer.put_u8(anim.size_y);
    writer.put_f32(anim.start);
    writer.put_f32(anim.length);
    writer.put_f32(anim.rate);
    writer.into_bytes()
}

/// Encodes a [`ParticleSystem`] into its raw `PSBlock` blob — the inverse of
/// [`decode_particle_system`].
///
/// Chooses the wire form the way the decoder distinguishes them: a system that
/// carries no trailing glow or blend-func data (neither `LL_PART_DATA_GLOW`
/// nor `LL_PART_DATA_BLEND` set in `part_flags`) is emitted as the legacy
/// fixed 86-byte form — the 68-byte source block immediately followed by the
/// 18-byte legacy particle block, with no size prefixes; otherwise the modern
/// form is emitted, prefixing the source and particle sub-blocks with their
/// `S32` sizes and appending the glow / blend-func bytes gated by those flags
/// (mirroring `LLPartSysData::isLegacyCompatible` and the server's pack). All
/// fixed-point fields are re-quantized with the exact inverse of the decoder's
/// `unpackFixed` (`LLDataPacker::packFixed`): clamp to range, scale by
/// `2^frac_bits`, and truncate toward zero.
#[must_use]
pub fn encode_particle_system(system: &ParticleSystem) -> Vec<u8> {
    let mut writer = Writer::new();
    let has_glow = system.part_flags & LL_PART_DATA_GLOW != 0;
    let has_blend = system.part_flags & LL_PART_DATA_BLEND != 0;
    if !has_glow && !has_blend {
        // Legacy 86-byte form: the two sub-blocks back to back, no size prefixes.
        encode_system(&mut writer, system);
        encode_legacy_part(&mut writer, system);
        return writer.into_bytes();
    }
    // Modern form: each sub-block prefixed with its S32 size.
    writer.put_u32(u32::try_from(PS_SYS_DATA_BLOCK_SIZE).unwrap_or(u32::MAX));
    encode_system(&mut writer, system);
    // The particle block's declared size covers the legacy fields plus whichever
    // of the optional glow / blend fields the flags request.
    let mut part_size = PS_LEGACY_PART_DATA_BLOCK_SIZE;
    if has_glow {
        part_size = part_size.saturating_add(2);
    }
    if has_blend {
        part_size = part_size.saturating_add(2);
    }
    writer.put_u32(u32::try_from(part_size).unwrap_or(u32::MAX));
    encode_legacy_part(&mut writer, system);
    if has_glow {
        writer.put_u8(trunc_to_u8(system.part_start_glow * 255.0));
        writer.put_u8(trunc_to_u8(system.part_end_glow * 255.0));
    }
    if has_blend {
        writer.put_u8(system.part_blend_func_source);
        writer.put_u8(system.part_blend_func_dest);
    }
    writer.into_bytes()
}

/// Encodes the 68-byte particle source block (the inverse of [`decode_system`],
/// a port of `LLPartSysData::packSystem`). The unsigned scalar fields are 8.8
/// (`max_age`/`start_age`/the burst fields) or 3.5 (`inner_angle`/`outer_angle`)
/// fixed-point; the angular-velocity and acceleration vectors are signed 8.7
/// fixed-point.
fn encode_system(writer: &mut Writer, system: &ParticleSystem) {
    writer.put_u32(system.crc);
    writer.put_u32(system.flags);
    writer.put_u8(system.pattern);
    pack_fixed_u16(writer, system.max_age, 8, 8);
    pack_fixed_u16(writer, system.start_age, 8, 8);
    pack_fixed_u8(writer, system.inner_angle, 3, 5);
    pack_fixed_u8(writer, system.outer_angle, 3, 5);
    pack_fixed_u16(writer, system.burst_rate, 8, 8);
    pack_fixed_u16(writer, system.burst_radius, 8, 8);
    pack_fixed_u16(writer, system.burst_speed_min, 8, 8);
    pack_fixed_u16(writer, system.burst_speed_max, 8, 8);
    writer.put_u8(system.burst_part_count);
    pack_fixed_vector_signed(writer, &system.angular_velocity, 8, 7);
    pack_fixed_vector_signed(writer, &system.acceleration, 8, 7);
    writer.put_uuid(system.texture_id);
    writer.put_uuid(system.target_id.uuid());
}

/// Encodes the 18-byte legacy particle-template block (the inverse of
/// [`decode_legacy_part`], a port of `LLPartData::packLegacy`): the flags, the
/// 8.8 fixed-point max age, the two RGBA colours verbatim, and the four 3.5
/// fixed-point start/end scale components.
fn encode_legacy_part(writer: &mut Writer, system: &ParticleSystem) {
    writer.put_u32(system.part_flags);
    pack_fixed_u16(writer, system.part_max_age, 8, 8);
    writer.bytes(&system.part_start_color);
    writer.bytes(&system.part_end_color);
    pack_fixed_u8(writer, system.part_start_scale[0], 3, 5);
    pack_fixed_u8(writer, system.part_start_scale[1], 3, 5);
    pack_fixed_u8(writer, system.part_end_scale[0], 3, 5);
    pack_fixed_u8(writer, system.part_end_scale[1], 3, 5);
}

/// Writes an unsigned fixed-point value as a `u16` (the inverse of
/// [`unpack_fixed_u16`], the viewer's `packFixed(false, int_bits, frac_bits)`
/// with `int_bits + frac_bits == 16`): clamp to `0..=2^int_bits`, scale by
/// `2^frac_bits`, and truncate toward zero.
fn pack_fixed_u16(writer: &mut Writer, value: f32, int_bits: u32, frac_bits: u32) {
    let max_val = pow2(int_bits);
    let clamped = value.clamp(0.0, max_val);
    writer.put_u16(trunc_to_u16(clamped * pow2(frac_bits)));
}

/// Writes an unsigned fixed-point value as a `u8` (the inverse of
/// [`unpack_fixed_u8`], `packFixed(false, int_bits, frac_bits)` with
/// `int_bits + frac_bits == 8`).
fn pack_fixed_u8(writer: &mut Writer, value: f32, int_bits: u32, frac_bits: u32) {
    let max_val = pow2(int_bits);
    let clamped = value.clamp(0.0, max_val);
    writer.put_u8(trunc_to_u8(clamped * pow2(frac_bits)));
}

/// Writes three signed fixed-point components as `u16`s (the inverse of
/// [`unpack_fixed_vector_signed`], `packFixed(true, int_bits, frac_bits)`): clamp
/// each component to `±2^int_bits`, bias by `+2^int_bits` to make it unsigned,
/// scale by `2^frac_bits`, and truncate toward zero.
fn pack_fixed_vector_signed(writer: &mut Writer, value: &Vector, int_bits: u32, frac_bits: u32) {
    let max_val = pow2(int_bits);
    let scale = pow2(frac_bits);
    let mut write = |component: f32| {
        let clamped = component.clamp(-max_val, max_val);
        writer.put_u16(trunc_to_u16((clamped + max_val) * scale));
    };
    write(value.x);
    write(value.y);
    write(value.z);
}

/// Truncates a non-negative `f32` toward zero into a `u8`, the inverse of the
/// fixed-point decoders' `raw / 2^frac_bits` de-quantization. The `as` cast
/// saturates rather than wraps, which only differs from the viewer's `(U8)` cast
/// at the exact clamp boundary (a value the inputs are pre-clamped to reach only
/// from the maximum, where the difference is a single quantum).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value pre-clamped to 0..=255; truncate-toward-zero matches LL's (U8) cast"
)]
const fn trunc_to_u8(value: f32) -> u8 {
    value as u8
}

/// Truncates a non-negative `f32` toward zero into a `u16`, the inverse of the
/// 16-bit fixed-point de-quantization. As with [`trunc_to_u8`], the saturating
/// `as` cast only differs from the viewer's wrapping `(U16)` at the clamp
/// boundary.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value pre-clamped to 0..=65535; truncate-toward-zero matches LL's (U16) cast"
)]
const fn trunc_to_u16(value: f32) -> u16 {
    value as u16
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::ObjectKey;
    use sl_types::lsl::Vector;
    use uuid::Uuid;

    use super::{
        LL_PART_DATA_BLEND, LL_PART_DATA_GLOW, PS_LEGACY_DATA_BLOCK_SIZE, PS_SYS_DATA_BLOCK_SIZE,
        decode_particle_system, decode_texture_anim, encode_particle_system, encode_texture_anim,
    };
    use crate::types::{ParticleSystem, TextureAnimation, particle_pattern, texture_anim_mode};

    type TestError = Box<dyn core::error::Error>;

    /// Asserts two `f32`s are equal to within a small tolerance (the crate lints
    /// against direct float comparison).
    fn close(actual: f32, expected: f32) -> Result<(), TestError> {
        if (actual - expected).abs() < 1e-4 {
            Ok(())
        } else {
            Err(format!("{actual} != {expected}").into())
        }
    }

    /// Appends `value` as `width` little-endian bytes (avoiding the crate's
    /// endian-byte-method lint).
    fn push_le(out: &mut Vec<u8>, value: u32, width: u32) {
        let mut emitted = 0_u32;
        while emitted < width {
            let shift = emitted.saturating_mul(8);
            out.push(u8::try_from((value >> shift) & 0xFF).unwrap_or(0));
            emitted = emitted.saturating_add(1);
        }
    }

    /// Appends an `f32` as four little-endian bytes.
    fn push_f32(out: &mut Vec<u8>, value: f32) {
        push_le(out, value.to_bits(), 4);
    }

    #[test]
    fn texture_anim_decodes_the_16_byte_block() -> Result<(), TestError> {
        let mut blob = vec![
            texture_anim_mode::ON | texture_anim_mode::LOOP,
            0xFF, // face = -1 (all faces)
            4,    // size_x
            2,    // size_y
        ];
        push_f32(&mut blob, 1.0); // start
        push_f32(&mut blob, 8.0); // length
        push_f32(&mut blob, 12.0); // rate
        assert_eq!(blob.len(), 16);
        let anim = decode_texture_anim(&blob).ok_or("a 16-byte block decodes")?;
        assert_eq!(anim.mode, texture_anim_mode::ON | texture_anim_mode::LOOP);
        assert_eq!(anim.face, -1);
        assert_eq!(anim.size_x, 4);
        assert_eq!(anim.size_y, 2);
        close(anim.start, 1.0)?;
        close(anim.length, 8.0)?;
        close(anim.rate, 12.0)?;
        Ok(())
    }

    #[test]
    fn texture_anim_rejects_a_wrong_size_blob_and_floors_nonsmooth_grid() -> Result<(), TestError> {
        // Empty (no animation) and short blobs do not decode.
        assert!(decode_texture_anim(&[]).is_none());
        assert!(decode_texture_anim(&[0_u8; 15]).is_none());
        // A non-SMOOTH animation with a 0×0 grid is floored to 1×1 (the viewer's
        // llmax), while SMOOTH keeps the zeros.
        let stepped = [0_u8; 16];
        let anim = decode_texture_anim(&stepped).ok_or("a 16-byte block decodes")?;
        assert_eq!((anim.size_x, anim.size_y), (1, 1));
        let mut smooth = vec![texture_anim_mode::SMOOTH, 0, 0, 0];
        smooth.extend_from_slice(&[0_u8; 12]);
        let anim = decode_texture_anim(&smooth).ok_or("a 16-byte block decodes")?;
        assert_eq!((anim.size_x, anim.size_y), (0, 0));
        Ok(())
    }

    /// Builds the 68-byte particle source block with the angular-velocity and
    /// acceleration set to zero (raw `0x8000` → `0.0` for a signed 8.7 fixed) and
    /// the given scalar/uuid fields, then returns it.
    fn build_system_block(crc: u32, flags: u32, image: Uuid, target: Uuid) -> Vec<u8> {
        let mut out = Vec::new();
        push_le(&mut out, crc, 4);
        push_le(&mut out, flags, 4);
        out.push(particle_pattern::EXPLODE); // pattern
        push_le(&mut out, 256, 2); // max_age = 256/256 = 1.0 (8.8 fixed)
        push_le(&mut out, 0, 2); // start_age = 0.0
        out.push(32); // inner_angle = 32/32 = 1.0 (3.5 fixed)
        out.push(64); // outer_angle = 64/32 = 2.0
        push_le(&mut out, 128, 2); // burst_rate = 128/256 = 0.5
        push_le(&mut out, 512, 2); // burst_radius = 512/256 = 2.0
        push_le(&mut out, 256, 2); // burst_speed_min = 1.0
        push_le(&mut out, 1024, 2); // burst_speed_max = 4.0
        out.push(10); // burst_part_count
        for _ in 0..6 {
            push_le(&mut out, 0x8000, 2); // angvel/accel components = 0.0
        }
        out.extend_from_slice(image.as_bytes());
        out.extend_from_slice(target.as_bytes());
        out
    }

    /// Builds the 18-byte legacy particle-template block.
    fn build_legacy_part_block(flags: u32) -> Vec<u8> {
        let mut out = Vec::new();
        push_le(&mut out, flags, 4);
        push_le(&mut out, 2560, 2); // part_max_age = 2560/256 = 10.0
        out.extend_from_slice(&[255, 0, 0, 255]); // start color (red)
        out.extend_from_slice(&[0, 0, 255, 128]); // end color (blue, half alpha)
        out.push(32); // start scale x = 1.0
        out.push(64); // start scale y = 2.0
        out.push(96); // end scale x = 3.0
        out.push(0); // end scale y = 0.0
        out
    }

    #[test]
    fn particle_system_decodes_the_legacy_86_byte_form() -> Result<(), TestError> {
        let image = Uuid::from_u128(0x1234);
        let target = Uuid::from_u128(0x5678);
        let mut blob = build_system_block(0xABCD, 0x02, image, target);
        blob.extend_from_slice(&build_legacy_part_block(0x11));
        assert_eq!(blob.len(), PS_LEGACY_DATA_BLOCK_SIZE);

        let ps = decode_particle_system(&blob).ok_or("legacy form decodes")?;
        assert_eq!(ps.crc, 0xABCD);
        assert_eq!(ps.flags, 0x02);
        assert_eq!(ps.pattern, particle_pattern::EXPLODE);
        close(ps.max_age, 1.0)?;
        close(ps.inner_angle, 1.0)?;
        close(ps.outer_angle, 2.0)?;
        close(ps.burst_rate, 0.5)?;
        close(ps.burst_radius, 2.0)?;
        close(ps.burst_speed_min, 1.0)?;
        close(ps.burst_speed_max, 4.0)?;
        assert_eq!(ps.burst_part_count, 10);
        close(ps.angular_velocity.x, 0.0)?;
        close(ps.acceleration.z, 0.0)?;
        assert_eq!(ps.texture_id, image);
        assert_eq!(ps.target_id, ObjectKey::from(target));
        assert_eq!(ps.part_flags, 0x11);
        close(ps.part_max_age, 10.0)?;
        assert_eq!(ps.part_start_color, [255, 0, 0, 255]);
        assert_eq!(ps.part_end_color, [0, 0, 255, 128]);
        close(ps.part_start_scale[0], 1.0)?;
        close(ps.part_start_scale[1], 2.0)?;
        close(ps.part_end_scale[0], 3.0)?;
        close(ps.part_end_scale[1], 0.0)?;
        // No glow/blend flags → defaults.
        close(ps.part_start_glow, 0.0)?;
        assert_eq!(ps.part_blend_func_source, 7);
        assert_eq!(ps.part_blend_func_dest, 9);
        Ok(())
    }

    #[test]
    fn particle_system_decodes_the_modern_form_with_glow_and_blend() -> Result<(), TestError> {
        let image = Uuid::from_u128(0x9);
        let target = Uuid::nil();
        let part_flags = 0x01 | LL_PART_DATA_GLOW | LL_PART_DATA_BLEND;
        let mut blob = Vec::new();
        // S32 source size, then the source block.
        push_le(
            &mut blob,
            u32::try_from(PS_SYS_DATA_BLOCK_SIZE).unwrap_or(0),
            4,
        );
        blob.extend_from_slice(&build_system_block(0x1, 0x0, image, target));
        // S32 particle size (legacy 18 + glow 2 + blend 2 = 22), then the block.
        push_le(&mut blob, 22, 4);
        blob.extend_from_slice(&build_legacy_part_block(part_flags));
        blob.push(255); // start glow → 1.0
        blob.push(128); // end glow → ~0.5
        blob.push(3); // blend source func
        blob.push(5); // blend dest func

        let ps = decode_particle_system(&blob).ok_or("modern form decodes")?;
        assert_eq!(ps.part_flags, part_flags);
        close(ps.part_start_glow, 1.0)?;
        close(ps.part_end_glow, 128.0 / 255.0)?;
        assert_eq!(ps.part_blend_func_source, 3);
        assert_eq!(ps.part_blend_func_dest, 5);
        Ok(())
    }

    #[test]
    fn particle_system_rejects_empty_and_bad_modern_size() {
        assert!(decode_particle_system(&[]).is_none());
        // A modern blob (not 86 bytes) whose declared source size is wrong.
        let mut blob = Vec::new();
        push_le(&mut blob, 999, 4);
        blob.extend_from_slice(&[0_u8; 20]);
        assert!(decode_particle_system(&blob).is_none());
    }

    /// Builds a [`ParticleSystem`] whose every fixed-point field holds a value
    /// that is exactly representable on the wire grid (8.8, 3.5 and signed 8.7),
    /// so an `encode` → `decode` round trip is bit-for-bit lossless. `part_flags`
    /// chooses the wire form: with the glow / blend bits set the glow / blend
    /// values below are carried, otherwise they hold their canonical defaults.
    fn exact_system(part_flags: u32) -> ParticleSystem {
        let glow = part_flags & LL_PART_DATA_GLOW != 0;
        let blend = part_flags & LL_PART_DATA_BLEND != 0;
        ParticleSystem {
            crc: 0xABCD,
            flags: 0x02,
            pattern: particle_pattern::ANGLE_CONE,
            max_age: 1.0,
            start_age: 0.5,
            inner_angle: 1.0,
            outer_angle: 2.0,
            burst_rate: 0.5,
            burst_radius: 2.0,
            burst_speed_min: 1.0,
            burst_speed_max: 4.0,
            burst_part_count: 10,
            angular_velocity: Vector {
                x: 1.5,
                y: -2.0,
                z: 0.0,
            },
            acceleration: Vector {
                x: 0.0,
                y: -0.5,
                z: 3.0,
            },
            texture_id: Uuid::from_u128(0x1234),
            target_id: ObjectKey::from(Uuid::from_u128(0x5678)),
            part_flags,
            part_max_age: 10.0,
            part_start_color: [255, 0, 0, 255],
            part_end_color: [0, 0, 255, 128],
            part_start_scale: [1.0, 2.0],
            part_end_scale: [3.0, 0.5],
            // Glow bytes 200 / 128 re-quantize losslessly (see the quantization
            // check); when the flag is clear the decoder yields the 0.0 default.
            part_start_glow: if glow { 200.0 / 255.0 } else { 0.0 },
            part_end_glow: if glow { 128.0 / 255.0 } else { 0.0 },
            part_blend_func_source: if blend { 3 } else { 7 },
            part_blend_func_dest: if blend { 5 } else { 9 },
        }
    }

    #[test]
    fn texture_anim_encode_is_the_inverse_of_decode() -> Result<(), TestError> {
        // decode → encode reproduces the original 16-byte block exactly.
        let mut blob = vec![
            texture_anim_mode::ON | texture_anim_mode::SMOOTH,
            0xFF, // face = -1
            4,
            2,
        ];
        push_f32(&mut blob, 1.0);
        push_f32(&mut blob, 8.0);
        push_f32(&mut blob, 12.0);
        let anim = decode_texture_anim(&blob).ok_or("a 16-byte block decodes")?;
        assert_eq!(encode_texture_anim(&anim), blob);

        // encode → decode preserves a hand-built animation value.
        let original = TextureAnimation {
            mode: texture_anim_mode::ON | texture_anim_mode::ROTATE,
            face: 3,
            size_x: 7,
            size_y: 1,
            start: -1.5,
            length: 2.25,
            rate: 0.5,
        };
        let encoded = encode_texture_anim(&original);
        assert_eq!(encoded.len(), 16);
        let decoded = decode_texture_anim(&encoded).ok_or("re-decodes")?;
        assert_eq!(decoded, original);
        Ok(())
    }

    #[test]
    fn particle_system_legacy_round_trips() -> Result<(), TestError> {
        // No glow / blend flags → the legacy fixed 86-byte form, no size prefixes.
        let system = exact_system(0x01);
        let blob = encode_particle_system(&system);
        assert_eq!(blob.len(), PS_LEGACY_DATA_BLOCK_SIZE);
        let decoded = decode_particle_system(&blob).ok_or("legacy form re-decodes")?;
        assert_eq!(decoded, system);
        Ok(())
    }

    #[test]
    fn particle_system_modern_round_trips_with_glow_and_blend() -> Result<(), TestError> {
        let part_flags = 0x01 | LL_PART_DATA_GLOW | LL_PART_DATA_BLEND;
        let system = exact_system(part_flags);
        let blob = encode_particle_system(&system);
        // Modern form: 8 bytes of S32 size prefixes + glow (2) + blend (2) over the
        // 86-byte legacy payload.
        assert_eq!(blob.len(), PS_LEGACY_DATA_BLOCK_SIZE + 8 + 4);
        let decoded = decode_particle_system(&blob).ok_or("modern form re-decodes")?;
        assert_eq!(decoded, system);

        // Only the glow flag → glow is carried but blend is not (and the form is
        // still modern because a trailing field is present).
        let glow_only = exact_system(0x01 | LL_PART_DATA_GLOW);
        let blob = encode_particle_system(&glow_only);
        assert_eq!(blob.len(), PS_LEGACY_DATA_BLOCK_SIZE + 8 + 2);
        assert_eq!(
            decode_particle_system(&blob).ok_or("glow-only re-decodes")?,
            glow_only
        );
        Ok(())
    }

    #[test]
    fn particle_system_decode_encode_is_idempotent_over_a_hand_built_blob() -> Result<(), TestError>
    {
        // The hand-built modern blob from the decode test re-encodes byte-for-byte
        // (every field in the builders is exactly representable on the wire grid).
        let image = Uuid::from_u128(0x9);
        let target = Uuid::nil();
        let part_flags = 0x01 | LL_PART_DATA_GLOW | LL_PART_DATA_BLEND;
        let mut blob = Vec::new();
        push_le(
            &mut blob,
            u32::try_from(PS_SYS_DATA_BLOCK_SIZE).unwrap_or(0),
            4,
        );
        blob.extend_from_slice(&build_system_block(0x1, 0x0, image, target));
        push_le(&mut blob, 22, 4);
        blob.extend_from_slice(&build_legacy_part_block(part_flags));
        blob.push(255);
        blob.push(128);
        blob.push(3);
        blob.push(5);

        let decoded = decode_particle_system(&blob).ok_or("hand-built blob decodes")?;
        assert_eq!(encode_particle_system(&decoded), blob);
        Ok(())
    }
}
