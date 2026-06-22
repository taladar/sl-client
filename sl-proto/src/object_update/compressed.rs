//! `ObjectUpdateCompressed` (`Data`) blob codec and compressed-flag handling.

use super::{read_nul_string, write_nul_string};
use crate::session::ZERO_VECTOR;
use crate::types::{Object, ObjectExtraParams, ObjectMotion, PrimShapeParams};
use core::ops::BitOrAssign;
use sl_wire::{Reader, RegionHandle, RegionLocalObjectId, Writer};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Compressed-update flags & sizes
// ---------------------------------------------------------------------------

/// The `CompressedFlags` bitfield carried in an `ObjectUpdateCompressed` blob,
/// gating which optional fields follow (mirrors LL's `CompressedFlags` in
/// `indra/llprimitive/object_flags.h`). Combine bits with `|`; query with
/// [`CompressedFlags::contains`].
///
/// Like the other protocol bitfields ([`sl_wire::ParcelFlags`],
/// [`sl_wire::Permissions`]) this is a newtype rather than a bare `u32` so the
/// flag-gated optional fields can be read and written by name instead of with
/// raw `& MASK != 0` masking. It stays private to this codec because the flags
/// only ever appear inside the packed `Data` blob this module decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CompressedFlags {
    /// The raw flag bits.
    bits: u32,
}

impl CompressedFlags {
    /// The object carries a scratchpad blob (a `u32` length then the bytes).
    const SCRATCHPAD: Self = Self { bits: 0x01 };
    /// The object carries a tree species byte.
    const TREE: Self = Self { bits: 0x02 };
    /// The object has floating text (`llSetText`).
    const HAS_TEXT: Self = Self { bits: 0x04 };
    /// The object has a legacy (≤ 86-byte) particle system block.
    const HAS_PARTICLES_LEGACY: Self = Self { bits: 0x08 };
    /// The object has an attached sound (id, gain, flags, radius follow).
    const HAS_SOUND: Self = Self { bits: 0x10 };
    /// The object is linked to a parent (a `ParentID` follows).
    const HAS_PARENT: Self = Self { bits: 0x20 };
    /// The object has a texture-animation block (after the texture entry).
    const TEXTURE_ANIM: Self = Self { bits: 0x40 };
    /// The object has a non-zero angular velocity (a vector follows).
    const HAS_ANGULAR_VELOCITY: Self = Self { bits: 0x80 };
    /// The object has a name-value pairs string.
    const HAS_NAME_VALUES: Self = Self { bits: 0x100 };
    /// The object has a media URL.
    const MEDIA_URL: Self = Self { bits: 0x200 };
    /// The object has a "new" (> 86-byte) particle system block, appended last.
    const HAS_PARTICLES_NEW: Self = Self { bits: 0x400 };

    /// The empty flag set.
    const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Builds flags from a raw value.
    const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Returns the raw flag bits.
    const fn bits(self) -> u32 {
        self.bits
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }
}

