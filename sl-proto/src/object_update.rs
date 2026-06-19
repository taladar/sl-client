//! Packed `ObjectData` / `Data` blob codecs for the object/scene-graph updates
//! (`ObjectUpdate`, `ImprovedTerseObjectUpdate`, `ObjectUpdateCompressed`).
//!
//! The generated LLUDP message codec frames each message and its per-object
//! blocks; the motion and compressed-object payloads inside those blocks are
//! hand-packed binary sub-codecs that live here. Each decoder (the client
//! direction, populating an [`Object`] / [`ObjectMotion`]) is paired with the
//! inverse encoder (the simulator direction, assembling the same wire blob), so
//! a server built on `sl-proto` can *send* the updates a viewer decodes.
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

use sl_types::lsl::{Rotation, Vector};
use sl_wire::{Reader, WireError, Writer};
use uuid::Uuid;

use crate::session::{IDENTITY_ROTATION, ZERO_VECTOR};
use crate::types::{Object, ObjectExtraParams, ObjectMotion, PrimShapeParams};

// ---------------------------------------------------------------------------
// Compressed-update flags & sizes
// ---------------------------------------------------------------------------

/// The `CompressedFlags` bitfield carried in an `ObjectUpdateCompressed` blob,
/// gating which optional fields follow (mirrors LL's `CompressedFlags`).
const COMPRESSED_SCRATCHPAD: u32 = 0x01;
/// The object carries a tree species byte.
const COMPRESSED_TREE: u32 = 0x02;
/// The object has floating text (`llSetText`).
const COMPRESSED_HAS_TEXT: u32 = 0x04;
/// The object has a legacy (≤ 86-byte) particle system block.
const COMPRESSED_HAS_PARTICLES_LEGACY: u32 = 0x08;
/// The object has an attached sound (id, gain, flags, radius follow).
const COMPRESSED_HAS_SOUND: u32 = 0x10;
/// The object is linked to a parent (a `ParentID` follows).
const COMPRESSED_HAS_PARENT: u32 = 0x20;
/// The object has a texture-animation block (after the texture entry).
const COMPRESSED_TEXTURE_ANIM: u32 = 0x40;
/// The object has a non-zero angular velocity (a vector follows).
const COMPRESSED_HAS_ANGULAR_VELOCITY: u32 = 0x80;
/// The object has a name-value pairs string.
const COMPRESSED_HAS_NAME_VALUES: u32 = 0x100;
/// The object has a media URL.
const COMPRESSED_MEDIA_URL: u32 = 0x200;
/// The object has a "new" (> 86-byte) particle system block, appended last.
const COMPRESSED_HAS_PARTICLES_NEW: u32 = 0x400;

/// The fixed byte size of a legacy particle-system block
/// (`PS_LEGACY_DATA_BLOCK_SIZE`: a 68-byte system block plus an 18-byte
/// particle-data block). The block carries no length prefix, so it can only be
/// skipped by its known size.
const COMPRESSED_LEGACY_PARTICLE_SIZE: usize = 86;

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

// ---------------------------------------------------------------------------
// `ImprovedTerseObjectUpdate` motion (`Data`) + wrapped `TextureEntry`
// ---------------------------------------------------------------------------

