//! Writing a [`SceneObjectGroup`] back out as the XML OpenSim stores.
//!
//! Element names, their order, which ones are omitted when they hold a default,
//! and the base64 of the packed blobs are all transcribed from
//! `SceneObjectSerializer.ToOriginalXmlFormat` / `SOPToXml2` / `WriteShape` /
//! `WriteTaskInventory`. The output carries no XML declaration, no indentation
//! and no line breaks, because OpenSim writes through an `XmlTextWriter` left
//! at its defaults and that is what one produces.
//!
//! Two deliberate differences from a byte-for-byte transcription, both of which
//! leave the document semantically identical:
//!
//! - numbers are Rust's shortest round-tripping float formatting, the same
//!   choice [`crate::encode`] makes for the text;
//! - the elements this module does not model
//!   ([`UnknownElement`]) are written back at the
//!   **end** of the part they came from rather than where they stood.
//!   OpenSim's reader dispatches each child element through a name → handler
//!   dictionary and ignores the ones it has no handler for
//!   (`ExternalRepresentationUtils.ExecuteReadProcessors`), so it reads the two
//!   orders the same way.

use core::fmt::Write as _;

use base64::Engine as _;
use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use super::model::{
    SceneObjectGroup, SceneObjectPart, SceneShape, SceneTaskInventoryItem, UnknownElement,
    prim_flags,
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "encode owns its `impl SceneObjectGroup` block, apart from the model's canonical impl"
)]
impl SceneObjectGroup {
    /// Encodes the group as the bytes a grid would serve for it.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.encode_to_string().into_bytes()
    }

    /// Encodes the group as its XML form.
    #[must_use]
    pub fn encode_to_string(&self) -> String {
        let mut out = String::new();
        out.push_str("<SceneObjectGroup>");
        out.push_str("<RootPart>");
        write_part(&mut out, &self.root);
        out.push_str("</RootPart>");
        out.push_str("<OtherParts>");
        for part in &self.other_parts {
            out.push_str("<Part>");
            write_part(&mut out, part);
            out.push_str("</Part>");
        }
        out.push_str("</OtherParts>");
        write_unknown(&mut out, &self.unknown);
        out.push_str("</SceneObjectGroup>");
        out
    }
}

