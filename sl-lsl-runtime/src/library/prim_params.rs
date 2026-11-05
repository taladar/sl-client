//! Library tranche: reading and writing the prim (`server-lsl-lib-prim-state`)
//! — the `PRIM_*` rule lists of `llSetPrimitiveParams`,
//! `llGetPrimitiveParams` and their link and OpenSim variants, implemented
//! once as a codec over the host's [`Prim`].
//!
//! A rule list is a small language of its own: a code, then the values that
//! code takes, repeated, with `PRIM_LINK_TARGET` moving on to other prims.
//! [`args`] reads the values as each grid reads them, [`shape`] converts
//! `PRIM_TYPE` to and from the prim's volume parameters, `set` applies a
//! list, `get` answers one. The grids' rules are those the `prim/*` probes
//! measured (book chapter `gridspec/lsl.md`, § Reading and writing the
//! prim) and, where nothing measured them, `LSL_Api.cs`'s.
//!
//! **What a grid stores is what it reads back.** OpenSim stores a colour, an
//! alpha and a glow as a byte and a texture's offset and rotation as the
//! 16-bit numbers they travel in; Second Life keeps a script's float. The
//! roundings here ([`opensim_colour`] and its siblings) are applied before a
//! prim goes back to the host, so the host stores the value the grid would
//! read.

pub(crate) mod args;
pub(crate) mod get;
pub(crate) mod set;
pub(crate) mod shape;

pub(crate) use get::answer;
pub(crate) use get::{get_params, link_get_params};
pub(crate) use set::{Changes, apply_rules, set_params};

use crate::vm::ScriptCtx;
use crate::vm::comms::Links;
use crate::vm::prim::{Face, Prim};

pub use shape::faces_of;

