//! A prim as a script reads and writes it: the state behind
//! `llSetPrimitiveParams` and `llGetPrimitiveParams`, the older per-face
//! calls, the identity setters, the effects and the prim's media.
//!
//! The host lends a prim as a [`Prim`] ([`Host::prim`](crate::vm::Host::prim))
//! and takes back the whole of it once a call has changed it
//! ([`Host::set_prim`](crate::vm::Host::set_prim)); every rule of a grid —
//! which values it clamps, which it stores as a byte, which it refuses —
//! is the library's, applied before the prim goes back, so a host only
//! translates between this and its own record of the prim. The values are
//! kept as a script set them: a grid that stores a colour as a byte has the
//! library round it first (`library::prim_params`).
//!
//! The shape is held as the volume parameters Linden Lab's viewer and both
//! grids keep ([`Shape`]), not as the `PRIM_TYPE` list a script writes; the
//! library converts one to the other, each flavour its own way.

use sl_types::lsl::{Rotation, Vector};

/// The texture every face of a new prim carries (Linden Lab's plywood).
pub const PLYWOOD: &str = "89556747-24cb-43ed-920b-47caed15465f";

/// `NULL_KEY`.
const NULL: &str = "00000000-0000-0000-0000-000000000000";

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// White.
const WHITE: Vector = Vector {
    x: 1.0,
    y: 1.0,
    z: 1.0,
};

/// A texture's repeats when they are not set: once each way.
const ONCE: Vector = Vector {
    x: 1.0,
    y: 1.0,
    z: 0.0,
};

/// The identity rotation.
const IDENTITY: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// The profile curve of a prim's cross-section (`LL_PCODE_PROFILE_*`), the
/// low nibble of [`Shape::profile_curve`].
pub mod profile {
    /// A circle (cylinder, torus).
    pub const CIRCLE: u8 = 0;
    /// A square (box, tube).
    pub const SQUARE: u8 = 1;
    /// An isosceles triangle (unused by scripts).
    pub const ISOTRI: u8 = 2;
    /// An equilateral triangle (prism, ring).
    pub const EQUALTRI: u8 = 3;
    /// A right triangle (unused by scripts).
    pub const RIGHTTRI: u8 = 4;
    /// A half circle (sphere).
    pub const CIRCLE_HALF: u8 = 5;
    /// The low nibble: the profile.
    pub const MASK: u8 = 0x0f;
    /// The high nibble: the hole's shape.
    pub const HOLE_MASK: u8 = 0xf0;
}

/// The path a prim's cross-section is swept along (`LL_PCODE_PATH_*`),
/// [`Shape::path_curve`].
pub mod path {
    /// A straight line (box, cylinder, prism).
    pub const LINE: u8 = 0x10;
    /// A circle (sphere, torus, tube, ring, sculpt).
    pub const CIRCLE: u8 = 0x20;
    /// A second circle.
    pub const CIRCLE2: u8 = 0x30;
    /// A test path.
    pub const TEST: u8 = 0x40;
    /// A flexible line.
    pub const FLEXIBLE: u8 = 0x80;
}

/// A sculpted or mesh prim's map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sculpt {
    /// The sculpt texture's or the mesh's asset key.
    pub map: String,
    /// `PRIM_SCULPT_TYPE_*` with its flags, as a script sets it.
    pub kind: i32,
}

/// A prim's shape as volume parameters, the way Linden Lab's viewer and both
/// grids keep it (`LLVolumeParams`). The fields are the parameters' own
/// values, not the bytes they travel in.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The profile in the low nibble, the hole's shape in the high one.
    pub profile_curve: u8,
    /// The path.
    pub path_curve: u8,
    /// Where the profile starts, 0 to 1.
    pub profile_begin: f32,
    /// Where the profile ends, 0 to 1.
    pub profile_end: f32,
    /// The hollow, 0 to 1.
    pub hollow: f32,
    /// Where the path starts, 0 to 1.
    pub path_begin: f32,
    /// Where the path ends, 0 to 1.
    pub path_end: f32,
    /// The path's scale: a straight path's top size (1 untapered, the
    /// `PRIM_TYPE` taper), a circular path's hole size.
    pub scale: [f32; 2],
    /// The top shear.
    pub shear: [f32; 2],
    /// The twist at the path's start, in half turns.
    pub twist_begin: f32,
    /// The twist at the path's end, in half turns.
    pub twist_end: f32,
    /// A circular path's radius offset.
    pub radius_offset: f32,
    /// A circular path's taper.
    pub taper: [f32; 2],
    /// A circular path's revolutions, 1 to 4.
    pub revolutions: f32,
    /// A circular path's skew.
    pub skew: f32,
    /// The map of a sculpted or mesh prim, [`None`] for a prim shaped by its
    /// parameters alone.
    pub sculpt: Option<Sculpt>,
}

