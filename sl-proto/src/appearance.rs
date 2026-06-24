//! Decoder and encoder for the packed `TextureEntry` blob carried by
//! `AvatarAppearance` (and `ObjectUpdate`).
//!
//! A `TextureEntry` stores eleven per-face fields — texture id, tint colour,
//! texture repeats/offsets/rotation, bump/shiny/fullbright, media flags, glow,
//! and material id — in a compact run-length form: each field is a default
//! value followed by `(face bitmask, value)` overrides terminated by a zero
//! byte. [`decode_texture_entry`] is a faithful port of the reference viewer's
//! `LLPrimitive::parseTEMessage` / `unpack_TEField` / `applyParsedTEMessage`
//! and [`encode_texture_entry`] of its inverse `LLPrimitive::packTEMessage` /
//! `packTEField` (`indra/llprimitive/llprimitive.cpp`), which agree with
//! OpenSim's `Primitive.Textures` packing.
//!
//! Integer shifts in the variable-length bitmask use `wrapping_shl` purely to
//! satisfy the crate's `arithmetic_side_effects` lint: the face count is capped
//! at [`MAX_FACES`] (≤ 64), so no shift can actually wrap. Floating-point
//! conversions are exact for the small integers involved.

use sl_types::key::TextureKey;
use sl_wire::{Reader, Writer};
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
            texture_id: TextureKey::from(texture_id.get(index).copied().unwrap_or_default()),
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
            material_id: material
                .get(index)
                .copied()
                .and_then(|id| (!id.is_nil()).then_some(id)),
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

/// Re-inverts a colour for the wire: the inverse of [`uninvert_color`]. The wire
/// stores `255 - channel` (so opaque white packs as all-zero), so the wire RGBA
/// is `255 - byte`.
const fn invert_color(rgba: [u8; 4]) -> [u8; 4] {
    [
        255_u8.wrapping_sub(rgba[0]),
        255_u8.wrapping_sub(rgba[1]),
        255_u8.wrapping_sub(rgba[2]),
        255_u8.wrapping_sub(rgba[3]),
    ]
}

/// Rounds a pre-clamped `f32` to an `i16` — the inverse of [`i16_to_f32`]
/// division. The cast lints are expected: the caller pre-clamps the value into
/// the `s16` range, and rounding before the (saturating) cast matches the
/// viewer's `ll_round`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "value pre-clamped to s16 range; round-then-cast matches LL's ll_round"
)]
const fn round_to_i16(value: f32) -> i16 {
    value.round() as i16
}

/// Rounds a pre-clamped `f32` (in `0..=255`) to a `u8` — the inverse of the
/// decoder's `u8 / 255` glow conversion. The cast lints are expected: the value
/// is pre-clamped non-negative and `≤ 255`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value pre-clamped to 0..=255; round-then-cast matches LL's ll_round"
)]
const fn round_to_u8(value: f32) -> u8 {
    value.round() as u8
}

/// Re-quantizes a texture offset to its packed `s16` — the inverse of the
/// decoder's `i16 / OFFSET_PACK_FACTOR`, matching the viewer's
/// `ll_round(clamp(offset, -1, 1) * 0x7FFF)`.
fn pack_offset(offset: f32) -> i16 {
    round_to_i16(offset.clamp(-1.0, 1.0) * OFFSET_PACK_FACTOR)
}

/// Re-quantizes a texture rotation (radians) to its packed `s16` — the inverse
/// of the decoder's `i16 / ROTATION_PACK_FACTOR * TAU`, matching the viewer's
/// `ll_round(fmod(rotation, 2π) / 2π * 0x8000)`.
fn pack_rotation(rotation: f32) -> i16 {
    let wrapped = rotation % core::f32::consts::TAU;
    round_to_i16(wrapped / core::f32::consts::TAU * ROTATION_PACK_FACTOR)
}

/// Re-quantizes a glow amount to its packed `u8` — the inverse of the decoder's
/// `u8 / 255`, matching the viewer's `ll_round(clamp(glow, 0, 1) * 0xFF)`.
fn pack_glow(glow: f32) -> u8 {
    round_to_u8(glow.clamp(0.0, 1.0) * 255.0)
}

