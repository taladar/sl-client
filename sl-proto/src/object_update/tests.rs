//! Round-trip tests for the object-update sub-codecs.

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;
use sl_wire::Writer;
use uuid::Uuid;

use super::{
    compressed_object, encode_compressed_object, encode_object_motion, encode_terse_object_data,
    encode_terse_texture_entry, full_object_motion, terse_texture_entry, terse_update,
};

type TestError = Box<dyn core::error::Error>;

/// A non-axis vector for motion fields.
const fn vec(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

#[test]
fn object_motion_round_trips_byte_for_byte() -> Result<(), TestError> {
    // A 60-byte (ordinary object) full-precision motion blob: position,
    // velocity, acceleration, the three packed-quaternion floats, and the
    // angular velocity, all little-endian f32.
    let mut writer = Writer::new();
    writer.put_vector3(&vec(128.5, -7.25, 42.0));
    writer.put_vector3(&vec(1.5, -2.0, 0.25));
    writer.put_vector3(&vec(-0.5, 0.0, 3.0));
    // A rotation whose s is reconstructed non-negative on decode.
    writer.put_f32(0.5);
    writer.put_f32(-0.5);
    writer.put_f32(0.5);
    writer.put_vector3(&vec(0.1, -0.2, 0.3));
    let blob = writer.into_bytes();
    assert_eq!(blob.len(), 60);

    let motion = full_object_motion(&blob);
    assert!(motion.collision_plane.is_none());
    let reencoded = encode_object_motion(&motion);
    assert_eq!(reencoded, blob);
    // Decoding the re-encoded blob yields the same motion.
    assert_eq!(full_object_motion(&reencoded), motion);
    Ok(())
}

#[test]
fn avatar_motion_round_trips_with_collision_plane() -> Result<(), TestError> {
    // A 76-byte avatar motion blob: a 16-byte LLVector4 collision plane
    // prefix then the 60-byte motion.
    let mut writer = Writer::new();
    writer.put_vector4([0.0, 0.0, 1.0, 0.75]);
    writer.put_vector3(&vec(10.0, 20.0, 30.0));
    writer.put_vector3(&vec(0.0, 0.0, 0.0));
    writer.put_vector3(&vec(0.0, 0.0, 0.0));
    writer.put_f32(0.0);
    writer.put_f32(0.0);
    writer.put_f32(0.0);
    writer.put_vector3(&vec(0.0, 0.0, 0.0));
    let blob = writer.into_bytes();
    assert_eq!(blob.len(), 76);

    let motion = full_object_motion(&blob);
    assert_eq!(motion.collision_plane, Some([0.0, 0.0, 1.0, 0.75]));
    let reencoded = encode_object_motion(&motion);
    assert_eq!(reencoded, blob);
    Ok(())
}

#[test]
fn terse_update_round_trips_byte_for_byte() -> Result<(), TestError> {
    // Build a 44-byte object terse Data blob. The 16-bit-quantized fields use
    // grid-point u16 values away from the snap-to-zero quantum (32767/32768),
    // so they re-encode to the same bytes.
    let mut writer = Writer::new();
    writer.put_u32(987_654);
    writer.put_u8(3);
    writer.put_u8(0); // no collision plane
    writer.put_vector3(&vec(64.0, -12.5, 8.0)); // full-precision position
    // velocity (±128), acceleration (±64): grid u16s.
    for q in [10_000_u16, 50_000, 40_000, 20_000, 45_000, 30_000] {
        writer.put_u16(q);
    }
    // rotation x, y, z, s (±1): grid u16s.
    for q in [15_000_u16, 48_000, 22_000, 60_000] {
        writer.put_u16(q);
    }
    // angular velocity (±64): grid u16s.
    for q in [12_000_u16, 52_000, 33_000] {
        writer.put_u16(q);
    }
    let blob = writer.into_bytes();
    assert_eq!(blob.len(), 44);

    let update = terse_update(&blob).ok_or("a 44-byte terse blob decodes")?;
    assert_eq!(update.local_id, 987_654);
    assert_eq!(update.state, 3);
    assert!(update.motion.collision_plane.is_none());
    let reencoded = encode_terse_object_data(&update);
    assert_eq!(reencoded, blob);
    Ok(())
}

#[test]
fn terse_texture_entry_wrapper_round_trips() -> Result<(), TestError> {
    let te = [9_u8, 8, 7, 6, 5, 4, 3, 2, 1];
    let field = encode_terse_texture_entry(&te);
    // Four-byte wrapper (inner length, two zero bytes) then the blob.
    assert_eq!(field.len(), te.len() + 4);
    let recovered = terse_texture_entry(&field).ok_or("a wrapped TE decodes")?;
    assert_eq!(recovered, te);
    // An empty / wrapper-only field carries no texture change.
    assert!(terse_texture_entry(&[]).is_none());
    assert!(terse_texture_entry(&[0, 0, 0, 0]).is_none());
    Ok(())
}

/// Builds a rich `ObjectUpdateCompressed` Data blob exercising the scratchpad
/// data, floating text, media URL, legacy particles, attached sound, parent,
/// name-values, texture animation, and angular-velocity fields.
fn rich_compressed_blob() -> Vec<u8> {
    // SCRATCHPAD | HAS_TEXT | HAS_PARTICLES_LEGACY | HAS_SOUND | HAS_PARENT
    // | TEXTURE_ANIM | HAS_ANGULAR_VELOCITY | HAS_NAME_VALUES | MEDIA_URL
    let cflags: u32 = 0x01 | 0x04 | 0x08 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x200;
    let mut writer = Writer::new();
    writer.put_uuid(Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888));
    writer.put_u32(424_242);
    writer.put_u8(9); // pcode
    writer.put_u8(0); // state
    writer.put_u32(0xDEAD_BEEF);
    writer.put_u8(3); // material
    writer.put_u8(0); // click action
    writer.put_vector3(&vec(2.0, 4.0, 8.0)); // scale
    writer.put_vector3(&vec(100.0, 50.0, 25.0)); // position
    writer.put_f32(0.0); // rotation x
    writer.put_f32(0.0); // rotation y
    writer.put_f32(0.0); // rotation z (s reconstructs to 1.0)
    writer.put_u32(cflags);
    writer.put_uuid(Uuid::from_u128(0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000));
    // HAS_ANGULAR_VELOCITY
    writer.put_vector3(&vec(0.0, 0.0, 1.5));
    // HAS_PARENT
    writer.put_u32(7);
    // SCRATCHPAD data (u32 length then bytes).
    writer.put_u32(2);
    writer.bytes(&[0xAB, 0xCD]);
    // HAS_TEXT: NUL string then RGBA colour.
    writer.bytes(b"hello\0");
    writer.bytes(&[255, 128, 64, 32]);
    // MEDIA_URL: NUL string.
    writer.bytes(b"http://example.com/m\0");
    // HAS_PARTICLES_LEGACY: 86 raw bytes.
    let legacy: Vec<u8> = (0..86_u32)
        .map(|n| u8::try_from(n & 0xFF).unwrap_or(0))
        .collect();
    writer.bytes(&legacy);
    // ExtraParams container: a lone zero count byte (no params).
    writer.put_u8(0);
    // HAS_SOUND: id, gain, flags, radius.
    writer.put_uuid(Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10));
    writer.put_f32(0.5);
    writer.put_u8(2);
    writer.put_f32(20.0);
    // HAS_NAME_VALUES: NUL string.
    writer.bytes(b"AttachItemID STRING RW SV foo\0");
    // Path/profile shape (23 bytes).
    writer.put_u8(16);
    writer.put_u16(0);
    writer.put_u16(50_000);
    writer.put_u8(100);
    writer.put_u8(100);
    writer.put_u8(0);
    writer.put_u8(0);
    writer.put_i8(0);
    writer.put_i8(0);
    writer.put_i8(0);
    writer.put_i8(0);
    writer.put_i8(0);
    writer.put_u8(0);
    writer.put_i8(0);
    writer.put_u8(1);
    writer.put_u16(0);
    writer.put_u16(50_000);
    writer.put_u16(0);
    // Packed texture entry (u32 length then bytes).
    writer.put_u32(5);
    writer.bytes(&[10, 20, 30, 40, 50]);
    // TEXTURE_ANIM (u32 length then a 16-byte block).
    writer.put_u32(16);
    writer.put_u8(1); // mode (non-SMOOTH)
    writer.put_i8(-1); // face
    writer.put_u8(2); // size x
    writer.put_u8(2); // size y
    writer.put_f32(0.0);
    writer.put_f32(4.0);
    writer.put_f32(1.0);
    writer.into_bytes()
}