impl Default for Shape {
    /// A new prim's: a half-metre box.
    fn default() -> Self {
        Self {
            profile_curve: profile::SQUARE,
            path_curve: path::LINE,
            profile_begin: 0.0,
            profile_end: 1.0,
            hollow: 0.0,
            path_begin: 0.0,
            path_end: 1.0,
            scale: [1.0, 1.0],
            shear: [0.0, 0.0],
            twist_begin: 0.0,
            twist_end: 0.0,
            radius_offset: 0.0,
            taper: [0.0, 0.0],
            revolutions: 1.0,
            skew: 0.0,
            sculpt: None,
        }
    }
}

/// A texture map of a face with its placement: the diffuse texture's, the
/// normal map's and the specular map's.
#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    /// The texture's key.
    pub texture: String,
    /// The repeats, `x` across and `y` up.
    pub repeats: Vector,
    /// The offsets.
    pub offsets: Vector,
    /// The rotation, in radians.
    pub rotation: f32,
}

impl Map {
    /// A map with no texture: `NULL_KEY`, repeated once, unmoved.
    #[must_use]
    pub fn none() -> Self {
        Self {
            texture: NULL.to_owned(),
            repeats: ONCE,
            offsets: ZERO,
            rotation: 0.0,
        }
    }
}

/// A face's specular map and what goes with it (`PRIM_SPECULAR`).
#[derive(Debug, Clone, PartialEq)]
pub struct Specular {
    /// The map.
    pub map: Map,
    /// The specular colour.
    pub color: Vector,
    /// The glossiness, 0 to 255.
    pub glossiness: i32,
    /// The environment intensity, 0 to 255.
    pub environment: i32,
}

impl Default for Specular {
    fn default() -> Self {
        Self {
            map: Map::none(),
            color: WHITE,
            glossiness: 51,
            environment: 0,
        }
    }
}

/// A face's glTF material overrides (`PRIM_GLTF_*`): each [`None`] where the
/// face keeps its material's own value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GltfOverrides {
    /// The base colour map, its repeats, offsets and rotation.
    pub base_color_map: Option<Map>,
    /// The base colour.
    pub base_color: Option<Vector>,
    /// The base colour's alpha.
    pub base_alpha: Option<f32>,
    /// The alpha mode.
    pub alpha_mode: Option<i32>,
    /// The alpha cutoff.
    pub alpha_cutoff: Option<f32>,
    /// Whether the face is drawn on both sides.
    pub double_sided: Option<bool>,
    /// The normal map, its repeats, offsets and rotation.
    pub normal_map: Option<Map>,
    /// The metallic-roughness map, its repeats, offsets and rotation.
    pub metallic_roughness_map: Option<Map>,
    /// The metallic factor.
    pub metallic: Option<f32>,
    /// The roughness factor.
    pub roughness: Option<f32>,
    /// The emissive map, its repeats, offsets and rotation.
    pub emissive_map: Option<Map>,
    /// The emissive tint.
    pub emissive: Option<Vector>,
}

/// One face of a prim.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The diffuse texture and its placement.
    pub texture: Map,
    /// The colour.
    pub color: Vector,
    /// The alpha.
    pub alpha: f32,
    /// The bump map (`PRIM_BUMP_*`).
    pub bump: i32,
    /// The shininess (`PRIM_SHINY_*`).
    pub shiny: i32,
    /// Whether it is lit by nothing (full bright).
    pub fullbright: bool,
    /// Its glow.
    pub glow: f32,
    /// Its texture mapping (`PRIM_TEXGEN_*`).
    pub texgen: i32,
    /// Its normal map.
    pub normal: Map,
    /// Its specular map.
    pub specular: Specular,
    /// Its alpha mode (`PRIM_ALPHA_MODE_*`).
    pub alpha_mode: i32,
    /// Its alpha mode's mask cutoff, 0 to 255.
    pub mask_cutoff: i32,
    /// Its glTF render material's key, `NULL_KEY` for none.
    pub render_material: String,
    /// Its glTF material overrides.
    pub gltf: GltfOverrides,
}

