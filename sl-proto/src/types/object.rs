//! In-world object schema: motion, shape, materials, animations, particles.

use sl_types::attachment::AttachmentPoint;
use sl_types::key::{AgentKey, GroupKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey};
use sl_types::lsl::Rotation;
use sl_types::lsl::Vector;
use sl_wire::Permissions5;
use sl_wire::ReflectionProbeFlags;
use sl_wire::RegionHandle;
use sl_wire::RegionLocalObjectId;
use uuid::Uuid;

use crate::scoped_id::{CircuitId, ScopedObjectId};

/// Linden `PCode` constants: the object-class byte (`p_code`) in an object
/// update, identifying what kind of entity an object is.
pub mod pcode {
    /// A primitive (an ordinary in-world object / prim).
    pub const PRIMITIVE: u8 = 9;
    /// An avatar.
    pub const AVATAR: u8 = 47;
    /// A grass patch.
    pub const GRASS: u8 = 95;
    /// A new-style (SL 1.x+) tree.
    pub const NEW_TREE: u8 = 111;
    /// A particle-system legacy object.
    pub const PARTICLE_SYSTEM: u8 = 143;
    /// A legacy tree.
    pub const TREE: u8 = 255;
}

/// An object's kinematic state, decoded from the packed `ObjectData`/`Data`
/// blob of an object update. Linear quantities are region-local; the rotation
/// is the object's orientation in its parent's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectMotion {
    /// Region-local position, in metres.
    pub position: Vector,
    /// Linear velocity, in metres/second.
    pub velocity: Vector,
    /// Linear acceleration, in metres/second².
    pub acceleration: Vector,
    /// Orientation (a unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (the rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
    /// The avatar collision (foot/standing) plane, present only for avatar
    /// updates and `None` for ordinary objects. The four components are the
    /// plane equation `[nx, ny, nz, d]`: a unit normal and a distance, giving
    /// the surface the avatar is standing on (niche, for inverse-kinematics /
    /// grounding). Decoded from the `LLVector4` prefix the simulator prepends to
    /// an avatar's motion blob.
    pub collision_plane: Option<[f32; 4]>,
}

