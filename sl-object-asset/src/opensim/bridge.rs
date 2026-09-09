//! Between a `<SceneObjectGroup>` part and the live object it serialises.
//!
//! The same job [`crate::bridge`] does for the Linden text, against the format
//! that can actually hold a prim. Where that module has a paragraph about what
//! the text loses, this one has almost nothing to say: the `Shape` block
//! carries the wire's `TextureEntry` and `ExtraParams` blobs byte for byte, so
//! glow, a legacy material id, flexi, light, sculpt, mesh and the rest cross
//! without being understood at all, and floating text, media, a texture
//! animation and a particle system each have an element.
//!
//! What it still cannot say is what the *wire* does not carry, which is a
//! different list and a much shorter one: a prim's sit target, its collision
//! sound, its pay-price buttons and its physics tunables never reach a viewer
//! in an object update, so a part built from one leaves them at their defaults.
//! A simulator serialising its own prim has them and should fill them in.
//!
//! # Two conversions that are not the identity
//!
//! - **Hover-text alpha is inverted.** OpenSim keeps `Color.A` as opacity and
//!   sends `0xFF - A` on the wire (`SceneObjectPart.GetTextColor`); the
//!   reference viewer inverts it straight back. So an opaque hover text is
//!   `255` in the XML and `0` in an [`Object::text_color`].
//! - **A light's alpha channel is its intensity.** The wire's `LLLightParams`
//!   colour packs the intensity into its alpha byte — OpenSim's own
//!   `ExtraParamsToBytes` says so in a comment — and unpacks it into
//!   `LightIntensity`, leaving `LightColorA` at the 1.0 it is constructed with.
//!   So the XML's `LightColorA` is *not* the wire's alpha and never was.

use sl_proto::{
    AgentKey, GroupKey, InventoryKey, LightData, LindenAmount, Object, ObjectExtraParams,
    ObjectKey, ObjectMotion, ObjectProperties, OwnerKey, Permissions, Permissions5,
    PrimShapeParams, RegionHandle, SculptData, TaskInventoryItem,
};
use sl_types::lsl::Vector;
use uuid::Uuid;

use crate::bridge::RezTarget;
use crate::model::ZERO_VECTOR;

use super::model::{SceneObjectGroup, SceneObjectPart, SceneShape, SceneTaskInventoryItem};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the bridge owns its `impl SceneObjectGroup` block, apart from the model's canonical impl"
)]
impl SceneObjectGroup {
    /// The XML form of a live linkset — what a **take** serialises on a grid
    /// imitating OpenSim.
    ///
    /// `linkset` is the object being taken and, after it, its children, which
    /// is the order the XML wants them in. An empty slice yields an empty
    /// group; a group with no parts is not something OpenSim writes, and a
    /// caller with nothing to file should not be calling this.
    ///
    /// Two things only the whole linkset knows are filled in here rather than
    /// in [`SceneObjectPart::from_object`]:
    ///
    /// - **link numbers.** A solitary prim is `0`, a linkset root is `1` and
    ///   its children count up from `2` — OpenSim's own convention, and the
    ///   numbers `llGetLinkNumber` reports.
    /// - **each child's `GroupPosition`.** OpenSim writes the *group's*
    ///   absolute position on every part and the offset from it separately; on
    ///   the wire a child carries only its offset, so the root's position is
    ///   copied onto the children here.
    #[must_use]
    pub fn from_linkset(linkset: &[Object]) -> Self {
        let Some((root_object, children)) = linkset.split_first() else {
            return Self::default();
        };
        let mut root = SceneObjectPart::from_object(root_object);
        root.link_num = if children.is_empty() { 0 } else { 1 };
        let group_position = root.group_position.clone();
        let other_parts = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let mut part = SceneObjectPart::from_object(child);
                part.group_position = group_position.clone();
                part.link_num = i32::try_from(index).unwrap_or(i32::MAX).saturating_add(2);
                part
            })
            .collect();
        Self {
            root,
            other_parts,
            unknown: Vec::new(),
        }
    }
}