/// Writes one `<SceneObjectPart>`, in `SOPToXml2`'s own element order.
///
/// Every `write!` here is to a `String`, whose `fmt::Write` cannot fail, so the
/// results are bound and dropped rather than propagated — the alternative is a
/// `fmt::Result` on every function in this module for an error that cannot
/// happen.
fn write_part(out: &mut String, part: &SceneObjectPart) {
    out.push_str(
        r#"<SceneObjectPart xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">"#,
    );
    write_bool(out, "AllowedDrop", part.allowed_drop);
    write_uuid(out, "CreatorID", part.creator_id);
    if let Some(creator_data) = &part.creator_data {
        write_text(out, "CreatorData", creator_data);
    }
    write_uuid(out, "FolderID", part.folder_id);
    write_number(out, "InventorySerial", part.inventory_serial);
    write_task_inventory(out, &part.task_inventory);
    write_uuid(out, "UUID", part.uuid);
    write_number(out, "LocalId", part.local_id);
    write_text(out, "Name", &part.name);
    write_number(out, "Material", part.material);
    write_bool(out, "PassTouches", part.pass_touches);
    write_bool(out, "PassCollisions", part.pass_collisions);
    write_number(out, "RegionHandle", part.region_handle);
    write_number(out, "ScriptAccessPin", part.script_access_pin);
    write_vector(out, "GroupPosition", &part.group_position);
    write_vector(out, "OffsetPosition", &part.offset_position);
    write_quaternion(out, "RotationOffset", &part.rotation_offset);
    write_vector(out, "Velocity", &part.velocity);
    write_vector(out, "AngularVelocity", &part.angular_velocity);
    write_vector(out, "Acceleration", &part.acceleration);
    write_text(out, "Description", &part.description);
    write_color(out, part.text_color);
    write_text(out, "Text", &part.text);
    write_text(out, "SitName", &part.sit_name);
    write_text(out, "TouchName", &part.touch_name);
    write_number(out, "LinkNum", part.link_num);
    write_number(out, "ClickAction", part.click_action);
    write_shape(out, &part.shape);
    write_vector(out, "Scale", &part.scale);
    write_quaternion(out, "SitTargetOrientation", &part.sit_target_orientation);
    write_vector(out, "SitTargetPosition", &part.sit_target_position);
    write_vector(out, "SitTargetPositionLL", &part.sit_target_position_ll);
    write_quaternion(
        out,
        "SitTargetOrientationLL",
        &part.sit_target_orientation_ll,
    );
    if let Some(stand_target) = &part.stand_target {
        write_vector(out, "StandTarget", stand_target);
    }
    write_number(out, "ParentID", part.parent_id);
    write_number(out, "CreationDate", part.creation_date);
    write_number(out, "Category", part.category);
    write_number(out, "SalePrice", part.sale_price);
    write_number(out, "ObjectSaleType", part.object_sale_type);
    write_number(out, "OwnershipCost", part.ownership_cost);
    write_uuid(out, "GroupID", part.group_id);
    write_uuid(out, "OwnerID", part.owner_id);
    write_uuid(out, "LastOwnerID", part.last_owner_id);
    write_uuid(out, "RezzerID", part.rezzer_id);
    write_number(out, "BaseMask", part.base_mask);
    write_number(out, "OwnerMask", part.owner_mask);
    write_number(out, "GroupMask", part.group_mask);
    write_number(out, "EveryoneMask", part.everyone_mask);
    write_number(out, "NextOwnerMask", part.next_owner_mask);
    write_text(out, "Flags", &prim_flags::to_names(part.flags));
    write_uuid(out, "CollisionSound", part.collision_sound);
    write_float(out, "CollisionSoundVolume", part.collision_sound_volume);
    if let Some(media_url) = &part.media_url {
        write_text(out, "MediaUrl", media_url);
    }
    write_vector(out, "AttachedPos", &part.attached_pos);
    write_bytes(out, "TextureAnimation", &part.texture_animation);
    write_bytes(out, "ParticleSystem", &part.particle_system);
    for (index, price) in part.pay_price.iter().enumerate() {
        write_number(out, &format!("PayPrice{index}"), *price);
    }
    write_float(out, "Buoyancy", part.buoyancy);
    write_vector(out, "Force", &part.force);
    write_vector(out, "Torque", &part.torque);
    write_bool(out, "VolumeDetectActive", part.volume_detect_active);
    if part.rotation_axis_locks != 0 {
        write_number(out, "RotationAxisLocks", part.rotation_axis_locks);
    }
    write_number(out, "PhysicsShapeType", part.physics_shape_type);
    if let Some(density) = part.density {
        write_float(out, "Density", density);
    }
    if let Some(friction) = part.friction {
        write_float(out, "Friction", friction);
    }
    if let Some(bounce) = part.bounce {
        write_float(out, "Bounce", bounce);
    }
    if let Some(gravity_modifier) = part.gravity_modifier {
        write_float(out, "GravityModifier", gravity_modifier);
    }
    write_vector(out, "CameraEyeOffset", &part.camera_eye_offset);
    write_vector(out, "CameraAtOffset", &part.camera_at_offset);
    write_uuid(out, "SoundID", part.sound_id);
    write_float(out, "SoundGain", part.sound_gain);
    write_number(out, "SoundFlags", part.sound_flags);
    write_float(out, "SoundRadius", part.sound_radius);
    write_bool(out, "SoundQueueing", part.sound_queueing);
    if let Some(sit_act_range) = part.sit_act_range {
        write_float(out, "SitActRange", sit_act_range);
    }
    write_unknown(out, &part.unknown);
    out.push_str("</SceneObjectPart>");
}

