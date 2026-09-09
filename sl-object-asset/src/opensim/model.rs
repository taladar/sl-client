//! The typed `<SceneObjectGroup>` model: a [`SceneObjectGroup`] of
//! [`SceneObjectPart`]s, each with its [`SceneShape`] and task inventory.
//!
//! Field names follow OpenSim's element names lower-cased and snake-cased
//! (`RotationOffset` → `rotation_offset`), because the XML is what this module
//! reads and writes and what a reader will have in front of them. Field
//! **types** follow what OpenSim's own reader casts each element to
//! (`SceneObjectSerializer`'s `Process*` handlers), so a value this model can
//! hold is a value OpenSim can hold.

use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use crate::model::{IDENTITY_ROTATION, ZERO_VECTOR};

/// A whole taken object: the linkset's root part and the rest of its parts.
///
/// The XML nests the two differently — `<RootPart><SceneObjectPart>…` and
/// `<OtherParts><Part><SceneObjectPart>…` — which is what makes the "original"
/// format different from OpenSim's `Xml2` one, where every part is a bare
/// `<SceneObjectPart>` under the group. This model says which part is the root
/// by holding it separately rather than by a marker inside it, because the XML
/// does the same.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneObjectGroup {
    /// The linkset root — the part a viewer selected and the name the item
    /// takes.
    pub root: SceneObjectPart,
    /// The rest of the linkset, in the order the group lists them.
    pub other_parts: Vec<SceneObjectPart>,
    /// Elements of the group OpenSim can write and this module does not model
    /// (`KeyframeMotion`, a saved script state), carried verbatim.
    pub unknown: Vec<UnknownElement>,
}

impl SceneObjectGroup {
    /// A group holding the single part `root`.
    #[must_use]
    pub const fn single(root: SceneObjectPart) -> Self {
        Self {
            root,
            other_parts: Vec::new(),
            unknown: Vec::new(),
        }
    }

    /// Every part, root first — the order a rez wants them in.
    pub fn parts(&self) -> impl Iterator<Item = &SceneObjectPart> {
        core::iter::once(&self.root).chain(self.other_parts.iter())
    }
}

