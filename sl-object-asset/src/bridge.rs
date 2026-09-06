//! Between an asset prim and the live object it serialises.
//!
//! The two encodings are unrelated: an [`sl_proto::Object`] is what an
//! `ObjectUpdate` puts on the wire (quantized shape bytes, a packed
//! `TextureEntry` blob, a region-local id), and a [`PrimBlock`] is the text a
//! grid stores. A take goes one way and a rez the other, so both directions are
//! here.
//!
//! **What the text cannot carry.** The asset format has no field for a face's
//! `glow` or legacy material id, none for the `ExtraParams` block (flexi,
//! light, sculpt, mesh, reflection probe), none for floating text
//! (`llSetText` — only its *colour* is written), none for a media URL and none
//! for a texture animation or particle system. A prim carrying any of those
//! does not survive the trip through the text. That is the format's limit, not
//! this crate's: inventing keywords for them would produce an asset no grid
//! could read. The wire form is likewise missing everything the simulator keeps
//! for itself (`task_valid`, `gpw_bias`, the birth and rez stamps), so a
//! round trip the other way leaves those at their defaults.

use sl_prim::PrimShape;
use sl_proto::{
    Object, ObjectExtraParams, ObjectMotion, PrimShapeParams, RegionHandle, RegionLocalObjectId,
    TextureEntry, TextureFace, TextureKey, decode_texture_entry, encode_texture_entry,
};
use uuid::Uuid;

use crate::model::{
    LegacyFace, LegacyPathParams, LegacyPermissions, LegacyProfileParams, LegacyShape, PrimBlock,
    PrimPlacement, PrimSound, ZERO_VECTOR,
};

/// Where a prim decoded from an asset is being rezzed: the ids only the region
/// doing the rez can mint, which the asset itself cannot supply.
///
/// The asset carries the ids the prim had *where it was taken from*; a rez —
/// in another region, or twice in the same one — has to mint new ones, so they
/// are passed in rather than read out of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RezTarget {
    /// The region the prim is being rezzed into.
    pub region_handle: RegionHandle,
    /// The region-local id the simulator has minted for it.
    pub local_id: RegionLocalObjectId,
    /// The full object key the simulator has minted for it.
    pub full_id: Uuid,
    /// The local id of the prim's parent — the linkset root for a child, or
    /// zero for a root or solitary prim.
    pub parent_id: RegionLocalObjectId,
}

impl PrimBlock {
    /// The asset form of a live object — what a **take** serialises.
    ///
    /// `face_count` is how many faces the prim renders, which decides how far
    /// the packed `TextureEntry` blob is unpacked: the same number a viewer
    /// passes to [`sl_proto::decode_texture_entry`]. A box has six; a
    /// tessellated shape's count is `sl_prim::tessellate` on the dequantized
    /// shape.
    ///
    /// The object's ids are carried across — [`task_id`](Self::task_id) is its
    /// object key and [`local_id`](Self::local_id) the region-local handle it
    /// had — because that is what the reference's own serialisation records,
    /// and a rez mints new ones regardless (see [`RezTarget`]).
    #[must_use]
    pub fn from_object(object: &Object, face_count: usize) -> Self {
        let entry = decode_texture_entry(&object.texture_entry, face_count);
        Self {
            task_id: object.full_id.uuid(),
            permissions: permissions_of(object),
            name: object
                .properties
                .as_ref()
                .map_or_else(String::new, |properties| properties.name.clone()),
            description: object
                .properties
                .as_ref()
                .map(|properties| properties.description.clone()),
            local_id: object.local_id.0,
            total_crc: object.crc,
            pcode: object.pcode,
            position: object.motion.position.clone(),
            old_position: object.motion.position.clone(),
            rotation: object.motion.rotation.clone(),
            placement: PrimPlacement::Free {
                velocity: object.motion.velocity.clone(),
                angular_velocity: object.motion.angular_velocity.clone(),
            },
            scale: object.scale.clone(),
            state: object.state,
            material: object.material,
            sound: PrimSound {
                sound_id: object.sound,
                gain: object.gain,
                radius: object.sound_radius,
                flags: object.sound_flags,
            },
            text_color: unit_color(object.text_color),
            shape: LegacyShape::from_params(&object.shape),
            faces: entry
                .faces
                .iter()
                .map(LegacyFace::from_texture_face)
                .collect(),
            name_values: object.name_value.lines().map(str::to_owned).collect(),
            sale_info: sale_info_of(object),
            ..Self::default()
        }
    }

