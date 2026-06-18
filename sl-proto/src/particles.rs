//! Structured decoders for a prim's texture-animation (`TextureAnim`) and
//! particle-system (`PSBlock`) blobs.
//!
//! Both are carried on an `ObjectUpdate` as raw byte fields and retained verbatim
//! on [`Object`](crate::Object); this module decodes them into the typed
//! [`TextureAnimation`] and [`ParticleSystem`] value types, mirroring the
//! reference viewer's `LLTextureAnim::unpackTAMessage` (`lltextureanim.cpp`) and
//! `LLPartSysData::unpackBlock` (`llpartdata.cpp`).

use sl_types::lsl::Vector;
use sl_wire::Reader;

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
        target_id,
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        LL_PART_DATA_BLEND, LL_PART_DATA_GLOW, PS_LEGACY_DATA_BLOCK_SIZE, PS_SYS_DATA_BLOCK_SIZE,
        decode_particle_system, decode_texture_anim,
    };
    use crate::types::{particle_pattern, texture_anim_mode};

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
        assert_eq!(ps.target_id, target);
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
}