/// One prim of a taken object: everything OpenSim writes between a
/// `<SceneObjectPart>` and its close.
///
/// Bools are what OpenSim writes as `true`/`false`; everything else is the
/// numeric or textual form of the matching `SceneObjectPart` property.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one element OpenSim writes as true/false; collapsing them into a bitfield would invent a grouping the format does not have"
)]
pub struct SceneObjectPart {
    /// Whether anyone may drop inventory into the prim (`AllowedDrop`).
    pub allowed_drop: bool,
    /// The prim's creator (`CreatorID`).
    pub creator_id: Uuid,
    /// The creator's HG identity (`CreatorData`), absent for a local creator —
    /// which is how OpenSim writes an empty one.
    pub creator_data: Option<String>,
    /// The inventory folder the prim's own contents live in (`FolderID`).
    pub folder_id: Uuid,
    /// The task-inventory serial (`InventorySerial`) a viewer caches its
    /// contents listing against.
    pub inventory_serial: u32,
    /// The prim's contents (`TaskInventory`), which OpenSim omits entirely when
    /// the prim is empty.
    pub task_inventory: Vec<SceneTaskInventoryItem>,
    /// The prim's own object key (`UUID`).
    pub uuid: Uuid,
    /// The region-local id the prim had when it was serialised (`LocalId`).
    pub local_id: u32,
    /// The prim's name (`Name`).
    pub name: String,
    /// The material code (`Material`, an `OpenMetaverse.Material`).
    pub material: u8,
    /// Whether touches pass to the linkset root (`PassTouches`).
    pub pass_touches: bool,
    /// Whether collisions pass to the linkset root (`PassCollisions`).
    pub pass_collisions: bool,
    /// The handle of the region the prim was taken from (`RegionHandle`).
    pub region_handle: u64,
    /// The `llRemoteLoadScriptPin` pin (`ScriptAccessPin`).
    pub script_access_pin: i32,
    /// The linkset root's region position (`GroupPosition`).
    pub group_position: Vector,
    /// This part's offset from the root (`OffsetPosition`); zero for the root.
    pub offset_position: Vector,
    /// The part's rotation (`RotationOffset`) — in the root's frame for a
    /// child.
    pub rotation_offset: Rotation,
    /// Linear velocity (`Velocity`).
    pub velocity: Vector,
    /// Angular velocity (`AngularVelocity`).
    pub angular_velocity: Vector,
    /// Linear acceleration (`Acceleration`).
    pub acceleration: Vector,
    /// The prim's description (`Description`).
    pub description: String,
    /// The hover text's colour as OpenSim stores it (`Color`): RGBA bytes whose
    /// **alpha is opacity**, the inverse of the wire's. See the module docs of
    /// [`opensim`](super).
    pub text_color: [u8; 4],
    /// The hover text (`Text`, `llSetText`).
    pub text: String,
    /// The sit-here pie-menu label (`SitName`).
    pub sit_name: String,
    /// The touch pie-menu label (`TouchName`).
    pub touch_name: String,
    /// The part's link number (`LinkNum`); `0` for a solitary prim, `1` for a
    /// linkset root.
    pub link_num: i32,
    /// The click action (`ClickAction`, an `OpenMetaverse.ClickAction`).
    pub click_action: u8,
    /// The shape block (`Shape`) — where the texture entry and extra params
    /// live.
    pub shape: SceneShape,
    /// The prim's size (`Scale`).
    pub scale: Vector,
    /// The sit target's rotation (`SitTargetOrientation`).
    pub sit_target_orientation: Rotation,
    /// The sit target's offset (`SitTargetPosition`).
    pub sit_target_position: Vector,
    /// The sit target's offset as the viewer's own convention states it
    /// (`SitTargetPositionLL`).
    pub sit_target_position_ll: Vector,
    /// The sit target's rotation in the viewer's convention
    /// (`SitTargetOrientationLL`).
    pub sit_target_orientation_ll: Rotation,
    /// Where an avatar stands up to (`StandTarget`), which OpenSim omits when
    /// it is zero.
    pub stand_target: Option<Vector>,
    /// The region-local id of the linkset root (`ParentID`); zero for a root.
    pub parent_id: u32,
    /// When the prim was created (`CreationDate`), in seconds since the Unix
    /// epoch.
    pub creation_date: i32,
    /// The prim's search category (`Category`).
    pub category: u32,
    /// The asking price (`SalePrice`).
    pub sale_price: i32,
    /// How the prim is offered for sale (`ObjectSaleType`).
    pub object_sale_type: u8,
    /// The prim's ownership cost (`OwnershipCost`), which no live grid charges.
    pub ownership_cost: i32,
    /// The group the prim is set to (`GroupID`).
    pub group_id: Uuid,
    /// The prim's owner (`OwnerID`).
    pub owner_id: Uuid,
    /// The prim's previous owner (`LastOwnerID`).
    pub last_owner_id: Uuid,
    /// Whoever rezzed the prim (`RezzerID`).
    pub rezzer_id: Uuid,
    /// The base permission mask (`BaseMask`).
    pub base_mask: u32,
    /// The owner's permission mask (`OwnerMask`).
    pub owner_mask: u32,
    /// The group's permission mask (`GroupMask`).
    pub group_mask: u32,
    /// Everyone's permission mask (`EveryoneMask`).
    pub everyone_mask: u32,
    /// The next owner's permission mask (`NextOwnerMask`).
    pub next_owner_mask: u32,
    /// The prim flags (`Flags`), which OpenSim writes as the **names** of the
    /// bits — see [`prim_flags`].
    pub flags: u32,
    /// The collision sound's asset id (`CollisionSound`).
    pub collision_sound: Uuid,
    /// The collision sound's gain (`CollisionSoundVolume`).
    pub collision_sound_volume: f32,
    /// The legacy single-URL object media (`MediaUrl`), which OpenSim omits
    /// when the prim has none.
    pub media_url: Option<String>,
    /// Where the prim hangs when it is worn (`AttachedPos`).
    pub attached_pos: Vector,
    /// The raw texture-animation blob (`TextureAnimation`, `llSetTextureAnim`).
    pub texture_animation: Vec<u8>,
    /// The raw particle-system blob (`ParticleSystem`, `llParticleSystem`).
    pub particle_system: Vec<u8>,
    /// The five `llSetPayPrice` buttons (`PayPrice0`..`PayPrice4`).
    pub pay_price: [i32; 5],
    /// The prim's buoyancy (`Buoyancy`).
    pub buoyancy: f32,
    /// A standing force on the prim (`Force`).
    pub force: Vector,
    /// A standing torque on the prim (`Torque`).
    pub torque: Vector,
    /// Whether the prim is a volume detector (`VolumeDetectActive`).
    pub volume_detect_active: bool,
    /// Which rotation axes are locked (`RotationAxisLocks`), which OpenSim
    /// omits when nothing is locked.
    pub rotation_axis_locks: u8,
    /// The physics shape type (`PhysicsShapeType`): prim, none or convex hull.
    pub physics_shape_type: u8,
    /// The prim's density (`Density`), absent when it is OpenSim's default of
    /// 1000 — which is how OpenSim writes it.
    pub density: Option<f32>,
    /// The prim's friction (`Friction`), absent at OpenSim's default of 0.6.
    pub friction: Option<f32>,
    /// The prim's restitution (`Bounce`), absent at OpenSim's default of 0.5.
    pub bounce: Option<f32>,
    /// The prim's gravity multiplier (`GravityModifier`), absent at OpenSim's
    /// default of 1.
    pub gravity_modifier: Option<f32>,
    /// The camera eye offset a sit uses (`CameraEyeOffset`).
    pub camera_eye_offset: Vector,
    /// The camera focus offset a sit uses (`CameraAtOffset`).
    pub camera_at_offset: Vector,
    /// The attached sound's asset id (`SoundID`).
    pub sound_id: Uuid,
    /// The attached sound's gain (`SoundGain`).
    pub sound_gain: f32,
    /// The attached sound's flags (`SoundFlags`).
    pub sound_flags: u8,
    /// The attached sound's cutoff radius (`SoundRadius`).
    pub sound_radius: f32,
    /// Whether the attached sound queues rather than restarts
    /// (`SoundQueueing`).
    pub sound_queueing: bool,
    /// How far away a sit may be triggered from (`SitActRange`), which OpenSim
    /// omits when it is effectively zero.
    pub sit_act_range: Option<f32>,
    /// Elements of the part OpenSim can write and this module does not model
    /// (`DynAttrs`, a vehicle, a physics-inertia block, `SOPAnims`), carried
    /// verbatim.
    pub unknown: Vec<UnknownElement>,
}