impl Default for Face {
    /// A new prim's face: plywood, white, opaque, nothing else.
    fn default() -> Self {
        Self {
            texture: Map {
                texture: PLYWOOD.to_owned(),
                repeats: ONCE,
                offsets: ZERO,
                rotation: 0.0,
            },
            color: WHITE,
            alpha: 1.0,
            bump: 0,
            shiny: 0,
            fullbright: false,
            glow: 0.0,
            texgen: 0,
            normal: Map::none(),
            specular: Specular::default(),
            alpha_mode: 1,
            mask_cutoff: 0,
            render_material: NULL.to_owned(),
            gltf: GltfOverrides::default(),
        }
    }
}

/// A prim's hover text (`llSetText`, `PRIM_TEXT`).
#[derive(Debug, Clone, PartialEq)]
pub struct HoverText {
    /// The text.
    pub text: String,
    /// Its colour.
    pub color: Vector,
    /// Its alpha.
    pub alpha: f32,
}

/// A prim's point light (`PRIM_POINT_LIGHT`).
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    /// Whether it shines.
    pub on: bool,
    /// Its colour.
    pub color: Vector,
    /// Its intensity, 0 to 1.
    pub intensity: f32,
    /// Its radius, in metres.
    pub radius: f32,
    /// Its falloff.
    pub falloff: f32,
}

/// What OpenSim's `osSetProjectionParams` makes of a prim's light: the image
/// it projects.
#[derive(Debug, Clone, PartialEq)]
pub struct Projector {
    /// Whether the light projects its texture.
    pub on: bool,
    /// The texture projected.
    pub texture: String,
    /// The field of view, in radians.
    pub fov: f32,
    /// The focus.
    pub focus: f32,
    /// The ambience.
    pub ambience: f32,
}

/// A prim's flexible path (`PRIM_FLEXIBLE`).
#[derive(Debug, Clone, PartialEq)]
pub struct Flexible {
    /// Whether the path is flexible.
    pub on: bool,
    /// The softness, 0 to 3.
    pub softness: i32,
    /// The gravity.
    pub gravity: f32,
    /// The friction.
    pub friction: f32,
    /// The wind sensitivity.
    pub wind: f32,
    /// The tension.
    pub tension: f32,
    /// A constant force.
    pub force: Vector,
}

/// A spin the viewer turns a prim with (`llTargetOmega`, `PRIM_OMEGA`): the
/// region never learns the angle.
#[derive(Debug, Clone, PartialEq)]
pub struct Omega {
    /// The axis.
    pub axis: Vector,
    /// The rate, in radians a second.
    pub spin: f32,
    /// The gain.
    pub gain: f32,
}

/// A prim's reflection probe (`PRIM_REFLECTION_PROBE`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionProbe {
    /// Whether the prim is a probe.
    pub on: bool,
    /// The ambiance.
    pub ambiance: f32,
    /// The near clip distance.
    pub clip: f32,
    /// `PRIM_REFLECTION_PROBE_*`.
    pub flags: i32,
}

/// A prim's texture animation (`llSetTextureAnim`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureAnim {
    /// `ANIM_ON` and the rest.
    pub mode: i32,
    /// The face, or `ALL_SIDES`.
    pub face: i32,
    /// Frames across.
    pub size_x: i32,
    /// Frames up.
    pub size_y: i32,
    /// The first frame.
    pub start: f32,
    /// How many frames.
    pub length: f32,
    /// Frames a second.
    pub rate: f32,
}

/// One value of a particle system's rule list.
#[derive(Debug, Clone, PartialEq)]
pub enum ParticleValue {
    /// An integer (a mask, a pattern, a blend function).
    Integer(i32),
    /// A float (an age, a radius, an angle, a rate, a scale).
    Float(f32),
    /// A vector (a colour, a scale, an acceleration, an omega).
    Vector(Vector),
    /// A key (the texture, the target).
    Key(String),
}

/// A prim's particle system (`llParticleSystem`): its rules by code, the
/// last value given for a code winning. The host packs them into what a
/// viewer reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleSystem {
    /// `(PSYS_*, value)` in the order given.
    pub rules: Vec<(i32, ParticleValue)>,
}