/// `PRIM_MATERIAL`.
pub(crate) const MATERIAL: i32 = 2;
/// `PRIM_PHYSICS`.
pub(crate) const PHYSICS: i32 = 3;
/// `PRIM_TEMP_ON_REZ`.
pub(crate) const TEMP_ON_REZ: i32 = 4;
/// `PRIM_PHANTOM`.
pub(crate) const PHANTOM: i32 = 5;
/// `PRIM_POSITION`.
pub(crate) const POSITION: i32 = 6;
/// `PRIM_SIZE`.
pub(crate) const SIZE: i32 = 7;
/// `PRIM_ROTATION`.
pub(crate) const ROTATION: i32 = 8;
/// `PRIM_TYPE`.
pub(crate) const TYPE: i32 = 9;
/// `PRIM_TEXTURE`.
pub(crate) const TEXTURE: i32 = 17;
/// `PRIM_COLOR`.
pub(crate) const COLOR: i32 = 18;
/// `PRIM_BUMP_SHINY`.
pub(crate) const BUMP_SHINY: i32 = 19;
/// `PRIM_FULLBRIGHT`.
pub(crate) const FULLBRIGHT: i32 = 20;
/// `PRIM_FLEXIBLE`.
pub(crate) const FLEXIBLE: i32 = 21;
/// `PRIM_TEXGEN`.
pub(crate) const TEXGEN: i32 = 22;
/// `PRIM_POINT_LIGHT`.
pub(crate) const POINT_LIGHT: i32 = 23;
/// `PRIM_CAST_SHADOWS`: retired; no grid answers it.
pub(crate) const CAST_SHADOWS: i32 = 24;
/// `PRIM_GLOW`.
pub(crate) const GLOW: i32 = 25;
/// `PRIM_TEXT`.
pub(crate) const TEXT: i32 = 26;
/// `PRIM_NAME`.
pub(crate) const NAME: i32 = 27;
/// `PRIM_DESC`.
pub(crate) const DESC: i32 = 28;
/// `PRIM_ROT_LOCAL`.
pub(crate) const ROT_LOCAL: i32 = 29;
/// `PRIM_PHYSICS_SHAPE_TYPE`.
pub(crate) const PHYSICS_SHAPE_TYPE: i32 = 30;
/// `PRIM_OMEGA`.
pub(crate) const OMEGA: i32 = 32;
/// `PRIM_POS_LOCAL`.
pub(crate) const POS_LOCAL: i32 = 33;
/// `PRIM_LINK_TARGET`.
pub(crate) const LINK_TARGET: i32 = 34;
/// `PRIM_SLICE`.
pub(crate) const SLICE: i32 = 35;
/// `PRIM_SPECULAR`.
pub(crate) const SPECULAR: i32 = 36;
/// `PRIM_NORMAL`.
pub(crate) const NORMAL: i32 = 37;
/// `PRIM_ALPHA_MODE`.
pub(crate) const ALPHA_MODE: i32 = 38;
/// `PRIM_ALLOW_UNSIT`.
pub(crate) const ALLOW_UNSIT: i32 = 39;
/// `PRIM_SCRIPTED_SIT_ONLY`.
pub(crate) const SCRIPTED_SIT_ONLY: i32 = 40;
/// `PRIM_SIT_TARGET`.
pub(crate) const SIT_TARGET: i32 = 41;
/// `PRIM_PROJECTOR`: no grid's rule lists answer it (`prim/params`).
pub(crate) const PROJECTOR: i32 = 42;
/// `PRIM_CLICK_ACTION`.
pub(crate) const CLICK_ACTION: i32 = 43;
/// `PRIM_REFLECTION_PROBE`.
pub(crate) const REFLECTION_PROBE: i32 = 44;
/// `PRIM_GLTF_NORMAL`.
pub(crate) const GLTF_NORMAL: i32 = 45;
/// `PRIM_GLTF_EMISSIVE`.
pub(crate) const GLTF_EMISSIVE: i32 = 46;
/// `PRIM_GLTF_METALLIC_ROUGHNESS`.
pub(crate) const GLTF_METALLIC_ROUGHNESS: i32 = 47;
/// `PRIM_GLTF_BASE_COLOR`.
pub(crate) const GLTF_BASE_COLOR: i32 = 48;
/// `PRIM_RENDER_MATERIAL`.
pub(crate) const RENDER_MATERIAL: i32 = 49;
/// `PRIM_SIT_FLAGS`.
pub(crate) const SIT_FLAGS: i32 = 50;
/// `PRIM_DAMAGE`.
pub(crate) const DAMAGE: i32 = 51;
/// `PRIM_HEALTH`.
pub(crate) const HEALTH: i32 = 52;
/// `PRIM_COLLISION_SOUND`.
pub(crate) const COLLISION_SOUND: i32 = 53;

/// `ALL_SIDES`.
pub(crate) const ALL_SIDES: i32 = -1;
/// `LINK_ROOT`.
pub(crate) const LINK_ROOT: i32 = 1;
/// `LINK_SET`.
pub(crate) const LINK_SET: i32 = -1;
/// `LINK_ALL_OTHERS`.
pub(crate) const LINK_ALL_OTHERS: i32 = -2;
/// `LINK_ALL_CHILDREN`.
pub(crate) const LINK_ALL_CHILDREN: i32 = -3;
/// `LINK_THIS`.
pub(crate) const LINK_THIS: i32 = -4;

/// OpenSim's `CHANGED_POSITION`, raised by a move (`prim/changed`).
pub(crate) const CHANGED_POSITION: i32 = 0x8000;
/// `CHANGED_COLOR`.
pub(crate) const CHANGED_COLOR: i32 = 0x2;
/// `CHANGED_SHAPE`.
pub(crate) const CHANGED_SHAPE: i32 = 0x4;
/// `CHANGED_SCALE`.
pub(crate) const CHANGED_SCALE: i32 = 0x8;
/// `CHANGED_TEXTURE`.
pub(crate) const CHANGED_TEXTURE: i32 = 0x10;
/// `CHANGED_MEDIA`.
pub(crate) const CHANGED_MEDIA: i32 = 0x800;
/// `CHANGED_RENDER_MATERIAL`.
pub(crate) const CHANGED_RENDER_MATERIAL: i32 = 0x1000;