/// A decoded `ImprovedTerseObjectUpdate` entry: the object's local id, its state
/// byte, and its new motion.
#[derive(Debug, Clone, PartialEq)]
pub struct TerseUpdate {
    /// The object's region-local id.
    pub local_id: u32,
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
        local_id,
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
    writer.put_u32(update.local_id);
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

// ---------------------------------------------------------------------------
// Compressed `ObjectUpdateCompressed` (`Data`)
// ---------------------------------------------------------------------------

/// Reads the compressed update's 23-byte path+profile shape blob — the path
/// block (`LLPathParams`, 16 bytes) then the profile block (`LLProfileParams`, 7
/// bytes), in the simulator's pack order — into a [`PrimShapeParams`]. The wire
/// values are the same raw quantized integers a full update sends as individual
/// fields.
fn read_compressed_shape(reader: &mut Reader<'_>) -> Option<PrimShapeParams> {
    let path_curve = reader.u8().ok()?;
    let path_begin = reader.u16().ok()?;
    let path_end = reader.u16().ok()?;
    let path_scale_x = reader.u8().ok()?;
    let path_scale_y = reader.u8().ok()?;
    let path_shear_x = reader.u8().ok()?;
    let path_shear_y = reader.u8().ok()?;
    let path_twist = reader.i8().ok()?;
    let path_twist_begin = reader.i8().ok()?;
    let path_radius_offset = reader.i8().ok()?;
    let path_taper_x = reader.i8().ok()?;
    let path_taper_y = reader.i8().ok()?;
    let path_revolutions = reader.u8().ok()?;
    let path_skew = reader.i8().ok()?;
    let profile_curve = reader.u8().ok()?;
    let profile_begin = reader.u16().ok()?;
    let profile_end = reader.u16().ok()?;
    let profile_hollow = reader.u16().ok()?;
    Some(PrimShapeParams {
        path_curve,
        profile_curve,
        path_begin,
        path_end,
        path_scale_x,
        path_scale_y,
        path_shear_x,
        path_shear_y,
        path_twist,
        path_twist_begin,
        path_radius_offset,
        path_taper_x,
        path_taper_y,
        path_revolutions,
        path_skew,
        profile_begin,
        profile_end,
        profile_hollow,
    })
}

/// Writes the 23-byte path+profile shape blob — the inverse of
/// [`read_compressed_shape`], in the same path-then-profile pack order.
fn write_compressed_shape(writer: &mut Writer, shape: &PrimShapeParams) {
    writer.put_u8(shape.path_curve);
    writer.put_u16(shape.path_begin);
    writer.put_u16(shape.path_end);
    writer.put_u8(shape.path_scale_x);
    writer.put_u8(shape.path_scale_y);
    writer.put_u8(shape.path_shear_x);
    writer.put_u8(shape.path_shear_y);
    writer.put_i8(shape.path_twist);
    writer.put_i8(shape.path_twist_begin);
    writer.put_i8(shape.path_radius_offset);
    writer.put_i8(shape.path_taper_x);
    writer.put_i8(shape.path_taper_y);
    writer.put_u8(shape.path_revolutions);
    writer.put_i8(shape.path_skew);
    writer.put_u8(shape.profile_curve);
    writer.put_u16(shape.profile_begin);
    writer.put_u16(shape.profile_end);
    writer.put_u16(shape.profile_hollow);
}

/// Decodes the packed `Data` blob of an `ObjectUpdateCompressed` entry into an
/// [`Object`]. The reliable fixed prefix (identity, scale, position, rotation,
/// flags, owner, optional angular velocity / parent / tree, floating text, and
/// media URL) is decoded first; then, best-effort, the trailing length-prefix-
/// less fields the simulator packs in order — legacy particle system,
/// `ExtraParams`, attached sound, name-values, the path/profile shape, the
/// packed texture entry, texture animation, and the "new" particle system —
/// mirroring the reference viewer's `LLViewerObject`/`LLVOVolume`
/// `processUpdateMessage` and OpenSim's `CreateCompressedUpdateBlock`. With the
/// trailing decode the sound, name-values, texture entry, and extra-params are
/// populated, so a compressed update yields the same decoded [`Object`] as a
/// full `ObjectUpdate`. Returns `None` only when the fixed prefix is too short;
/// a malformed tail simply leaves the later fields at their defaults.
pub(crate) fn compressed_object(
    blob: &[u8],
    region_handle: u64,
    update_flags: u32,
) -> Option<Object> {
    let mut reader = Reader::new(blob);
    let full_id = reader.uuid().ok()?;
    let local_id = reader.u32().ok()?;
    let pcode = reader.u8().ok()?;
    let state = reader.u8().ok()?;
    let crc = reader.u32().ok()?;
    let material = reader.u8().ok()?;
    let click_action = reader.u8().ok()?;
    let scale = reader.vector3().ok()?;
    let position = reader.vector3().ok()?;
    // Rotation is a packed quaternion (three floats, w reconstructed).
    let rotation = reader.quaternion().ok()?;
    let cflags = reader.u32().ok()?;
    let owner_id = reader.uuid().ok()?;
    let angular_velocity = if cflags & COMPRESSED_HAS_ANGULAR_VELOCITY != 0 {
        reader.vector3().ok()?
    } else {
        ZERO_VECTOR
    };
    let parent_id = if cflags & COMPRESSED_HAS_PARENT != 0 {
        reader.u32().ok()?
    } else {
        0
    };
    // The generic `Data` field: a tree's genome byte (carried inline under the
    // tree flag) or a scratchpad blob. Captured so the compressed object exposes
    // the same `data` a full update carries in its `Data` field.
    let data = if cflags & COMPRESSED_TREE != 0 {
        vec![reader.u8().ok()?]
    } else if cflags & COMPRESSED_SCRATCHPAD != 0 {
        let size = reader.u32().ok()?;
        reader.take(usize::try_from(size).ok()?).ok()?.to_vec()
    } else {
        Vec::new()
    };
    let (text, text_color) = if cflags & COMPRESSED_HAS_TEXT != 0 {
        let text = read_nul_string(&mut reader)?;
        let color = reader.take_array::<4>().ok()?;
        (text, color)
    } else {
        (String::new(), [0; 4])
    };
    let media_url = if cflags & COMPRESSED_MEDIA_URL != 0 {
        read_nul_string(&mut reader)?
    } else {
        String::new()
    };
    let mut object = Object {
        region_handle,
        local_id,
        full_id,
        parent_id,
        pcode,
        state,
        crc,
        material,
        click_action,
        update_flags,
        scale,
        motion: ObjectMotion {
            position,
            velocity: ZERO_VECTOR,
            acceleration: ZERO_VECTOR,
            rotation,
            angular_velocity,
            // Compressed updates carry no collision plane (avatars use the full
            // or terse path).
            collision_plane: None,
        },
        owner_id,
        sound: Uuid::nil(),
        gain: 0.0,
        sound_flags: 0,
        sound_radius: 0.0,
        text,
        text_color,
        name_value: String::new(),
        media_url,
        texture_entry: Vec::new(),
        texture_anim: Vec::new(),
        texture_animation: None,
        shape: PrimShapeParams::default(),
        particle_system: Vec::new(),
        particles: None,
        data,
        extra: ObjectExtraParams::default(),
        extra_params: Vec::new(),
        properties: None,
        // The deprecated joint fields are not carried by compressed updates.
        joint_type: 0,
        joint_pivot: ZERO_VECTOR,
        joint_axis_or_anchor: ZERO_VECTOR,
    };
    // Best-effort decode of the trailing fields: a short/garbled tail leaves the
    // remaining fields at their defaults rather than discarding the whole object.
    let _ignored = compressed_object_trailing(&mut reader, cflags, &mut object);
    Some(object)
}

/// Decodes the trailing, length-prefix-less fields of an `ObjectUpdateCompressed`
/// blob — packed by the simulator in this fixed order after the media URL — into
/// `object`. Best-effort: the first field that runs short short-circuits the
/// walk (every later field's position depends on the earlier ones being fully
/// read), leaving `object`'s already-decoded fields in place.
fn compressed_object_trailing(
    reader: &mut Reader<'_>,
    cflags: u32,
    object: &mut Object,
) -> Option<()> {
    // Legacy particle system: a fixed-size block with no length prefix.
    if cflags & COMPRESSED_HAS_PARTICLES_LEGACY != 0 {
        object.particle_system = reader.take(COMPRESSED_LEGACY_PARTICLE_SIZE).ok()?.to_vec();
        object.particles = crate::particles::decode_particle_system(&object.particle_system);
    }
    // ExtraParams container (always present, if only as a zero count byte).
    let extra_len = crate::extra_params::extra_params_len(reader.peek_rest());
    let extra_params = reader.take(extra_len).ok()?;
    object.extra = crate::extra_params::decode_extra_params(extra_params);
    object.extra_params = extra_params.to_vec();
    // Attached sound: id, gain, flags, cutoff radius.
    if cflags & COMPRESSED_HAS_SOUND != 0 {
        object.sound = reader.uuid().ok()?;
        object.gain = reader.f32().ok()?;
        object.sound_flags = reader.u8().ok()?;
        object.sound_radius = reader.f32().ok()?;
    }
    // Name-value pairs string.
    if cflags & COMPRESSED_HAS_NAME_VALUES != 0 {
        object.name_value = read_nul_string(reader)?;
    }
    // Path+profile shape parameters: a fixed-size block with no length prefix.
    object.shape = read_compressed_shape(reader)?;
    // Packed texture entry: a little-endian u32 byte length then that many bytes.
    let te_len = usize::try_from(reader.u32().ok()?).ok()?;
    object.texture_entry = reader.take(te_len).ok()?.to_vec();
    // Texture animation: a little-endian u32 byte length then that many bytes.
    if cflags & COMPRESSED_TEXTURE_ANIM != 0 {
        let anim_len = usize::try_from(reader.u32().ok()?).ok()?;
        object.texture_anim = reader.take(anim_len).ok()?.to_vec();
        object.texture_animation = crate::particles::decode_texture_anim(&object.texture_anim);
    }
    // The "new" (> 86-byte) particle system, when present, is the final field —
    // it carries its own internal size, so the rest of the blob is its payload.
    if cflags & COMPRESSED_HAS_PARTICLES_NEW != 0 {
        object.particle_system = reader.take_rest().to_vec();
        object.particles = crate::particles::decode_particle_system(&object.particle_system);
    }
    Some(())
}

/// Computes the `CompressedFlags` bitfield for `object` from which fields it
/// carries — the inverse of how `compressed_object` reads the flags to gate its
/// optional fields. A non-empty generic `data` is always emitted as the
/// scratchpad form (`COMPRESSED_SCRATCHPAD`), never the single-byte tree form;
/// both decode back to the same `data`, so the round trip is lossless. A
/// particle system is classified legacy (`COMPRESSED_HAS_PARTICLES_LEGACY`)
/// when its raw blob is exactly the 86-byte legacy size, else "new".
fn compressed_flags(object: &Object) -> u32 {
    let mut flags = 0_u32;
    if !object.data.is_empty() {
        flags |= COMPRESSED_SCRATCHPAD;
    }
    if !object.text.is_empty() {
        flags |= COMPRESSED_HAS_TEXT;
    }
    if object.particle_system.len() == COMPRESSED_LEGACY_PARTICLE_SIZE {
        flags |= COMPRESSED_HAS_PARTICLES_LEGACY;
    } else if !object.particle_system.is_empty() {
        flags |= COMPRESSED_HAS_PARTICLES_NEW;
    }
    if object.sound != Uuid::nil() {
        flags |= COMPRESSED_HAS_SOUND;
    }
    if object.parent_id != 0 {
        flags |= COMPRESSED_HAS_PARENT;
    }
    if !object.texture_anim.is_empty() {
        flags |= COMPRESSED_TEXTURE_ANIM;
    }
    if object.motion.angular_velocity != ZERO_VECTOR {
        flags |= COMPRESSED_HAS_ANGULAR_VELOCITY;
    }
    if !object.name_value.is_empty() {
        flags |= COMPRESSED_HAS_NAME_VALUES;
    }
    if !object.media_url.is_empty() {
        flags |= COMPRESSED_MEDIA_URL;
    }
    flags
}

/// Encodes an [`Object`] into the packed `Data` blob of an
/// `ObjectUpdateCompressed` entry — the inverse of `compressed_object`. The
/// fixed prefix (identity, scale, position, rotation, the computed
/// `CompressedFlags`, owner, and the flag-gated angular velocity / parent /
/// scratchpad data / floating text / media URL) is written first, then the
/// trailing length-prefix-less fields in the simulator's fixed order: the legacy
/// particle system, the `ExtraParams` container, the attached sound, the
/// name-values, the path/profile shape, the packed texture entry, the texture
/// animation, and the "new" particle system.
///
/// The raw `texture_entry` / `texture_anim` / `particle_system` byte fields are
/// emitted verbatim (a server assembles them with the
/// [`encode_texture_entry`](crate::encode_texture_entry) /
/// [`encode_particle_system`](crate::encode_particle_system) /
/// [`encode_texture_anim`](crate::encode_texture_anim) sub-codecs). The
/// `ExtraParams` container is taken from the raw `extra_params` field when
/// present, and otherwise rebuilt from the decoded
/// [`extra`](Object::extra) via [`encode_extra_params`](crate::encode_extra_params),
/// so the container is always a valid framed block and the trailing fields stay
/// aligned.
#[must_use]
pub fn encode_compressed_object(object: &Object) -> Vec<u8> {
    let mut writer = Writer::new();
    let cflags = compressed_flags(object);
    writer.put_uuid(object.full_id);
    writer.put_u32(object.local_id);
    writer.put_u8(object.pcode);
    writer.put_u8(object.state);
    writer.put_u32(object.crc);
    writer.put_u8(object.material);
    writer.put_u8(object.click_action);
    writer.put_vector3(&object.scale);
    writer.put_vector3(&object.motion.position);
    writer.put_quaternion(&object.motion.rotation);
    writer.put_u32(cflags);
    writer.put_uuid(object.owner_id);
    if cflags & COMPRESSED_HAS_ANGULAR_VELOCITY != 0 {
        writer.put_vector3(&object.motion.angular_velocity);
    }
    if cflags & COMPRESSED_HAS_PARENT != 0 {
        writer.put_u32(object.parent_id);
    }
    // Generic data: the scratchpad form (a u32 length then the bytes).
    if cflags & COMPRESSED_SCRATCHPAD != 0 {
        writer.put_u32(u32::try_from(object.data.len()).unwrap_or(u32::MAX));
        writer.bytes(&object.data);
    }
    if cflags & COMPRESSED_HAS_TEXT != 0 {
        write_nul_string(&mut writer, &object.text);
        writer.bytes(&object.text_color);
    }
    if cflags & COMPRESSED_MEDIA_URL != 0 {
        write_nul_string(&mut writer, &object.media_url);
    }
    // Trailing fields, in the simulator's fixed pack order.
    if cflags & COMPRESSED_HAS_PARTICLES_LEGACY != 0 {
        writer.bytes(&object.particle_system);
    }
    // ExtraParams container: always present (at minimum a zero count byte).
    if object.extra_params.is_empty() {
        writer.bytes(&crate::extra_params::encode_extra_params(&object.extra));
    } else {
        writer.bytes(&object.extra_params);
    }
    if cflags & COMPRESSED_HAS_SOUND != 0 {
        writer.put_uuid(object.sound);
        writer.put_f32(object.gain);
        writer.put_u8(object.sound_flags);
        writer.put_f32(object.sound_radius);
    }
    if cflags & COMPRESSED_HAS_NAME_VALUES != 0 {
        write_nul_string(&mut writer, &object.name_value);
    }
    write_compressed_shape(&mut writer, &object.shape);
    // Packed texture entry: a u32 length then the bytes (always present).
    writer.put_u32(u32::try_from(object.texture_entry.len()).unwrap_or(u32::MAX));
    writer.bytes(&object.texture_entry);
    if cflags & COMPRESSED_TEXTURE_ANIM != 0 {
        writer.put_u32(u32::try_from(object.texture_anim.len()).unwrap_or(u32::MAX));
        writer.bytes(&object.texture_anim);
    }
    // The "new" particle system is the final field (decoded as the rest).
    if cflags & COMPRESSED_HAS_PARTICLES_NEW != 0 {
        writer.bytes(&object.particle_system);
    }
    writer.into_bytes()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::Vector;
    use sl_wire::Writer;
    use uuid::Uuid;

    use super::{
        compressed_object, encode_compressed_object, encode_object_motion,
        encode_terse_object_data, encode_terse_texture_entry, full_object_motion,
        terse_texture_entry, terse_update,
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
        let roundtrip =
            compressed_object(&reencoded, 42, 0x55).ok_or("the re-encoded blob decodes")?;
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
}