    /// The live object this asset prim rezzes into, under the ids `target`
    /// mints.
    ///
    /// The prim's own [`local_id`](Self::local_id) and
    /// [`task_id`](Self::task_id) are deliberately **not** reused: they name a
    /// prim in the region the object was taken from, where something else may
    /// hold them now.
    #[must_use]
    pub fn to_object(&self, target: RezTarget) -> Object {
        let entry = TextureEntry {
            faces: self.faces.iter().map(LegacyFace::to_texture_face).collect(),
        };
        // A child prim's lines hold its offset from the root rather than a
        // motion: it rezzes at rest, at the offset that parents it.
        let motion = match &self.placement {
            PrimPlacement::Free {
                velocity,
                angular_velocity,
            } => ObjectMotion {
                position: self.position.clone(),
                velocity: velocity.clone(),
                acceleration: ZERO_VECTOR,
                rotation: self.rotation.clone(),
                angular_velocity: angular_velocity.clone(),
                collision_plane: None,
            },
            PrimPlacement::Child { position, rotation } => ObjectMotion {
                position: position.clone(),
                velocity: ZERO_VECTOR,
                acceleration: ZERO_VECTOR,
                rotation: rotation.clone(),
                angular_velocity: ZERO_VECTOR,
                collision_plane: None,
            },
        };
        Object {
            region_handle: target.region_handle,
            local_id: target.local_id,
            circuit: sl_proto::CircuitId::default(),
            full_id: target.full_id.into(),
            parent_id: target.parent_id,
            pcode: self.pcode,
            state: self.state,
            crc: self.total_crc,
            material: self.material,
            click_action: 0,
            update_flags: 0,
            scale: self.scale.clone(),
            motion,
            // Null unless the object has sound or particles, which is the
            // protocol's own rule for this field.
            owner_id: Uuid::nil(),
            sound: self.sound.sound_id,
            gain: self.sound.gain,
            sound_flags: self.sound.flags,
            sound_radius: self.sound.radius,
            text: String::new(),
            text_color: byte_color(self.text_color),
            name_value: self.name_values.join("\n"),
            media_url: None,
            texture_entry: encode_texture_entry(&entry),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: self.shape.to_params(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: ObjectExtraParams::default(),
            // The properties are a *reply* to a select, not part of an object
            // update: a rez streams the object and answers `ObjectProperties`
            // separately, out of the item the asset was filed under.
            properties: None,
            joint_type: 0,
            joint_pivot: ZERO_VECTOR,
            joint_axis_or_anchor: ZERO_VECTOR,
        }
    }
}

/// How many faces a volume prim of this shape renders — the `face_count` a
/// take should pass to [`PrimBlock::from_object`], and the number of `faces`
/// blocks the asset will hold.
///
/// It is the shape that decides: a box has six, a cut prim gains the cut edges,
/// a hollow one gains its inner surface. The count comes from tessellating at
/// the *lowest* level of detail, which is the cheapest way to ask and yields
/// the same answer as the highest (`sl_prim`'s own
/// `lod_triangle_counts` tests pin that the face set does not change with
/// detail).
///
/// Not for a mesh or sculpt prim: their face count is a property of the mesh
/// asset, not of the shape block, and the caller has to read it from there.
#[must_use]
pub fn rendered_face_count(shape: &PrimShapeParams) -> usize {
    sl_prim::tessellate(&PrimShape::from_params(shape), sl_prim::PrimLod::Low).face_count()
}

/// The permission block an object's properties state, or an all-zero block for
/// an object whose `ObjectProperties` have not arrived — which is the honest
/// answer: a take that was never told the permissions must not invent open
/// ones.
fn permissions_of(object: &Object) -> LegacyPermissions {
    object
        .properties
        .as_ref()
        .map_or_else(LegacyPermissions::default, |properties| LegacyPermissions {
            base_mask: properties.permissions.base.bits(),
            owner_mask: properties.permissions.owner.bits(),
            group_mask: properties.permissions.group.bits(),
            everyone_mask: properties.permissions.everyone.bits(),
            next_owner_mask: properties.permissions.next_owner.bits(),
            creator_id: properties.creator_id.uuid(),
            owner_id: properties.owner.uuid(),
            last_owner_id: properties.last_owner_id,
            group_id: properties
                .group
                .map_or_else(Uuid::nil, |group| group.uuid()),
            group_owned: properties.owner.is_group(),
        })
}

/// The sale block an object's properties state. The wire carries the sale type
/// as a `SALE_TYPE_*` code and the text as a keyword, so the codes are mapped
/// here rather than guessed at either end.
fn sale_info_of(object: &Object) -> crate::model::LegacySaleInfo {
    use crate::model::{LegacySaleInfo, LegacySaleType};
    object
        .properties
        .as_ref()
        .map_or_else(LegacySaleInfo::default, |properties| LegacySaleInfo {
            sale_type: match properties.sale_type {
                1 => LegacySaleType::Original,
                2 => LegacySaleType::Copy,
                3 => LegacySaleType::Contents,
                _not_for_sale => LegacySaleType::NotForSale,
            },
            sale_price: properties
                .sale_price
                .as_ref()
                .map_or(0, |price| i32::try_from(price.0).unwrap_or(i32::MAX)),
        })
}

/// An RGBA byte colour as the `0.0..=1.0` floats the asset text carries.
fn unit_color(bytes: [u8; 4]) -> [f32; 4] {
    bytes.map(|channel| f32::from(channel) / f32::from(u8::MAX))
}

/// A `0.0..=1.0` RGBA colour as the bytes the wire carries, rounding rather
/// than truncating so an exact `1.0` comes back as `255` and not `254`.
fn byte_color(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| round_to_u8(channel * f32::from(u8::MAX)))
}