impl Default for SceneObjectPart {
    /// A part with every field at the value OpenSim's own
    /// `SceneObjectPart` constructor leaves it at, so a caller can state the
    /// handful of fields it knows and take the rest.
    fn default() -> Self {
        Self {
            allowed_drop: false,
            creator_id: Uuid::nil(),
            creator_data: None,
            folder_id: Uuid::nil(),
            inventory_serial: 0,
            task_inventory: Vec::new(),
            uuid: Uuid::nil(),
            local_id: 0,
            name: String::new(),
            material: 0,
            pass_touches: false,
            pass_collisions: false,
            region_handle: 0,
            script_access_pin: 0,
            group_position: ZERO_VECTOR,
            offset_position: ZERO_VECTOR,
            rotation_offset: IDENTITY_ROTATION,
            velocity: ZERO_VECTOR,
            angular_velocity: ZERO_VECTOR,
            acceleration: ZERO_VECTOR,
            description: String::new(),
            text_color: [0, 0, 0, 0],
            text: String::new(),
            sit_name: String::new(),
            touch_name: String::new(),
            link_num: 0,
            click_action: 0,
            shape: SceneShape::default(),
            scale: ZERO_VECTOR,
            sit_target_orientation: IDENTITY_ROTATION,
            sit_target_position: ZERO_VECTOR,
            sit_target_position_ll: ZERO_VECTOR,
            sit_target_orientation_ll: IDENTITY_ROTATION,
            stand_target: None,
            parent_id: 0,
            creation_date: 0,
            category: 0,
            sale_price: 0,
            object_sale_type: 0,
            ownership_cost: 0,
            group_id: Uuid::nil(),
            owner_id: Uuid::nil(),
            last_owner_id: Uuid::nil(),
            rezzer_id: Uuid::nil(),
            base_mask: 0,
            owner_mask: 0,
            group_mask: 0,
            everyone_mask: 0,
            next_owner_mask: 0,
            flags: 0,
            collision_sound: Uuid::nil(),
            collision_sound_volume: 0.0,
            media_url: None,
            attached_pos: ZERO_VECTOR,
            texture_animation: Vec::new(),
            particle_system: Vec::new(),
            pay_price: [0; 5],
            buoyancy: 0.0,
            force: ZERO_VECTOR,
            torque: ZERO_VECTOR,
            volume_detect_active: false,
            rotation_axis_locks: 0,
            physics_shape_type: 0,
            density: None,
            friction: None,
            bounce: None,
            gravity_modifier: None,
            camera_eye_offset: ZERO_VECTOR,
            camera_at_offset: ZERO_VECTOR,
            sound_id: Uuid::nil(),
            sound_gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            sound_queueing: false,
            sit_act_range: None,
            unknown: Vec::new(),
        }
    }
}