#[test]
fn compressed_object_round_trips() -> Result<(), TestError> {
    let blob = rich_compressed_blob();
    let object = compressed_object(&blob, 42, 0x55).ok_or("the blob decodes")?;
    // Spot-check a few decoded fields.
    assert_eq!(object.local_id, 424_242);
    assert_eq!(object.parent_id, 7);
    assert_eq!(object.text, "hello");
    assert_eq!(object.media_url, "http://example.com/m");
    assert_eq!(object.data, vec![0xAB, 0xCD]);
    assert_eq!(object.particle_system.len(), 86);
    assert_eq!(object.texture_entry, vec![10, 20, 30, 40, 50]);

    // Re-encoding then decoding yields an identical object, and the blob is
    // reproduced byte-for-byte (the encoder is the exact inverse).
    let reencoded = encode_compressed_object(&object);
    assert_eq!(reencoded, blob);
    let roundtrip = compressed_object(&reencoded, 42, 0x55).ok_or("the re-encoded blob decodes")?;
    assert_eq!(roundtrip, object);
    Ok(())
}

#[test]
fn minimal_compressed_object_round_trips() -> Result<(), TestError> {
    // cflags = 0: only the mandatory ExtraParams container, shape, and
    // texture entry follow the fixed prefix.
    let mut writer = Writer::new();
    writer.put_uuid(Uuid::from_u128(1));
    writer.put_u32(5);
    writer.put_u8(9);
    writer.put_u8(0);
    writer.put_u32(0);
    writer.put_u8(0);
    writer.put_u8(0);
    writer.put_vector3(&vec(1.0, 1.0, 1.0));
    writer.put_vector3(&vec(0.0, 0.0, 0.0));
    writer.put_f32(0.0);
    writer.put_f32(0.0);
    writer.put_f32(0.0);
    writer.put_u32(0); // cflags
    writer.put_uuid(Uuid::nil());
    writer.put_u8(0); // ExtraParams: zero count
    // Shape (23 bytes, all zero is acceptable).
    writer.bytes(&[0_u8; 23]);
    writer.put_u32(0); // texture entry length
    let blob = writer.into_bytes();

    let object = compressed_object(&blob, 1, 0).ok_or("the minimal blob decodes")?;
    let reencoded = encode_compressed_object(&object);
    assert_eq!(reencoded, blob);
    assert_eq!(
        compressed_object(&reencoded, 1, 0).ok_or("the re-encoded blob decodes")?,
        object
    );
    Ok(())
}