/// How many faces OpenSim's texture entry may index (`MAX_FACES`): a face
/// past a prim's own but under this reads as the entry's default face, one
/// at or past it throws (`prim/params-rules`, `face-100`).
pub(crate) const OPENSIM_MAX_FACES: i32 = 45;

/// The name Second Life writes in brackets for a rule's code in a refusal,
/// and OpenSim after its arrow.
#[must_use]
pub(crate) const fn rule_name(code: i32) -> &'static str {
    match code {
        MATERIAL => "PRIM_MATERIAL",
        PHYSICS => "PRIM_PHYSICS",
        TEMP_ON_REZ => "PRIM_TEMP_ON_REZ",
        PHANTOM => "PRIM_PHANTOM",
        POSITION => "PRIM_POSITION",
        SIZE => "PRIM_SIZE",
        ROTATION => "PRIM_ROTATION",
        TYPE => "PRIM_TYPE",
        TEXTURE => "PRIM_TEXTURE",
        COLOR => "PRIM_COLOR",
        BUMP_SHINY => "PRIM_BUMP_SHINY",
        FULLBRIGHT => "PRIM_FULLBRIGHT",
        FLEXIBLE => "PRIM_FLEXIBLE",
        TEXGEN => "PRIM_TEXGEN",
        POINT_LIGHT => "PRIM_POINT_LIGHT",
        GLOW => "PRIM_GLOW",
        TEXT => "PRIM_TEXT",
        NAME => "PRIM_NAME",
        DESC => "PRIM_DESC",
        ROT_LOCAL => "PRIM_ROT_LOCAL",
        PHYSICS_SHAPE_TYPE => "PRIM_PHYSICS_SHAPE_TYPE",
        OMEGA => "PRIM_OMEGA",
        POS_LOCAL => "PRIM_POS_LOCAL",
        LINK_TARGET => "PRIM_LINK_TARGET",
        SLICE => "PRIM_SLICE",
        SPECULAR => "PRIM_SPECULAR",
        NORMAL => "PRIM_NORMAL",
        ALPHA_MODE => "PRIM_ALPHA_MODE",
        ALLOW_UNSIT => "PRIM_ALLOW_UNSIT",
        SCRIPTED_SIT_ONLY => "PRIM_SCRIPTED_SIT_ONLY",
        SIT_TARGET => "PRIM_SIT_TARGET",
        CLICK_ACTION => "PRIM_CLICK_ACTION",
        REFLECTION_PROBE => "PRIM_REFLECTION_PROBE",
        GLTF_NORMAL => "PRIM_GLTF_NORMAL",
        GLTF_EMISSIVE => "PRIM_GLTF_EMISSIVE",
        GLTF_METALLIC_ROUGHNESS => "PRIM_GLTF_METALLIC_ROUGHNESS",
        GLTF_BASE_COLOR => "PRIM_GLTF_BASE_COLOR",
        RENDER_MATERIAL => "PRIM_RENDER_MATERIAL",
        SIT_FLAGS => "PRIM_SIT_FLAGS",
        DAMAGE => "PRIM_DAMAGE",
        HEALTH => "PRIM_HEALTH",
        COLLISION_SOUND => "PRIM_COLLISION_SOUND",
        _ => "",
    }
}

