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
//! does not survive the trip through the text — which this module's own
//! `the_text_carries_none_of_the_modern_prim` asserts field by field rather
//! than leaving to this paragraph. That is the format's limit, not this
//! crate's: inventing keywords for them would produce an asset no grid could
//! read, and there is no grid left to check a guess against (see the crate
//! docs, "Do not rez out of these bytes"). A grid that needs a lossless take →
//! rez must keep the object, not these bytes. The wire form is likewise missing
//! everything the simulator keeps for itself (`task_valid`, `gpw_bias`, the
//! birth and rez stamps), so a round trip the other way leaves those at their
//! defaults.

use sl_prim::PrimShape;
use sl_proto::{
    AgentKey, GroupKey, InventoryKey, LindenAmount, Object, ObjectExtraParams, ObjectKey,
    ObjectMotion, ObjectProperties, OwnerKey, Permissions, Permissions5, PrimShapeParams,
    RegionHandle, RegionLocalObjectId, TextureEntry, TextureFace, TextureKey, decode_texture_entry,
    encode_texture_entry,
};
use uuid::Uuid;

use crate::model::{
    LegacyFace, LegacyPathParams, LegacyPermissions, LegacyProfileParams, LegacySaleType,
    LegacyShape, PrimBlock, PrimPlacement, PrimSound, ZERO_VECTOR,
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
            placement: placement_of(object),
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

    /// The properties record this asset prim describes, under the object key
    /// `object_id` the rez minted — the reply a select of the rezzed object is
    /// answered with, and the half [`to_object`](Self::to_object) deliberately
    /// leaves empty.
    ///
    /// The inverse of what [`from_object`](Self::from_object) wrote: the name,
    /// the description, the permission block and the sale terms. Everything
    /// else in the record is something the asset does not carry and the
    /// *simulator* owns — the item and folder the object was rezzed from, the
    /// task inventory's serial, the linkset's aggregate permission rollups, the
    /// texture id list a viewer's inventory panel shows — and is left at the
    /// value that says "not applicable" rather than invented here. A rezzing
    /// simulator fills those in; it is the one that knows them.
    ///
    /// The owner is the asset's, which for a rez is the owner the object had
    /// when it was **taken**. A simulator rezzing it for somebody else
    /// overwrites that with the rezzing agent, which is a decision about
    /// ownership rather than about the format.
    #[must_use]
    pub fn to_properties(&self, object_id: ObjectKey) -> ObjectProperties {
        let owner = AgentKey::from(self.permissions.owner_id);
        ObjectProperties {
            object_id,
            creator_id: AgentKey::from(self.permissions.creator_id),
            owner: if self.permissions.group_owned {
                OwnerKey::Group(GroupKey::from(self.permissions.group_id))
            } else {
                OwnerKey::Agent(owner)
            },
            group: (!self.permissions.group_id.is_nil())
                .then(|| GroupKey::from(self.permissions.group_id)),
            last_owner_id: self.permissions.last_owner_id,
            // Not in the asset: `birthtime` is the simulator's own bookkeeping
            // stamp in microseconds, not the item's creation date in seconds,
            // and reading one as the other would be a date a century out.
            creation_date: 0,
            permissions: Permissions5 {
                base: Permissions::from_bits(self.permissions.base_mask),
                owner: Permissions::from_bits(self.permissions.owner_mask),
                group: Permissions::from_bits(self.permissions.group_mask),
                everyone: Permissions::from_bits(self.permissions.everyone_mask),
                next_owner: Permissions::from_bits(self.permissions.next_owner_mask),
            },
            ownership_cost: LindenAmount(0),
            sale_type: self.sale_info.sale_type.to_code(),
            sale_price: match self.sale_info.sale_type {
                LegacySaleType::NotForSale => None,
                // The wire's asking price is unsigned and the text's is not, so
                // a negative one is read as free rather than wrapped into a
                // fortune.
                _for_sale => Some(LindenAmount(
                    u64::try_from(self.sale_info.sale_price).unwrap_or(0),
                )),
            },
            category: 0,
            inventory_serial: 0,
            item_id: InventoryKey::from(Uuid::nil()),
            folder_id: None,
            from_task_id: self.from_task_id.map(ObjectKey::from),
            aggregate_perms: 0,
            aggregate_perm_textures: 0,
            aggregate_perm_textures_owner: 0,
            name: self.name.clone(),
            description: self.description.clone().unwrap_or_default(),
            touch_name: String::new(),
            sit_name: String::new(),
            texture_ids: Vec::new(),
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

/// What a prim states about its motion: a root's (or a solitary prim's)
/// velocities, or a child's offset from its root.
///
/// The wire tells the two apart by [`parent_id`](Object::parent_id), which is
/// zero for everything that is not in a linkset. A child's `childpos` /
/// `childrot` is exactly what an object update already carries in its motion
/// block — a child's position on the wire *is* the offset from its root — so
/// the two are the same numbers under different names.
///
/// The block's own [`position`](PrimBlock::position) is left at that offset for
/// a child as well, where the reference writes the world position it had. The
/// world position is not on the wire and cannot be recovered from the child
/// alone; a rez reads the offset ([`PrimBlock::to_object`] takes a child's
/// placement, not its `pos`), so the round trip is exact either way and the
/// number written is one this crate was actually given.
fn placement_of(object: &Object) -> PrimPlacement {
    if object.parent_id.0 == 0 {
        PrimPlacement::Free {
            velocity: object.motion.velocity.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
        }
    } else {
        PrimPlacement::Child {
            position: object.motion.position.clone(),
            rotation: object.motion.rotation.clone(),
        }
    }
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
    use crate::model::LegacySaleInfo;
    object
        .properties
        .as_ref()
        .map_or_else(LegacySaleInfo::default, |properties| LegacySaleInfo {
            sale_type: LegacySaleType::from_code(properties.sale_type),
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
        AgentKey, InventoryKey, LindenAmount, ObjectExtraParams, ObjectKey, ObjectProperties,
        OwnerKey, Permissions, Permissions5, PrimShapeParams, RegionLocalObjectId, TextureEntry,
        TextureFace, TextureKey, decode_texture_entry, encode_texture_entry,
    };
    use sl_types::lsl::Vector;
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

    /// **The record of what this format cannot carry**, asserted rather than
    /// described.
    ///
    /// Every field named in the module docs is set on a live object, taken to
    /// the text, encoded, decoded and rezzed back — and every one of them comes
    /// back empty. That is the format, not a defect here: there is no keyword
    /// for any of them in either reference capture, in any surviving
    /// `exportLegacyStream`, or anywhere in the reference viewer's history, and
    /// inventing one would write an asset no grid could read.
    ///
    /// It is a test rather than a comment because the list is load-bearing: a
    /// grid that routes its own take → rez through this format loses exactly
    /// this much of a prim, which is why the fake grid does not
    /// (`sl_fake_grid`'s take keeps the linkset it removed). The day a keyword
    /// for one of these is found, this test is what says which line of the
    /// bridge to change.
    #[test]
    fn the_text_carries_none_of_the_modern_prim() {
        let glowing = TextureFace {
            glow: 0.5,
            material_id: Some(Uuid::from_u128(0x0A7E_21A1)),
            ..TextureFace::new(TextureKey::from(Uuid::from_u128(1)))
        };
        let entry = TextureEntry {
            faces: vec![glowing; 6],
        };
        let mut taken = crate::test_support::box_object(Uuid::from_u128(0x0DD), &entry);
        taken.text = "hover text".to_owned();
        taken.media_url = "http://example.invalid/".parse().ok();
        taken.click_action = 3;
        taken.extra = ObjectExtraParams {
            light: Some(sl_proto::LightData {
                color: [255, 200, 100, 255],
                radius: 10.0,
                cutoff: 0.0,
                falloff: 0.75,
            }),
            ..ObjectExtraParams::default()
        };
        taken.extra_params = sl_proto::encode_extra_params(&taken.extra);
        taken.particle_system = vec![1, 2, 3];

        let prim = PrimBlock::from_object(&taken, 6);
        let text = crate::model::ObjectAsset::linkset(prim, Vec::new()).encode();
        let decoded = crate::model::ObjectAsset::decode(&text);
        let reread = decoded
            .as_ref()
            .ok()
            .and_then(|asset| asset.root())
            .cloned()
            .unwrap_or_else(|| PrimBlock::from_object(&taken, 6));
        assert!(decoded.is_ok(), "the asset this take wrote does not decode");
        let rezzed = reread.to_object(RezTarget {
            region_handle: taken.region_handle,
            local_id: taken.local_id,
            full_id: Uuid::from_u128(0x0DD),
            parent_id: taken.parent_id,
        });

        // The face keeps its texture and its tint, and loses its glow and its
        // legacy material: the `faces` block ends at `media_flags`.
        let faces = decode_texture_entry(&rezzed.texture_entry, 6);
        let face = faces.faces.first().unwrap_or(&glowing);
        assert_eq!(
            face.texture_id, glowing.texture_id,
            "the texture is carried"
        );
        assert!(face.glow.abs() < f32::EPSILON, "glow is not in the text");
        assert_eq!(face.material_id, None, "a material id is not in the text");

        // The whole of the rest of the list.
        assert_eq!(rezzed.text, "", "floating text is not in the text");
        assert_eq!(rezzed.media_url, None, "a media URL is not in the text");
        assert_eq!(rezzed.click_action, 0, "a click action is not in the text");
        assert_eq!(
            rezzed.extra,
            ObjectExtraParams::default(),
            "the ExtraParams block -- flexi, light, sculpt, mesh, light image, \
             extended mesh, render material, reflection probe -- is not in the text"
        );
        assert!(rezzed.extra_params.is_empty());
        assert!(
            rezzed.particle_system.is_empty(),
            "a particle system is not in the text"
        );
        assert!(
            rezzed.texture_anim.is_empty(),
            "a texture animation is not in the text"
        );

        // What it *does* carry, so the test cannot pass by rezzing nothing.
        assert_eq!(rezzed.shape, taken.shape);
        assert_eq!(rezzed.scale, taken.scale);
        assert_eq!(rezzed.text_color, taken.text_color, "the colour is written");
    }

    /// A **child** prim is serialised as one: its wire motion block holds its
    /// offset from the root rather than a velocity, so the take writes
    /// `childpos` / `childrot` and the rez reads them back.
    ///
    /// The two directions have to agree about which pair carries the offset or
    /// a taken linkset comes back with its children piled at the root — the
    /// same failure a link that forgot to reframe the child produces, and just
    /// as invisible in a log of ids.
    #[test]
    fn a_child_prim_is_taken_and_rezzed_by_its_offset() {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(1))); 6],
        };
        let mut taken = crate::test_support::box_object(Uuid::from_u128(0x5678), &entry);
        // Linked: the parent's id is what tells the two apart, and the position
        // is then the offset from the root rather than a region position.
        taken.parent_id = RegionLocalObjectId(9);
        taken.motion.position = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.5,
        };
        let prim = PrimBlock::from_object(&taken, 6);
        assert_eq!(
            prim.placement,
            PrimPlacement::Child {
                position: taken.motion.position.clone(),
                rotation: taken.motion.rotation.clone(),
            }
        );
        let rezzed = prim.to_object(RezTarget {
            region_handle: taken.region_handle,
            local_id: RegionLocalObjectId(101),
            full_id: Uuid::from_u128(0x9999),
            parent_id: RegionLocalObjectId(100),
        });
        assert_eq!(rezzed.motion.position, taken.motion.position);
        assert_eq!(rezzed.parent_id, RegionLocalObjectId(100));
    }

    /// The properties record survives the same trip: the name, description,
    /// permission masks and sale terms a take wrote come back out of the text.
    ///
    /// The ids the record carries that the *asset* cannot know — the item and
    /// folder it was rezzed from, the contents serial — come back empty, which
    /// is the point: they are the rezzing simulator's to fill in, and a decoder
    /// that invented them would hand a viewer a "find in inventory" that leads
    /// nowhere.
    #[test]
    fn a_properties_record_round_trips_through_the_asset_form() {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(2))); 6],
        };
        let mut taken = crate::test_support::box_object(Uuid::from_u128(0xABCD), &entry);
        let creator = AgentKey::from(Uuid::from_u128(0x0C_2EA704));
        let owner = AgentKey::from(Uuid::from_u128(0x0_0E4E));
        let properties = ObjectProperties {
            creator_id: creator,
            owner: OwnerKey::Agent(owner),
            last_owner_id: Uuid::from_u128(0x1A57),
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7F_FF_FF_FF),
                owner: Permissions::from_bits(0x0008_2000),
                group: Permissions::empty(),
                everyone: Permissions::from_bits(0x0002_0000),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
            sale_type: 2,
            sale_price: Some(LindenAmount(250)),
            name: "Taken Thing".to_owned(),
            description: "with a description".to_owned(),
            ..default_properties(taken.full_id)
        };
        taken.properties = Some(properties.clone());

        let prim = PrimBlock::from_object(&taken, 6);
        let rezzed = prim.to_properties(ObjectKey::from(Uuid::from_u128(0xFEED)));
        assert_eq!(rezzed.object_id, ObjectKey::from(Uuid::from_u128(0xFEED)));
        assert_eq!(rezzed.creator_id, creator);
        assert_eq!(rezzed.owner, OwnerKey::Agent(owner));
        assert_eq!(rezzed.last_owner_id, properties.last_owner_id);
        assert_eq!(rezzed.permissions, properties.permissions);
        assert_eq!(rezzed.sale_type, properties.sale_type);
        assert_eq!(rezzed.sale_price, properties.sale_price);
        assert_eq!(rezzed.name, properties.name);
        assert_eq!(rezzed.description, properties.description);
        assert_eq!(rezzed.item_id, InventoryKey::from(Uuid::nil()));
        assert_eq!(rezzed.folder_id, None);
        assert_eq!(rezzed.inventory_serial, 0);
    }

    /// A record whose every asset-carried field is at its zero value — the base
    /// the round-trip test above overrides, kept here so that test states only
    /// what it is asserting.
    fn default_properties(object_id: sl_proto::ObjectKey) -> ObjectProperties {
        ObjectProperties {
            object_id,
            creator_id: AgentKey::from(Uuid::nil()),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
            group: None,
            last_owner_id: Uuid::nil(),
            creation_date: 0,
            permissions: Permissions5 {
                base: Permissions::empty(),
                owner: Permissions::empty(),
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::empty(),
            },
            ownership_cost: LindenAmount(0),
            sale_type: 0,
            sale_price: None,
            category: 0,
            inventory_serial: 0,
            item_id: InventoryKey::from(Uuid::nil()),
            folder_id: None,
            from_task_id: None,
            aggregate_perms: 0,
            aggregate_perm_textures: 0,
            aggregate_perm_textures_owner: 0,
            name: String::new(),
            description: String::new(),
            touch_name: String::new(),
            sit_name: String::new(),
            texture_ids: Vec::new(),
        }
    }
}
