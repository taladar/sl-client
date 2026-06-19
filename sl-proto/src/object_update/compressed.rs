//! `ObjectUpdateCompressed` (`Data`) blob codec and compressed-flag handling.

use super::{read_nul_string, write_nul_string};
use crate::session::ZERO_VECTOR;
use crate::types::{Object, ObjectExtraParams, ObjectMotion, PrimShapeParams};
use sl_wire::{Reader, Writer};
use uuid::Uuid;

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
