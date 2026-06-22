//! Object interaction and editing value types: clicks, materials, transforms.

use super::pcode;
use sl_types::key::{AgentKey, GroupKey};
use sl_types::lsl::Rotation;
use sl_types::lsl::Vector;
use sl_wire::Permissions5;
use sl_wire::RegionLocalObjectId;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Object interaction & editing (#17): value types for the editing surface.
// ---------------------------------------------------------------------------

/// The left-click behaviour of an object (`ClickAction` / `CLICK_ACTION_*`), as
/// set by [`Session::set_object_click_action`](crate::Session::set_object_click_action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ClickAction {
    /// The default: clicking touches the object (`CLICK_ACTION_TOUCH`, also
    /// `CLICK_ACTION_NONE`).
    #[default]
    Touch,
    /// Clicking sits the avatar on the object (`CLICK_ACTION_SIT`).
    Sit,
    /// Clicking buys the object (`CLICK_ACTION_BUY`).
    Buy,
    /// Clicking pays the object (`CLICK_ACTION_PAY`).
    Pay,
    /// Clicking opens the object's contents (`CLICK_ACTION_OPEN`).
    Open,
    /// Clicking plays the parcel media (`CLICK_ACTION_PLAY`).
    Play,
    /// Clicking opens the object's media (`CLICK_ACTION_OPEN_MEDIA`).
    OpenMedia,
    /// Clicking zooms the camera to the object (`CLICK_ACTION_ZOOM`).
    Zoom,
    /// Clicking is disabled (`CLICK_ACTION_DISABLED`).
    Disabled,
    /// Clicks are ignored / pass through (`CLICK_ACTION_IGNORE`).
    Ignore,
}

impl ClickAction {
    /// The `ClickAction` wire byte for this action.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Touch => 0,
            Self::Sit => 1,
            Self::Buy => 2,
            Self::Pay => 3,
            Self::Open => 4,
            Self::Play => 5,
            Self::OpenMedia => 6,
            Self::Zoom => 7,
            Self::Disabled => 8,
            Self::Ignore => 9,
        }
    }

    /// Classifies a `ClickAction` wire byte (unknown values map to `Touch`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Sit,
            2 => Self::Buy,
            3 => Self::Pay,
            4 => Self::Open,
            5 => Self::Play,
            6 => Self::OpenMedia,
            7 => Self::Zoom,
            8 => Self::Disabled,
            9 => Self::Ignore,
            _ => Self::Touch,
        }
    }
}

/// An object's physical material (`LL_MCODE_*`), as set by
/// [`Session::set_object_material`](crate::Session::set_object_material). The
/// material governs the object's collision sound and default friction/density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Material {
    /// Stone (`LL_MCODE_STONE`).
    Stone,
    /// Metal (`LL_MCODE_METAL`).
    Metal,
    /// Glass (`LL_MCODE_GLASS`).
    Glass,
    /// Wood (`LL_MCODE_WOOD`) — the viewer's default for a new prim.
    #[default]
    Wood,
    /// Flesh (`LL_MCODE_FLESH`).
    Flesh,
    /// Plastic (`LL_MCODE_PLASTIC`).
    Plastic,
    /// Rubber (`LL_MCODE_RUBBER`).
    Rubber,
    /// Light (`LL_MCODE_LIGHT`).
    Light,
}

impl Material {
    /// The `LL_MCODE_*` wire byte for this material.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Stone => 0,
            Self::Metal => 1,
            Self::Glass => 2,
            Self::Wood => 3,
            Self::Flesh => 4,
            Self::Plastic => 5,
            Self::Rubber => 6,
            Self::Light => 7,
        }
    }

    /// Classifies an `LL_MCODE_*` wire byte (unknown values map to `Wood`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Stone,
            1 => Self::Metal,
            2 => Self::Glass,
            4 => Self::Flesh,
            5 => Self::Plastic,
            6 => Self::Rubber,
            7 => Self::Light,
            _ => Self::Wood,
        }
    }
}