/// Writes the `<Shape>` block, in `WriteShape`'s own element order.
///
/// `ProfileShape` and `HollowShape` are **derived** from
/// [`profile_curve`](SceneShape::profile_curve) rather than stored beside it,
/// because that is how OpenSim's own `PrimitiveBaseShape.ProfileCurve` property
/// works: it is the two enums composed, not a field of its own.
fn write_shape(out: &mut String, shape: &SceneShape) {
    out.push_str("<Shape>");
    write_number(out, "ProfileCurve", shape.profile_curve);
    write_bytes(out, "TextureEntry", &shape.texture_entry);
    write_bytes(out, "ExtraParams", &shape.extra_params);
    write_number(out, "PathBegin", shape.path_begin);
    write_number(out, "PathCurve", shape.path_curve);
    write_number(out, "PathEnd", shape.path_end);
    write_number(out, "PathRadiusOffset", shape.path_radius_offset);
    write_number(out, "PathRevolutions", shape.path_revolutions);
    write_number(out, "PathScaleX", shape.path_scale_x);
    write_number(out, "PathScaleY", shape.path_scale_y);
    write_number(out, "PathShearX", shape.path_shear_x);
    write_number(out, "PathShearY", shape.path_shear_y);
    write_number(out, "PathSkew", shape.path_skew);
    write_number(out, "PathTaperX", shape.path_taper_x);
    write_number(out, "PathTaperY", shape.path_taper_y);
    write_number(out, "PathTwist", shape.path_twist);
    write_number(out, "PathTwistBegin", shape.path_twist_begin);
    write_number(out, "PCode", shape.pcode);
    write_number(out, "ProfileBegin", shape.profile_begin);
    write_number(out, "ProfileEnd", shape.profile_end);
    write_number(out, "ProfileHollow", shape.profile_hollow);
    write_number(out, "State", shape.state);
    write_number(out, "LastAttachPoint", shape.last_attach_point);
    write_text(out, "ProfileShape", profile_shape_name(shape.profile_curve));
    write_text(out, "HollowShape", hollow_shape_name(shape.profile_curve));
    write_uuid(out, "SculptTexture", shape.sculpt_texture);
    write_number(out, "SculptType", shape.sculpt_type);
    write_number(out, "FlexiSoftness", shape.flexi_softness);
    write_float(out, "FlexiTension", shape.flexi_tension);
    write_float(out, "FlexiDrag", shape.flexi_drag);
    write_float(out, "FlexiGravity", shape.flexi_gravity);
    write_float(out, "FlexiWind", shape.flexi_wind);
    write_float(out, "FlexiForceX", shape.flexi_force_x);
    write_float(out, "FlexiForceY", shape.flexi_force_y);
    write_float(out, "FlexiForceZ", shape.flexi_force_z);
    write_float(out, "LightColorR", shape.light_color_r);
    write_float(out, "LightColorG", shape.light_color_g);
    write_float(out, "LightColorB", shape.light_color_b);
    write_float(out, "LightColorA", shape.light_color_a);
    write_float(out, "LightRadius", shape.light_radius);
    write_float(out, "LightCutoff", shape.light_cutoff);
    write_float(out, "LightFalloff", shape.light_falloff);
    write_float(out, "LightIntensity", shape.light_intensity);
    write_bool(out, "FlexiEntry", shape.flexi_entry);
    write_bool(out, "LightEntry", shape.light_entry);
    write_bool(out, "SculptEntry", shape.sculpt_entry);
    if let Some(media) = &shape.media {
        write_text(out, "Media", media);
    }
    write_unknown(out, &shape.unknown);
    out.push_str("</Shape>");
}