/// A prim's `Shape` block: the path and profile parameters, and the two packed
/// blobs that carry everything the Linden text has no keyword for.
///
/// `TextureEntry` and `ExtraParams` are the **wire** blobs, base64-encoded and
/// otherwise untouched — which is why a face's glow, a flexi path, a light, a
/// sculpt or a mesh survives this format and not the other one.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneShape {
    /// The profile curve byte (`ProfileCurve`): the hollow shape in the high
    /// nibble and the profile shape in the low one, which is how OpenSim's own
    /// property composes it.
    pub profile_curve: u8,
    /// The packed `TextureEntry` blob, verbatim.
    pub texture_entry: Vec<u8>,
    /// The packed `ExtraParams` blob, verbatim.
    pub extra_params: Vec<u8>,
    /// The path cut start (`PathBegin`), quantized.
    pub path_begin: u16,
    /// The path curve byte (`PathCurve`).
    pub path_curve: u8,
    /// The path cut end (`PathEnd`), quantized.
    pub path_end: u16,
    /// The path radius offset (`PathRadiusOffset`), quantized.
    pub path_radius_offset: i8,
    /// The path revolutions (`PathRevolutions`), quantized.
    pub path_revolutions: u8,
    /// The path top-size X (`PathScaleX`), quantized.
    pub path_scale_x: u8,
    /// The path top-size Y (`PathScaleY`), quantized.
    pub path_scale_y: u8,
    /// The path shear X (`PathShearX`), quantized.
    pub path_shear_x: u8,
    /// The path shear Y (`PathShearY`), quantized.
    pub path_shear_y: u8,
    /// The path skew (`PathSkew`), quantized.
    pub path_skew: i8,
    /// The path taper X (`PathTaperX`), quantized.
    pub path_taper_x: i8,
    /// The path taper Y (`PathTaperY`), quantized.
    pub path_taper_y: i8,
    /// The path twist end (`PathTwist`), quantized.
    pub path_twist: i8,
    /// The path twist start (`PathTwistBegin`), quantized.
    pub path_twist_begin: i8,
    /// The object class byte (`PCode`).
    pub pcode: u8,
    /// The profile cut start (`ProfileBegin`), quantized.
    pub profile_begin: u16,
    /// The profile cut end (`ProfileEnd`), quantized.
    pub profile_end: u16,
    /// The profile hollow fraction (`ProfileHollow`), quantized.
    pub profile_hollow: u16,
    /// The prim's `state` byte (`State`) — an attachment's swizzled point.
    pub state: u8,
    /// The attachment point the prim was last worn on (`LastAttachPoint`).
    pub last_attach_point: u8,
    /// The sculpt map or mesh asset (`SculptTexture`).
    pub sculpt_texture: Uuid,
    /// The sculpt type (`SculptType`), `5` for a mesh.
    pub sculpt_type: u8,
    /// The flexi softness (`FlexiSoftness`).
    pub flexi_softness: i32,
    /// The flexi tension (`FlexiTension`).
    pub flexi_tension: f32,
    /// The flexi drag (`FlexiDrag`).
    pub flexi_drag: f32,
    /// The flexi gravity (`FlexiGravity`).
    pub flexi_gravity: f32,
    /// The flexi wind sensitivity (`FlexiWind`).
    pub flexi_wind: f32,
    /// The flexi force's X component (`FlexiForceX`).
    pub flexi_force_x: f32,
    /// The flexi force's Y component (`FlexiForceY`).
    pub flexi_force_y: f32,
    /// The flexi force's Z component (`FlexiForceZ`).
    pub flexi_force_z: f32,
    /// The light's red channel (`LightColorR`).
    pub light_color_r: f32,
    /// The light's green channel (`LightColorG`).
    pub light_color_g: f32,
    /// The light's blue channel (`LightColorB`).
    pub light_color_b: f32,
    /// The light's alpha (`LightColorA`).
    pub light_color_a: f32,
    /// The light's radius (`LightRadius`).
    pub light_radius: f32,
    /// The light's cutoff (`LightCutoff`).
    pub light_cutoff: f32,
    /// The light's falloff (`LightFalloff`).
    pub light_falloff: f32,
    /// The light's intensity (`LightIntensity`).
    pub light_intensity: f32,
    /// Whether the prim has flexi parameters (`FlexiEntry`).
    pub flexi_entry: bool,
    /// Whether the prim has light parameters (`LightEntry`).
    pub light_entry: bool,
    /// Whether the prim has sculpt/mesh parameters (`SculptEntry`).
    pub sculpt_entry: bool,
    /// The per-face media list (`Media`), which OpenSim writes as its own
    /// nested document inside the element's text and this module carries
    /// verbatim. Absent when the prim has no per-face media.
    pub media: Option<String>,
    /// Shape elements this module does not model, carried verbatim.
    pub unknown: Vec<UnknownElement>,
}