/// How an object is offered for sale (`EForSale`), as set by
/// [`Session::set_object_for_sale`](crate::Session::set_object_for_sale) and
/// reported in [`ObjectProperties::sale_type`](crate::ObjectProperties::sale_type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SaleType {
    /// Not for sale (`FS_NOT`).
    #[default]
    NotForSale,
    /// The original object is sold and removed from the world (`FS_ORIGINAL`).
    Original,
    /// A copy is sold, leaving the original in place (`FS_COPY`).
    Copy,
    /// The object's contents are sold (`FS_CONTENTS`).
    Contents,
}

impl SaleType {
    /// The `EForSale` wire byte for this sale type.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::NotForSale => 0,
            Self::Original => 1,
            Self::Copy => 2,
            Self::Contents => 3,
        }
    }

    /// Classifies an `EForSale` wire byte (unknown values map to `NotForSale`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Original,
            2 => Self::Copy,
            3 => Self::Contents,
            _ => Self::NotForSale,
        }
    }
}

/// Where a derezzed object should go (the `Destination` of `DeRezObject`, LL's
/// `EDeRezDestination` / `DRD_*`), as passed to
/// [`Session::derez_objects`](crate::Session::derez_objects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeRezDestination {
    /// Save into agent inventory, leaving a copy in world (`DRD_SAVE_INTO_AGENT_INVENTORY`).
    SaveIntoAgentInventory,
    /// Acquire into agent inventory, trying to leave a copy (`DRD_ACQUIRE_TO_AGENT_INVENTORY`).
    AcquireToAgentInventory,
    /// Save into a task's (prim's) inventory (`DRD_SAVE_INTO_TASK_INVENTORY`); the
    /// destination id is the target task's id.
    SaveIntoTaskInventory,
    /// Wear as an attachment (`DRD_ATTACHMENT`).
    Attachment,
    /// Take into agent inventory, deleting from the world (`DRD_TAKE_INTO_AGENT_INVENTORY`);
    /// the destination id is the inventory folder.
    TakeIntoAgentInventory,
    /// Force take a copy to the god inventory (`DRD_FORCE_TO_GOD_INVENTORY`).
    ForceToGodInventory,
    /// Delete to the trash (`DRD_TRASH`); the destination id is the trash folder.
    Trash,
    /// Detach an attachment to inventory (`DRD_ATTACHMENT_TO_INV`).
    AttachmentToInventory,
    /// An existing attachment (`DRD_ATTACHMENT_EXISTS`).
    AttachmentExists,
    /// Return to the owner's inventory (`DRD_RETURN_TO_OWNER`).
    ReturnToOwner,
    /// Return a deeded object to the last owner's inventory (`DRD_RETURN_TO_LAST_OWNER`).
    ReturnToLastOwner,
}

impl DeRezDestination {
    /// The `DRD_*` wire byte for this destination.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::SaveIntoAgentInventory => 0,
            Self::AcquireToAgentInventory => 1,
            Self::SaveIntoTaskInventory => 2,
            Self::Attachment => 3,
            Self::TakeIntoAgentInventory => 4,
            Self::ForceToGodInventory => 5,
            Self::Trash => 6,
            Self::AttachmentToInventory => 7,
            Self::AttachmentExists => 8,
            Self::ReturnToOwner => 9,
            Self::ReturnToLastOwner => 10,
        }
    }
}

/// Which permission mask an `ObjectPermissions` change targets (the `Field`
/// byte; LL's `PERM_BASE`/`PERM_OWNER`/…), passed to
/// [`Session::set_object_permissions`](crate::Session::set_object_permissions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PermissionField {
    /// The base permissions mask (`PERM_BASE`).
    Base,
    /// The owner permissions mask (`PERM_OWNER`).
    Owner,
    /// The group permissions mask (`PERM_GROUP`).
    Group,
    /// The everyone permissions mask (`PERM_EVERYONE`).
    Everyone,
    /// The next-owner permissions mask (`PERM_NEXT_OWNER`).
    NextOwner,
}

impl PermissionField {
    /// The `Field` wire byte selecting this mask.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Base => 0x01,
            Self::Owner => 0x02,
            Self::Group => 0x04,
            Self::Everyone => 0x08,
            Self::NextOwner => 0x10,
        }
    }
}