/// The sound a prim plays on (`llLoopSound` and the rest of the attached
/// sound), and how.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachedSound {
    /// The sound's asset key, `NULL_KEY` for none.
    pub sound: String,
    /// Its volume, 0 to 1.
    pub volume: f32,
    /// Whether it loops.
    pub looped: bool,
    /// Whether it is its object's sync master.
    pub master: bool,
    /// Whether it waits on a master.
    pub slave: bool,
}

/// What a prim does with sounds: what it plays on, its radius, whether a new
/// sound waits for the one playing.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundSettings {
    /// The sound it plays on, if it does.
    pub playing: Option<AttachedSound>,
    /// How far its sounds carry, in metres; 0 for the default.
    pub radius: f32,
    /// Whether a sound waits for the one playing to end.
    pub queueing: bool,
    /// The collision sound and its volume (`llCollisionSound`).
    pub collision: Option<(String, f32)>,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            playing: None,
            radius: 0.0,
            queueing: false,
            collision: None,
        }
    }
}

/// A face's shared media (`llSetPrimMediaParams`), by `PRIM_MEDIA_*` code.
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per `PRIM_MEDIA_*` rule, each set and read on its own; a bit set would \
              hide which rule each answer is"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// `PRIM_MEDIA_ALT_IMAGE_ENABLE`.
    pub alt_image_enable: bool,
    /// `PRIM_MEDIA_CONTROLS`.
    pub controls: i32,
    /// `PRIM_MEDIA_CURRENT_URL`.
    pub current_url: String,
    /// `PRIM_MEDIA_HOME_URL`.
    pub home_url: String,
    /// `PRIM_MEDIA_AUTO_LOOP`.
    pub auto_loop: bool,
    /// `PRIM_MEDIA_AUTO_PLAY`.
    pub auto_play: bool,
    /// `PRIM_MEDIA_AUTO_SCALE`.
    pub auto_scale: bool,
    /// `PRIM_MEDIA_AUTO_ZOOM`.
    pub auto_zoom: bool,
    /// `PRIM_MEDIA_FIRST_CLICK_INTERACT`.
    pub first_click_interact: bool,
    /// `PRIM_MEDIA_WIDTH_PIXELS`.
    pub width: i32,
    /// `PRIM_MEDIA_HEIGHT_PIXELS`.
    pub height: i32,
    /// `PRIM_MEDIA_WHITELIST_ENABLE`.
    pub whitelist_enable: bool,
    /// `PRIM_MEDIA_WHITELIST`, comma-separated.
    pub whitelist: String,
    /// `PRIM_MEDIA_PERMS_INTERACT`.
    pub perms_interact: i32,
    /// `PRIM_MEDIA_PERMS_CONTROL`.
    pub perms_control: i32,
}

impl Default for Media {
    /// What a face's media is before a rule sets a value.
    fn default() -> Self {
        Self {
            alt_image_enable: false,
            controls: 0,
            current_url: String::new(),
            home_url: String::new(),
            auto_loop: false,
            auto_play: false,
            auto_scale: false,
            auto_zoom: false,
            first_click_interact: false,
            width: 0,
            height: 0,
            whitelist_enable: false,
            whitelist: String::new(),
            perms_interact: 7,
            perms_control: 7,
        }
    }
}