/// A cached scene object (a primitive or avatar) for the current region,
/// assembled from `ObjectUpdate` / `ObjectUpdateCompressed` and kept current by
/// later full, compressed, and motion-only (`ImprovedTerseObjectUpdate`)
/// updates. Surfaced via [`Event::ObjectAdded`](crate::Event::ObjectAdded) / [`Event::ObjectUpdated`](crate::Event::ObjectUpdated) and
/// removed via [`Event::ObjectRemoved`](crate::Event::ObjectRemoved).
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// The region the object lives in (its [`RegionHandle`]).
    pub region_handle: RegionHandle,
    /// The region-local id (the transient handle the simulator uses; not stable
    /// across region crossings or relogins).
    pub local_id: RegionLocalObjectId,
    /// The circuit instance this object was learned on, paired with
    /// [`local_id`](Self::local_id) to form its [`ScopedObjectId`]. Stamped by
    /// the [`Session`](crate::Session) when the object is cached; the
    /// default/zero [`CircuitId`] on a freshly decoded object that has not been
    /// through the cache. Read it via [`scoped_id`](Self::scoped_id).
    pub circuit: CircuitId,
    /// The object's persistent global id.
    pub full_id: ObjectKey,
    /// The local id of the parent object this is linked/attached to, or 0 if it
    /// has no parent (a root object).
    pub parent_id: RegionLocalObjectId,
    /// The object class (see the [`pcode`] constants).
    pub pcode: u8,
    /// The raw object `state` byte, passed through verbatim. Its meaning
    /// depends on [`pcode`](Self::pcode): for a tree/grass it is the species,
    /// and for an *attachment* (a non-zero-state prim) it holds the
    /// attachment-point id with its nibbles swapped — read it via
    /// [`attachment_point`](Self::attachment_point), not directly.
    pub state: u8,
    /// The simulator's per-object CRC (used for object-cache validation).
    pub crc: u32,
    /// The material code.
    pub material: u8,
    /// The click action (`CLICK_ACTION_*`).
    pub click_action: u8,
    /// The object/prim flags bitfield (`PrimFlags`), from the update's
    /// `UpdateFlags`.
    pub update_flags: u32,
    /// The object's size, in metres along each axis.
    pub scale: Vector,
    /// The object's kinematic state.
    pub motion: ObjectMotion,
    /// The owner's id (only meaningful when the object has sound or particles;
    /// otherwise the simulator sends a null id — see the LL protocol "hack").
    pub owner_id: Uuid,
    /// The attached sound's asset id (null if none).
    pub sound: Uuid,
    /// The attached sound's gain.
    pub gain: f32,
    /// The attached sound's flags.
    pub sound_flags: u8,
    /// The attached sound's cutoff radius, in metres.
    pub sound_radius: f32,
    /// The object's floating text (`llSetText`), empty if none.
    pub text: String,
    /// The floating-text colour as RGBA bytes.
    pub text_color: [u8; 4],
    /// The object's name-value pairs (e.g. an attachment's `AttachItemID`), as
    /// the raw newline-separated string; empty if none.
    pub name_value: String,
    /// The media URL set on the object, empty if none.
    pub media_url: String,
    /// The raw `TextureEntry` blob (per-face texture/colour data), undecoded.
    /// Decode with
    /// [`decode_texture_entry`](crate::decode_texture_entry).
    pub texture_entry: Vec<u8>,
    /// The raw texture-animation (`TextureAnim`) blob (`llSetTextureAnim`),
    /// undecoded; empty if the object has no texture animation.
    pub texture_anim: Vec<u8>,
    /// The decoded [`TextureAnimation`] (`llSetTextureAnim`) parameters, or `None`
    /// when the object has no texture animation (or the blob is not the expected
    /// 16 bytes). Decoded from [`texture_anim`](Self::texture_anim).
    pub texture_animation: Option<TextureAnimation>,
    /// The decoded path/profile [`shape`](PrimShapeParams) parameters of a volume
    /// prim. Zeroed for object classes that carry no shape (e.g. avatars).
    pub shape: PrimShapeParams,
    /// The raw particle-system (`PSBlock`) blob (`llParticleSystem`), undecoded;
    /// empty if the object has no particle system.
    pub particle_system: Vec<u8>,
    /// The decoded [`ParticleSystem`] (`llParticleSystem`) parameters, or `None`
    /// when the object has no particle system (or the blob fails to decode).
    /// Decoded from [`particle_system`](Self::particle_system).
    pub particles: Option<ParticleSystem>,
    /// The raw generic-`Data` field: tree/grass genome bytes for a tree object,
    /// or the linkset prim count for a root prim (one byte). Empty if absent.
    pub data: Vec<u8>,
    /// The raw `ExtraParams` blob (flexi/light/sculpt/mesh parameters), as
    /// received on the wire.
    pub extra_params: Vec<u8>,
    /// The decoded [`ExtraParams`](ObjectExtraParams) sub-blocks
    /// (flexi/light/sculpt/light-image/extended-mesh/render-material/reflection
    /// probe), populated from both full and compressed `ObjectUpdate`s.
    pub extra: ObjectExtraParams,
    /// The object's extended properties (creator, permissions, name,
    /// description, …) once an [`Event::ObjectProperties`](crate::Event::ObjectProperties) has been received for
    /// it; `None` until then.
    pub properties: Option<ObjectProperties>,
    /// The deprecated legacy joint type (`JointType`). Part of the long-obsolete
    /// physical-joint mechanism; carried by a full `ObjectUpdate` but virtually
    /// always zero on modern grids. Surfaced verbatim for fidelity. Not carried
    /// by compressed updates (left zero there).
    pub joint_type: u8,
    /// The deprecated legacy joint pivot point (`JointPivot`), in object-local
    /// metres. See [`joint_type`](Self::joint_type); usually the zero vector.
    pub joint_pivot: Vector,
    /// The deprecated legacy joint axis or anchor (`JointAxisOrAnchor`). See
    /// [`joint_type`](Self::joint_type); usually the zero vector.
    pub joint_axis_or_anchor: Vector,
}

/// The `ATTACHMENT_ADD` flag the simulator may OR into a freshly un-swizzled
/// attachment id to mark an "add" (rather than "replace") attach. It is not
/// part of the attachment point itself and is stripped before the point is
/// returned. Mirrors the reference viewer's `ATTACHMENT_ADD` constant.
const ATTACHMENT_ADD: u8 = 0x80;

/// Reverse the simulator's attachment-point nibble-swap on an object `state`
/// byte and strip the [`ATTACHMENT_ADD`] flag, yielding the plain attachment
/// point. For an attachment the point id is hidden in `state` with its upper
/// and lower nibbles swapped (kept for backward compatibility with old objects
/// that used only the upper nibble); this mirrors the reference viewer's
/// `ATTACHMENT_ID_FROM_STATE` macro.
const fn attachment_point_from_state(state: u8) -> u8 {
    (((state & 0xf0) >> 4) | ((state & 0x0f) << 4)) & !ATTACHMENT_ADD
}

impl Object {
    /// This object's [`ScopedObjectId`] — its region-local id paired with the
    /// circuit it was learned on. Pass it to the object
    /// [`Session`](crate::Session) methods (or [`Session::object`](crate::Session::object))
    /// so the id can only be acted upon against the circuit it belongs to.
    #[must_use]
    pub const fn scoped_id(&self) -> ScopedObjectId {
        ScopedObjectId::new(self.circuit, self.local_id)
    }