impl SceneObjectPart {
    /// The XML form of one live object.
    ///
    /// The part's own ids are carried across — [`uuid`](Self::uuid) is the
    /// object key and [`local_id`](Self::local_id) the region-local handle it
    /// had — because that is what OpenSim's own serialisation records, and a
    /// rez mints new ones regardless (see [`RezTarget`]).
    #[must_use]
    pub fn from_object(object: &Object) -> Self {
        let properties = object.properties.as_ref();
        let child = object.parent_id.0 != 0;
        Self {
            // OpenSim keeps this beside the `AllowInventoryDrop` flag it also
            // sends; the wire carries only the flag, so it is read back out of
            // it rather than left false on every taken prim.
            allowed_drop: object.update_flags & ALLOW_INVENTORY_DROP != 0,
            creator_id: properties
                .map_or_else(Uuid::nil, |properties| properties.creator_id.uuid()),
            creator_data: None,
            folder_id: Uuid::nil(),
            inventory_serial: properties
                .and_then(|properties| u32::try_from(properties.inventory_serial).ok())
                .unwrap_or(0),
            task_inventory: Vec::new(),
            uuid: object.full_id.uuid(),
            local_id: object.local_id.0,
            name: properties.map_or_else(String::new, |properties| properties.name.clone()),
            material: object.material,
            pass_touches: false,
            pass_collisions: false,
            region_handle: object.region_handle.0,
            script_access_pin: 0,
            // A child's wire position *is* its offset from the root; the root's
            // is the group's. `from_linkset` copies the root's onto the
            // children, which is the one thing a single part cannot know.
            group_position: if child {
                ZERO_VECTOR
            } else {
                object.motion.position.clone()
            },
            offset_position: if child {
                object.motion.position.clone()
            } else {
                ZERO_VECTOR
            },
            rotation_offset: object.motion.rotation.clone(),
            velocity: object.motion.velocity.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
            acceleration: object.motion.acceleration.clone(),
            description: properties
                .map_or_else(String::new, |properties| properties.description.clone()),
            text_color: stored_text_color(object.text_color),
            text: object.text.clone(),
            sit_name: properties.map_or_else(String::new, |properties| properties.sit_name.clone()),
            touch_name: properties
                .map_or_else(String::new, |properties| properties.touch_name.clone()),
            link_num: 0,
            click_action: object.click_action,
            shape: SceneShape::from_object(object),
            scale: object.scale.clone(),
            parent_id: object.parent_id.0,
            // The wire's `creation_date` is the simulator's birth stamp in
            // microseconds and OpenSim's `CreationDate` is a Unix second, so
            // the two are not the same number under different names.
            creation_date: properties
                .and_then(|properties| {
                    i32::try_from(properties.creation_date / MICROS_PER_SECOND).ok()
                })
                .unwrap_or(0),
            category: properties.map_or(0, |properties| properties.category),
            sale_price: properties
                .and_then(|properties| properties.sale_price.as_ref())
                .and_then(|price| i32::try_from(price.0).ok())
                .unwrap_or(0),
            object_sale_type: properties.map_or(0, |properties| properties.sale_type),
            ownership_cost: properties
                .and_then(|properties| i32::try_from(properties.ownership_cost.0).ok())
                .unwrap_or(0),
            group_id: properties
                .and_then(|properties| properties.group)
                .map_or_else(Uuid::nil, |group| group.uuid()),
            owner_id: properties.map_or_else(Uuid::nil, |properties| properties.owner.uuid()),
            last_owner_id: properties.map_or_else(Uuid::nil, |properties| properties.last_owner_id),
            rezzer_id: Uuid::nil(),
            base_mask: properties.map_or(0, |properties| properties.permissions.base.bits()),
            owner_mask: properties.map_or(0, |properties| properties.permissions.owner.bits()),
            group_mask: properties.map_or(0, |properties| properties.permissions.group.bits()),
            everyone_mask: properties
                .map_or(0, |properties| properties.permissions.everyone.bits()),
            next_owner_mask: properties
                .map_or(0, |properties| properties.permissions.next_owner.bits()),
            flags: object.update_flags,
            media_url: object.media_url.as_ref().map(url::Url::to_string),
            texture_animation: object.texture_anim.clone(),
            particle_system: object.particle_system.clone(),
            sound_id: object.sound,
            sound_gain: object.gain,
            sound_flags: object.sound_flags,
            sound_radius: object.sound_radius,
            ..Self::default()
        }
    }

