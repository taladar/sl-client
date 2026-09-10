//! Reading OpenSim's `<SceneObjectGroup>` XML back into a
//! [`SceneObjectGroup`].
//!
//! The element → field mapping is OpenSim's own processor table
//! (`SceneObjectSerializer`'s `m_SOPXmlProcessors`, `m_ShapeXmlProcessors` and
//! `m_TaskInventoryXmlProcessors`) read the other way round, and like OpenSim's
//! this reader is **order-independent**: it dispatches on each child element's
//! name rather than expecting a sequence. An element it has no field for is
//! kept verbatim ([`UnknownElement`]) instead of dropped.
//!
//! Where it is stricter than OpenSim, deliberately: a number outside its
//! element's type is an error here and a silent C# cast there
//! (`(byte)ReadElementContentAsInt` turns 300 into 44), and a `Flags` name the
//! table does not know is an error here where OpenSim logs it and leaves the
//! prim's flags at their default. Neither is a body OpenSim would ever write,
//! and a decoder that quietly changed a value would make the crate's round-trip
//! tests pass over a body that is not the one it was given.

use base64::Engine as _;
use roxmltree::Node;
use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use super::model::{
    SceneObjectGroup, SceneObjectPart, SceneShape, SceneTaskInventoryItem, UnknownElement,
    prim_flags,
};

/// What can go wrong reading a `<SceneObjectGroup>` body.
#[derive(Debug, thiserror::Error)]
pub enum SceneObjectXmlError {
    /// The body is not valid UTF-8.
    #[error("scene object XML is not valid UTF-8: {source}")]
    NotUtf8 {
        /// The underlying decode error.
        source: core::str::Utf8Error,
    },
    /// The body is not well-formed XML, or is nested past what the parser will
    /// walk.
    #[error("scene object XML is malformed: {source}")]
    Malformed {
        /// The underlying parse error.
        source: roxmltree::Error,
    },
    /// The document's root element is not `<SceneObjectGroup>`.
    #[error("expected a SceneObjectGroup document, found {found:?}")]
    NotAGroup {
        /// The root element that was found instead.
        found: String,
    },
    /// A structural element the format requires is missing.
    #[error("scene object XML has no {element}")]
    Missing {
        /// The element that should have been there.
        element: &'static str,
    },
    /// An integer element could not be read, or does not fit the type OpenSim
    /// keeps it in.
    #[error("invalid {field} integer {value:?}")]
    InvalidInteger {
        /// The element being read.
        field: String,
        /// The offending text.
        value: String,
    },
    /// A floating-point element could not be read.
    #[error("invalid {field} number {value:?}")]
    InvalidNumber {
        /// The element being read.
        field: String,
        /// The offending text.
        value: String,
    },
    /// A boolean element is neither `true` nor `false`.
    #[error("invalid {field} boolean {value:?}")]
    InvalidBoolean {
        /// The element being read.
        field: String,
        /// The offending text.
        value: String,
    },
    /// An id element could not be read.
    #[error("invalid {field} id {value:?}")]
    InvalidUuid {
        /// The element being read.
        field: String,
        /// The offending text.
        value: String,
    },
    /// A base64 element could not be decoded.
    #[error("invalid {field} base64: {source}")]
    InvalidBase64 {
        /// The element being read.
        field: String,
        /// The underlying decode error.
        source: base64::DecodeError,
    },
    /// A `Flags` element names a prim flag OpenSim's own enum does not have.
    #[error("unknown prim flag {name:?}")]
    UnknownPrimFlag {
        /// The offending name.
        name: String,
    },
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "decode owns its `impl SceneObjectGroup` block, apart from the model's canonical impl"
)]
impl SceneObjectGroup {
    /// Reads a group from the bytes a grid serves for it.
    ///
    /// # Errors
    ///
    /// [`SceneObjectXmlError`] when the bytes are not UTF-8, not well-formed
    /// XML, not a `<SceneObjectGroup>`, or hold an element whose content does
    /// not read as the type OpenSim keeps it in.
    pub fn decode(bytes: &[u8]) -> Result<Self, SceneObjectXmlError> {
        let text = core::str::from_utf8(bytes)
            .map_err(|source| SceneObjectXmlError::NotUtf8 { source })?;
        Self::parse(text)
    }