impl Default for SceneShape {
    /// A shape at OpenSim's own `PrimitiveBaseShape` defaults, with no texture
    /// entry and no extra params.
    fn default() -> Self {
        Self {
            profile_curve: 0,
            texture_entry: Vec::new(),
            extra_params: Vec::new(),
            path_begin: 0,
            path_curve: 0,
            path_end: 0,
            path_radius_offset: 0,
            path_revolutions: 0,
            path_scale_x: 0,
            path_scale_y: 0,
            path_shear_x: 0,
            path_shear_y: 0,
            path_skew: 0,
            path_taper_x: 0,
            path_taper_y: 0,
            path_twist: 0,
            path_twist_begin: 0,
            pcode: 0,
            profile_begin: 0,
            profile_end: 0,
            profile_hollow: 0,
            state: 0,
            last_attach_point: 0,
            sculpt_texture: Uuid::nil(),
            sculpt_type: 0,
            flexi_softness: 0,
            flexi_tension: 0.0,
            flexi_drag: 0.0,
            flexi_gravity: 0.0,
            flexi_wind: 0.0,
            flexi_force_x: 0.0,
            flexi_force_y: 0.0,
            flexi_force_z: 0.0,
            light_color_r: 0.0,
            light_color_g: 0.0,
            light_color_b: 0.0,
            light_color_a: 0.0,
            light_radius: 0.0,
            light_cutoff: 0.0,
            light_falloff: 0.0,
            light_intensity: 0.0,
            flexi_entry: false,
            light_entry: false,
            sculpt_entry: false,
            media: None,
            unknown: Vec::new(),
        }
    }
}