    /// The live object this part rezzes into, under the ids `target` mints.
    ///
    /// The part's own [`local_id`](Self::local_id) and [`uuid`](Self::uuid) are
    /// deliberately **not** reused: they name a prim in the region the object
    /// was taken from, where something else may hold them now.
    #[must_use]
    pub fn to_object(&self, target: RezTarget) -> Object {
        let child = target.parent_id.0 != 0;
        Object {
            region_handle: RegionHandle(self.region_handle),
            local_id: target.local_id,
            circuit: sl_proto::CircuitId::default(),
            full_id: target.full_id.into(),
            parent_id: target.parent_id,
            pcode: self.shape.pcode,
            state: self.shape.state,
            // Not in the XML: the CRC is the simulator's own object-cache
            // bookkeeping, and a rezzing simulator computes a fresh one.
            crc: 0,
            material: self.material,
            click_action: self.click_action,
            update_flags: self.flags,
            scale: self.scale.clone(),
            motion: ObjectMotion {
                // A child rezzes at the offset that parents it; a root at the
                // group position the take stored.
                position: if child {
                    self.offset_position.clone()
                } else {
                    self.group_position.clone()
                },
                velocity: self.velocity.clone(),
                acceleration: self.acceleration.clone(),
                rotation: self.rotation_offset.clone(),
                angular_velocity: self.angular_velocity.clone(),
                collision_plane: None,
            },
            // Null unless the object has sound or particles, which is the
            // protocol's own rule for this field.
            owner_id: Uuid::nil(),
            sound: self.sound_id,
            gain: self.sound_gain,
            sound_flags: self.sound_flags,
            sound_radius: self.sound_radius,
            text: self.text.clone(),
            text_color: wire_text_color(self.text_color),
            name_value: String::new(),
            media_url: self
                .media_url
                .as_ref()
                .and_then(|url| url::Url::parse(url).ok()),
            texture_entry: self.shape.texture_entry.clone(),
            texture_anim: self.texture_animation.clone(),
            texture_animation: sl_proto::decode_texture_anim(&self.texture_animation),
            shape: self.shape.to_params(),
            particle_system: self.particle_system.clone(),
            particles: sl_proto::decode_particle_system(&self.particle_system),
            data: Vec::new(),
            extra_params: self.shape.extra_params.clone(),
            extra: self.shape.to_extra_params(),
            // The properties are a *reply* to a select, not part of an object
            // update: a rez streams the object and answers `ObjectProperties`
            // separately, through `to_properties`.
            properties: None,
            joint_type: 0,
            joint_pivot: ZERO_VECTOR,
            joint_axis_or_anchor: ZERO_VECTOR,
        }
    }