    /// Reads a group from its XML form.
    ///
    /// # Errors
    ///
    /// As [`decode`](Self::decode), minus the UTF-8 case.
    pub fn parse(text: &str) -> Result<Self, SceneObjectXmlError> {
        let document = sl_llsd::parse_guarded_xml(text)
            .map_err(|source| SceneObjectXmlError::Malformed { source })?;
        let group = document.root_element();
        if group.tag_name().name() != "SceneObjectGroup" {
            return Err(SceneObjectXmlError::NotAGroup {
                found: group.tag_name().name().to_owned(),
            });
        }
        let mut root = None;
        let mut other_parts = Vec::new();
        let mut unknown = Vec::new();
        for child in group.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "RootPart" => {
                    let part = only_part(child).ok_or(SceneObjectXmlError::Missing {
                        element: "a SceneObjectPart inside RootPart",
                    })?;
                    root = Some(parse_part(part, text)?);
                }
                "OtherParts" => {
                    for wrapper in child.children().filter(Node::is_element) {
                        let part = only_part(wrapper).ok_or(SceneObjectXmlError::Missing {
                            element: "a SceneObjectPart inside Part",
                        })?;
                        other_parts.push(parse_part(part, text)?);
                    }
                }
                _unmodelled => unknown.push(capture(child, text)),
            }
        }
        Ok(Self {
            root: root.ok_or(SceneObjectXmlError::Missing {
                element: "a RootPart",
            })?,
            other_parts,
            unknown,
        })
    }
}

/// The `<SceneObjectPart>` inside a `<RootPart>` or `<Part>` wrapper.
fn only_part<'a>(wrapper: Node<'a, 'a>) -> Option<Node<'a, 'a>> {
    wrapper
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "SceneObjectPart")
}

