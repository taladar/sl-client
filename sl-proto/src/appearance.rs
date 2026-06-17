//! Decoder for the packed `TextureEntry` blob carried by `AvatarAppearance`
//! (and `ObjectUpdate`).
//!
//! A `TextureEntry` stores eleven per-face fields — texture id, tint colour,
//! texture repeats/offsets/rotation, bump/shiny/fullbright, media flags, glow,
//! and material id — in a compact run-length form: each field is a default
//! value followed by `(face bitmask, value)` overrides terminated by a zero
//! byte. This is a faithful port of the reference viewer's
//! `LLPrimitive::parseTEMessage` / `unpack_TEField` / `applyParsedTEMessage`
//! (`indra/llprimitive/llprimitive.cpp`), which agrees with OpenSim's
//! `Primitive.Textures` packing.
//!
//! Integer shifts in the variable-length bitmask use `wrapping_shl` purely to
//! satisfy the crate's `arithmetic_side_effects` lint: the face count is capped
//! at [`MAX_FACES`] (≤ 64), so no shift can actually wrap. Floating-point
//! conversions are exact for the small integers involved.

use sl_wire::Reader;
use uuid::Uuid;

use crate::types::{TextureEntry, TextureFace};

/// The largest number of faces we will decode. The wire face bitmask is at most
/// 64 bits wide (LL's `exception_faces` is a `U64`), and an avatar has 45 slots
/// (`TEX_NUM_INDICES`); cap here so the bit indexing stays valid and a bogus
/// face count cannot drive a large allocation.
pub const MAX_FACES: usize = 64;

/// The texture-rotation pack factor (LL's `TEXTURE_ROTATION_PACK_FACTOR`,
/// `0x8000`): a packed `s16` rotation is `round(rotation / 2π * 0x8000)`.
const ROTATION_PACK_FACTOR: f32 = 32768.0;

/// The texture-offset pack factor (`0x7FFF`): a packed `s16` offset is
/// `round(clamp(offset, -1, 1) * 0x7FFF)`.
const OFFSET_PACK_FACTOR: f32 = 32767.0;

/// Converts a packed `s16` (`|v| ≤ 0x8000`, exact in an `f32`) to `f32`. There
/// is no `From<i16>` for `f32` in const context, so the cast lint is expected;
/// the conversion is exact, so there is no precision loss.
#[expect(
    clippy::as_conversions,
    reason = "i16 (|v| <= 2^15) to f32 is exact; no const From impl"
)]
const fn i16_to_f32(value: i16) -> f32 {
    value as f32
}

/// Converts a `u8` (exact in an `f32`) to `f32`. There is no `From<u8>` for
/// `f32` in const context, so the cast lint is expected.
#[expect(
    clippy::as_conversions,
    reason = "u8 to f32 is exact; no const From impl"
)]
const fn u8_to_f32(value: u8) -> f32 {
    value as f32
}