/// The shape parameters of a primitive to rez via
/// [`Session::rez_object`](crate::Session::rez_object) (`ObjectAdd`). Start from
/// [`PrimShape::cube`] (a unit box) and adjust as needed; the path/profile
/// fields use the same quantized wire encoding the viewer sends.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimShape {
    /// The object class (almost always [`pcode::PRIMITIVE`], a volume prim).
    pub pcode: u8,
    /// The object material (see [`Material`]).
    pub material: Material,
    /// The `AddFlags` bitfield (`PrimFlags`); 0 for an ordinary, unselected,
    /// non-physical prim.
    pub add_flags: u32,
    /// The path curve byte (`LL_PCODE_PATH_*`).
    pub path_curve: u8,
    /// The profile curve byte (`LL_PCODE_PROFILE_*`, with the hollow shape in the
    /// high nibble).
    pub profile_curve: u8,
    /// The path cut start, quantized (`begin / 0.00002`).
    pub path_begin: u16,
    /// The path cut end, quantized (`50000 - end / 0.00002`).
    pub path_end: u16,
    /// The path top-size X, quantized (`200 - scale_x / 0.01`).
    pub path_scale_x: u8,
    /// The path top-size Y, quantized (`200 - scale_y / 0.01`).
    pub path_scale_y: u8,
    /// The path shear X, quantized (`shear_x / 0.01`).
    pub path_shear_x: u8,
    /// The path shear Y, quantized (`shear_y / 0.01`).
    pub path_shear_y: u8,
    /// The path twist end, quantized (`twist / 0.01`).
    pub path_twist: i8,
    /// The path twist start, quantized (`twist_begin / 0.01`).
    pub path_twist_begin: i8,
    /// The path radius offset, quantized (`radius_offset / 0.01`).
    pub path_radius_offset: i8,
    /// The path taper X, quantized (`taper_x / 0.01`).
    pub path_taper_x: i8,
    /// The path taper Y, quantized (`taper_y / 0.01`).
    pub path_taper_y: i8,
    /// The path revolutions, quantized (`(revolutions - 1) / 0.015`).
    pub path_revolutions: u8,
    /// The path skew, quantized (`skew / 0.01`).
    pub path_skew: i8,
    /// The profile cut start, quantized (`begin / 0.00002`).
    pub profile_begin: u16,
    /// The profile cut end, quantized (`50000 - end / 0.00002`).
    pub profile_end: u16,
    /// The profile hollow fraction, quantized (`hollow / 0.00002`).
    pub profile_hollow: u16,
    /// The size of the prim, in metres along each axis.
    pub scale: Vector,
    /// The orientation of the prim.
    pub rotation: Rotation,
    /// The region-local position to rez at.
    pub position: Vector,
    /// The object/attachment state byte (0 for a plain prim).
    pub state: u8,
}

impl PrimShape {
    /// A unit (0.5 m) cube at `position` with the viewer's default new-prim
    /// settings (wood, square profile, line path, identity rotation). Mutate the
    /// returned struct to change the shape or size before passing it to
    /// [`Session::rez_object`](crate::Session::rez_object).
    #[must_use]
    pub const fn cube(position: Vector) -> Self {
        Self {
            pcode: pcode::PRIMITIVE,
            material: Material::Wood,
            add_flags: 0,
            // LL_PCODE_PATH_LINE
            path_curve: 0x10,
            // LL_PCODE_PROFILE_SQUARE
            profile_curve: 0x01,
            path_begin: 0,
            path_end: 0,
            // 200 - 1.0 / 0.01 = 100 (full top size on both axes)
            path_scale_x: 100,
            path_scale_y: 100,
            path_shear_x: 0,
            path_shear_y: 0,
            path_twist: 0,
            path_twist_begin: 0,
            path_radius_offset: 0,
            path_taper_x: 0,
            path_taper_y: 0,
            path_revolutions: 0,
            path_skew: 0,
            profile_begin: 0,
            profile_end: 0,
            profile_hollow: 0,
            scale: Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            position,
            state: 0,
        }
    }
}