    /// The [`ScopedObjectId`] of this object's parent (the linkset root or the
    /// avatar it is attached to), scoped to the same circuit. The parent id is
    /// [`RegionLocalObjectId`]`(0)` (a region-local zero) for a root/unparented
    /// object — check [`parent_id`](Self::parent_id) against zero first if that
    /// distinction matters.
    #[must_use]
    pub const fn scoped_parent_id(&self) -> ScopedObjectId {
        ScopedObjectId::new(self.circuit, self.parent_id)
    }

    /// The raw, un-swizzled attachment-point id this object is worn on, or
    /// `None` if it is not an attachment.
    ///
    /// For an attachment the simulator hides the attachment-point id inside the
    /// raw [`state`](Self::state) byte with its upper and lower nibbles swapped
    /// (kept for backward compatibility with very old objects that used only the
    /// upper nibble), so reading `state` directly yields the wrong number. This
    /// accessor reverses the swap — the reference viewer's
    /// `ATTACHMENT_ID_FROM_STATE` — and strips the transient `ATTACHMENT_ADD`
    /// (`0x80`) bit, returning the plain attachment-point id (e.g. `1` = chest,
    /// `6` = right hand, `35` = HUD center 1).
    ///
    /// Returns `None` for anything that is not a non-zero-`state` prim: plain
    /// prims (`state == 0`) and trees/grass (whose `state` byte instead carries
    /// the species), mirroring the viewer's `LLVOVolume::isAttachment`.
    ///
    /// Prefer [`attachment_point`](Self::attachment_point) for a named point;
    /// this raw form additionally covers any future id the [`AttachmentPoint`]
    /// enum does not yet name.
    #[must_use]
    pub const fn attachment_point_id(&self) -> Option<u8> {
        if self.pcode == pcode::PRIMITIVE && self.state != 0 {
            Some(attachment_point_from_state(self.state))
        } else {
            None
        }
    }

    /// The named attachment point this object is worn on, or `None` if it is not
    /// an attachment (or is attached to an id the [`AttachmentPoint`] enum does
    /// not name — see [`attachment_point_id`](Self::attachment_point_id) for the
    /// raw form).
    ///
    /// Decodes the un-swizzled [`attachment_point_id`](Self::attachment_point_id)
    /// into the shared [`AttachmentPoint`] enum, covering both avatar points
    /// (e.g. chest, right hand) and HUD points (e.g. top-left, center).
    #[must_use]
    pub fn attachment_point(&self) -> Option<AttachmentPoint> {
        self.attachment_point_id()
            .and_then(|id| AttachmentPoint::from_repr(usize::from(id)))
    }

    /// The object's [`name_value`](Self::name_value) pairs, parsed from the raw
    /// newline-separated string into structured [`NameValue`] entries (e.g. an
    /// attachment's `AttachItemID`). Empty when the object carries none. Lines
    /// that have no name/type are skipped. The parse mirrors the reference
    /// viewer's `LLNameValue` string constructor: each line is
    /// `name type [class] [sendto] data`, where `class` and `sendto` are present
    /// only when the token is one of the recognized keywords.
    #[must_use]
    pub fn name_values(&self) -> Vec<NameValue> {
        self.name_value
            .lines()
            .filter_map(NameValue::parse_line)
            .collect()
    }

    /// The data value of the first [`name_value`](Self::name_value) pair named
    /// `name`, or `None` if the object has no such pair. A convenience over
    /// [`name_values`](Self::name_values) for the common single-key lookup (e.g.
    /// `object.name_value_data("AttachItemID")`).
    #[must_use]
    pub fn name_value_data(&self, name: &str) -> Option<String> {
        self.name_value
            .lines()
            .filter_map(NameValue::parse_line)
            .find(|pair| pair.name == name)
            .map(|pair| pair.value)
    }
}

/// One parsed entry of an object's packed `name_value` string (the reference
/// viewer's `LLNameValue`). Produced by [`Object::name_values`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValue {
    /// The entry's name (the lookup key, e.g. `AttachItemID`).
    pub name: String,
    /// The declared value type token (`STRING`, `F32`, `S32`, `VEC3`, `U32`,
    /// `ASSET`, `U64`, …); empty if the line carried none.
    pub value_type: String,
    /// The access class token: `R` (read-only) or `RW` (read-write). Defaults to
    /// `RW` when the line omits it (matching the viewer).
    pub class: String,
    /// The send-to token: `S`/`DS`/`SV`/`DSV`. Defaults to `S` (sim only) when the
    /// line omits it (matching the viewer).
    pub sendto: String,
    /// The entry's data value, as the verbatim remainder of the line.
    pub value: String,
}