/// Reads one `<SceneObjectPart>`, dispatching on each child element's name the
/// way OpenSim's processor table does.
///
/// Fields the body does not mention keep their [`SceneObjectPart::default`]
/// value, which is what OpenSim leaves them at too: its reader starts from a
/// freshly constructed part and only writes what it finds.
fn parse_part(node: Node<'_, '_>, text: &str) -> Result<SceneObjectPart, SceneObjectXmlError> {
    let mut part = SceneObjectPart::default();
    for child in node.children().filter(Node::is_element) {
        let name = child.tag_name().name();
        match name {
            "AllowedDrop" => part.allowed_drop = boolean(child, name)?,
            "CreatorID" => part.creator_id = id(child, name)?,
            "CreatorData" => part.creator_data = Some(element_text(child)),
            "FolderID" => part.folder_id = id(child, name)?,
            "InventorySerial" => part.inventory_serial = number(child, name)?,
            "TaskInventory" => part.task_inventory = parse_task_inventory(child)?,
            "UUID" => part.uuid = id(child, name)?,
            "LocalId" => part.local_id = number(child, name)?,
            "Name" => part.name = element_text(child),
            "Material" => part.material = number(child, name)?,
            "PassTouches" => part.pass_touches = boolean(child, name)?,
            "PassCollisions" => part.pass_collisions = boolean(child, name)?,
            "RegionHandle" => part.region_handle = number(child, name)?,
            "ScriptAccessPin" => part.script_access_pin = number(child, name)?,
            "GroupPosition" => part.group_position = vector(child, name)?,
            "OffsetPosition" => part.offset_position = vector(child, name)?,
            "RotationOffset" => part.rotation_offset = quaternion(child, name)?,
            "Velocity" => part.velocity = vector(child, name)?,
            "AngularVelocity" => part.angular_velocity = vector(child, name)?,
            "Acceleration" => part.acceleration = vector(child, name)?,
            "Description" => part.description = element_text(child),
            "Color" => part.text_color = color(child, name)?,
            "Text" => part.text = element_text(child),
            "SitName" => part.sit_name = element_text(child),
            "TouchName" => part.touch_name = element_text(child),
            "LinkNum" => part.link_num = number(child, name)?,
            "ClickAction" => part.click_action = number(child, name)?,
            "Shape" => part.shape = parse_shape(child, text)?,
            "Scale" => part.scale = vector(child, name)?,
            "SitTargetOrientation" => part.sit_target_orientation = quaternion(child, name)?,
            "SitTargetPosition" => part.sit_target_position = vector(child, name)?,
            "SitTargetPositionLL" => part.sit_target_position_ll = vector(child, name)?,
            "SitTargetOrientationLL" => part.sit_target_orientation_ll = quaternion(child, name)?,
            "StandTarget" => part.stand_target = Some(vector(child, name)?),
            "ParentID" => part.parent_id = number(child, name)?,
            "CreationDate" => part.creation_date = number(child, name)?,
            "Category" => part.category = number(child, name)?,
            "SalePrice" => part.sale_price = number(child, name)?,
            "ObjectSaleType" => part.object_sale_type = number(child, name)?,
            "OwnershipCost" => part.ownership_cost = number(child, name)?,
            "GroupID" => part.group_id = id(child, name)?,
            "OwnerID" => part.owner_id = id(child, name)?,
            "LastOwnerID" => part.last_owner_id = id(child, name)?,
            "RezzerID" => part.rezzer_id = id(child, name)?,
            "BaseMask" => part.base_mask = number(child, name)?,
            "OwnerMask" => part.owner_mask = number(child, name)?,
            "GroupMask" => part.group_mask = number(child, name)?,
            "EveryoneMask" => part.everyone_mask = number(child, name)?,
            "NextOwnerMask" => part.next_owner_mask = number(child, name)?,
            "Flags" => {
                part.flags = prim_flags::from_names(&element_text(child))
                    .map_err(|name| SceneObjectXmlError::UnknownPrimFlag { name })?;
            }
            "CollisionSound" => part.collision_sound = id(child, name)?,
            "CollisionSoundVolume" => part.collision_sound_volume = float(child, name)?,
            "MediaUrl" => part.media_url = Some(element_text(child)),
            "AttachedPos" => part.attached_pos = vector(child, name)?,
            "TextureAnimation" => part.texture_animation = bytes(child, name)?,
            "ParticleSystem" => part.particle_system = bytes(child, name)?,
            "PayPrice0" => set_pay_price(&mut part, 0, number(child, name)?),
            "PayPrice1" => set_pay_price(&mut part, 1, number(child, name)?),
            "PayPrice2" => set_pay_price(&mut part, 2, number(child, name)?),
            "PayPrice3" => set_pay_price(&mut part, 3, number(child, name)?),
            "PayPrice4" => set_pay_price(&mut part, 4, number(child, name)?),
            "Buoyancy" => part.buoyancy = float(child, name)?,
            "Force" => part.force = vector(child, name)?,
            "Torque" => part.torque = vector(child, name)?,
            "VolumeDetectActive" => part.volume_detect_active = boolean(child, name)?,
            "RotationAxisLocks" => part.rotation_axis_locks = number(child, name)?,
            "PhysicsShapeType" => part.physics_shape_type = number(child, name)?,
            "Density" => part.density = Some(float(child, name)?),
            "Friction" => part.friction = Some(float(child, name)?),
            "Bounce" => part.bounce = Some(float(child, name)?),
            "GravityModifier" => part.gravity_modifier = Some(float(child, name)?),
            "CameraEyeOffset" => part.camera_eye_offset = vector(child, name)?,
            "CameraAtOffset" => part.camera_at_offset = vector(child, name)?,
            "SoundID" => part.sound_id = id(child, name)?,
            "SoundGain" => part.sound_gain = float(child, name)?,
            "SoundFlags" => part.sound_flags = number(child, name)?,
            "SoundRadius" => part.sound_radius = float(child, name)?,
            "SoundQueueing" => part.sound_queueing = boolean(child, name)?,
            "SitActRange" => part.sit_act_range = Some(float(child, name)?),
            _unmodelled => part.unknown.push(capture(child, text)),
        }
    }
    Ok(part)
}