    /// The properties record this part describes, under the object key
    /// `object_id` the rez minted — the reply a select of the rezzed object is
    /// answered with, and the half [`to_object`](Self::to_object) deliberately
    /// leaves empty.
    ///
    /// The item and folder the object was rezzed from are the *simulator's* to
    /// fill in: they name where this rez came from, which the body cannot know.
    #[must_use]
    pub fn to_properties(&self, object_id: ObjectKey) -> ObjectProperties {
        ObjectProperties {
            object_id,
            creator_id: AgentKey::from(self.creator_id),
            owner: OwnerKey::Agent(AgentKey::from(self.owner_id)),
            group: (!self.group_id.is_nil()).then(|| GroupKey::from(self.group_id)),
            last_owner_id: self.last_owner_id,
            creation_date: u64::try_from(self.creation_date)
                .unwrap_or(0)
                .saturating_mul(MICROS_PER_SECOND),
            permissions: Permissions5 {
                base: Permissions::from_bits(self.base_mask),
                owner: Permissions::from_bits(self.owner_mask),
                group: Permissions::from_bits(self.group_mask),
                everyone: Permissions::from_bits(self.everyone_mask),
                next_owner: Permissions::from_bits(self.next_owner_mask),
            },
            ownership_cost: LindenAmount(u64::try_from(self.ownership_cost).unwrap_or(0)),
            sale_type: self.object_sale_type,
            sale_price: (self.object_sale_type != SALE_TYPE_NOT_FOR_SALE)
                .then(|| LindenAmount(u64::try_from(self.sale_price).unwrap_or(0))),
            category: self.category,
            inventory_serial: i16::try_from(self.inventory_serial).unwrap_or(i16::MAX),
            item_id: InventoryKey::from(Uuid::nil()),
            folder_id: None,
            from_task_id: None,
            aggregate_perms: 0,
            aggregate_perm_textures: 0,
            aggregate_perm_textures_owner: 0,
            name: self.name.clone(),
            description: self.description.clone(),
            touch_name: self.touch_name.clone(),
            sit_name: self.sit_name.clone(),
            texture_ids: Vec::new(),
        }
    }

    /// The prim's contents as the XML states them, from the task inventory a
    /// simulator holds for it.
    ///
    /// A take writes the prim's contents into the body — OpenSim's
    /// `WriteTaskInventory` — and this is the conversion for it. It is
    /// separate from [`from_object`](Self::from_object) because an object
    /// update carries no contents at all: what a prim holds reaches a viewer
    /// only through a `ReplyTaskInventory`, so the caller has to have asked.
    pub fn set_task_inventory(&mut self, serial: i16, items: &[TaskInventoryItem]) {
        self.inventory_serial = u32::try_from(serial).unwrap_or(0);
        self.task_inventory = items
            .iter()
            .map(|item| task_item(item, self.uuid, self.folder_id))
            .collect();
    }
}