impl NameValue {
    /// Parses one `name_value` line into a [`NameValue`], or `None` when the line
    /// has no name or no type. The `class` and `sendto` tokens are optional and
    /// recognized only by the viewer's keyword sets; anything else is taken as the
    /// start of the data value.
    fn parse_line(line: &str) -> Option<Self> {
        const CLASS_TOKENS: [&str; 5] = ["R", "RW", "READ_ONLY", "READ_WRITE", "CALLBACK"];
        const SENDTO_TOKENS: [&str; 8] = [
            "S",
            "DS",
            "SV",
            "DSV",
            "SIM",
            "SIM_SPACE",
            "SIM_VIEWER",
            "SIM_SPACE_VIEWER",
        ];
        let mut rest = line.trim_start();
        let take = |rest: &mut &str| -> Option<String> {
            let trimmed = rest.trim_start();
            if trimmed.is_empty() {
                return None;
            }
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let token = trimmed.get(..end)?.to_owned();
            *rest = trimmed.get(end..).unwrap_or("");
            Some(token)
        };
        let name = take(&mut rest)?;
        let value_type = take(&mut rest)?;
        // `class` is present only if the next token is a recognized keyword.
        let next = rest.trim_start();
        let class = if CLASS_TOKENS
            .iter()
            .any(|token| starts_with_token(next, token))
        {
            take(&mut rest).unwrap_or_default()
        } else {
            "RW".to_owned()
        };
        // `sendto` is likewise present only for a recognized keyword.
        let next = rest.trim_start();
        let sendto = if SENDTO_TOKENS
            .iter()
            .any(|token| starts_with_token(next, token))
        {
            take(&mut rest).unwrap_or_default()
        } else {
            "S".to_owned()
        };
        Some(Self {
            name,
            value_type,
            class,
            sendto,
            value: rest.trim_start().to_owned(),
        })
    }
}

/// Whether `text` begins with the whitespace-delimited token `token` (the token
/// followed by whitespace or end-of-string), used to decide whether an optional
/// `name_value` class/sendto keyword is present.
fn starts_with_token(text: &str, token: &str) -> bool {
    text.strip_prefix(token)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
}

/// The path/profile shape parameters of a volume prim, as carried (in raw
/// quantized wire form) by both full and compressed `ObjectUpdate`s. The values
/// are the simulator's quantized integers — the same encoding [`PrimShape`](crate::PrimShape) uses
/// to *send* a shape — not dequantized floats; the quantization for each field
/// matches the like-named [`PrimShape`](crate::PrimShape) field (e.g. `path_begin / 0.00002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrimShapeParams {
    /// The path curve byte (`LL_PCODE_PATH_*`).
    pub path_curve: u8,
    /// The profile curve byte (`LL_PCODE_PROFILE_*`, hollow shape in the high
    /// nibble).
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
}

/// The decoded `ExtraParams` sub-blocks of an [`Object`]. The `ExtraParams` blob
/// in an `ObjectUpdate` is a list of optional typed parameters (each a Linden
/// `LLNetworkData` subtype); each field here is present only if the object
/// carries that parameter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectExtraParams {
    /// Flexible-path ("flexi") parameters (`PARAMS_FLEXIBLE`, `0x10`).
    pub flexible: Option<FlexibleData>,
    /// Point/spot-light parameters (`PARAMS_LIGHT`, `0x20`).
    pub light: Option<LightData>,
    /// Sculpt / mesh parameters (`PARAMS_SCULPT` `0x30` or `PARAMS_MESH`
    /// `0x60` — a mesh is carried in the same block).
    pub sculpt: Option<SculptData>,
    /// Projected-light texture parameters (`PARAMS_LIGHT_IMAGE`, `0x40`).
    pub light_image: Option<LightImage>,
    /// Extended-mesh flags (`PARAMS_EXTENDED_MESH`, `0x70`).
    pub extended_mesh: Option<ExtendedMesh>,
    /// Per-face GLTF (PBR) render-material asset references
    /// (`PARAMS_RENDER_MATERIAL`, `0x80`); empty if the object has none.
    pub render_material: Vec<RenderMaterialRef>,
    /// Reflection-probe parameters (`PARAMS_REFLECTION_PROBE`, `0x90`).
    pub reflection_probe: Option<ReflectionProbe>,
}

/// Flexible-path ("flexi") parameters (`LLFlexibleObjectData`): the prim's path
/// bends under simulated softbody physics.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexibleData {
    /// The softness / simulate-LOD level (0–3): how finely the path flexes.
    pub softness: u8,
    /// Path stiffness (resistance to bending).
    pub tension: f32,
    /// Air friction (how quickly motion damps).
    pub air_friction: f32,
    /// Gravity applied to the path tip.
    pub gravity: f32,
    /// Sensitivity to region wind.
    pub wind_sensitivity: f32,
    /// A constant force pushing the path (zero if the sim did not send it).
    pub user_force: Vector,
}