/// Writes one of the five pay-price buttons without indexing the array.
fn set_pay_price(part: &mut SceneObjectPart, button: usize, price: i32) {
    if let Some(slot) = part.pay_price.get_mut(button) {
        *slot = price;
    }
}

/// Reads a `<Shape>` block.
fn parse_shape(node: Node<'_, '_>, text: &str) -> Result<SceneShape, SceneObjectXmlError> {
    let mut shape = SceneShape::default();
    // `ProfileShape` and `HollowShape` are the two nibbles of `ProfileCurve`,
    // and OpenSim writes all three. Its own reader applies them in document
    // order, which puts the two nibbles last -- so they are collected and
    // folded in afterwards rather than fought over element by element.
    let mut profile_shape = None;
    let mut hollow_shape = None;
    for child in node.children().filter(Node::is_element) {
        let name = child.tag_name().name();
        match name {
            "ProfileCurve" => shape.profile_curve = number(child, name)?,
            "TextureEntry" => shape.texture_entry = bytes(child, name)?,
            "ExtraParams" => shape.extra_params = bytes(child, name)?,
            "PathBegin" => shape.path_begin = number(child, name)?,
            "PathCurve" => shape.path_curve = number(child, name)?,
            "PathEnd" => shape.path_end = number(child, name)?,
            "PathRadiusOffset" => shape.path_radius_offset = number(child, name)?,
            "PathRevolutions" => shape.path_revolutions = number(child, name)?,
            "PathScaleX" => shape.path_scale_x = number(child, name)?,
            "PathScaleY" => shape.path_scale_y = number(child, name)?,
            "PathShearX" => shape.path_shear_x = number(child, name)?,
            "PathShearY" => shape.path_shear_y = number(child, name)?,
            "PathSkew" => shape.path_skew = number(child, name)?,
            "PathTaperX" => shape.path_taper_x = number(child, name)?,
            "PathTaperY" => shape.path_taper_y = number(child, name)?,
            "PathTwist" => shape.path_twist = number(child, name)?,
            "PathTwistBegin" => shape.path_twist_begin = number(child, name)?,
            "PCode" => shape.pcode = number(child, name)?,
            "ProfileBegin" => shape.profile_begin = number(child, name)?,
            "ProfileEnd" => shape.profile_end = number(child, name)?,
            "ProfileHollow" => shape.profile_hollow = number(child, name)?,
            "State" => shape.state = number(child, name)?,
            "LastAttachPoint" => shape.last_attach_point = number(child, name)?,
            "ProfileShape" => {
                profile_shape = Some(profile_shape_nibble(&element_text(child), name)?);
            }
            "HollowShape" => hollow_shape = Some(hollow_shape_nibble(&element_text(child), name)?),
            "SculptTexture" => shape.sculpt_texture = id(child, name)?,
            "SculptType" => shape.sculpt_type = number(child, name)?,
            "FlexiSoftness" => shape.flexi_softness = number(child, name)?,
            "FlexiTension" => shape.flexi_tension = float(child, name)?,
            "FlexiDrag" => shape.flexi_drag = float(child, name)?,
            "FlexiGravity" => shape.flexi_gravity = float(child, name)?,
            "FlexiWind" => shape.flexi_wind = float(child, name)?,
            "FlexiForceX" => shape.flexi_force_x = float(child, name)?,
            "FlexiForceY" => shape.flexi_force_y = float(child, name)?,
            "FlexiForceZ" => shape.flexi_force_z = float(child, name)?,
            "LightColorR" => shape.light_color_r = float(child, name)?,
            "LightColorG" => shape.light_color_g = float(child, name)?,
            "LightColorB" => shape.light_color_b = float(child, name)?,
            "LightColorA" => shape.light_color_a = float(child, name)?,
            "LightRadius" => shape.light_radius = float(child, name)?,
            "LightCutoff" => shape.light_cutoff = float(child, name)?,
            "LightFalloff" => shape.light_falloff = float(child, name)?,
            "LightIntensity" => shape.light_intensity = float(child, name)?,
            "FlexiEntry" => shape.flexi_entry = boolean(child, name)?,
            "LightEntry" => shape.light_entry = boolean(child, name)?,
            "SculptEntry" => shape.sculpt_entry = boolean(child, name)?,
            "Media" => shape.media = Some(element_text(child)),
            _unmodelled => shape.unknown.push(capture(child, text)),
        }
    }
    if let Some(nibble) = profile_shape {
        shape.profile_curve = (shape.profile_curve & 0xf0) | nibble;
    }
    if let Some(nibble) = hollow_shape {
        shape.profile_curve = (shape.profile_curve & 0x0f) | nibble;
    }
    Ok(shape)
}