/// A prim as the library reads and writes it.
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per `PRIM_*` flag rule (physics, phantom, temporary, unsit, scripted \
              sit only, …), each set and read on its own; a bit set would hide which rule each \
              answer is"
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Prim {
    /// Its key; read only.
    pub key: String,
    /// Its creator's key; read only.
    pub creator: String,
    /// Its name.
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its region position; read only — a move writes
    /// [`Self::local_position`]. A worn prim's is its wearer's.
    pub position: Vector,
    /// Its position from its parent: an unworn root's region position, a
    /// worn root's offset on its attachment point, a child's offset from the
    /// root. The library has capped a move already; the host keeps an unworn
    /// root out of the ground and in the region.
    pub local_position: Vector,
    /// Its rotation in the region; read only — a turn writes
    /// [`Self::local_rotation`].
    pub rotation: Rotation,
    /// Its rotation from its parent: a root's rotation in the region (a worn
    /// root's on its point), a child's from the root.
    pub local_rotation: Rotation,
    /// Its linkset root's rotation in the region; read only.
    pub root_rotation: Rotation,
    /// Its size, in metres.
    pub scale: Vector,
    /// Its shape.
    pub shape: Shape,
    /// Its faces, in face order: as many as its shape has.
    pub faces: Vec<Face>,
    /// Its hover text.
    pub text: HoverText,
    /// Its light.
    pub light: Light,
    /// The image its light projects (OpenSim's projector).
    pub projector: Projector,
    /// Its flexible path.
    pub flexible: Flexible,
    /// Its material (`PRIM_MATERIAL_*`).
    pub material: i32,
    /// Whether it is phantom.
    pub phantom: bool,
    /// Whether it is physical.
    pub physics: bool,
    /// Whether it is temporary.
    pub temp_on_rez: bool,
    /// Its `STATUS_*` flags but physics and phantom, which are
    /// [`Self::physics`] and [`Self::phantom`]: a fresh prim's are the three
    /// rotation flags.
    pub status: i32,
    /// Whether `llVolumeDetect` is on: the prim reads as phantom, and a
    /// `STATUS_PHANTOM` clear leaves it so (`physics/status`).
    pub volume_detect: bool,
    /// Its physics shape (`PRIM_PHYSICS_SHAPE_*`).
    pub physics_shape: i32,
    /// Its spin.
    pub omega: Omega,
    /// Its click action (`CLICK_ACTION_*`).
    pub click_action: i32,
    /// Whether a sitter may stand up by itself (`PRIM_ALLOW_UNSIT`).
    pub allow_unsit: bool,
    /// Whether only a script may seat an avatar (`PRIM_SCRIPTED_SIT_ONLY`).
    pub scripted_sit_only: bool,
    /// The sit flags that are not read from elsewhere: `SIT_FLAG_NO_COLLIDE`
    /// and `SIT_FLAG_NO_DAMAGE`.
    pub sit_flags: i32,
    /// Its reflection probe.
    pub reflection_probe: ReflectionProbe,
    /// The damage it deals and its kind (`PRIM_DAMAGE`).
    pub damage: (f32, i32),
    /// Its health (`PRIM_HEALTH`).
    pub health: f32,
    /// Its texture animation.
    pub texture_anim: Option<TextureAnim>,
    /// Its particle system.
    pub particles: Option<ParticleSystem>,
    /// What it does with sounds.
    pub sound: SoundSettings,
    /// Each face's shared media, [`None`] for a face with none; as many as
    /// it has faces.
    pub media: Vec<Option<Media>>,
    /// Whether it has had media on a face: OpenSim keeps no media list
    /// before the first set, and a clear finding none raises no `changed`
    /// there (`events/timer-reset`); Second Life raises one all the same.
    pub has_media_list: bool,
}

/// The three rotation flags, `STATUS_ROTATE_X | STATUS_ROTATE_Y |
/// STATUS_ROTATE_Z`: a fresh prim's status on every grid.
pub const STATUS_ROTATE_XYZ: i32 = 2 | 4 | 8;

impl Prim {
    /// A new half-metre plywood box keyed `key`, at the origin, created by
    /// `creator` — what a viewer's rez makes.
    #[must_use]
    pub fn new_box(key: &str, creator: &str) -> Self {
        Self {
            key: key.to_owned(),
            creator: creator.to_owned(),
            name: "Object".to_owned(),
            description: String::new(),
            position: ZERO,
            local_position: ZERO,
            rotation: IDENTITY,
            local_rotation: IDENTITY,
            root_rotation: IDENTITY,
            scale: Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
            shape: Shape::default(),
            faces: vec![Face::default(); 6],
            text: HoverText {
                text: String::new(),
                color: ZERO,
                alpha: 1.0,
            },
            light: Light {
                on: false,
                color: ZERO,
                intensity: 1.0,
                radius: 0.0,
                falloff: 0.0,
            },
            projector: Projector {
                on: false,
                texture: NULL.to_owned(),
                fov: 0.0,
                focus: 0.0,
                ambience: 0.0,
            },
            flexible: Flexible {
                on: false,
                softness: 0,
                gravity: 0.0,
                friction: 0.0,
                wind: 0.0,
                tension: 0.0,
                force: ZERO,
            },
            material: 3,
            phantom: false,
            physics: false,
            temp_on_rez: false,
            status: STATUS_ROTATE_XYZ,
            volume_detect: false,
            physics_shape: 0,
            omega: Omega {
                axis: ZERO,
                spin: 0.0,
                gain: 0.0,
            },
            click_action: 0,
            allow_unsit: true,
            scripted_sit_only: false,
            sit_flags: 0,
            reflection_probe: ReflectionProbe {
                on: false,
                ambiance: 0.0,
                clip: 0.0,
                flags: 0,
            },
            damage: (0.0, 0),
            health: 0.0,
            texture_anim: None,
            particles: None,
            sound: SoundSettings::default(),
            media: vec![None; 6],
            has_media_list: false,
        }
    }
}