/// Decodes a packed `TextureEntry` blob into up to `face_count` faces (clamped
/// to [`MAX_FACES`]). Returns an empty entry for an empty blob.
///
/// For an avatar (`AvatarAppearance`) pass
/// [`avatar_texture::COUNT`](crate::avatar_texture::COUNT); the resulting faces
/// are indexed by the [`avatar_texture`](crate::avatar_texture) slot constants.
/// A malformed or truncated blob decodes as far as it can; fields that cannot be
/// read keep their natural defaults (nil texture, opaque-white tint, unit scale,
/// zero offset/rotation/glow).
#[must_use]
pub fn decode_texture_entry(bytes: &[u8], face_count: usize) -> TextureEntry {
    let count = face_count.min(MAX_FACES);
    if bytes.is_empty() || count == 0 {
        return TextureEntry { faces: Vec::new() };
    }

    // The viewer appends a 0x00 so the final (material) field — which is not
    // zero-terminated on the wire — terminates like all the others.
    let mut buf = Vec::with_capacity(bytes.len().saturating_add(1));
    buf.extend_from_slice(bytes);
    buf.push(0);

    // Per-face raw arrays, default-initialised; each field overwrites in place.
    let mut texture_id = vec![Uuid::nil(); count];
    let mut color = vec![[0u8; 4]; count];
    let mut scale_s = vec![1.0_f32; count];
    let mut scale_t = vec![1.0_f32; count];
    let mut offset_s = vec![0_i16; count];
    let mut offset_t = vec![0_i16; count];
    let mut rotation = vec![0_i16; count];
    let mut bump = vec![0_u8; count];
    let mut media = vec![0_u8; count];
    let mut glow = vec![0_u8; count];
    let mut material = vec![Uuid::nil(); count];

    let mut reader = Reader::new(&buf);
    // Fields are decoded in wire order; `&&` short-circuits once a field cannot
    // be read so a truncated blob leaves the remaining fields at their defaults.
    let _decoded = unpack_field(&mut reader, &mut texture_id, |r| r.uuid())
        && unpack_field(&mut reader, &mut color, Reader::take_array::<4>)
        && unpack_field(&mut reader, &mut scale_s, Reader::f32)
        && unpack_field(&mut reader, &mut scale_t, Reader::f32)
        && unpack_field(&mut reader, &mut offset_s, Reader::i16)
        && unpack_field(&mut reader, &mut offset_t, Reader::i16)
        && unpack_field(&mut reader, &mut rotation, Reader::i16)
        && unpack_field(&mut reader, &mut bump, Reader::u8)
        && unpack_field(&mut reader, &mut media, Reader::u8)
        && unpack_field(&mut reader, &mut glow, Reader::u8)
        && unpack_field(&mut reader, &mut material, |r| r.uuid());

    let faces = (0..count)
        .map(|index| TextureFace {
            texture_id: texture_id.get(index).copied().unwrap_or_default(),
            color: uninvert_color(color.get(index).copied().unwrap_or([0; 4])),
            scale_s: scale_s.get(index).copied().unwrap_or(1.0),
            scale_t: scale_t.get(index).copied().unwrap_or(1.0),
            offset_s: i16_to_f32(offset_s.get(index).copied().unwrap_or(0)) / OFFSET_PACK_FACTOR,
            offset_t: i16_to_f32(offset_t.get(index).copied().unwrap_or(0)) / OFFSET_PACK_FACTOR,
            rotation: i16_to_f32(rotation.get(index).copied().unwrap_or(0)) / ROTATION_PACK_FACTOR
                * core::f32::consts::TAU,
            bump_shiny_fullbright: bump.get(index).copied().unwrap_or(0),
            media_flags: media.get(index).copied().unwrap_or(0),
            glow: u8_to_f32(glow.get(index).copied().unwrap_or(0)) / 255.0,
            material_id: material.get(index).copied().unwrap_or_default(),
        })
        .collect();

    TextureEntry { faces }
}

/// Un-inverts a wire colour: the wire stores `255 - channel` (so the common
/// opaque white tint packs as all-zero), so the true RGBA is `255 - byte`.
const fn uninvert_color(raw: [u8; 4]) -> [u8; 4] {
    [
        255_u8.wrapping_sub(raw[0]),
        255_u8.wrapping_sub(raw[1]),
        255_u8.wrapping_sub(raw[2]),
        255_u8.wrapping_sub(raw[3]),
    ]
}