/// Rounds a `0..=255` float to the nearest byte, saturating.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is rounded and clamped into 0..=255 first, so the conversion is exact"
)]
const fn round_to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, BYTE_MAX_AS_F32) as u8
}

/// `u8::MAX` as the `f32` the colour rounding clamps against.
const BYTE_MAX_AS_F32: f32 = 255.0;

impl LegacyShape {
    /// The asset form of a wire shape block, dequantized.
    #[must_use]
    pub fn from_params(params: &PrimShapeParams) -> Self {
        let shape = PrimShape::from_params(params);
        Self {
            path: LegacyPathParams {
                // The curve bytes are carried verbatim rather than through
                // `sl_prim`'s enums: an unknown curve byte falls back to a line
                // there, and an asset must write back what it was given.
                curve: params.path_curve,
                begin: shape.path_begin,
                end: shape.path_end,
                scale_x: shape.path_scale_x,
                scale_y: shape.path_scale_y,
                shear_x: shape.path_shear_x,
                shear_y: shape.path_shear_y,
                twist: shape.twist_end,
                twist_begin: shape.twist_begin,
                radius_offset: shape.radius_offset,
                taper_x: shape.taper_x,
                taper_y: shape.taper_y,
                revolutions: shape.revolutions,
                skew: shape.skew,
            },
            profile: LegacyProfileParams {
                curve: params.profile_curve,
                begin: shape.profile_begin,
                end: shape.profile_end,
                hollow: shape.hollow,
            },
        }
    }

    /// The wire form of this shape block, quantized.
    #[must_use]
    pub fn to_params(&self) -> PrimShapeParams {
        let shape = PrimShape {
            path_curve: sl_prim::PathCurve::from_byte(self.path.curve),
            profile_curve: sl_prim::ProfileCurve::from_byte(self.profile.curve),
            hole_type: sl_prim::HoleType::from_byte(self.profile.curve),
            path_begin: self.path.begin,
            path_end: self.path.end,
            path_scale_x: self.path.scale_x,
            path_scale_y: self.path.scale_y,
            path_shear_x: self.path.shear_x,
            path_shear_y: self.path.shear_y,
            twist_begin: self.path.twist_begin,
            twist_end: self.path.twist,
            radius_offset: self.path.radius_offset,
            taper_x: self.path.taper_x,
            taper_y: self.path.taper_y,
            revolutions: self.path.revolutions,
            skew: self.path.skew,
            profile_begin: self.profile.begin,
            profile_end: self.profile.end,
            hollow: self.profile.hollow,
        };
        PrimShapeParams {
            // Again the raw bytes, so a curve neither this crate nor `sl_prim`
            // knows is passed through instead of being flattened to a line.
            path_curve: self.path.curve,
            profile_curve: self.profile.curve,
            ..shape.to_params()
        }
    }
}

impl LegacyFace {
    /// The asset form of a decoded wire face.
    ///
    /// The wire packs bump, shininess and full-bright into one byte; the text
    /// splits full-bright out and keeps the other two together, which is the
    /// reference's own `getBumpShiny` / `getFullbright` split. `glow` and the
    /// legacy material id have nowhere to go and are dropped.
    #[must_use]
    pub fn from_texture_face(face: &TextureFace) -> Self {
        Self {
            image_id: face.texture_id.uuid(),
            color: unit_color(face.color),
            scale_s: face.scale_s,
            scale_t: face.scale_t,
            offset_s: face.offset_s,
            offset_t: face.offset_t,
            rotation: face.rotation,
            bump: face.bump_shiny_fullbright & BUMP_SHINY_MASK,
            fullbright: face.fullbright(),
            media_flags: face.media_flags,
        }
    }