/// Reads a `<TaskInventory>` block: the prim's contents.
fn parse_task_inventory(
    node: Node<'_, '_>,
) -> Result<Vec<SceneTaskInventoryItem>, SceneObjectXmlError> {
    let mut items = Vec::new();
    for entry in node.children().filter(Node::is_element) {
        let mut item = SceneTaskInventoryItem::default();
        for child in entry.children().filter(Node::is_element) {
            let name = child.tag_name().name();
            match name {
                "AssetID" => item.asset_id = id(child, name)?,
                "BasePermissions" => item.base_permissions = number(child, name)?,
                "CreationDate" => item.creation_date = number(child, name)?,
                "CreatorID" => item.creator_id = id(child, name)?,
                "CreatorData" => item.creator_data = Some(element_text(child)),
                "Description" => item.description = element_text(child),
                "EveryonePermissions" => item.everyone_permissions = number(child, name)?,
                "Flags" => item.flags = number(child, name)?,
                "GroupID" => item.group_id = id(child, name)?,
                "GroupPermissions" => item.group_permissions = number(child, name)?,
                "InvType" => item.inv_type = number(child, name)?,
                "ItemID" => item.item_id = id(child, name)?,
                "OldItemID" => item.old_item_id = id(child, name)?,
                "LastOwnerID" => item.last_owner_id = id(child, name)?,
                "Name" => item.name = element_text(child),
                "NextPermissions" => item.next_permissions = number(child, name)?,
                "OwnerID" => item.owner_id = id(child, name)?,
                "CurrentPermissions" => item.current_permissions = number(child, name)?,
                "ParentID" => item.parent_id = id(child, name)?,
                "ParentPartID" => item.parent_part_id = id(child, name)?,
                "PermsGranter" => item.perms_granter = id(child, name)?,
                "PermsMask" => item.perms_mask = number(child, name)?,
                "Type" => item.item_type = number(child, name)?,
                "OwnerChanged" => item.owner_changed = boolean(child, name)?,
                // A task item has no `unknown`: it is a flat record OpenSim
                // writes whole, and an element outside the list is one neither
                // side has a field for.
                _unmodelled => {}
            }
        }
        items.push(item);
    }
    Ok(items)
}

/// One element's source, from its `<` to its close, kept for re-emission.
fn capture(node: Node<'_, '_>, text: &str) -> UnknownElement {
    UnknownElement {
        name: node.tag_name().name().to_owned(),
        xml: text.get(node.range()).unwrap_or_default().to_owned(),
    }
}