/// Whether OpenSim's rule lists have the rule `code` at all: the newer
/// rules are `unsupported parameter` there, set or get (`prim/params`).
pub(crate) const fn opensim_has(code: i32) -> bool {
    matches!(
        code,
        MATERIAL
            | PHYSICS
            | TEMP_ON_REZ
            | PHANTOM
            | POSITION
            | SIZE
            | ROTATION
            | TYPE
            | TEXTURE
            | COLOR
            | BUMP_SHINY
            | FULLBRIGHT
            | FLEXIBLE
            | TEXGEN
            | POINT_LIGHT
            | GLOW
            | TEXT
            | NAME
            | DESC
            | ROT_LOCAL
            | PHYSICS_SHAPE_TYPE
            | OMEGA
            | POS_LOCAL
            | LINK_TARGET
            | SLICE
            | SPECULAR
            | NORMAL
            | ALPHA_MODE
            | SIT_TARGET
    )
}

/// Whether Second Life's rule lists have the rule `code`: every rule but
/// the retired `PRIM_CAST_SHADOWS` and `PRIM_PROJECTOR`, which are
/// `unknown parameter` (`prim/params`).
pub(crate) const fn second_life_has(code: i32) -> bool {
    !rule_name(code).is_empty() && !matches!(code, CAST_SHADOWS | PROJECTOR)
}

/// Whether Second Life's setters take the rule `code`: every rule its rule
/// lists have, and `PRIM_PROJECTOR`, which a set takes without a word and a
/// get then calls an `unknown parameter` (`errors/flood-window`'s `P`).
pub(crate) const fn second_life_sets(code: i32) -> bool {
    second_life_has(code) || code == PROJECTOR
}

/// `value` in `[0, 1]`, as both grids clamp a colour component or an alpha.
pub(crate) const fn unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// A unit value as the byte OpenSim stores it, truncated
/// (`(byte)(value * 255)`), read back.
pub(crate) fn byte_truncated(value: f32) -> f32 {
    let byte = crate::num::float_to_int(unit(value) * 255.0);
    crate::num::int_to_float(byte) / 255.0
}

/// A unit value as a rounded byte (`llround(value * 255)`), read back: how
/// Second Life stores a light's colour and intensity, and OpenSim a
/// specular colour.
pub(crate) fn byte_rounded(value: f32) -> f32 {
    let byte = crate::num::float_to_int((unit(value) * 255.0).round());
    crate::num::int_to_float(byte) / 255.0
}

/// A glow as OpenSim stores it: `(byte)(glow * 255)` with the byte wrapping,
/// not clamped (`prim/params`: 2 reads 0.996078, −1 reads 0.003922).
pub(crate) fn opensim_glow(glow: f32) -> f32 {
    let wide = crate::num::float_to_int(glow * 255.0);
    let byte = wide.rem_euclid(256);
    crate::num::int_to_float(byte) / 255.0
}

/// A colour as OpenSim stores it: each component clamped, then a byte.
pub(crate) fn opensim_colour(colour: &sl_types::lsl::Vector) -> sl_types::lsl::Vector {
    sl_types::lsl::Vector {
        x: byte_truncated(colour.x),
        y: byte_truncated(colour.y),
        z: byte_truncated(colour.z),
    }
}

/// A texture offset as OpenSim stores it: within `±1`, then the rounded
/// 16-bit number the texture entry carries (`0.25` reads `0.250008`).
pub(crate) fn opensim_offset(offset: f32) -> f32 {
    let quanta = crate::num::float_to_int((offset.clamp(-1.0, 1.0) * 32_767.0).round());
    crate::num::int_to_float(quanta) / 32_767.0
}

/// A texture rotation as OpenSim stores it: taken modulo 2π, then the
/// truncated 16-bit number the texture entry carries (`7` reads
/// `0.716753`).
pub(crate) fn opensim_rotation(rotation: f32) -> f32 {
    let turn = core::f32::consts::TAU;
    let wrapped = rotation % turn;
    let quanta = crate::num::float_to_int(wrapped / turn * 32_768.0);
    crate::num::int_to_float(quanta) / 32_768.0 * turn
}