/// Point/spot-light parameters (`LLLightParams`): the object emits light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightData {
    /// The light colour, RGBA as sent on the wire (sRGB).
    pub color: [u8; 4],
    /// The light radius, in metres.
    pub radius: f32,
    /// The spotlight cutoff angle.
    pub cutoff: f32,
    /// The light falloff exponent.
    pub falloff: f32,
}

/// Sculpt or mesh parameters (`LLSculptParams`): the prim's shape comes from a
/// sculpt texture or a mesh asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SculptData {
    /// The sculpt texture or mesh asset id.
    pub texture: Uuid,
    /// The sculpt type byte (`LL_SCULPT_TYPE_*` in the low bits — sphere/torus/
    /// plane/cylinder/mesh — plus invert/mirror/animesh flag bits).
    pub sculpt_type: u8,
}

/// Projected-light texture parameters (`LLLightImageParams`): a light projects
/// an image.
#[derive(Debug, Clone, PartialEq)]
pub struct LightImage {
    /// The projected texture id.
    pub texture: Uuid,
    /// The projection parameters `(field-of-view, focus, ambiance)`.
    pub params: Vector,
}

/// Extended-mesh flags (`LLExtendedMeshParams`), e.g. animated-mesh
/// (`ANIMATED_MESH_ENABLED_FLAG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedMesh {
    /// The extended-mesh flag bits.
    pub flags: u32,
}

/// One per-face GLTF (PBR) render-material reference
/// (`LLRenderMaterialParams::Entry`): the material asset applied to a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderMaterialRef {
    /// The texture-entry (face) index the material applies to.
    pub face: u8,
    /// The render-material asset id (an `AT_MATERIAL` / GLTF material).
    pub material_id: Uuid,
}

/// A PBR **reflection probe**: a Second Life-specific per-object property
/// (`ExtraParams` type `0x90`, `LLReflectionProbeParams`) marking an object as a
/// probe that captures the surrounding environment for image-based lighting and
/// reflections. The probe itself is rendered by the viewer (there is no asset to
/// fetch); these are just its volume parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReflectionProbe {
    /// The probe's ambiance (irradiance) scale.
    pub ambiance: f32,
    /// The near-clip distance of the probe's reflection capture, in metres.
    pub clip_distance: f32,
    /// The probe's flag set (`FLAG_BOX_VOLUME` / `FLAG_DYNAMIC` / `FLAG_MIRROR`):
    /// whether the influence volume is a box rather than a sphere, whether
    /// dynamic objects (e.g. avatars) are rendered into the probe, and whether
    /// the probe drives a realtime mirror.
    pub flags: ReflectionProbeFlags,
}

/// Mode (`mMode`) bit flags for a [`TextureAnimation`] (`LLTextureAnim`), matching
/// the LSL `llSetTextureAnim` flags and the reference viewer's `LLTextureAnim`
/// enum.
pub mod texture_anim_mode {
    /// The animation is running (`ON`); cleared means the prim is static.
    pub const ON: u8 = 0x01;
    /// Loop the animation (`LOOP`).
    pub const LOOP: u8 = 0x02;
    /// Play the animation in reverse (`REVERSE`).
    pub const REVERSE: u8 = 0x04;
    /// Bounce back and forth rather than restart (`PING_PONG`).
    pub const PING_PONG: u8 = 0x08;
    /// Slide smoothly rather than step frame-by-frame (`SMOOTH`).
    pub const SMOOTH: u8 = 0x10;
    /// Rotate the texture instead of paging frames (`ROTATE`); `start`/`length`
    /// are then start/end angles in radians.
    pub const ROTATE: u8 = 0x20;
    /// Scale the texture instead of paging frames (`SCALE`); `start`/`length` are
    /// then start/end scales.
    pub const SCALE: u8 = 0x40;
}

/// A prim's texture-animation parameters (`TextureAnim` / `LLTextureAnim`, set by
/// `llSetTextureAnim`): a 16-byte block driving an animated, rotating, or scaling
/// texture on one or all of a prim's faces. Decoded by
/// [`decode_texture_anim`](crate::decode_texture_anim).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureAnimation {
    /// The mode bit field (see [`texture_anim_mode`]). With
    /// [`ON`](texture_anim_mode::ON) clear the prim has no active animation.
    pub mode: u8,
    /// The face the animation applies to, or `-1` for all faces.
    pub face: i8,
    /// The number of horizontal frames in the texture grid (the `x` argument of
    /// `llSetTextureAnim`). For a non-[`SMOOTH`](texture_anim_mode::SMOOTH)
    /// animation a zero is treated by the viewer as 1.
    pub size_x: u8,
    /// The number of vertical frames in the texture grid (the `y` argument).
    pub size_y: u8,
    /// The start frame (or, in [`ROTATE`](texture_anim_mode::ROTATE)/
    /// [`SCALE`](texture_anim_mode::SCALE) mode, the start angle/scale).
    pub start: f32,
    /// The number of frames to display (or, in rotate/scale mode, the end
    /// angle/scale).
    pub length: f32,
    /// The playback rate, in frames per second (or radians/second when rotating).
    pub rate: f32,
}