impl BitOrAssign for CompressedFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

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
    region_handle: RegionHandle,
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
    let cflags = CompressedFlags::from_bits(reader.u32().ok()?);
    let owner_id = reader.uuid().ok()?;
    let angular_velocity = if cflags.contains(CompressedFlags::HAS_ANGULAR_VELOCITY) {
        reader.vector3().ok()?
    } else {
        ZERO_VECTOR
    };
    let parent_id = if cflags.contains(CompressedFlags::HAS_PARENT) {
        reader.u32().ok()?
    } else {
        0
    };
    // The generic `Data` field: a tree's genome byte (carried inline under the
    // tree flag) or a scratchpad blob. Captured so the compressed object exposes
    // the same `data` a full update carries in its `Data` field.
    let data = if cflags.contains(CompressedFlags::TREE) {
        vec![reader.u8().ok()?]
    } else if cflags.contains(CompressedFlags::SCRATCHPAD) {
        let size = reader.u32().ok()?;
        reader.take(usize::try_from(size).ok()?).ok()?.to_vec()
    } else {
        Vec::new()
    };
    let (text, text_color) = if cflags.contains(CompressedFlags::HAS_TEXT) {
        let text = read_nul_string(&mut reader)?;
        let color = reader.take_array::<4>().ok()?;
        (text, color)
    } else {
        (String::new(), [0; 4])
    };
    let media_url = if cflags.contains(CompressedFlags::MEDIA_URL) {
        read_nul_string(&mut reader)?
    } else {
        String::new()
    };
    let mut object = Object {
        region_handle,
        local_id: RegionLocalObjectId(local_id),
        full_id,
        parent_id: RegionLocalObjectId(parent_id),
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
    cflags: CompressedFlags,
    object: &mut Object,
) -> Option<()> {
    // Legacy particle system: a fixed-size block with no length prefix.
    if cflags.contains(CompressedFlags::HAS_PARTICLES_LEGACY) {
        object.particle_system = reader.take(COMPRESSED_LEGACY_PARTICLE_SIZE).ok()?.to_vec();
        object.particles = crate::particles::decode_particle_system(&object.particle_system);
    }
    // ExtraParams container (always present, if only as a zero count byte).
    let extra_len = crate::extra_params::extra_params_len(reader.peek_rest());
    let extra_params = reader.take(extra_len).ok()?;
    object.extra = crate::extra_params::decode_extra_params(extra_params);
    object.extra_params = extra_params.to_vec();
    // Attached sound: id, gain, flags, cutoff radius.
    if cflags.contains(CompressedFlags::HAS_SOUND) {
        object.sound = reader.uuid().ok()?;
        object.gain = reader.f32().ok()?;
        object.sound_flags = reader.u8().ok()?;
        object.sound_radius = reader.f32().ok()?;
    }
    // Name-value pairs string.
    if cflags.contains(CompressedFlags::HAS_NAME_VALUES) {
        object.name_value = read_nul_string(reader)?;
    }
    // Path+profile shape parameters: a fixed-size block with no length prefix.
    object.shape = read_compressed_shape(reader)?;
    // Packed texture entry: a little-endian u32 byte length then that many bytes.
    let te_len = usize::try_from(reader.u32().ok()?).ok()?;
    object.texture_entry = reader.take(te_len).ok()?.to_vec();
    // Texture animation: a little-endian u32 byte length then that many bytes.
    if cflags.contains(CompressedFlags::TEXTURE_ANIM) {
        let anim_len = usize::try_from(reader.u32().ok()?).ok()?;
        object.texture_anim = reader.take(anim_len).ok()?.to_vec();
        object.texture_animation = crate::particles::decode_texture_anim(&object.texture_anim);
    }
    // The "new" (> 86-byte) particle system, when present, is the final field —
    // it carries its own internal size, so the rest of the blob is its payload.
    if cflags.contains(CompressedFlags::HAS_PARTICLES_NEW) {
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
/// particle system is classified legacy (`CompressedFlags::HAS_PARTICLES_LEGACY`)
/// when its raw blob is exactly the 86-byte legacy size, else "new".
fn compressed_flags(object: &Object) -> CompressedFlags {
    let mut flags = CompressedFlags::empty();
    if !object.data.is_empty() {
        flags |= CompressedFlags::SCRATCHPAD;
    }
    if !object.text.is_empty() {
        flags |= CompressedFlags::HAS_TEXT;
    }
    if object.particle_system.len() == COMPRESSED_LEGACY_PARTICLE_SIZE {
        flags |= CompressedFlags::HAS_PARTICLES_LEGACY;
    } else if !object.particle_system.is_empty() {
        flags |= CompressedFlags::HAS_PARTICLES_NEW;
    }
    if object.sound != Uuid::nil() {
        flags |= CompressedFlags::HAS_SOUND;
    }
    if object.parent_id != RegionLocalObjectId(0) {
        flags |= CompressedFlags::HAS_PARENT;
    }
    if !object.texture_anim.is_empty() {
        flags |= CompressedFlags::TEXTURE_ANIM;
    }
    if object.motion.angular_velocity != ZERO_VECTOR {
        flags |= CompressedFlags::HAS_ANGULAR_VELOCITY;
    }
    if !object.name_value.is_empty() {
        flags |= CompressedFlags::HAS_NAME_VALUES;
    }
    if !object.media_url.is_empty() {
        flags |= CompressedFlags::MEDIA_URL;
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
    writer.put_u32(object.local_id.0);
    writer.put_u8(object.pcode);
    writer.put_u8(object.state);
    writer.put_u32(object.crc);
    writer.put_u8(object.material);
    writer.put_u8(object.click_action);
    writer.put_vector3(&object.scale);
    writer.put_vector3(&object.motion.position);
    writer.put_quaternion(&object.motion.rotation);
    writer.put_u32(cflags.bits());
    writer.put_uuid(object.owner_id);
    if cflags.contains(CompressedFlags::HAS_ANGULAR_VELOCITY) {
        writer.put_vector3(&object.motion.angular_velocity);
    }
    if cflags.contains(CompressedFlags::HAS_PARENT) {
        writer.put_u32(object.parent_id.0);
    }
    // Generic data: the scratchpad form (a u32 length then the bytes).
    if cflags.contains(CompressedFlags::SCRATCHPAD) {
        writer.put_u32(u32::try_from(object.data.len()).unwrap_or(u32::MAX));
        writer.bytes(&object.data);
    }
    if cflags.contains(CompressedFlags::HAS_TEXT) {
        write_nul_string(&mut writer, &object.text);
        writer.bytes(&object.text_color);
    }
    if cflags.contains(CompressedFlags::MEDIA_URL) {
        write_nul_string(&mut writer, &object.media_url);
    }
    // Trailing fields, in the simulator's fixed pack order.
    if cflags.contains(CompressedFlags::HAS_PARTICLES_LEGACY) {
        writer.bytes(&object.particle_system);
    }
    // ExtraParams container: always present (at minimum a zero count byte).
    if object.extra_params.is_empty() {
        writer.bytes(&crate::extra_params::encode_extra_params(&object.extra));
    } else {
        writer.bytes(&object.extra_params);
    }
    if cflags.contains(CompressedFlags::HAS_SOUND) {
        writer.put_uuid(object.sound);
        writer.put_f32(object.gain);
        writer.put_u8(object.sound_flags);
        writer.put_f32(object.sound_radius);
    }
    if cflags.contains(CompressedFlags::HAS_NAME_VALUES) {
        write_nul_string(&mut writer, &object.name_value);
    }
    write_compressed_shape(&mut writer, &object.shape);
    // Packed texture entry: a u32 length then the bytes (always present).
    writer.put_u32(u32::try_from(object.texture_entry.len()).unwrap_or(u32::MAX));
    writer.bytes(&object.texture_entry);
    if cflags.contains(CompressedFlags::TEXTURE_ANIM) {
        writer.put_u32(u32::try_from(object.texture_anim.len()).unwrap_or(u32::MAX));
        writer.bytes(&object.texture_anim);
    }
    // The "new" particle system is the final field (decoded as the rest).
    if cflags.contains(CompressedFlags::HAS_PARTICLES_NEW) {
        writer.bytes(&object.particle_system);
    }
    writer.into_bytes()
}

#[cfg(test)]
mod compressed_flags_tests {
    use super::CompressedFlags;
    use pretty_assertions::assert_eq;

    #[test]
    fn named_bits_match_the_viewer_constants() {
        // The raw values mirror LL's `CompressedFlags` enum.
        assert_eq!(CompressedFlags::SCRATCHPAD.bits(), 0x01);
        assert_eq!(CompressedFlags::TREE.bits(), 0x02);
        assert_eq!(CompressedFlags::HAS_TEXT.bits(), 0x04);
        assert_eq!(CompressedFlags::HAS_PARTICLES_LEGACY.bits(), 0x08);
        assert_eq!(CompressedFlags::HAS_SOUND.bits(), 0x10);
        assert_eq!(CompressedFlags::HAS_PARENT.bits(), 0x20);
        assert_eq!(CompressedFlags::TEXTURE_ANIM.bits(), 0x40);
        assert_eq!(CompressedFlags::HAS_ANGULAR_VELOCITY.bits(), 0x80);
        assert_eq!(CompressedFlags::HAS_NAME_VALUES.bits(), 0x100);
        assert_eq!(CompressedFlags::MEDIA_URL.bits(), 0x200);
        assert_eq!(CompressedFlags::HAS_PARTICLES_NEW.bits(), 0x400);
    }

    #[test]
    fn round_trips_every_raw_value_bit_identically() {
        // The newtype must be a transparent view over the raw `u32` so the packed
        // `Data` blob's flag word stays byte-identical to before the refactor.
        for raw in [0u32, 0x01, 0x0a4, 0x7ff, 0xffff_ffff] {
            assert_eq!(CompressedFlags::from_bits(raw).bits(), raw);
        }
    }

    #[test]
    fn contains_and_union_behave() {
        let mut flags = CompressedFlags::empty();
        assert!(!flags.contains(CompressedFlags::HAS_SOUND));
        flags |= CompressedFlags::HAS_SOUND;
        flags |= CompressedFlags::MEDIA_URL;
        assert!(flags.contains(CompressedFlags::HAS_SOUND));
        assert!(flags.contains(CompressedFlags::MEDIA_URL));
        assert!(!flags.contains(CompressedFlags::HAS_PARENT));
        assert_eq!(flags.bits(), 0x10 | 0x200);
    }
}