/// One item in a prim's contents, as OpenSim's `WriteTaskInventory` states it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneTaskInventoryItem {
    /// The item's asset (`AssetID`).
    pub asset_id: Uuid,
    /// The base permission mask (`BasePermissions`).
    pub base_permissions: u32,
    /// When the item was created (`CreationDate`), in seconds since the Unix
    /// epoch.
    pub creation_date: u32,
    /// The item's creator (`CreatorID`).
    pub creator_id: Uuid,
    /// The creator's HG identity (`CreatorData`), absent for a local creator.
    pub creator_data: Option<String>,
    /// The item's description (`Description`).
    pub description: String,
    /// Everyone's permission mask (`EveryonePermissions`).
    pub everyone_permissions: u32,
    /// The item flags (`Flags`).
    pub flags: u32,
    /// The group the item is set to (`GroupID`).
    pub group_id: Uuid,
    /// The group's permission mask (`GroupPermissions`).
    pub group_permissions: u32,
    /// The inventory type (`InvType`).
    pub inv_type: i32,
    /// The item's own id (`ItemID`).
    pub item_id: Uuid,
    /// The id the item had before it was copied into this prim (`OldItemID`).
    pub old_item_id: Uuid,
    /// The item's previous owner (`LastOwnerID`).
    pub last_owner_id: Uuid,
    /// The item's name (`Name`).
    pub name: String,
    /// The next owner's permission mask (`NextPermissions`).
    pub next_permissions: u32,
    /// The item's owner (`OwnerID`).
    pub owner_id: Uuid,
    /// The current permission mask (`CurrentPermissions`).
    pub current_permissions: u32,
    /// The prim the item lives in (`ParentID`).
    pub parent_id: Uuid,
    /// The prim's own key again (`ParentPartID`), which OpenSim keeps beside
    /// the folder id.
    pub parent_part_id: Uuid,
    /// Whoever a script in the item was granted permissions by
    /// (`PermsGranter`).
    pub perms_granter: Uuid,
    /// What those permissions are (`PermsMask`).
    pub perms_mask: i32,
    /// The asset type (`Type`).
    pub item_type: i32,
    /// Whether the item changed hands with the prim (`OwnerChanged`).
    pub owner_changed: bool,
}

/// An element this module does not model, kept exactly as it was written.
///
/// The same bargain [`PrimBlock::unknown`](crate::PrimBlock::unknown) makes for
/// the text: a body that lost a block on a re-save would be a lossy editor, and
/// a block nobody here understands is still one OpenSim may act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownElement {
    /// The element's name, which is what decides where it is written back.
    pub name: String,
    /// The element's source, from its `<` to its close — re-emitted verbatim.
    pub xml: String,
}

/// OpenSim's `PrimFlags` names, which is how it writes a prim's flags: the
/// names of the set bits, comma-separated by C#'s own enum formatting and then
/// stripped of the commas (`SceneObjectSerializer.WriteFlags`), and parsed back
/// by putting the commas in again (`Util.ReadEnum`).
///
/// The names and values are the ones in the `OpenMetaverseTypes.dll` OpenSim
/// ships, read out of the assembly rather than remembered: this fork's
/// `ObjectTransfer` is `0x0002_0000` where upstream libopenmetaverse has
/// `0x0004_0000`, and a table written from memory would put every flag above
/// `AllowInventoryDrop` under the wrong name.
///
/// The vocabulary is dated in places — bit 15 is `JointWheel` here and
/// `FLAGS_INCLUDE_IN_SEARCH` to a viewer — but it is *OpenSim's* vocabulary,
/// and the point of this module is to write what OpenSim would.
pub mod prim_flags {
    /// Every bit of `OpenMetaverse.PrimFlags` with the name OpenSim spells it,
    /// in ascending value order — which is also the order C# joins them in.
    ///
    /// All thirty-two bits are named, so no flags value can fail to decompose:
    /// C# falls back to printing the number when a bit has no name, and this
    /// table is why that never happens here.
    pub const NAMES: [(u32, &str); 32] = [
        (1 << 0, "Physics"),
        (1 << 1, "CreateSelected"),
        (1 << 2, "ObjectModify"),
        (1 << 3, "ObjectCopy"),
        (1 << 4, "ObjectAnyOwner"),
        (1 << 5, "ObjectYouOwner"),
        (1 << 6, "Scripted"),
        (1 << 7, "Touch"),
        (1 << 8, "ObjectMove"),
        (1 << 9, "Money"),
        (1 << 10, "Phantom"),
        (1 << 11, "InventoryEmpty"),
        (1 << 12, "JointHinge"),
        (1 << 13, "JointP2P"),
        (1 << 14, "JointLP2P"),
        (1 << 15, "JointWheel"),
        (1 << 16, "AllowInventoryDrop"),
        (1 << 17, "ObjectTransfer"),
        (1 << 18, "ObjectGroupOwned"),
        (1 << 19, "ObjectYouOfficer"),
        (1 << 20, "CameraDecoupled"),
        (1 << 21, "AnimSource"),
        (1 << 22, "CameraSource"),
        (1 << 23, "CastShadows"),
        (1 << 24, "DieAtEdge"),
        (1 << 25, "ReturnAtEdge"),
        (1 << 26, "Sandbox"),
        (1 << 27, "Flying"),
        (1 << 28, "ObjectOwnerModify"),
        (1 << 29, "TemporaryOnRez"),
        (1 << 30, "Temporary"),
        (1 << 31, "ZlibCompressed"),
    ];