/// Particle-flow pattern (`mPattern`) values for a [`ParticleSystem`], matching
/// the reference viewer's `LLPartSysData::LL_PART_SRC_PATTERN_*` enum.
pub mod particle_pattern {
    /// Particles drop from the source (`LL_PART_SRC_PATTERN_DROP`).
    pub const DROP: u8 = 0x01;
    /// Particles explode outward from the source (`LL_PART_SRC_PATTERN_EXPLODE`).
    pub const EXPLODE: u8 = 0x02;
    /// Particles emit along an angle (`LL_PART_SRC_PATTERN_ANGLE`).
    pub const ANGLE: u8 = 0x04;
    /// Particles emit within a cone (`LL_PART_SRC_PATTERN_ANGLE_CONE`).
    pub const ANGLE_CONE: u8 = 0x08;
    /// Particles emit within an empty (hollow) cone
    /// (`LL_PART_SRC_PATTERN_ANGLE_CONE_EMPTY`).
    pub const ANGLE_CONE_EMPTY: u8 = 0x10;
}

/// A prim's particle system (`PSBlock` / `LLPartSysData`, set by
/// `llParticleSystem`): the source parameters plus the template particle
/// parameters the source emits. Decoded by
/// [`decode_particle_system`](crate::decode_particle_system) from both the legacy
/// (86-byte) and modern (size-prefixed, glow/blend-extended) wire forms.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSystem {
    /// The system CRC (a non-zero value marks a live system; zero means "no
    /// system").
    pub crc: u32,
    /// The source flags (`LL_PART_SRC_*` — object-relative accel/velocity and the
    /// new-angle flag).
    pub flags: u32,
    /// The emission pattern (see [`particle_pattern`]).
    pub pattern: u8,
    /// The source's maximum lifetime, in seconds (0 = forever).
    pub max_age: f32,
    /// The age at which the system starts, in seconds.
    pub start_age: f32,
    /// The inner emission angle, in radians (for the angle/cone patterns).
    pub inner_angle: f32,
    /// The outer emission angle, in radians.
    pub outer_angle: f32,
    /// How often a burst of particles is emitted, in seconds.
    pub burst_rate: f32,
    /// The emission radius, in metres.
    pub burst_radius: f32,
    /// The minimum particle launch speed, in metres/second.
    pub burst_speed_min: f32,
    /// The maximum particle launch speed, in metres/second.
    pub burst_speed_max: f32,
    /// How many particles are emitted per burst.
    pub burst_part_count: u8,
    /// The angular velocity of the emission axis, in radians/second per axis.
    pub angular_velocity: Vector,
    /// The acceleration applied to each particle, in metres/second² per axis.
    pub acceleration: Vector,
    /// The particle texture asset id (nil for the default).
    pub texture_id: Uuid,
    /// The target object the particles follow/aim at (for the target patterns and
    /// the `TARGET_POS`/`TARGET_LINEAR` particle flags); nil if none.
    pub target_id: ObjectKey,
    /// The per-particle flags (`LL_PART_*_MASK` — interpolation, bounce, wind,
    /// follow, emissive, beam, ribbon, and the glow/blend system-set bits).
    pub part_flags: u32,
    /// Each particle's maximum age, in seconds.
    pub part_max_age: f32,
    /// The particle start colour, RGBA as sent on the wire.
    pub part_start_color: [u8; 4],
    /// The particle end colour, RGBA as sent on the wire.
    pub part_end_color: [u8; 4],
    /// The particle start scale `(x, y)`, in metres.
    pub part_start_scale: [f32; 2],
    /// The particle end scale `(x, y)`, in metres.
    pub part_end_scale: [f32; 2],
    /// The particle start glow (0–1); 0 unless the system carries glow data.
    pub part_start_glow: f32,
    /// The particle end glow (0–1); 0 unless the system carries glow data.
    pub part_end_glow: f32,
    /// The source blend function (`LL_PART_BF_*`); defaults to source-alpha unless
    /// the system carries blend data.
    pub part_blend_func_source: u8,
    /// The destination blend function (`LL_PART_BF_*`); defaults to
    /// one-minus-source-alpha unless the system carries blend data.
    pub part_blend_func_dest: u8,
}