    /// The wire form of this asset face, re-packing the bump / shiny /
    /// full-bright byte.
    #[must_use]
    pub fn to_texture_face(&self) -> TextureFace {
        let fullbright = if self.fullbright { FULLBRIGHT_BIT } else { 0 };
        TextureFace {
            texture_id: TextureKey::from(self.image_id),
            color: byte_color(self.color),
            scale_s: self.scale_s,
            scale_t: self.scale_t,
            offset_s: self.offset_s,
            offset_t: self.offset_t,
            rotation: self.rotation,
            bump_shiny_fullbright: (self.bump & BUMP_SHINY_MASK) | fullbright,
            media_flags: self.media_flags,
            glow: 0.0,
            material_id: None,
        }
    }
}

/// The bits of the wire's packed bump byte that the text's `bump` field holds:
/// the bump type (low five) and the shininess (top two), leaving out the
/// full-bright bit the text writes separately (LL's `TEM_BUMP_SHINY_MASK`).
const BUMP_SHINY_MASK: u8 = 0xc0 | 0x1f;

/// The full-bright bit of the wire's packed bump byte
/// (`TEM_FULLBRIGHT_MASK << TEM_FULLBRIGHT_SHIFT`).
const FULLBRIGHT_BIT: u8 = 0x20;

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{
        PrimShapeParams, TextureEntry, TextureFace, TextureKey, decode_texture_entry,
        encode_texture_entry,
    };
    use uuid::Uuid;

    use super::{LegacyFace, LegacyShape, RezTarget};
    use crate::model::{PrimBlock, PrimPlacement};

    /// A wire shape survives the trip out to the text and back: the dequantized
    /// floats re-quantize to the bytes they came from.
    #[test]
    fn a_wire_shape_round_trips_through_the_asset_form() {
        let params = PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x21,
            path_begin: 5_000,
            path_end: 10_000,
            path_scale_x: 150,
            path_scale_y: 200,
            path_shear_x: 250,
            path_shear_y: 0,
            path_twist: -20,
            path_twist_begin: 15,
            path_radius_offset: -5,
            path_taper_x: 30,
            path_taper_y: -30,
            path_revolutions: 66,
            path_skew: 10,
            profile_begin: 2_500,
            profile_end: 7_500,
            profile_hollow: 12_500,
        };
        assert_eq!(LegacyShape::from_params(&params).to_params(), params);
    }

    /// A face survives the same trip, with the bump / shiny / full-bright byte
    /// split apart and re-packed.
    #[test]
    fn a_face_round_trips_including_its_packed_bump_byte() {
        let face = TextureFace {
            texture_id: TextureKey::from(Uuid::from_u128(0x99)),
            color: [10, 20, 30, 255],
            scale_s: 2.0,
            scale_t: -1.0,
            offset_s: 0.25,
            offset_t: -0.25,
            rotation: 1.5,
            // A bump type, the full-bright bit and a shininess at once.
            bump_shiny_fullbright: 0x03 | 0x20 | 0x80,
            media_flags: 1,
            glow: 0.0,
            material_id: None,
        };
        assert_eq!(LegacyFace::from_texture_face(&face).to_texture_face(), face);
    }

    /// A live object taken to the asset form and rezzed back is the same
    /// object, once the ids the rez mints are accounted for.
    #[test]
    fn an_object_round_trips_through_the_asset_form() {
        let entry = TextureEntry {
            faces: (0..6_u128)
                .map(|index| TextureFace::new(TextureKey::from(Uuid::from_u128(index))))
                .collect(),
        };
        let taken = crate::test_support::box_object(Uuid::from_u128(0x1234), &entry);
        let prim = PrimBlock::from_object(&taken, 6);
        assert_eq!(prim.task_id, Uuid::from_u128(0x1234));
        assert_eq!(prim.faces.len(), 6);
        assert_eq!(prim.placement, PrimPlacement::default());
        let rezzed = prim.to_object(RezTarget {
            region_handle: taken.region_handle,
            local_id: taken.local_id,
            full_id: Uuid::from_u128(0x1234),
            parent_id: taken.parent_id,
        });
        assert_eq!(rezzed.shape, taken.shape);
        assert_eq!(rezzed.pcode, taken.pcode);
        assert_eq!(rezzed.state, taken.state);
        assert_eq!(rezzed.scale, taken.scale);
        assert_eq!(decode_texture_entry(&rezzed.texture_entry, 6), entry);
        assert_eq!(
            encode_texture_entry(&entry),
            encode_texture_entry(&decode_texture_entry(&taken.texture_entry, 6))
        );
    }
}