/// A sound a script plays once or preloads, which no later call changes —
/// what a host sends the viewers in earshot and keeps nothing of.
#[derive(Debug, Clone, PartialEq)]
pub enum SoundCue {
    /// `llPlaySound`: play `sound` once from the prim, at `volume`.
    Play {
        /// The sound's asset key.
        sound: String,
        /// Its volume, 0 to 1.
        volume: f32,
        /// Whether it waits for its object's sync master
        /// (`llPlaySoundSlave`).
        slave: bool,
    },
    /// `llTriggerSound`: play `sound` once where the prim is, not following
    /// it, at `volume`; `bounds` limits who hears it to a box of the region
    /// (`llTriggerSoundLimited`, its corners).
    Trigger {
        /// The sound's asset key.
        sound: String,
        /// Its volume, 0 to 1.
        volume: f32,
        /// The box it is heard in, its lower and upper corners.
        bounds: Option<(Vector, Vector)>,
    },
    /// `llPreloadSound`: have the viewers in earshot fetch `sound`.
    Preload {
        /// The sound's asset key.
        sound: String,
    },
}

/// What a script reads of any object or avatar in the region by its key
/// (`llGetObjectDetails`), as the host knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectDetails {
    /// Whether it is an avatar rather than a prim.
    pub avatar: bool,
    /// Its name: a prim's own, an avatar's legacy name.
    pub name: String,
    /// A prim's description; an avatar's is empty.
    pub description: String,
    /// Its region position.
    pub position: Vector,
    /// Its rotation in the region.
    pub rotation: Rotation,
    /// Its velocity.
    pub velocity: Vector,
    /// Its owner: an avatar owns itself.
    pub owner: String,
    /// Its group, `NULL_KEY` for none.
    pub group: String,
    /// Its creator: an avatar's is `NULL_KEY`.
    pub creator: String,
    /// Its last owner.
    pub last_owner: String,
    /// The key of its linkset's root: an avatar's own, a seated avatar's the
    /// seat's root.
    pub root: String,
    /// The attachment point its object is worn on, 0 for none.
    pub attached_point: i32,
    /// The scripts in its object and those running (an avatar: in what it
    /// wears).
    pub scripts: (i32, i32),
    /// The memory its object's scripts may use, in bytes.
    pub script_memory: i32,
    /// The prims of its object (an avatar: the prims it wears).
    pub prim_count: i32,
    /// The items in its object's prims.
    pub inventory_count: i32,
    /// Its click action (`CLICK_ACTION_*`).
    pub click_action: i32,
    /// Its spin.
    pub omega: Vector,
    /// Its size, in metres.
    pub scale: Vector,
    /// Its material (`PRIM_MATERIAL_*`).
    pub material: i32,
    /// Whether it is physical, phantom, temporary.
    pub flags: (bool, bool, bool),
    /// Its hover text, colour and alpha.
    pub text: HoverText,
    /// The land impact, streaming, physics and server costs.
    pub costs: (f32, f32, f32, f32),
    /// Whether its object is selected by a viewer, and how many sit on it.
    pub selected: (i32, i32),
    /// When its object was rezzed, in the region's calendar time as
    /// microseconds since the UNIX epoch (`OBJECT_REZ_TIME`); [`None`] for an
    /// avatar.
    pub rezzed: Option<i64>,
    /// The key of what rezzed it, `NULL_KEY` for an avatar's rez.
    pub rezzer: String,
    /// Its permission masks: base, owner, group, everyone, next owner.
    pub masks: [i32; 5],
    /// An avatar's hover height, 0 for a prim.
    pub hover_height: f32,
    /// An avatar's body shape type (0 female, 1 male), −1 for a prim.
    pub body_shape: f32,
    /// An avatar's animated attachments.
    pub animated: i32,
}