/// An object's extended properties (`ObjectProperties`), delivered after the
/// object is selected (see
/// [`Session::request_object_properties`](crate::Session::request_object_properties)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProperties {
    /// The object's persistent global id.
    pub object_id: ObjectKey,
    /// The creator's id.
    pub creator_id: AgentKey,
    /// The current owner — an agent, or a group when the object is deeded to a
    /// group (signalled on the wire by a null `OwnerID`).
    pub owner: OwnerKey,
    /// The group the object is set to, or `None` when no group is set (a
    /// group-*owned* object reports its group via [`owner`](Self::owner)).
    pub group: Option<GroupKey>,
    /// The previous owner's id.
    pub last_owner_id: Uuid,
    /// The creation timestamp (seconds since the Unix epoch).
    pub creation_date: u64,
    /// The base / owner / group / everyone / next-owner permission masks.
    pub permissions: Permissions5,
    /// The ownership cost, in L$.
    pub ownership_cost: i32,
    /// The sale type (`SALE_TYPE_*`).
    pub sale_type: u8,
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The object category code.
    pub category: u32,
    /// The task-inventory serial; bumps whenever the object's contents change,
    /// so a client can detect task-inventory mutations without re-fetching.
    pub inventory_serial: i16,
    /// The inventory item this object was rezzed from (nil if not applicable),
    /// used to correlate an in-world object back to its inventory item — needed
    /// for attachments and "find in inventory".
    pub item_id: InventoryKey,
    /// The inventory folder the source item lives in (nil if not applicable).
    pub folder_id: InventoryFolderKey,
    /// The task (object) this item came from, when it was rezzed from another
    /// object's contents (nil if not applicable).
    pub from_task_id: ObjectKey,
    /// The aggregate permission rollup across the linkset's contents — the
    /// build-floater "next owner can…" summary.
    pub aggregate_perms: u8,
    /// The aggregate permission rollup for textures across the linkset.
    pub aggregate_perm_textures: u8,
    /// The owner-facing aggregate permission rollup for textures.
    pub aggregate_perm_textures_owner: u8,
    /// The object's name.
    pub name: String,
    /// The object's description.
    pub description: String,
    /// The custom touch-action label, empty if none.
    pub touch_name: String,
    /// The custom sit-action label, empty if none.
    pub sit_name: String,
    /// The linkset's concatenated texture-asset ids (the wire carries them as a
    /// run of 16-byte UUIDs); empty if the sim sent none.
    pub texture_ids: Vec<Uuid>,
}