/// Encodes a [`TextureEntry`] into the packed wire blob — the inverse of
/// [`decode_texture_entry`]. Returns an empty blob for an entry with no faces
/// (which decodes back to an empty entry), and otherwise emits the eleven
/// run-length fields in wire order (texture id, tint colour, scale S/T, offset
/// S/T, rotation, bump/shiny/fullbright, media, glow, material id), each — bar
/// the trailing material field, which the decoder self-terminates — followed by
/// the zero bitmask that terminates it.
///
/// Faithfully mirrors the reference viewer's `LLPrimitive::packTEMessage`: the
/// per-face natural-unit values are re-quantized to their wire forms (colour
/// re-inverted to `255 - channel`, offsets/rotation/glow re-quantized with the
/// same pack factors the decoder divides by), then each field is run-length
/// packed (default value plus `(face bitmask, value)` overrides). Faces beyond
/// [`MAX_FACES`] are dropped (the wire face bitmask is only 64 bits wide),
/// matching the decoder's cap.
#[must_use]
pub fn encode_texture_entry(entry: &TextureEntry) -> Vec<u8> {
    let count = entry.faces.len().min(MAX_FACES);
    if count == 0 {
        return Vec::new();
    }
    let faces = entry.faces.get(..count).unwrap_or(&[]);

    // Re-quantize each field into a per-face array, in face-index order, exactly
    // as the viewer's packTEMessage fills its parallel arrays before packing.
    let texture_id: Vec<Uuid> = faces.iter().map(|face| face.texture_id.uuid()).collect();
    let color: Vec<[u8; 4]> = faces.iter().map(|face| invert_color(face.color)).collect();
    let scale_s: Vec<f32> = faces.iter().map(|face| face.scale_s).collect();
    let scale_t: Vec<f32> = faces.iter().map(|face| face.scale_t).collect();
    let offset_s: Vec<i16> = faces
        .iter()
        .map(|face| pack_offset(face.offset_s))
        .collect();
    let offset_t: Vec<i16> = faces
        .iter()
        .map(|face| pack_offset(face.offset_t))
        .collect();
    let rotation: Vec<i16> = faces
        .iter()
        .map(|face| pack_rotation(face.rotation))
        .collect();
    let bump: Vec<u8> = faces
        .iter()
        .map(|face| face.bump_shiny_fullbright)
        .collect();
    let media: Vec<u8> = faces.iter().map(|face| face.media_flags).collect();
    let glow: Vec<u8> = faces.iter().map(|face| pack_glow(face.glow)).collect();
    let material: Vec<Uuid> = faces
        .iter()
        .map(|face| face.material_id.unwrap_or_else(Uuid::nil))
        .collect();

    let mut writer = Writer::new();
    pack_field(&mut writer, &texture_id, Writer::put_uuid);
    writer.put_u8(0);
    pack_field(&mut writer, &color, |w, value| w.bytes(&value));
    writer.put_u8(0);
    pack_field(&mut writer, &scale_s, Writer::put_f32);
    writer.put_u8(0);
    pack_field(&mut writer, &scale_t, Writer::put_f32);
    writer.put_u8(0);
    pack_field(&mut writer, &offset_s, Writer::put_i16);
    writer.put_u8(0);
    pack_field(&mut writer, &offset_t, Writer::put_i16);
    writer.put_u8(0);
    pack_field(&mut writer, &rotation, Writer::put_i16);
    writer.put_u8(0);
    pack_field(&mut writer, &bump, Writer::put_u8);
    writer.put_u8(0);
    pack_field(&mut writer, &media, Writer::put_u8);
    writer.put_u8(0);
    pack_field(&mut writer, &glow, Writer::put_u8);
    writer.put_u8(0);
    // The final (material) field is not zero-terminated on the wire; the decoder
    // appends the terminator itself, so none is written here.
    pack_field(&mut writer, &material, Writer::put_uuid);
    writer.into_bytes()
}

/// Packs one run-length field — the inverse of [`unpack_field`]. Mirrors the
/// viewer's `LLPrimitive::packTEField`: the **last** face's value is written as
/// the default, then faces are scanned from the second-to-last down to the
/// first; each value not already covered by a higher-indexed face is emitted as
/// a `(face bitmask, value)` override whose bitmask flags every face at or below
/// the current index that shares the value. The caller writes the terminating
/// zero bitmask between fields.
fn pack_field<T: Copy + PartialEq>(
    writer: &mut Writer,
    values: &[T],
    write_value: impl Fn(&mut Writer, T),
) {
    let count = values.len().min(MAX_FACES);
    let Some(last_index) = count.checked_sub(1) else {
        return;
    };
    let Some(default) = values.get(last_index).copied() else {
        return;
    };
    write_value(writer, default);

    // face_index walks from last_index - 1 down to 0 inclusive.
    let mut face_index = last_index;
    while let Some(prev) = face_index.checked_sub(1) {
        face_index = prev;
        let Some(value) = values.get(face_index).copied() else {
            continue;
        };
        // Skip if a higher-indexed face already carried this value (it is then
        // covered by that face's default/override or by the field default).
        let already_sent = (face_index.saturating_add(1)..=last_index)
            .any(|index| values.get(index).copied() == Some(value));
        if already_sent {
            continue;
        }
        // Flag every face at or below this index that shares the value.
        let mut exception_faces: u64 = 0;
        for index in 0..=face_index {
            if values.get(index).copied() == Some(value) {
                let bit = u32::try_from(index).map_or(0, |shift| 1_u64.wrapping_shl(shift));
                exception_faces |= bit;
            }
        }
        write_face_bitmask(writer, exception_faces);
        write_value(writer, value);
    }
}