    /// What C# prints for a flags value with no bits set.
    pub const NONE: &str = "None";

    /// The flags value spelled the way OpenSim writes it: the set bits' names
    /// separated by single spaces, or `None`.
    #[must_use]
    pub fn to_names(bits: u32) -> String {
        let mut out = String::new();
        for (bit, name) in NAMES {
            if bits & bit != 0 {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(name);
            }
        }
        if out.is_empty() { NONE.to_owned() } else { out }
    }

    /// The flags value a spelling like `Physics Scripted` means, or the name
    /// that is not one of ours.
    ///
    /// Separators are spaces *and* commas, because C#'s `Enum.Parse` — which is
    /// what OpenSim reaches after putting the commas back — accepts both, and a
    /// body written by an older OpenSim still has its commas.
    ///
    /// # Errors
    ///
    /// The offending name, when the text holds one this table does not.
    pub fn from_names(text: &str) -> Result<u32, String> {
        let mut bits = 0;
        for token in text.split([' ', ',']).filter(|token| !token.is_empty()) {
            if token == NONE {
                continue;
            }
            if let Some(&(bit, _name)) = NAMES.iter().find(|&&(_bit, name)| name == token) {
                bits |= bit;
                continue;
            }
            // A bare number is what C# writes for a value whose bits have no
            // names, and `Enum.Parse` reads one back -- so a body that carries
            // one is still a body OpenSim would load.
            match token.parse::<u32>() {
                Ok(value) => bits |= value,
                Err(_not_a_number) => return Err(token.to_owned()),
            }
        }
        Ok(bits)
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// The spelling is the set bits' names in ascending value order, which is
    /// the order C# joins a flags enum in — a viewer-familiar prim (scripted,
    /// touchable, owned by you) reads as the sentence OpenSim would write.
    #[test]
    fn flags_are_spelled_in_ascending_bit_order() {
        let bits = (1 << 5) | (1 << 6) | (1 << 7);
        assert_eq!(prim_flags::to_names(bits), "ObjectYouOwner Scripted Touch");
    }

    /// No bits set is `None`, not the empty string: an empty `Flags` element
    /// would make `Enum.Parse` throw and take the whole body down with it.
    #[test]
    fn no_flags_is_spelled_none() {
        assert_eq!(prim_flags::to_names(0), prim_flags::NONE);
        assert_eq!(prim_flags::from_names(prim_flags::NONE), Ok(0));
    }

    /// Every bit round-trips, including the all-ones value: the table names all
    /// thirty-two, so nothing can fall through to a number.
    #[test]
    fn every_bit_round_trips() {
        for (bit, _name) in prim_flags::NAMES {
            assert_eq!(
                prim_flags::from_names(&prim_flags::to_names(bit)),
                Ok(bit),
                "flag bit {bit:#x} did not survive its own spelling"
            );
        }
        assert_eq!(
            prim_flags::from_names(&prim_flags::to_names(u32::MAX)),
            Ok(u32::MAX)
        );
    }

    /// Commas are read as separators too. OpenSim strips them on the way out
    /// and puts them back on the way in, so a body written by something that
    /// did not strip them is still one it would load.
    #[test]
    fn commas_separate_as_well_as_spaces() {
        assert_eq!(
            prim_flags::from_names("Physics, Scripted"),
            prim_flags::from_names("Physics Scripted")
        );
    }

    /// A name outside the table is refused rather than skipped. C# would throw
    /// here and OpenSim would log the whole body as unreadable; quietly
    /// dropping the flag would be a body that says something different from
    /// what it was given.
    #[test]
    fn an_unknown_flag_name_is_refused() {
        assert_eq!(
            prim_flags::from_names("Physics Sparkly"),
            Err("Sparkly".to_owned())
        );
    }
}