/// Writes the `<TaskInventory>` block, or nothing at all when the prim is
/// empty — which is what OpenSim does, and why an empty prim's body has no
/// such element to read back.
fn write_task_inventory(out: &mut String, items: &[SceneTaskInventoryItem]) {
    if items.is_empty() {
        return;
    }
    out.push_str("<TaskInventory>");
    for item in items {
        out.push_str("<TaskInventoryItem>");
        write_uuid(out, "AssetID", item.asset_id);
        write_number(out, "BasePermissions", item.base_permissions);
        write_number(out, "CreationDate", item.creation_date);
        write_uuid(out, "CreatorID", item.creator_id);
        if let Some(creator_data) = &item.creator_data {
            write_text(out, "CreatorData", creator_data);
        }
        write_text(out, "Description", &item.description);
        write_number(out, "EveryonePermissions", item.everyone_permissions);
        write_number(out, "Flags", item.flags);
        write_uuid(out, "GroupID", item.group_id);
        write_number(out, "GroupPermissions", item.group_permissions);
        write_number(out, "InvType", item.inv_type);
        write_uuid(out, "ItemID", item.item_id);
        write_uuid(out, "OldItemID", item.old_item_id);
        write_uuid(out, "LastOwnerID", item.last_owner_id);
        write_text(out, "Name", &item.name);
        write_number(out, "NextPermissions", item.next_permissions);
        write_uuid(out, "OwnerID", item.owner_id);
        write_number(out, "CurrentPermissions", item.current_permissions);
        write_uuid(out, "ParentID", item.parent_id);
        write_uuid(out, "ParentPartID", item.parent_part_id);
        write_uuid(out, "PermsGranter", item.perms_granter);
        write_number(out, "PermsMask", item.perms_mask);
        write_number(out, "Type", item.item_type);
        write_bool(out, "OwnerChanged", item.owner_changed);
        out.push_str("</TaskInventoryItem>");
    }
    out.push_str("</TaskInventory>");
}

/// Writes the hover text's `<Color>` block, whose four channels are bytes
/// written as integers — OpenSim's `Color` is a `System.Drawing.Color`, and its
/// reader casts each float straight back to a byte.
fn write_color(out: &mut String, color: [u8; 4]) {
    out.push_str("<Color>");
    for (name, channel) in ["R", "G", "B", "A"].into_iter().zip(color) {
        write_number(out, name, channel);
    }
    out.push_str("</Color>");
}

/// Writes a `<Name><UUID>…</UUID></Name>` pair — how OpenSim writes every id
/// unless it was asked for the older `Guid` spelling, which nothing here asks
/// for.
fn write_uuid(out: &mut String, name: &str, id: Uuid) {
    let _written = write!(out, "<{name}><UUID>{id}</UUID></{name}>");
}

/// Writes a `<Name><X/><Y/><Z/></Name>` vector.
fn write_vector(out: &mut String, name: &str, vector: &Vector) {
    let _written = write!(out, "<{name}>");
    write_float(out, "X", vector.x);
    write_float(out, "Y", vector.y);
    write_float(out, "Z", vector.z);
    let _written = write!(out, "</{name}>");
}

/// Writes a `<Name><X/><Y/><Z/><W/></Name>` quaternion. The scalar component is
/// `W` here and `s` in [`Rotation`], which is the only spelling difference.
fn write_quaternion(out: &mut String, name: &str, rotation: &Rotation) {
    let _written = write!(out, "<{name}>");
    write_float(out, "X", rotation.x);
    write_float(out, "Y", rotation.y);
    write_float(out, "Z", rotation.z);
    write_float(out, "W", rotation.s);
    let _written = write!(out, "</{name}>");
}

/// Writes an element holding base64 of `data`.
///
/// An empty blob is still written, because OpenSim's writer has no "omit it"
/// branch for one — and it is written as an open/close **pair** rather than
/// self-closing, unlike an empty string. That is not an inconsistency here but
/// a transcription of one: `XmlTextWriter.WriteString("")` writes nothing and
/// leaves the start tag to collapse, while `WriteBase64` closes the start tag
/// before it looks at the count.
fn write_bytes(out: &mut String, name: &str, data: &[u8]) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let _written = write!(out, "<{name}>{encoded}</{name}>");
}

/// Writes an element holding a number.
fn write_number(out: &mut String, name: &str, value: impl core::fmt::Display) {
    let _written = write!(out, "<{name}>{value}</{name}>");
}

/// Writes an element holding a float, in Rust's shortest round-tripping form
/// (see the module docs).
fn write_float(out: &mut String, name: &str, value: f32) {
    let _written = write!(out, "<{name}>{value}</{name}>");
}

/// Writes an element holding `true` or `false` — C#'s `ToString().ToLower()`,
/// which is what OpenSim's `Util.ReadBoolean` reads back.
fn write_bool(out: &mut String, name: &str, value: bool) {
    write_number(out, name, value);
}