impl SceneShape {
    /// The XML shape block of a live object.
    ///
    /// The two packed blobs are copied out of the wire form untouched, and the
    /// flexi / light / sculpt elements beside them are written from the
    /// *decoded* sub-blocks. OpenSim writes both too, and its reader applies
    /// the blob first and the elements after — so the two must agree, and here
    /// they do because they come from the same object.
    #[must_use]
    pub fn from_object(object: &Object) -> Self {
        let flexible = object.extra.flexible.as_ref();
        let light = object.extra.light.as_ref();
        let sculpt = object.extra.sculpt.as_ref();
        Self {
            profile_curve: object.shape.profile_curve,
            texture_entry: object.texture_entry.clone(),
            extra_params: object.extra_params.clone(),
            path_begin: object.shape.path_begin,
            path_curve: object.shape.path_curve,
            path_end: object.shape.path_end,
            path_radius_offset: object.shape.path_radius_offset,
            path_revolutions: object.shape.path_revolutions,
            path_scale_x: object.shape.path_scale_x,
            path_scale_y: object.shape.path_scale_y,
            path_shear_x: object.shape.path_shear_x,
            path_shear_y: object.shape.path_shear_y,
            path_skew: object.shape.path_skew,
            path_taper_x: object.shape.path_taper_x,
            path_taper_y: object.shape.path_taper_y,
            path_twist: object.shape.path_twist,
            path_twist_begin: object.shape.path_twist_begin,
            pcode: object.pcode,
            profile_begin: object.shape.profile_begin,
            profile_end: object.shape.profile_end,
            profile_hollow: object.shape.profile_hollow,
            state: object.state,
            last_attach_point: 0,
            sculpt_texture: sculpt.map_or_else(Uuid::nil, |sculpt| sculpt.texture.uuid()),
            sculpt_type: sculpt.map_or(0, |sculpt| sculpt.sculpt_type),
            flexi_softness: flexible.map_or(0, |flexible| i32::from(flexible.softness)),
            flexi_tension: flexible.map_or(0.0, |flexible| flexible.tension),
            flexi_drag: flexible.map_or(0.0, |flexible| flexible.air_friction),
            flexi_gravity: flexible.map_or(0.0, |flexible| flexible.gravity),
            flexi_wind: flexible.map_or(0.0, |flexible| flexible.wind_sensitivity),
            flexi_force_x: flexible.map_or(0.0, |flexible| flexible.user_force.x),
            flexi_force_y: flexible.map_or(0.0, |flexible| flexible.user_force.y),
            flexi_force_z: flexible.map_or(0.0, |flexible| flexible.user_force.z),
            light_color_r: light.map_or(0.0, |light| unit(light.color.first().copied())),
            light_color_g: light.map_or(0.0, |light| unit(light.color.get(1).copied())),
            light_color_b: light.map_or(0.0, |light| unit(light.color.get(2).copied())),
            // Not the wire's alpha: OpenSim constructs this at 1.0 and the wire
            // has nowhere to put it, because the alpha byte is the intensity.
            light_color_a: OPAQUE_LIGHT_ALPHA,
            light_radius: light.map_or(0.0, |light| light.radius),
            light_cutoff: light.map_or(0.0, |light| light.cutoff),
            light_falloff: light.map_or(0.0, |light| light.falloff),
            light_intensity: light.map_or(0.0, |light| unit(light.color.get(3).copied())),
            flexi_entry: flexible.is_some(),
            light_entry: light.is_some(),
            sculpt_entry: sculpt.is_some(),
            media: None,
            unknown: Vec::new(),
        }
    }

    /// The wire shape parameters this block states.
    #[must_use]
    pub const fn to_params(&self) -> PrimShapeParams {
        PrimShapeParams {
            path_curve: self.path_curve,
            profile_curve: self.profile_curve,
            path_begin: self.path_begin,
            path_end: self.path_end,
            path_scale_x: self.path_scale_x,
            path_scale_y: self.path_scale_y,
            path_shear_x: self.path_shear_x,
            path_shear_y: self.path_shear_y,
            path_twist: self.path_twist,
            path_twist_begin: self.path_twist_begin,
            path_radius_offset: self.path_radius_offset,
            path_taper_x: self.path_taper_x,
            path_taper_y: self.path_taper_y,
            path_revolutions: self.path_revolutions,
            path_skew: self.path_skew,
            profile_begin: self.profile_begin,
            profile_end: self.profile_end,
            profile_hollow: self.profile_hollow,
        }
    }

    /// The decoded `ExtraParams` sub-blocks this shape block states.
    ///
    /// Read out of the **elements**, not out of the packed blob beside them.
    /// That is the way round OpenSim reads them (the blob first, the elements
    /// over the top), and it is the way that still works for a body whose blob
    /// is empty — which a hand-written fixture's often is.
    #[must_use]
    pub fn to_extra_params(&self) -> ObjectExtraParams {
        ObjectExtraParams {
            flexible: self.flexi_entry.then(|| sl_proto::FlexibleData {
                softness: u8::try_from(self.flexi_softness).unwrap_or(0),
                tension: self.flexi_tension,
                air_friction: self.flexi_drag,
                gravity: self.flexi_gravity,
                wind_sensitivity: self.flexi_wind,
                user_force: Vector {
                    x: self.flexi_force_x,
                    y: self.flexi_force_y,
                    z: self.flexi_force_z,
                },
            }),
            light: self.light_entry.then(|| LightData {
                color: [
                    byte(self.light_color_r),
                    byte(self.light_color_g),
                    byte(self.light_color_b),
                    byte(self.light_intensity),
                ],
                radius: self.light_radius,
                cutoff: self.light_cutoff,
                falloff: self.light_falloff,
            }),
            sculpt: self
                .sculpt_entry
                .then(|| SculptData::new(self.sculpt_texture, self.sculpt_type)),
            ..ObjectExtraParams::default()
        }
    }
}