/// The physics/flag toggles of an `ObjectFlagUpdate`, set by
/// [`Session::set_object_flags`](crate::Session::set_object_flags). Build with
/// [`ObjectFlagSettings::default`] (all false) and set the flags to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the four independent boolean toggles of the ObjectFlagUpdate wire message"
)]
pub struct ObjectFlagSettings {
    /// Whether the object is physical (`UsePhysics`).
    pub use_physics: bool,
    /// Whether the object is temporary (auto-deleted; `IsTemporary`).
    pub is_temporary: bool,
    /// Whether the object is phantom (no collisions; `IsPhantom`).
    pub is_phantom: bool,
    /// Whether the object casts shadows (`CastsShadows`, legacy/unused).
    pub casts_shadows: bool,
}

/// A move/scale/rotate change applied to an object via
/// [`Session::update_object`](crate::Session::update_object)
/// (`MultipleObjectUpdate`). Set only the components to change; leave the rest
/// `None`. `group` edits the whole linkset (root-relative); `uniform` keeps a
/// scale change proportional about the object's centre.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectTransform {
    /// The new region-local position, if the position is being changed.
    pub position: Option<Vector>,
    /// The new orientation, if the rotation is being changed.
    pub rotation: Option<Rotation>,
    /// The new size in metres, if the scale is being changed.
    pub scale: Option<Vector>,
    /// Apply to the whole linkset rather than the single prim (the `LINK_SET`
    /// bit, `0x08`).
    pub group: bool,
    /// Scale uniformly about the object's centre (the `UNIFORM` bit, `0x10`).
    /// Only meaningful when [`scale`](Self::scale) is set.
    pub uniform: bool,
}

impl ObjectTransform {
    /// The `MultipleObjectUpdate` `Type` byte for this change: the OR of the
    /// position (`0x01`), rotation (`0x02`), scale (`0x04`), group (`0x08`), and
    /// uniform (`0x10`) bits actually present.
    #[must_use]
    pub const fn type_byte(&self) -> u8 {
        let mut flags = 0_u8;
        if self.position.is_some() {
            flags |= 0x01;
        }
        if self.rotation.is_some() {
            flags |= 0x02;
        }
        if self.scale.is_some() {
            flags |= 0x04;
        }
        if self.group {
            flags |= 0x08;
        }
        if self.uniform {
            flags |= 0x10;
        }
        flags
    }
}

/// A region maturity / content rating, from the `SimAccess` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Maturity {
    /// General ("PG") content.
    Pg,
    /// Moderate ("Mature") content.
    Mature,
    /// Adult content.
    Adult,
    /// Unknown or unrated (the grid did not provide a recognised value).
    Unknown,
}

impl Maturity {
    /// Classifies the `SimAccess` byte from a handshake/region/teleport message.
    #[must_use]
    pub const fn from_sim_access(sim_access: u8) -> Self {
        match sim_access {
            sl_wire::sim_access::PG => Self::Pg,
            sl_wire::sim_access::MATURE => Self::Mature,
            sl_wire::sim_access::ADULT => Self::Adult,
            _ => Self::Unknown,
        }
    }

    /// The `SimAccess` byte for this maturity (`Unknown` maps to PG), used when
    /// setting a region's maturity via `setregioninfo`.
    #[must_use]
    pub const fn to_sim_access(self) -> u8 {
        match self {
            Self::Mature => sl_wire::sim_access::MATURE,
            Self::Adult => sl_wire::sim_access::ADULT,
            Self::Pg | Self::Unknown => sl_wire::sim_access::PG,
        }
    }

    /// Classifies the short maturity code carried by the login response
    /// `agent_access`/`agent_access_max` fields: `"PG"`, `"M"` (mature), or
    /// `"A"` (adult). Unrecognised or absent codes map to [`Maturity::Unknown`].
    #[must_use]
    pub fn from_login_access(code: Option<&str>) -> Self {
        match code {
            Some("PG") => Self::Pg,
            Some("M") => Self::Mature,
            Some("A") => Self::Adult,
            _ => Self::Unknown,
        }
    }
}

/// The flags describing how and why a teleport happened, carried by
/// `TeleportFinish` (and `TeleportProgress`) as the `TeleportFlags` U32
/// bitfield. Mirrors the reference viewer's `TELEPORT_FLAGS_*`
/// (`indra/llmessage/llteleportflags.h`). Surfaced by
/// [`Event::TeleportFinished`](crate::Event::TeleportFinished).
///
/// Note: OpenSim collapses the flags it sends on `TeleportFinish` to
/// [`VIA_LOCATION`](Self::VIA_LOCATION) (plus [`IS_FLYING`](Self::IS_FLYING)),
/// so the full set of `VIA_*` reasons is only observable on the Second Life
/// grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportFlags(pub u32);