/// An element's text content, with every text child joined — an entity
/// reference can split one run of text into several nodes, and reading only the
/// first would silently truncate at the first `&amp;`.
fn element_text(node: Node<'_, '_>) -> String {
    node.children().filter_map(|child| child.text()).collect()
}

/// Reads an integer element into whichever integer type the caller keeps it in.
fn number<T: core::str::FromStr>(
    node: Node<'_, '_>,
    field: &str,
) -> Result<T, SceneObjectXmlError> {
    let text = element_text(node);
    text.trim()
        .parse()
        .map_err(|_not_an_integer| SceneObjectXmlError::InvalidInteger {
            field: field.to_owned(),
            value: text,
        })
}

/// Reads a float element.
fn float(node: Node<'_, '_>, field: &str) -> Result<f32, SceneObjectXmlError> {
    let text = element_text(node);
    text.trim()
        .parse()
        .map_err(|_not_a_number| SceneObjectXmlError::InvalidNumber {
            field: field.to_owned(),
            value: text,
        })
}

/// Reads a `true`/`false` element, the way `Util.ReadBoolean` does: C#'s
/// `bool.TryParse` ignores case and surrounding space, and OpenSim's helper
/// also takes the `1`/`0` an older body may carry.
fn boolean(node: Node<'_, '_>, field: &str) -> Result<bool, SceneObjectXmlError> {
    let text = element_text(node);
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _neither => Err(SceneObjectXmlError::InvalidBoolean {
            field: field.to_owned(),
            value: text,
        }),
    }
}

/// Reads a `<Name><UUID>…</UUID></Name>` id, taking the older `Guid` spelling
/// too — OpenSim writes that one when it is asked for an archive an older
/// simulator can read, and its reader accepts both.
fn id(node: Node<'_, '_>, field: &str) -> Result<Uuid, SceneObjectXmlError> {
    let inner = node
        .children()
        .find(|child| child.is_element() && matches!(child.tag_name().name(), "UUID" | "Guid"))
        .map_or_else(|| element_text(node), element_text);
    Uuid::parse_str(inner.trim()).map_err(|_not_an_id| SceneObjectXmlError::InvalidUuid {
        field: field.to_owned(),
        value: inner,
    })
}

/// Reads a base64 element, taking an empty or self-closed one as no bytes.
fn bytes(node: Node<'_, '_>, field: &str) -> Result<Vec<u8>, SceneObjectXmlError> {
    let text = element_text(node);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|source| SceneObjectXmlError::InvalidBase64 {
            field: field.to_owned(),
            source,
        })
}

/// Reads a `<Name><X/><Y/><Z/></Name>` vector.
fn vector(node: Node<'_, '_>, field: &str) -> Result<Vector, SceneObjectXmlError> {
    Ok(Vector {
        x: component(node, field, "X")?,
        y: component(node, field, "Y")?,
        z: component(node, field, "Z")?,
    })
}

/// Reads a `<Name><X/><Y/><Z/><W/></Name>` quaternion.
fn quaternion(node: Node<'_, '_>, field: &str) -> Result<Rotation, SceneObjectXmlError> {
    Ok(Rotation {
        x: component(node, field, "X")?,
        y: component(node, field, "Y")?,
        z: component(node, field, "Z")?,
        s: component(node, field, "W")?,
    })
}

/// Reads the hover text's `<Color>` block, whose channels are bytes written as
/// numbers and read back through a float — which is what OpenSim's own
/// `ProcessColor` does, cast and all.
fn color(node: Node<'_, '_>, field: &str) -> Result<[u8; 4], SceneObjectXmlError> {
    Ok([
        channel(node, field, "R")?,
        channel(node, field, "G")?,
        channel(node, field, "B")?,
        channel(node, field, "A")?,
    ])
}