/// One task-inventory item as the XML states it.
///
/// `parent` is the prim holding it and `folder` the prim's contents folder,
/// because OpenSim writes both and an item does not carry either: a task item
/// belongs to whichever prim's block it is written inside.
fn task_item(item: &TaskInventoryItem, parent: Uuid, folder: Uuid) -> SceneTaskInventoryItem {
    SceneTaskInventoryItem {
        asset_id: item.asset_id.map_or_else(Uuid::nil, |asset| asset.uuid()),
        base_permissions: item.permissions.base.bits(),
        creation_date: u32::try_from(item.creation_date).unwrap_or(0),
        creator_id: item.creator_id.uuid(),
        creator_data: None,
        description: item.description.clone(),
        everyone_permissions: item.permissions.everyone.bits(),
        flags: item.flags,
        group_id: item.group.map_or_else(Uuid::nil, |group| group.uuid()),
        group_permissions: item.permissions.group.bits(),
        inv_type: item.inv_type.to_code(),
        item_id: item.item_id.uuid(),
        // Not the item's own id: `OldItemID` names what it was copied from, and
        // a simulator that has forgotten leaves it nil rather than repeating
        // the new one.
        old_item_id: Uuid::nil(),
        last_owner_id: item.last_owner_id.uuid(),
        name: item.name.clone(),
        next_permissions: item.permissions.next_owner.bits(),
        owner_id: item.owner.uuid(),
        current_permissions: item.permissions.owner.bits(),
        parent_id: folder,
        parent_part_id: parent,
        perms_granter: Uuid::nil(),
        perms_mask: 0,
        item_type: item.asset_type.to_code(),
        owner_changed: false,
    }
}

/// The hover-text colour as OpenSim stores it, from the bytes the wire carries
/// — the alpha inverted, and only the alpha. See the module docs.
fn stored_text_color(wire: [u8; 4]) -> [u8; 4] {
    let mut stored = wire;
    if let Some(alpha) = stored.get_mut(3) {
        *alpha = u8::MAX.saturating_sub(*alpha);
    }
    stored
}

/// The hover-text colour as the wire carries it, from what OpenSim stores —
/// the same inversion, which is its own inverse.
fn wire_text_color(stored: [u8; 4]) -> [u8; 4] {
    stored_text_color(stored)
}

/// A colour byte as the `0.0..=1.0` float OpenSim's light elements carry.
fn unit(channel: Option<u8>) -> f32 {
    f32::from(channel.unwrap_or(0)) / f32::from(u8::MAX)
}

/// A `0.0..=1.0` float as the colour byte the wire carries, rounding rather
/// than truncating so an exact `1.0` comes back as `255` and not `254`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is rounded and clamped into 0..=255 first, so the conversion is exact"
)]
fn byte(value: f32) -> u8 {
    (value * f32::from(u8::MAX)).round().clamp(0.0, 255.0) as u8
}

/// The `PrimFlags` bit that mirrors OpenSim's `AllowedDrop`
/// (`AllowInventoryDrop`, and the reference's `FLAGS_ALLOW_INVENTORY_DROP`).
const ALLOW_INVENTORY_DROP: u32 = 1 << 16;

/// `LLSaleInfo`'s "not for sale" code, which is what an object with no asking
/// price carries.
const SALE_TYPE_NOT_FOR_SALE: u8 = 0;

/// What OpenSim constructs `LightColorA` at, and the only value the wire can
/// ever produce for it (see the module docs).
const OPAQUE_LIGHT_ALPHA: f32 = 1.0;