impl TeleportFlags {
    /// Set the agent's home to the teleport target (`SET_HOME_TO_TARGET`, a
    /// newbie leaving the prelude).
    pub const SET_HOME_TO_TARGET: u32 = 1 << 0;
    /// Set the agent's last location to the target (`SET_LAST_TO_TARGET`).
    pub const SET_LAST_TO_TARGET: u32 = 1 << 1;
    /// Teleport via a lure / teleport offer (`VIA_LURE`).
    pub const VIA_LURE: u32 = 1 << 2;
    /// Teleport via a landmark (`VIA_LANDMARK`).
    pub const VIA_LANDMARK: u32 = 1 << 3;
    /// Teleport via an explicit location (`VIA_LOCATION`).
    pub const VIA_LOCATION: u32 = 1 << 4;
    /// Teleport to the agent's home (`VIA_HOME`).
    pub const VIA_HOME: u32 = 1 << 5;
    /// Teleport via a telehub (`VIA_TELEHUB`).
    pub const VIA_TELEHUB: u32 = 1 << 6;
    /// Teleport as part of logging in (`VIA_LOGIN`).
    pub const VIA_LOGIN: u32 = 1 << 7;
    /// Teleport via a godlike lure (`VIA_GODLIKE_LURE`).
    pub const VIA_GODLIKE_LURE: u32 = 1 << 8;
    /// The teleport was performed with god powers (`GODLIKE`).
    pub const GODLIKE: u32 = 1 << 9;
    /// An emergency ("911") teleport (`FLAGS_911`).
    pub const NINE_ONE_ONE: u32 = 1 << 10;
    /// Cancelling the teleport is disabled (`DISABLE_CANCEL`, used by
    /// `llTeleportAgentHome`).
    pub const DISABLE_CANCEL: u32 = 1 << 11;
    /// Teleport via a region id (`VIA_REGION_ID`).
    pub const VIA_REGION_ID: u32 = 1 << 12;
    /// The agent was flying when the teleport started (`IS_FLYING`).
    pub const IS_FLYING: u32 = 1 << 13;
    /// Show the reset-home UI on arrival (`SHOW_RESET_HOME`).
    pub const SHOW_RESET_HOME: u32 = 1 << 14;
    /// Force a redirect to some location (`FORCE_REDIRECT`, used when kicking
    /// someone from land).
    pub const FORCE_REDIRECT: u32 = 1 << 15;
    /// Teleport via global coordinates (`VIA_GLOBAL_COORDS`).
    pub const VIA_GLOBAL_COORDS: u32 = 1 << 16;
    /// The teleport stays within the same region (`WITHIN_REGION`).
    pub const WITHIN_REGION: u32 = 1 << 17;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

/// A region product type, inferred from the `ProductSKU`/`ProductName` strings.
/// OpenSim grids usually leave these empty, yielding [`ProductType::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProductType {
    /// A full ("Estate" / "Standalone") region.
    FullRegion,
    /// A homestead region.
    Homestead,
    /// An openspace ("void") region.
    Openspace,
    /// Unknown / unrecognised (commonly OpenSim, which omits the fields).
    Unknown,
}

impl ProductType {
    /// Classifies a region from its `ProductSKU` and `ProductName` strings.
    #[must_use]
    pub fn classify(product_sku: &str, product_name: &str) -> Self {
        let haystack = format!("{product_sku} {product_name}").to_lowercase();
        if haystack.contains("homestead") {
            Self::Homestead
        } else if haystack.contains("openspace") || haystack.contains("open space") {
            Self::Openspace
        } else if haystack.contains("estate")
            || haystack.contains("full")
            || haystack.contains("standalone")
        {
            Self::FullRegion
        } else {
            Self::Unknown
        }
    }
}

// ---------------------------------------------------------------------------
// Object commerce & rez (G6): purchase, pay, and the raycast/notecard rez paths.
// ---------------------------------------------------------------------------