/// One colour channel: a number read as a float and truncated toward zero, the
/// way C#'s `(int)` cast does, then clamped into a byte.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is truncated and clamped into 0..=255 first, so the conversion is exact"
)]
fn channel(node: Node<'_, '_>, field: &str, axis: &str) -> Result<u8, SceneObjectXmlError> {
    Ok(component(node, field, axis)?.trunc().clamp(0.0, 255.0) as u8)
}

/// One named component of a vector, quaternion or colour block. A component the
/// block does not carry reads as zero, which is what OpenSim's `Util.ReadVector`
/// leaves it at.
fn component(node: Node<'_, '_>, field: &str, axis: &str) -> Result<f32, SceneObjectXmlError> {
    let Some(child) = node
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == axis)
    else {
        return Ok(0.0);
    };
    float(child, &format!("{field}/{axis}"))
}

/// The low nibble a `ProfileShape` element names, taking the enum's own member
/// names and the bare number C# prints for a value it has no name for.
fn profile_shape_nibble(text: &str, field: &str) -> Result<u8, SceneObjectXmlError> {
    let named = match text.trim() {
        "Circle" => Some(0),
        "Square" => Some(1),
        "IsometricTriangle" => Some(2),
        "EquilateralTriangle" => Some(3),
        "RightTriangle" => Some(4),
        "HalfCircle" => Some(5),
        _numeric => None,
    };
    named.map_or_else(|| nibble(text, field, 0x0f), Ok)
}

/// The high nibble a `HollowShape` element names, on the same rule as
/// [`profile_shape_nibble`]. The enum's values are the nibble in place, so
/// `Circle` is 16 rather than 1.
fn hollow_shape_nibble(text: &str, field: &str) -> Result<u8, SceneObjectXmlError> {
    let named = match text.trim() {
        "Same" => Some(0),
        "Circle" => Some(16),
        "Square" => Some(32),
        "Triangle" => Some(48),
        _numeric => None,
    };
    named.map_or_else(|| nibble(text, field, 0xf0), Ok)
}