/// Microseconds in a second: the wire's object birth stamp is in the first unit
/// and OpenSim's `CreationDate` in the second.
const MICROS_PER_SECOND: u64 = 1_000_000;

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{TextureEntry, TextureFace, TextureKey, decode_texture_entry};

    use super::*;

    /// An encode that would not read back.
    type TestError = Box<dyn core::error::Error>;

    /// The mirror of [`crate::bridge`]'s `the_text_carries_none_of_the_modern_prim`,
    /// and the reason this module exists: the same prim, the same list, through
    /// the format OpenSim actually writes — and every one of them comes back.
    ///
    /// A face's glow and its legacy material id, an `ExtraParams` light, a flexi
    /// path, a mesh, floating text, a media URL, a click action, a texture
    /// animation and a particle system are set on a live object, taken, encoded,
    /// decoded and rezzed. The text loses all ten; this loses none. That is the
    /// whole difference between the two `AssetType::Object` formats, stated
    /// where a change to either bridge would break it.
    #[test]
    fn the_xml_carries_the_whole_modern_prim() -> Result<(), TestError> {
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
        taken.text_color = [10, 20, 30, 40];
        taken.media_url = "http://example.invalid/".parse().ok();
        taken.click_action = 3;
        taken.extra = ObjectExtraParams {
            light: Some(LightData {
                color: [255, 200, 100, 128],
                radius: 10.0,
                cutoff: 0.0,
                falloff: 0.75,
            }),
            flexible: Some(sl_proto::FlexibleData {
                softness: 2,
                tension: 1.5,
                air_friction: 0.25,
                gravity: 0.5,
                wind_sensitivity: 0.125,
                user_force: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            }),
            sculpt: Some(SculptData::new(Uuid::from_u128(0x0E54), 5)),
            ..ObjectExtraParams::default()
        };
        taken.extra_params = sl_proto::encode_extra_params(&taken.extra);
        taken.particle_system = vec![1, 2, 3];
        taken.texture_anim = vec![4, 5, 6];

        let body = SceneObjectGroup::from_linkset(std::slice::from_ref(&taken)).encode();
        let rezzed = SceneObjectGroup::decode(&body)?.root.to_object(RezTarget {
            region_handle: taken.region_handle,
            local_id: taken.local_id,
            full_id: Uuid::from_u128(0x0DD),
            parent_id: taken.parent_id,
        });

        // The face keeps its texture, its glow *and* its legacy material: the
        // `TextureEntry` blob crosses this format without being looked at.
        // Compared against the *wire* face rather than the one it was built
        // from, because glow is a quantized byte on the wire -- that rounding
        // is `encode_texture_entry`'s and would be lost in this assertion.
        let sent = decode_texture_entry(&taken.texture_entry, 6);
        let faces = decode_texture_entry(&rezzed.texture_entry, 6);
        assert_eq!(faces, sent, "the whole texture entry crosses untouched");
        let face = faces.faces.first().ok_or("no face came back")?;
        assert_eq!(face.texture_id, glowing.texture_id);
        assert!(face.glow > 0.0, "glow is carried: {}", face.glow);
        assert_eq!(
            face.material_id, glowing.material_id,
            "the material id is carried"
        );

        // The whole of the rest of the list.
        assert_eq!(rezzed.text, "hover text");
        assert_eq!(rezzed.text_color, taken.text_color, "and its colour");
        assert_eq!(rezzed.media_url, taken.media_url);
        assert_eq!(rezzed.click_action, 3);
        assert_eq!(rezzed.extra, taken.extra, "the whole ExtraParams block");
        assert_eq!(rezzed.extra_params, taken.extra_params, "blob and all");
        assert_eq!(rezzed.particle_system, taken.particle_system);
        assert_eq!(rezzed.texture_anim, taken.texture_anim);
        assert_eq!(rezzed.shape, taken.shape);
        assert_eq!(rezzed.scale, taken.scale);
        Ok(())
    }

    /// A **child** prim is serialised as one: OpenSim states the group's
    /// position on every part and the offset from it separately, and the wire
    /// carries only the offset. A rez that read the wrong one would pile a
    /// taken linkset's children at the root.
    #[test]
    fn a_child_prim_keeps_its_offset_and_the_group_keeps_its_position() -> Result<(), TestError> {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(1))); 6],
        };
        let mut root = crate::test_support::box_object(Uuid::from_u128(0x5678), &entry);
        root.local_id = sl_proto::RegionLocalObjectId(10);
        let mut child = crate::test_support::box_object(Uuid::from_u128(0x9ABC), &entry);
        child.local_id = sl_proto::RegionLocalObjectId(11);
        child.parent_id = root.local_id;
        child.motion.position = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.5,
        };

        let group = SceneObjectGroup::from_linkset(&[root.clone(), child.clone()]);
        let reread = SceneObjectGroup::decode(&group.encode())?;
        let filed = reread
            .other_parts
            .first()
            .ok_or("the child was not filed")?;
        assert_eq!(filed.link_num, 2, "a child counts up from two");
        assert_eq!(reread.root.link_num, 1, "a linkset root is link one");
        assert_eq!(
            filed.group_position, root.motion.position,
            "the child states the group's position"
        );
        assert_eq!(
            filed.offset_position, child.motion.position,
            "and its own offset from it"
        );
        let rezzed = filed.to_object(RezTarget {
            region_handle: child.region_handle,
            local_id: sl_proto::RegionLocalObjectId(21),
            full_id: Uuid::from_u128(0x9ABC),
            parent_id: sl_proto::RegionLocalObjectId(20),
        });
        assert_eq!(rezzed.motion.position, child.motion.position);
        Ok(())
    }

    /// A solitary prim is link zero, not link one. `llGetLinkNumber` says so,
    /// and a script that branched on it would take the linkset path for an
    /// object that has no linkset.
    #[test]
    fn a_solitary_prim_is_link_zero() {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(1))); 6],
        };
        let alone = crate::test_support::box_object(Uuid::from_u128(0x1111), &entry);
        let group = SceneObjectGroup::from_linkset(std::slice::from_ref(&alone));
        assert_eq!(group.root.link_num, 0);
        assert!(group.other_parts.is_empty());
    }

    /// Hover-text alpha is inverted between the two, and it is the *only*
    /// channel that is. An opaque hover text is `0` on the wire and `255` in
    /// the XML; a bridge that copied it straight through would file every
    /// visible label as an invisible one.
    #[test]
    fn hover_text_alpha_is_inverted_and_nothing_else_is() {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(1))); 6],
        };
        let mut opaque = crate::test_support::box_object(Uuid::from_u128(0x2222), &entry);
        opaque.text_color = [10, 20, 30, 0];
        let part = SceneObjectPart::from_object(&opaque);
        assert_eq!(part.text_color, [10, 20, 30, 255]);
        assert_eq!(wire_text_color(part.text_color), opaque.text_color);
    }

    /// A light's intensity travels in the wire colour's alpha byte — OpenSim's
    /// own `ExtraParamsToBytes` says so — so the XML's `LightColorA` is not
    /// that byte and stays at the 1.0 OpenSim constructs it with. Reading one
    /// as the other would file a half-bright lamp as a half-transparent one.
    #[test]
    fn a_lights_alpha_byte_is_its_intensity() {
        let entry = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(1))); 6],
        };
        let mut lamp = crate::test_support::box_object(Uuid::from_u128(0x3333), &entry);
        lamp.extra = ObjectExtraParams {
            light: Some(LightData {
                color: [255, 255, 255, 128],
                radius: 5.0,
                cutoff: 0.0,
                falloff: 1.0,
            }),
            ..ObjectExtraParams::default()
        };
        let shape = SceneShape::from_object(&lamp);
        assert_eq!(shape.light_color_a.to_bits(), 1.0_f32.to_bits());
        assert!(
            (shape.light_intensity - 128.0 / 255.0).abs() < f32::EPSILON,
            "the alpha byte did not become the intensity: {}",
            shape.light_intensity
        );
        assert_eq!(shape.to_extra_params(), lamp.extra);
    }
}