/// Writes an element holding escaped text, self-closing when the text is empty
/// — which is what an `XmlTextWriter` produces for an empty string.
fn write_text(out: &mut String, name: &str, text: &str) {
    if text.is_empty() {
        let _written = write!(out, "<{name} />");
        return;
    }
    let _written = write!(out, "<{name}>");
    push_escaped(out, text);
    let _written = write!(out, "</{name}>");
}

/// Appends `text` with the XML metacharacters escaped.
///
/// Exactly what a `System.Xml.XmlTextWriter` escapes in element text, and no
/// more: quotes and apostrophes are legal there and it leaves them alone, so
/// escaping them would make a document that reads back identically but does not
/// *look* like OpenSim's. The carriage return is escaped because an XML parser
/// would otherwise normalise it away, and `XmlTextWriter` escapes it for that
/// reason.
fn push_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            other => out.push(other),
        }
    }
}

/// Re-emits the elements this module does not model, exactly as they were read.
fn write_unknown(out: &mut String, unknown: &[UnknownElement]) {
    for element in unknown {
        out.push_str(&element.xml);
    }
}

/// `ProfileShape`'s spelling for the low nibble of a profile curve byte: the
/// enum member's name, or the number itself for a nibble the enum does not
/// define — which is what C# prints for an undefined enum value, and what
/// `Enum.Parse` reads back.
const fn profile_shape_name(profile_curve: u8) -> &'static str {
    match profile_curve & 0x0f {
        0 => "Circle",
        1 => "Square",
        2 => "IsometricTriangle",
        3 => "EquilateralTriangle",
        4 => "RightTriangle",
        5 => "HalfCircle",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        10 => "10",
        11 => "11",
        12 => "12",
        13 => "13",
        14 => "14",
        _fifteen => "15",
    }
}