/// An object's condensed broadcast properties (`ObjectPropertiesFamily`),
/// delivered after a
/// [`Session::request_object_properties_family`](crate::Session::request_object_properties_family).
/// Unlike the full [`ObjectProperties`] (which needs the object selected), the
/// family reply carries just the owner/permissions/sale summary a viewer shows
/// on hover or in the pay/report dialogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectPropertiesFamily {
    /// The request flags echoed back from the request (e.g. `OBJECT_PAY_REQUEST`
    /// `0x04`), letting a viewer route the reply to the dialog that asked.
    pub request_flags: u32,
    /// The object's persistent global id.
    pub object_id: ObjectKey,
    /// The current owner — an agent, or a group when the object is deeded to a
    /// group (signalled on the wire by a null `OwnerID`).
    pub owner: OwnerKey,
    /// The group the object is set to, or `None` when no group is set (a
    /// group-*owned* object reports its group via [`owner`](Self::owner)).
    pub group: Option<GroupKey>,
    /// The base / owner / group / everyone / next-owner permission masks.
    pub permissions: Permissions5,
    /// The ownership cost, in L$.
    pub ownership_cost: i32,
    /// The sale type (`SALE_TYPE_*`).
    pub sale_type: u8,
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The object category code.
    pub category: u32,
    /// The previous owner's id.
    pub last_owner_id: Uuid,
    /// The object's name.
    pub name: String,
    /// The object's description.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Terrain heightmaps (#18): the patched-DCT-compressed `LayerData` layers.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// An [`Object`] carrying only the given raw `name_value` string; all other
    /// fields are defaulted to values irrelevant to the parser under test.
    fn object_with_name_value(name_value: &str) -> super::Object {
        test_object(0, 0, name_value)
    }

    fn test_object(pcode: u8, state: u8, name_value: &str) -> super::Object {
        super::Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(0),
            circuit: CircuitId::default(),
            full_id: ObjectKey::from(super::Uuid::nil()),
            parent_id: RegionLocalObjectId(0),
            pcode,
            state,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            motion: super::ObjectMotion {
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                acceleration: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: super::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                collision_plane: None,
            },
            owner_id: super::Uuid::nil(),
            sound: super::Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: name_value.to_owned(),
            media_url: String::new(),
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: super::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: super::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            joint_axis_or_anchor: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        }
    }

    /// The [`ObjectKey`] wrapper on [`Object::full_id`] round-trips
    /// bit-identically to the raw `Uuid` it replaced (wrap then unwrap is the
    /// identity), so the codec boundary is transparent.
    #[test]
    fn object_key_round_trips_raw_uuid() {
        let raw = super::Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);
        let key = ObjectKey::from(raw);
        assert_eq!(key.uuid(), raw);
        let object = test_object(0, 0, "");
        // The default object's full id is the nil object key, not a stray value.
        assert_eq!(object.full_id, ObjectKey::from(super::Uuid::nil()));
    }

    #[test]
    fn name_value_parses_class_and_sendto() -> Result<(), String> {
        // The common attachment form: name type class sendto data.
        let object = object_with_name_value(
            "AttachItemID STRING RW SV 11111111-2222-3333-4444-555555555555",
        );
        let pairs = object.name_values();
        let pair = pairs.first().ok_or("expected one pair")?;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pair.name, "AttachItemID");
        assert_eq!(pair.value_type, "STRING");
        assert_eq!(pair.class, "RW");
        assert_eq!(pair.sendto, "SV");
        assert_eq!(pair.value, "11111111-2222-3333-4444-555555555555");
        assert_eq!(
            object.name_value_data("AttachItemID").as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(object.name_value_data("Missing"), None);
        Ok(())
    }

    #[test]
    fn name_value_defaults_omitted_class_and_sendto() -> Result<(), String> {
        // No class/sendto keyword: the token after the type starts the data, and
        // class/sendto default to RW/S (matching the viewer).
        let object = object_with_name_value("FooBar STRING hello world");
        let pairs = object.name_values();
        let pair = pairs.first().ok_or("expected one pair")?;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pair.name, "FooBar");
        assert_eq!(pair.value_type, "STRING");
        assert_eq!(pair.class, "RW");
        assert_eq!(pair.sendto, "S");
        assert_eq!(pair.value, "hello world");
        Ok(())
    }

    #[test]
    fn name_value_parses_multiple_lines_and_skips_blanks() -> Result<(), String> {
        let object =
            object_with_name_value("A STRING RW S one\n\nB S32 RW DSV 42\n   \nincomplete");
        let pairs = object.name_values();
        // The blank lines are skipped; "incomplete" (name only, no type) too.
        let first = pairs.first().ok_or("expected a first pair")?;
        let second = pairs.get(1).ok_or("expected a second pair")?;
        assert_eq!(pairs.len(), 2);
        assert_eq!(first.name, "A");
        assert_eq!(first.value, "one");
        assert_eq!(second.name, "B");
        assert_eq!(second.sendto, "DSV");
        assert_eq!(second.value, "42");
        Ok(())
    }

    #[test]
    fn attachment_point_unswizzles_state_nibbles() {
        use super::AttachmentPoint;
        use sl_types::attachment::AvatarAttachmentPoint;
        // The simulator swaps the nibbles of the point id. Right hand (6 = 0x06)
        // travels the wire as 0x60; chest (1 = 0x01) as 0x10. The accessor must
        // swap back, both as a raw id and as the named enum variant.
        for (point, wire) in [
            (AvatarAttachmentPoint::RightHand, 0x60_u8),
            (AvatarAttachmentPoint::Chest, 0x10),
        ] {
            let object = test_object(super::pcode::PRIMITIVE, wire, "");
            assert_eq!(
                object.attachment_point(),
                Some(AttachmentPoint::Avatar(point))
            );
        }
        // Right hand (6) as the raw id.
        let hand = test_object(super::pcode::PRIMITIVE, 0x60, "");
        assert_eq!(hand.attachment_point_id(), Some(6));
        // The transient ATTACHMENT_ADD (0x80) bit is stripped: un-swizzling
        // 0x68 gives 0x86, and stripping 0x80 leaves right hand (6).
        let adding = test_object(super::pcode::PRIMITIVE, 0x68, "");
        assert_eq!(adding.attachment_point_id(), Some(6));
        assert_eq!(
            adding.attachment_point(),
            Some(AttachmentPoint::Avatar(AvatarAttachmentPoint::RightHand))
        );
    }

    #[test]
    fn attachment_point_decodes_hud_points() {
        use super::AttachmentPoint;
        use sl_types::attachment::HudAttachmentPoint;
        // HUD center 1 (id 35 = 0x23) travels the wire nibble-swapped as 0x32.
        // The unified AttachmentPoint enum names HUD points, so both accessors
        // surface it.
        let hud = test_object(super::pcode::PRIMITIVE, 0x32, "");
        assert_eq!(hud.attachment_point_id(), Some(35));
        assert_eq!(
            hud.attachment_point(),
            Some(AttachmentPoint::Hud(HudAttachmentPoint::Center))
        );
    }

    #[test]
    fn attachment_point_none_for_non_attachments() {
        // A plain prim (state 0) is not an attachment.
        let prim = test_object(super::pcode::PRIMITIVE, 0, "");
        assert_eq!(prim.attachment_point_id(), None);
        assert_eq!(prim.attachment_point(), None);
        // A tree's non-zero state byte is its species, not an attachment point.
        let tree = test_object(super::pcode::TREE, 3, "");
        assert_eq!(tree.attachment_point_id(), None);
        // Grass likewise.
        let grass = test_object(super::pcode::GRASS, 1, "");
        assert_eq!(grass.attachment_point_id(), None);
    }
}