/// One object to purchase in an `ObjectBuy`, as passed to
/// [`Session::buy_object`](crate::Session::buy_object). The sale type and price
/// must match what the object advertises (from its
/// [`ObjectPropertiesFamily`](crate::ObjectPropertiesFamily) or
/// [`ObjectProperties`](crate::ObjectProperties)); the simulator rejects a
/// mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectBuyItem {
    /// The object's region-local id (the root prim).
    pub local_id: RegionLocalObjectId,
    /// How the object is offered for sale.
    pub sale_type: SaleType,
    /// The advertised sale price, in L$.
    pub sale_price: i32,
}

/// The parameters for rezzing an in-world object out of an embedded notecard
/// asset (`RezObjectFromNotecard`), as passed to
/// [`Session::rez_object_from_notecard`](crate::Session::rez_object_from_notecard).
/// The ray fields place the new object exactly as the regular inventory-rez
/// path does.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent boolean toggles of the RezObjectFromNotecard wire block"
)]
pub struct NotecardRez {
    /// The active group the new object is set to ([`Uuid::nil`] for none).
    pub group_id: GroupKey,
    /// The task (prim) whose inventory holds the notecard, when rezzing from an
    /// in-world object's contents ([`Uuid::nil`] when rezzing from the agent's
    /// own inventory notecard).
    pub from_task_id: Uuid,
    /// When set, the simulator trusts `ray_end` rather than raycasting.
    pub bypass_raycast: bool,
    /// The ray's start point (region-local).
    pub ray_start: Vector,
    /// The ray's end point (region-local).
    pub ray_end: Vector,
    /// The object the ray is cast against ([`Uuid::nil`] for the terrain).
    pub ray_target_id: Uuid,
    /// Whether `ray_end` is the actual intersection point.
    pub ray_end_is_intersection: bool,
    /// Whether the rezzed object should be left selected.
    pub rez_selected: bool,
    /// Whether to remove the source notecard item after rezzing.
    pub remove_item: bool,
    /// The item flags to apply to the rezzed object.
    pub item_flags: u32,
    /// The group permissions mask for the rezzed object.
    pub group_mask: u32,
    /// The everyone permissions mask for the rezzed object.
    pub everyone_mask: u32,
    /// The next-owner permissions mask for the rezzed object.
    pub next_owner_mask: u32,
    /// The notecard inventory item the object asset is embedded in.
    pub notecard_item_id: Uuid,
    /// The object that holds the notecard ([`Uuid::nil`] when the notecard is in
    /// the agent's own inventory).
    pub object_id: Uuid,
    /// The embedded inventory item ids to rez out of the notecard.
    pub item_ids: Vec<Uuid>,
}

/// A full inventory item to restore to the world at its last in-world position
/// (`RezRestoreToWorld`), as passed to
/// [`Session::rez_restore_to_world`](crate::Session::rez_restore_to_world). The
/// message is [`UDPDeprecated`] on the wire, but a viewer can still send it; the
/// simulator rezzes the object back where it last sat.
///
/// [`UDPDeprecated`]: https://wiki.secondlife.com/wiki/Message_Layout
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreItem {
    /// The inventory item id to restore.
    pub item_id: Uuid,
    /// The folder the item lives in.
    pub folder_id: Uuid,
    /// The item's creator (for the rezzed object's permissions).
    pub creator_id: AgentKey,
    /// The item's owner (for the rezzed object's permissions).
    pub owner_id: Uuid,
    /// The group the item is set to (for permissions).
    pub group_id: GroupKey,
    /// The base / owner / group / everyone / next-owner permission masks.
    pub permissions: Permissions5,
    /// Whether the item is group-owned.
    pub group_owned: bool,
    /// A caller-chosen transaction id correlating the operation.
    pub transaction_id: Uuid,
    /// The asset type (`AssetType`).
    pub asset_type: i8,
    /// The inventory type (`InventoryType`).
    pub inv_type: i8,
    /// The item flags.
    pub flags: u32,
    /// How the item is offered for sale.
    pub sale_type: SaleType,
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
    /// The creation timestamp (seconds since the Unix epoch).
    pub creation_date: i32,
    /// The item's CRC.
    pub crc: u32,
}