/// `HollowShape`'s spelling for the high nibble of a profile curve byte, on the
/// same rule as [`profile_shape_name`]. The enum's values are the nibble in
/// place (`Circle` is 16, not 1), so the byte is masked rather than shifted.
const fn hollow_shape_name(profile_curve: u8) -> &'static str {
    match profile_curve & 0xf0 {
        0 => "Same",
        16 => "Circle",
        32 => "Square",
        48 => "Triangle",
        64 => "64",
        80 => "80",
        96 => "96",
        112 => "112",
        128 => "128",
        144 => "144",
        160 => "160",
        176 => "176",
        192 => "192",
        208 => "208",
        224 => "224",
        _two_hundred_and_forty => "240",
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The wrappers are the whole difference between this format and OpenSim's
    /// `Xml2` one, where every part is a bare `<SceneObjectPart>` under the
    /// group: a reader that followed `RootPart` / `OtherParts` / `Part` into
    /// the wrong nesting would find no parts at all.
    #[test]
    fn a_linkset_is_wrapped_root_part_then_other_parts() {
        let group = SceneObjectGroup {
            root: SceneObjectPart {
                name: "root".to_owned(),
                ..SceneObjectPart::default()
            },
            other_parts: vec![SceneObjectPart {
                name: "child".to_owned(),
                ..SceneObjectPart::default()
            }],
            unknown: Vec::new(),
        };
        let xml = group.encode_to_string();
        let root_at = xml.find("<RootPart><SceneObjectPart ");
        let others_at = xml.find("<OtherParts><Part><SceneObjectPart ");
        assert!(root_at.is_some(), "no wrapped root part in {xml}");
        assert!(others_at.is_some(), "no wrapped child part in {xml}");
        assert!(root_at < others_at, "the root is not written first");
        assert!(xml.contains("<Name>root</Name>"), "root name missing");
        assert!(xml.contains("<Name>child</Name>"), "child name missing");
    }

    /// A solitary prim still gets the `<OtherParts>` element, empty. OpenSim
    /// writes it unconditionally, and its reader walks into it before deciding
    /// there is nothing there.
    #[test]
    fn a_solitary_prim_still_has_an_empty_other_parts() {
        let xml = SceneObjectGroup::single(SceneObjectPart::default()).encode_to_string();
        assert!(
            xml.contains("<OtherParts></OtherParts>"),
            "no empty OtherParts in {xml}"
        );
    }

    /// Text is escaped, and only where XML needs it: a prim named with an
    /// ampersand and an apostrophe writes the first escaped and the second as
    /// itself, which is what OpenSim's own writer produces.
    #[test]
    fn a_name_is_escaped_the_way_the_reference_writer_escapes_it() {
        let part = SceneObjectPart {
            name: "Bell & Hammer's <box>".to_owned(),
            ..SceneObjectPart::default()
        };
        let xml = SceneObjectGroup::single(part).encode_to_string();
        assert!(
            xml.contains("<Name>Bell &amp; Hammer's &lt;box&gt;</Name>"),
            "escaping is not the reference's in {xml}"
        );
    }

    /// An empty string is a self-closing element, not an open/close pair: that
    /// is what an `XmlTextWriter` writes, and a prim with no description is the
    /// common case rather than the odd one.
    #[test]
    fn an_empty_string_is_a_self_closing_element() {
        let xml = SceneObjectGroup::single(SceneObjectPart::default()).encode_to_string();
        assert!(
            xml.contains("<Description />"),
            "no self-closed empty in {xml}"
        );
    }

    /// The two nibbles of the profile curve are written as their enum names,
    /// because that is what OpenSim's reader parses them back as — and the
    /// hollow enum's values are the nibble in place, so a hollow *circle* is
    /// `Circle` from the high nibble and not from the low one.
    #[test]
    fn the_profile_curve_is_spelled_as_its_two_enums() {
        let shape = SceneShape {
            // A hollow-circle cut through a square profile.
            profile_curve: 0x11,
            ..SceneShape::default()
        };
        let xml = SceneObjectGroup::single(SceneObjectPart {
            shape,
            ..SceneObjectPart::default()
        })
        .encode_to_string();
        assert!(xml.contains("<ProfileCurve>17</ProfileCurve>"), "{xml}");
        assert!(xml.contains("<ProfileShape>Square</ProfileShape>"), "{xml}");
        assert!(xml.contains("<HollowShape>Circle</HollowShape>"), "{xml}");
    }

    /// The packed blobs go out as base64, which is the whole reason this format
    /// carries what the text cannot: the bytes are not interpreted at all.
    #[test]
    fn the_packed_blobs_are_base64_of_exactly_what_they_were_given() {
        let shape = SceneShape {
            extra_params: vec![1, 0x10, 0x05],
            texture_entry: vec![0xff],
            ..SceneShape::default()
        };
        let xml = SceneObjectGroup::single(SceneObjectPart {
            shape,
            ..SceneObjectPart::default()
        })
        .encode_to_string();
        assert!(xml.contains("<ExtraParams>ARAF</ExtraParams>"), "{xml}");
        assert!(xml.contains("<TextureEntry>/w==</TextureEntry>"), "{xml}");
    }

    /// A prim with nothing in it has no `<TaskInventory>` element at all, which
    /// is what makes the element's absence readable as "empty" rather than as
    /// "this writer forgot".
    #[test]
    fn an_empty_prim_writes_no_task_inventory_element() {
        let xml = SceneObjectGroup::single(SceneObjectPart::default()).encode_to_string();
        assert!(!xml.contains("TaskInventory"), "{xml}");
    }

    /// The four physics tunables are written only when they differ from
    /// OpenSim's defaults, which is how OpenSim writes them — a body that
    /// stated all four would be one a `Density` default could never be read
    /// out of.
    #[test]
    fn the_physics_tunables_are_omitted_at_their_defaults() {
        let plain = SceneObjectGroup::single(SceneObjectPart::default()).encode_to_string();
        for element in ["Density", "Friction", "Bounce", "GravityModifier"] {
            assert!(
                !plain.contains(element),
                "{element} written by default: {plain}"
            );
        }
        let heavy = SceneObjectGroup::single(SceneObjectPart {
            density: Some(2000.0),
            ..SceneObjectPart::default()
        })
        .encode_to_string();
        assert!(heavy.contains("<Density>2000</Density>"), "{heavy}");
    }
}