/// The faces of `prim` a set of face `face` writes: the one face, or every
/// face for `ALL_SIDES` — and on Second Life for any negative face
/// (`prim/params-rules`: −2 is `ALL_SIDES` there, nothing on OpenSim). A
/// face past the prim's own is none.
pub(crate) fn faces_set(prim: &Prim, face: i32, opensim_flavour: bool) -> Vec<usize> {
    let all = face == ALL_SIDES || (face < 0 && !opensim_flavour);
    if all {
        return (0..prim.faces.len()).collect();
    }
    usize::try_from(face)
        .ok()
        .filter(|index| *index < prim.faces.len())
        .into_iter()
        .collect()
}

/// What a get of face `face` reads, as each grid reads it.
#[derive(Debug)]
pub(crate) enum FacesRead {
    /// The faces read, in face order.
    Faces(Vec<Face>),
    /// OpenSim threw: the face is out of its texture entry's range.
    Thrown(u32),
}

/// The faces of `prim` a get of face `face` reads: the one face, or every
/// face for `ALL_SIDES` (and on Second Life any negative face). Past the
/// prim's faces Second Life reads none; OpenSim reads its entry's default
/// face under [`OPENSIM_MAX_FACES`], and throws at or past it and for a
/// negative face other than `ALL_SIDES` (`prim/params-rules`).
pub(crate) fn faces_get(prim: &Prim, face: i32, opensim_flavour: bool) -> FacesRead {
    if face == ALL_SIDES || (face < 0 && !opensim_flavour) {
        return FacesRead::Faces(prim.faces.clone());
    }
    if opensim_flavour && !(0..OPENSIM_MAX_FACES).contains(&face) {
        return FacesRead::Thrown(face.cast_unsigned());
    }
    let Ok(index) = usize::try_from(face) else {
        return FacesRead::Faces(Vec::new());
    };
    match prim.faces.get(index) {
        Some(found) => FacesRead::Faces(vec![found.clone()]),
        None if opensim_flavour => FacesRead::Faces(vec![Face::default()]),
        None => FacesRead::Faces(Vec::new()),
    }
}

/// The keys of the prims a link argument names for a set
/// (`llSetLinkPrimitiveParamsFast`, `PRIM_LINK_TARGET`): those
/// [`Links::named`] names (`prim/params-rules`, `prim/link`).
pub(crate) fn set_targets(ctx: &mut ScriptCtx<'_>, link: i32) -> Vec<String> {
    let caller = ctx.caller();
    let flavour = ctx.flavour();
    ctx.host()
        .links(caller)
        .named(link, flavour)
        .into_iter()
        .map(|prim| prim.key.clone())
        .collect()
}

/// The keys of the prims a link argument names for a get
/// (`llGetLinkPrimitiveParams`, `PRIM_LINK_TARGET` in a get): on Second Life
/// a single prim alone — a number or `LINK_THIS`, never a constant naming
/// several, which answers nothing even where it would name one; on OpenSim
/// every prim [`Links::named`] names, in link order (`prim/params-rules`,
/// `prim/link`).
pub(crate) fn get_targets(ctx: &mut ScriptCtx<'_>, link: i32) -> Vec<String> {
    if !ctx.flavour().is_opensim() && matches!(link, LINK_SET | LINK_ALL_OTHERS | LINK_ALL_CHILDREN)
    {
        return Vec::new();
    }
    set_targets(ctx, link)
}

/// The key of the caller's own prim.
pub(crate) fn own_key(ctx: &mut ScriptCtx<'_>) -> Option<String> {
    let caller = ctx.caller();
    ctx.host()
        .links(caller)
        .this_prim()
        .map(|prim| prim.key.clone())
}

/// Whether the prim keyed `key` is a child of the linkset `links`
/// describes: a prim of a set of several that is not its root.
pub(crate) fn is_child(links: &Links, key: &str) -> bool {
    links.prims.len() > 1 && links.prims.first().is_some_and(|root| root.key != key)
}