/// Writes a non-zero face bitmask as the variable-length big-endian base-128
/// integer the decoder reassembles: seven bits per byte, most-significant group
/// first, every byte but the last carrying the `0x80` continuation bit.
fn write_face_bitmask(writer: &mut Writer, faces: u64) {
    // Split into 7-bit groups, least-significant first.
    let mut groups: Vec<u8> = Vec::new();
    let mut remaining = faces;
    loop {
        groups.push(u8::try_from(remaining & 0x7F).unwrap_or(0));
        remaining = remaining.wrapping_shr(7);
        if remaining == 0 {
            break;
        }
    }
    // Emit most-significant group first; all but the final group continue.
    let last = groups.len().saturating_sub(1);
    for (position, group) in groups.iter().rev().enumerate() {
        if position < last {
            writer.put_u8(group | 0x80);
        } else {
            writer.put_u8(*group);
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{TextureKey, decode_texture_entry, encode_texture_entry, unpack_field};
    use crate::types::{TextureEntry, TextureFace};
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
        assert_eq!(entry.texture_id(0), Some(TextureKey::from(face0_tex)));
        assert_eq!(entry.texture_id(1), Some(TextureKey::from(default_tex)));
        assert_eq!(entry.texture_id(2), Some(TextureKey::from(default_tex)));
        let face = entry.face(1).ok_or("expected face 1")?;
        assert_eq!(face.color, [255, 255, 255, 255]);
        assert!((face.scale_s - 1.0).abs() < f32::EPSILON);
        assert!(face.offset_s.abs() < f32::EPSILON);
        assert!(face.glow.abs() < f32::EPSILON);
        Ok(())
    }

    /// An empty entry encodes to an empty blob (which decodes back to empty).
    #[test]
    fn encode_empty_entry_is_empty() {
        assert!(encode_texture_entry(&TextureEntry { faces: Vec::new() }).is_empty());
    }

    /// A face built from exactly-representable values, with a shared run across
    /// the last two faces, survives an `encode` → `decode` round trip
    /// byte-for-byte — exercising the default-plus-override packing (the shared
    /// run becomes the default; the differing first face an override), the
    /// colour re-inversion, and the offset/rotation/glow re-quantization.
    #[test]
    fn encode_then_decode_round_trips() {
        let tex_a = Uuid::from_u128(0xaaaa);
        let tex_b = Uuid::from_u128(0xbbbb);
        // face0 differs; faces 1 and 2 share a value (become the field default).
        let face0 = TextureFace {
            texture_id: TextureKey::from(tex_a),
            color: [255, 0, 128, 255],
            scale_s: 2.0,
            scale_t: 0.5,
            // 0.5 * 0x7FFF = 16383.5 rounds to 16384; decode gives 16384/0x7FFF,
            // which re-encodes to 16384 — so go through one decode first below.
            offset_s: 0.0,
            offset_t: 0.0,
            rotation: 0.0,
            bump_shiny_fullbright: 0x21,
            media_flags: 1,
            glow: 0.0,
            material_id: None,
        };
        let shared = TextureFace {
            texture_id: TextureKey::from(tex_b),
            color: [255, 255, 255, 255],
            scale_s: 1.0,
            scale_t: 1.0,
            offset_s: 0.0,
            offset_t: 0.0,
            rotation: 0.0,
            bump_shiny_fullbright: 0,
            media_flags: 0,
            glow: 0.0,
            material_id: None,
        };
        let entry = TextureEntry {
            faces: vec![face0, shared, shared],
        };

        let blob = encode_texture_entry(&entry);
        let decoded = decode_texture_entry(&blob, 3);
        assert_eq!(decoded, entry);
    }

    /// Encoding is idempotent through the codec: decoding a hand-built blob (with
    /// non-trivial quantized offset/rotation/glow and a multi-face override),
    /// re-encoding, and decoding again reproduces the same entry — so the
    /// re-quantization is a true inverse of the decoder's de-quantization for
    /// values already on the wire grid.
    #[test]
    fn re_encode_is_idempotent_on_decoded_values() {
        let default_tex = Uuid::from_u128(0x1111);
        let override_tex = Uuid::from_u128(0x2222);

        let mut writer = Writer::new();
        // Texture: default + faces 0 and 3 override (bitmask 0b1001 = 9).
        writer.put_uuid(default_tex);
        writer.put_u8(0b0000_1001);
        writer.put_uuid(override_tex);
        writer.put_u8(0);
        // Colour: a non-white default tint + terminator.
        writer.put_u8(10);
        writer.put_u8(20);
        writer.put_u8(30);
        writer.put_u8(0);
        writer.put_u8(0);
        // scale_s / scale_t.
        writer.put_f32(1.5);
        writer.put_u8(0);
        writer.put_f32(3.0);
        writer.put_u8(0);
        // offset_s, offset_t, rotation: non-zero quantized values.
        writer.put_i16(8000);
        writer.put_u8(0);
        writer.put_i16(-12000);
        writer.put_u8(0);
        writer.put_i16(4096);
        writer.put_u8(0);
        // bump, media, glow.
        writer.put_u8(0x80);
        writer.put_u8(0);
        writer.put_u8(2);
        writer.put_u8(0);
        writer.put_u8(200);
        writer.put_u8(0);
        // material (final field, not terminated on the wire).
        writer.put_uuid(Uuid::nil());
        let blob = writer.into_bytes();

        let first = decode_texture_entry(&blob, 4);
        let second = decode_texture_entry(&encode_texture_entry(&first), 4);
        assert_eq!(first, second);
    }
}