/// A bare number in a shape-enum element, masked to the nibble it belongs in.
fn nibble(text: &str, field: &str, mask: u8) -> Result<u8, SceneObjectXmlError> {
    text.trim()
        .parse::<u8>()
        .map(|value| value & mask)
        .map_err(|_not_an_integer| SceneObjectXmlError::InvalidInteger {
            field: field.to_owned(),
            value: text.to_owned(),
        })
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A decode that was not supposed to fail, or one that was.
    type TestError = Box<dyn core::error::Error>;

    /// Every field a body states comes back, and the reader does not care what
    /// order they are in: OpenSim's own dispatches on the element name, so a
    /// body written by a newer simulator with a field inserted anywhere still
    /// reads.
    #[test]
    fn a_part_reads_whatever_order_its_elements_are_in() -> Result<(), TestError> {
        let xml = "<SceneObjectGroup><RootPart><SceneObjectPart>\
             <Name>Lamp</Name><Material>7</Material><LocalId>4242</LocalId>\
             </SceneObjectPart></RootPart><OtherParts /></SceneObjectGroup>";
        let shuffled = "<SceneObjectGroup><RootPart><SceneObjectPart>\
             <LocalId>4242</LocalId><Name>Lamp</Name><Material>7</Material>\
             </SceneObjectPart></RootPart><OtherParts /></SceneObjectGroup>";
        let group = SceneObjectGroup::parse(xml)?;
        assert_eq!(group, SceneObjectGroup::parse(shuffled)?);
        assert_eq!(group.root.name, "Lamp");
        assert_eq!(group.root.material, 7);
        assert_eq!(group.root.local_id, 4242);
        Ok(())
    }

    /// A document that is not a group is refused by name rather than read as an
    /// empty one — the two live grids disagree about this asset class, and a
    /// Linden text body handed to this reader must say so instead of decoding
    /// to a prim with no fields.
    #[test]
    fn something_that_is_not_a_group_is_refused() -> Result<(), TestError> {
        let error = SceneObjectGroup::parse("<CoalescedObject />")
            .err()
            .ok_or("a coalesced object is not a group")?;
        assert!(
            matches!(error, SceneObjectXmlError::NotAGroup { ref found } if found == "CoalescedObject"),
            "unexpected error {error:?}"
        );
        Ok(())
    }

    /// An element with no field here is kept whole, not dropped: OpenSim writes
    /// a vehicle, a physics-inertia block and dynamic attributes this module
    /// does not model, and a decode/encode round trip that lost them would be a
    /// lossy editor.
    #[test]
    fn an_unmodelled_element_survives_verbatim() -> Result<(), TestError> {
        let xml = "<SceneObjectGroup><RootPart><SceneObjectPart>\
             <Name>Cart</Name><DynAttrs><llsd><map /></llsd></DynAttrs>\
             </SceneObjectPart></RootPart><OtherParts /></SceneObjectGroup>";
        let group = SceneObjectGroup::parse(xml)?;
        assert_eq!(group.root.unknown.len(), 1);
        assert_eq!(
            group
                .root
                .unknown
                .first()
                .map(|element| element.xml.as_str()),
            Some("<DynAttrs><llsd><map /></llsd></DynAttrs>")
        );
        assert!(
            group
                .encode_to_string()
                .contains("<DynAttrs><llsd><map /></llsd></DynAttrs>"),
            "the unmodelled element was not written back"
        );
        Ok(())
    }

    /// The two shape nibbles win over the `ProfileCurve` they were written
    /// from, because that is the order OpenSim applies them in — a body whose
    /// three elements disagree is read the same way by both.
    #[test]
    fn the_shape_nibbles_win_over_the_profile_curve() -> Result<(), TestError> {
        let xml = "<SceneObjectGroup><RootPart><SceneObjectPart><Shape>\
             <ProfileCurve>0</ProfileCurve>\
             <ProfileShape>Square</ProfileShape><HollowShape>Circle</HollowShape>\
             </Shape></SceneObjectPart></RootPart><OtherParts /></SceneObjectGroup>";
        assert_eq!(SceneObjectGroup::parse(xml)?.root.shape.profile_curve, 0x11);
        Ok(())
    }

    /// A number that does not fit the type OpenSim keeps the element in is an
    /// error rather than a silent truncation. OpenSim would cast 300 to 44 and
    /// rez a prim made of the wrong material; nothing writes such a body, and a
    /// decoder that invented a value would hide whatever did.
    #[test]
    fn an_out_of_range_number_is_refused() -> Result<(), TestError> {
        let xml = "<SceneObjectGroup><RootPart><SceneObjectPart>\
             <Material>300</Material>\
             </SceneObjectPart></RootPart><OtherParts /></SceneObjectGroup>";
        let error = SceneObjectGroup::parse(xml)
            .err()
            .ok_or("300 is not a material")?;
        assert!(
            matches!(error, SceneObjectXmlError::InvalidInteger { ref field, .. } if field == "Material"),
            "unexpected error {error:?}"
        );
        Ok(())
    }

    /// A body with no `RootPart` is refused: OpenSim's reader would build a
    /// group out of whatever the reader happened to be pointing at, and an
    /// empty prim is not a better answer than an error.
    #[test]
    fn a_body_without_a_root_part_is_refused() -> Result<(), TestError> {
        let error = SceneObjectGroup::parse("<SceneObjectGroup><OtherParts /></SceneObjectGroup>")
            .err()
            .ok_or("a group needs a root part")?;
        assert!(
            matches!(error, SceneObjectXmlError::Missing { element } if element == "a RootPart"),
            "unexpected error {error:?}"
        );
        Ok(())
    }
}