/// Unpacks one run-length field from `reader` into `dest`: reads the default
/// value (filling every slot), then `(face bitmask, value)` overrides until the
/// terminating zero bitmask or the reader runs out. Returns `false` if the
/// reader was exhausted before the field's terminator (a truncated blob).
fn unpack_field<'a, T: Copy>(
    reader: &mut Reader<'a>,
    dest: &mut [T],
    read_value: impl Fn(&mut Reader<'a>) -> Result<T, sl_wire::WireError>,
) -> bool {
    let Ok(default) = read_value(reader) else {
        return false;
    };
    for slot in dest.iter_mut() {
        *slot = default;
    }
    loop {
        // The face bitmask is a variable-length big-endian base-128 integer:
        // seven bits per byte, the high bit marking continuation.
        let mut faces: u64 = 0;
        loop {
            let Ok(byte) = reader.u8() else {
                return false;
            };
            faces = faces.wrapping_shl(7) | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                break;
            }
        }
        if faces == 0 {
            // The terminating zero bitmask: this field is complete.
            return true;
        }
        let Ok(value) = read_value(reader) else {
            return false;
        };
        for (index, slot) in dest.iter_mut().enumerate() {
            let bit = u32::try_from(index).map_or(0, |shift| 1_u64.wrapping_shl(shift));
            if faces & bit != 0 {
                *slot = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{decode_texture_entry, unpack_field};
    use sl_wire::{Reader, Writer};
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Packs a single-field `TextureEntry` body holding one UUID default for all
    /// faces (no overrides), terminated, matching the viewer's `packTEField`.
    #[test]
    fn unpack_field_fills_default_across_faces() {
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
        let mut writer = Writer::new();
        writer.put_uuid(id);
        writer.put_u8(0); // terminating zero bitmask
        let bytes = writer.into_bytes();

        let mut reader = Reader::new(&bytes);
        let mut dest = [Uuid::nil(); 5];
        assert!(unpack_field(&mut reader, &mut dest, |r| r.uuid()));
        assert!(dest.iter().all(|slot| *slot == id));
    }

    /// A `(bitmask, value)` override assigns the value to exactly the flagged
    /// faces and leaves the rest at the default.
    #[test]
    fn unpack_field_applies_face_override() {
        let default_id = Uuid::from_u128(1);
        let override_id = Uuid::from_u128(2);
        let mut writer = Writer::new();
        writer.put_uuid(default_id);
        // Faces 0 and 2 (bitmask 0b101 = 5), single byte (high bit clear).
        writer.put_u8(0b0000_0101);
        writer.put_uuid(override_id);
        writer.put_u8(0); // terminator
        let bytes = writer.into_bytes();

        let mut reader = Reader::new(&bytes);
        let mut dest = [Uuid::nil(); 4];
        assert!(unpack_field(&mut reader, &mut dest, |r| r.uuid()));
        assert_eq!(dest, [override_id, default_id, override_id, default_id]);
    }

    /// An empty blob decodes to an empty entry; a count of zero too.
    #[test]
    fn empty_blob_decodes_empty() {
        assert!(decode_texture_entry(&[], 45).faces.is_empty());
        assert!(decode_texture_entry(&[1, 2, 3], 0).faces.is_empty());
    }

    /// A full round-trip: a texture-only entry (default texture for all faces,
    /// one override) decodes back to the expected per-face texture ids, with the
    /// default surface parameters (opaque-white tint, unit scale, zero offset).
    #[test]
    fn decode_texture_entry_recovers_textures_and_defaults() -> Result<(), TestError> {
        let default_tex = Uuid::from_u128(0xaaaa);
        let face0_tex = Uuid::from_u128(0xbbbb);

        let mut writer = Writer::new();
        // Texture field: default + face-0 override + terminator.
        writer.put_uuid(default_tex);
        writer.put_u8(0b0000_0001); // face 0
        writer.put_uuid(face0_tex);
        writer.put_u8(0);
        // Colour field: all-zero default (= opaque white) + terminator.
        for _ in 0..4 {
            writer.put_u8(0);
        }
        writer.put_u8(0);
        // scale_s / scale_t defaults 1.0 + terminators.
        writer.put_f32(1.0);
        writer.put_u8(0);
        writer.put_f32(1.0);
        writer.put_u8(0);
        // offset_s, offset_t, rotation defaults 0 (i16) + terminators.
        writer.put_i16(0);
        writer.put_u8(0);
        writer.put_i16(0);
        writer.put_u8(0);
        writer.put_i16(0);
        writer.put_u8(0);
        // bump, media, glow defaults 0 (u8) + terminators.
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_u8(0);
        // material default nil (the last field is not terminated on the wire).
        writer.put_uuid(Uuid::nil());
        let bytes = writer.into_bytes();

        let entry = decode_texture_entry(&bytes, 3);
        assert_eq!(entry.faces.len(), 3);
        assert_eq!(entry.texture_id(0), Some(face0_tex));
        assert_eq!(entry.texture_id(1), Some(default_tex));
        assert_eq!(entry.texture_id(2), Some(default_tex));
        let face = entry.face(1).ok_or("expected face 1")?;
        assert_eq!(face.color, [255, 255, 255, 255]);
        assert!((face.scale_s - 1.0).abs() < f32::EPSILON);
        assert!(face.offset_s.abs() < f32::EPSILON);
        assert!(face.glow.abs() < f32::EPSILON);
        Ok(())
    }
}
