//! Applying a `PRIM_*` rule list (`llSetPrimitiveParams` and its family).
//!
//! The list is read rule by rule; `PRIM_LINK_TARGET` ends a run of rules and
//! names the prims the next run is for. Each run is applied to each of its
//! prims in link order — what OpenSim's `SetPrimParams` does, part after
//! part — and the prim goes back to the host at the run's end with the
//! `changed` its rules raised. A rule that is refused stops the whole list,
//! on both grids; a rule with too few values does nothing and stops it
//! silently (`prim/params-rules`).

use sl_types::lsl::{Rotation, Vector};

use super::args::{Reader, Stop};
use super::shape::{self, Asked};
use super::{
    ALLOW_UNSIT, ALPHA_MODE, BUMP_SHINY, CHANGED_COLOR, CHANGED_POSITION, CHANGED_RENDER_MATERIAL,
    CHANGED_SCALE, CHANGED_SHAPE, CHANGED_TEXTURE, CLICK_ACTION, COLLISION_SOUND, COLOR, DAMAGE,
    DESC, FLEXIBLE, FULLBRIGHT, GLOW, GLTF_BASE_COLOR, GLTF_EMISSIVE, GLTF_METALLIC_ROUGHNESS,
    GLTF_NORMAL, HEALTH, LINK_TARGET, MATERIAL, NAME, NORMAL, OMEGA, PHANTOM, PHYSICS,
    PHYSICS_SHAPE_TYPE, POINT_LIGHT, POS_LOCAL, POSITION, PROJECTOR, REFLECTION_PROBE,
    RENDER_MATERIAL, ROT_LOCAL, ROTATION, SCRIPTED_SIT_ONLY, SIT_FLAGS, SIT_TARGET, SIZE, SLICE,
    SPECULAR, TEMP_ON_REZ, TEXGEN, TEXT, TEXTURE, TYPE, byte_rounded, byte_truncated, faces_set,
    is_child, opensim_colour, opensim_glow, opensim_has, opensim_offset, opensim_rotation,
    rule_name, second_life_sets, unit,
};
use crate::library::dotnet;
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::prim::{Face, Map, Media, Prim, path};
use crate::vm::seating::SitTarget;

/// `INVENTORY_TEXTURE`.
const INVENTORY_TEXTURE: i32 = 0;
/// `INVENTORY_SOUND`.
pub(crate) const INVENTORY_SOUND: i32 = 1;

/// `SIT_FLAG_ALLOW_UNSIT`.
const SIT_FLAG_ALLOW_UNSIT: i32 = 0x02;
/// `SIT_FLAG_SCRIPTED_ONLY`.
const SIT_FLAG_SCRIPTED_ONLY: i32 = 0x04;
/// The sit flags kept as they are set: `SIT_FLAG_NO_COLLIDE` and
/// `SIT_FLAG_NO_DAMAGE`.
const SIT_FLAGS_KEPT: i32 = 0x30;

/// How far one move of a root goes, in metres (`prim/placement`).
const REACH_M: f32 = 10.0;

/// The longest hover text, in bytes (`prim/faces`).
pub(crate) const TEXT_BYTES: usize = 254;
/// The longest name Second Life keeps, in bytes (`prim/identity`).
const SECOND_LIFE_NAME_BYTES: usize = 63;
/// The longest description, in bytes, on both grids (`prim/identity`).
const DESCRIPTION_BYTES: usize = 127;

/// The `changed` one write raises in the prim: on Second Life one event
/// carrying every kind of change, on OpenSim one event per kind
/// (`prim/changed`: colour and size in one call).
#[derive(Debug, Default)]
pub(crate) struct Changes {
    /// The kinds raised, in the order raised, each once.
    bits: Vec<i32>,
}

impl Changes {
    /// Raise `bit`.
    pub(crate) fn raise(&mut self, bit: i32) {
        if !self.bits.contains(&bit) {
            self.bits.push(bit);
        }
    }

    /// The events to raise, one per element.
    pub(crate) fn events(&self, opensim_flavour: bool) -> Vec<i32> {
        if opensim_flavour {
            self.bits.clone()
        } else {
            let joined = self.bits.iter().fold(0, |all, bit| all | bit);
            if joined == 0 {
                Vec::new()
            } else {
                vec![joined]
            }
        }
    }
}

/// A prim a run of rules is applied to.
#[derive(Debug, Clone)]
struct Target {
    /// Its key.
    key: String,
    /// Its link number.
    link: i32,
    /// Whether it is a child of a linkset.
    child: bool,
}

/// How a run of rules ended.
enum Ended {
    /// At the end of the list.
    End,
    /// At a `PRIM_LINK_TARGET` naming the next run's prims.
    Link(i32),
    /// At a refusal or a rule short of values: nothing more runs.
    Stop,
}

/// The targets the keys `keys` name, from the caller's linkset (a key not
/// in it is a prim of another object, OpenSim's functions' way, its own
/// root).
fn targets(ctx: &mut ScriptCtx<'_>, keys: &[String]) -> Vec<Target> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    keys.iter()
        .map(|key| Target {
            key: key.clone(),
            link: links
                .prims
                .iter()
                .find(|prim| &prim.key == key)
                .map_or(0, |prim| prim.link),
            child: is_child(&links, key),
        })
        .collect()
}

/// `llSetPrimitiveParams` and its family: apply `rules` to the prims the
/// link argument `link` names (`LINK_THIS` for the caller's own), the
/// refusals worded for `function`.
pub(crate) fn set_params(
    ctx: &mut ScriptCtx<'_>,
    function: &'static str,
    link: i32,
    rules: &[Element],
) {
    let keys = super::set_targets(ctx, link);
    apply_rules(ctx, function, &keys, rules);
}

/// Applies `rules` to the prims keyed `keys`, then to the prims each
/// `PRIM_LINK_TARGET` names. A run with no prim to apply to ends the list
/// (`prim/params-rules`: a target of link 2 on a single prim, then link 1,
/// sets nothing on either grid).
pub(crate) fn apply_rules(
    ctx: &mut ScriptCtx<'_>,
    function: &'static str,
    keys: &[String],
    rules: &[Element],
) {
    let opensim_flavour = ctx.flavour().is_opensim();
    let mut reader = Reader::new(ctx, function, rules);
    let mut prims = targets(ctx, keys);
    loop {
        if prims.is_empty() {
            return;
        }
        let (start, number) = (reader.at, reader.number);
        let mut ended = Ended::End;
        for target in &prims {
            reader.at = start;
            reader.number = number;
            let caller = ctx.caller();
            let Some(mut prim) = ctx.host().prim(caller, &target.key) else {
                continue;
            };
            let mut changes = Changes::default();
            ended = run(ctx, &mut reader, target, &mut prim, &mut changes);
            let events = changes.events(opensim_flavour);
            ctx.host().set_prim(caller, &prim, &events);
        }
        match ended {
            Ended::End | Ended::Stop => return,
            Ended::Link(link) => {
                let keys = super::set_targets(ctx, link);
                prims = targets(ctx, &keys);
            }
        }
    }
}

/// Applies rules from where `reader` is to `prim` until the list ends, a
/// `PRIM_LINK_TARGET` names other prims, or a rule stops the list.
fn run(
    ctx: &mut ScriptCtx<'_>,
    reader: &mut Reader<'_>,
    target: &Target,
    prim: &mut Prim,
    changes: &mut Changes,
) -> Ended {
    let opensim_flavour = ctx.flavour().is_opensim();
    while !reader.done() {
        let Some(code) = reader.code() else {
            reader.begin("");
            unknown(ctx, reader);
            return Ended::Stop;
        };
        reader.begin(rule_name(code));
        let known = if opensim_flavour {
            opensim_has(code)
        } else {
            second_life_sets(code)
        };
        if !known {
            unknown(ctx, reader);
            return Ended::Stop;
        }
        if code == LINK_TARGET {
            return match reader.integer(ctx, None) {
                Ok(link) => Ended::Link(link),
                Err(_) => Ended::Stop,
            };
        }
        if rule(ctx, reader, code, target, prim, changes).is_err() {
            return Ended::Stop;
        }
    }
    Ended::End
}

/// Says that the rule being read is no rule the grid has: Second Life's
/// `unknown rule.`, OpenSim's `unsupported parameter`.
fn unknown(ctx: &mut ScriptCtx<'_>, reader: &Reader<'_>) {
    if ctx.flavour().is_opensim() {
        ctx.refuse_to_owner(format!(
            "{}: Error running rule #{}: arg #0 - unsupported parameter",
            reader.function, reader.number,
        ));
        ctx.sleep(1.0);
    } else {
        ctx.refuse_to_owner(format!(
            "{} error running rule #{}: unknown rule.",
            reader.function, reader.number,
        ));
    }
}

/// Second Life's `bounds error; <value> is not in (<min>, <max>).` for an
/// integer rule value.
///
/// # Errors
///
/// [`Stop::Refused`], having said so, when `value` is out of range.
fn in_bounds(
    ctx: &mut ScriptCtx<'_>,
    reader: &Reader<'_>,
    value: i32,
    min: i32,
    max: i32,
) -> Result<i32, Stop> {
    if (min..=max).contains(&value) {
        return Ok(value);
    }
    reader.error(
        ctx,
        &format!("bounds error; {value} is not in ({min}, {max})."),
    );
    Err(Stop::Refused)
}

/// [`in_bounds`] for a float value, written as Second Life writes it (`2`,
/// `-1`).
///
/// # Errors
///
/// [`Stop::Refused`], having said so, when `value` is out of range.
fn in_unit(ctx: &mut ScriptCtx<'_>, reader: &Reader<'_>, value: f32) -> Result<f32, Stop> {
    if (0.0..=1.0).contains(&value) {
        return Ok(value);
    }
    reader.error(ctx, &format!("bounds error; {value} is not in (0, 1)."));
    Err(Stop::Refused)
}

/// The asset a texture argument names: a key as it is, else the texture of
/// that name in the caller's prim, else [`None`].
pub(crate) fn texture_named(ctx: &mut ScriptCtx<'_>, given: &str) -> Option<String> {
    asset_named(ctx, given, INVENTORY_TEXTURE)
}

/// The asset of type `asset_type` a name-or-key argument names.
pub(crate) fn asset_named(ctx: &mut ScriptCtx<'_>, given: &str, asset_type: i32) -> Option<String> {
    if dotnet::is_uuid(given) {
        return Some(given.to_ascii_lowercase());
    }
    let caller = ctx.caller();
    ctx.host().inventory_key(caller, given, asset_type)
}

/// `text` cut to whole characters within `bytes` bytes.
pub(crate) fn cut_to_bytes(text: &str, bytes: usize) -> String {
    let mut end = 0;
    for (at, character) in text.char_indices() {
        let next = at.saturating_add(character.len_utf8());
        if next > bytes {
            break;
        }
        end = next;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// A name or description as Second Life keeps it: cut within `bytes`, and a
/// pipe, a newline and every byte of a character outside ASCII a `?`
/// (`prim/identity`).
fn second_life_label(text: &str, bytes: usize) -> String {
    cut_to_bytes(text, bytes)
        .chars()
        .flat_map(|character| {
            let count = if character == '|' || character == '\n' || !character.is_ascii() {
                character.len_utf8()
            } else {
                0
            };
            let kept = (count == 0).then_some(character);
            kept.into_iter().chain(core::iter::repeat_n('?', count))
        })
        .collect()
}

/// The name a setter gives a prim, as the grid keeps it: cut to 63 bytes and
/// cleaned on Second Life, kept whole on OpenSim.
pub(crate) fn kept_name(text: &str, opensim_flavour: bool) -> String {
    if opensim_flavour {
        text.to_owned()
    } else {
        second_life_label(text, SECOND_LIFE_NAME_BYTES)
    }
}

/// The description a setter gives a prim: cut to 127 bytes on both grids,
/// and cleaned on Second Life.
pub(crate) fn kept_description(text: &str, opensim_flavour: bool) -> String {
    if opensim_flavour {
        cut_to_bytes(text, DESCRIPTION_BYTES)
    } else {
        second_life_label(text, DESCRIPTION_BYTES)
    }
}

/// A text alpha as OpenSim reads it back: it stores `255 − alpha·255` and
/// reads that byte as the alpha (`prim/params`: 0.5 reads 0.501961, 1 reads
/// 0; `prim/faces`: `llSetText` with 2 reads 0, with −1 reads 1).
pub(crate) fn opensim_text_alpha(alpha: f32) -> f32 {
    let stored = 255_i32.saturating_sub(crate::num::float_to_int(alpha * 255.0));
    crate::num::int_to_float(stored.clamp(0, 255)) / 255.0
}

/// A rotation normalised, the zero rotation the identity (`prim/placement`).
pub(crate) fn normalised(rotation: &Rotation) -> Rotation {
    let length = rotation
        .s
        .mul_add(
            rotation.s,
            rotation.z.mul_add(
                rotation.z,
                rotation.y.mul_add(rotation.y, rotation.x * rotation.x),
            ),
        )
        .sqrt();
    if length <= f32::EPSILON || !length.is_finite() {
        return Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
    }
    Rotation {
        x: rotation.x / length,
        y: rotation.y / length,
        z: rotation.z / length,
        s: rotation.s / length,
    }
}

/// `to`, or the point [`REACH_M`] from `from` towards it where it is
/// farther.
pub(crate) fn within_reach(from: &Vector, to: &Vector) -> Vector {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    let distance = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
    if distance <= REACH_M {
        return to.clone();
    }
    let share = REACH_M / distance;
    Vector {
        x: dx.mul_add(share, from.x),
        y: dy.mul_add(share, from.y),
        z: dz.mul_add(share, from.z),
    }
}

/// A scale as the grid keeps it: each axis within 0.01 and 64 m on Second
/// Life, within 0.001 and 256 m on OpenSim — a zero or negative axis the
/// least (`prim/placement`).
pub(crate) fn kept_scale(scale: &Vector, opensim_flavour: bool) -> Vector {
    let (min, max) = if opensim_flavour {
        (0.001, 256.0)
    } else {
        (0.01, 64.0)
    };
    let axis = |value: f32| {
        if value <= 0.0 || value.is_nan() {
            min
        } else {
            value.clamp(min, max)
        }
    };
    Vector {
        x: axis(scale.x),
        y: axis(scale.y),
        z: axis(scale.z),
    }
}

/// Writes `scale` into `prim`, raising `CHANGED_SCALE` as the grid does:
/// on Second Life only for another scale, on OpenSim for every set
/// (`prim/changed`).
pub(crate) fn set_scale(
    prim: &mut Prim,
    scale: &Vector,
    opensim_flavour: bool,
    changes: &mut Changes,
) {
    let kept = kept_scale(scale, opensim_flavour);
    if opensim_flavour || kept != prim.scale {
        changes.raise(CHANGED_SCALE);
    }
    prim.scale = kept;
}

/// Writes `shape` into `prim`, its faces and media grown or cut to the
/// count the shape has (a new face is a copy of the first).
pub(crate) fn set_shape(prim: &mut Prim, shape: crate::vm::prim::Shape) {
    let count = usize::try_from(shape::faces_of(&shape)).unwrap_or(1).max(1);
    let first = prim.faces.first().cloned().unwrap_or_default();
    prim.faces.resize(count, first);
    prim.media.resize(count, None);
    prim.shape = shape;
}

/// Writes a face's colour as the grid stores it, raising `CHANGED_COLOR`
/// (on Second Life for every set; on OpenSim for another colour).
pub(crate) fn set_face_colour(
    face: &mut Face,
    colour: &Vector,
    opensim_flavour: bool,
    clamp: bool,
    changes: &mut Changes,
) {
    let kept = if opensim_flavour {
        opensim_colour(colour)
    } else if clamp {
        Vector {
            x: unit(colour.x),
            y: unit(colour.y),
            z: unit(colour.z),
        }
    } else {
        colour.clone()
    };
    if !opensim_flavour || kept != face.color {
        changes.raise(CHANGED_COLOR);
    }
    face.color = kept;
}

/// Writes a face's alpha as the grid stores it, raising `CHANGED_COLOR` on
/// both grids, the same alpha again included (`prim/changed`).
pub(crate) fn set_face_alpha(
    face: &mut Face,
    alpha: f32,
    opensim_flavour: bool,
    changes: &mut Changes,
) {
    face.alpha = if opensim_flavour {
        byte_truncated(alpha)
    } else {
        unit(alpha)
    };
    changes.raise(CHANGED_COLOR);
}

/// Raises OpenSim's `CHANGED_TEXTURE` when a face's look other than its
/// colour changed; Second Life raises nothing for a script's texture
/// setters (`prim/changed`).
pub(crate) fn textured(before: &Face, after: &Face, opensim_flavour: bool, changes: &mut Changes) {
    if opensim_flavour && before != after {
        changes.raise(CHANGED_TEXTURE);
    }
}

/// A map's placement as the grid stores it: OpenSim's offsets and rotation
/// in their 16-bit numbers, Second Life's as given.
pub(crate) fn placed_map(
    texture: String,
    repeats: &Vector,
    offsets: &Vector,
    rotation: f32,
    opensim_flavour: bool,
) -> Map {
    if opensim_flavour {
        Map {
            texture,
            repeats: Vector {
                x: repeats.x,
                y: repeats.y,
                z: 0.0,
            },
            offsets: Vector {
                x: opensim_offset(offsets.x),
                y: opensim_offset(offsets.y),
                z: 0.0,
            },
            rotation: opensim_rotation(rotation),
        }
    } else {
        Map {
            texture,
            repeats: Vector {
                x: repeats.x,
                y: repeats.y,
                z: 0.0,
            },
            offsets: Vector {
                x: offsets.x,
                y: offsets.y,
                z: 0.0,
            },
            rotation,
        }
    }
}

/// Applies `change` to each face of `prim` the face argument `face` names,
/// raising OpenSim's `CHANGED_TEXTURE` for a face it changed.
fn each_face(
    prim: &mut Prim,
    face: i32,
    opensim_flavour: bool,
    changes: &mut Changes,
    mut change: impl FnMut(&mut Face),
) {
    for index in faces_set(prim, face, opensim_flavour) {
        if let Some(found) = prim.faces.get_mut(index) {
            let before = found.clone();
            change(found);
            textured(&before, found, opensim_flavour, changes);
        }
    }
}

/// Reads a map's texture, repeats, offsets and rotation (`PRIM_TEXTURE`,
/// `PRIM_NORMAL`, `PRIM_SPECULAR`, the glTF maps).
///
/// # Errors
///
/// [`Stop`] when a value is short or refused.
fn read_map(
    ctx: &mut ScriptCtx<'_>,
    reader: &mut Reader<'_>,
) -> Result<(String, Vector, Vector, f32), Stop> {
    let texture = reader.name_or_key(ctx, Some("texture"))?;
    let repeats = reader.vector(ctx, Some("repeats"))?;
    let offsets = reader.vector(ctx, Some("offsets"))?;
    let rotation = reader.float(ctx, Some("rotation"))?;
    Ok((texture, repeats, offsets, rotation))
}

/// Reads the shape a `PRIM_TYPE` rule asks for and makes it, or says why
/// it will not.
///
/// # Errors
///
/// [`Stop`] when a value is short or refused, or the type is none.
fn read_type(
    ctx: &mut ScriptCtx<'_>,
    reader: &mut Reader<'_>,
    prim: &mut Prim,
    changes: &mut Changes,
) -> Result<(), Stop> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let kind = reader.integer(ctx, Some("type"))?;
    let named = match kind {
        shape::TYPE_BOX => "PRIM_TYPE_BOX",
        shape::TYPE_CYLINDER => "PRIM_TYPE_CYLINDER",
        shape::TYPE_PRISM => "PRIM_TYPE_PRISM",
        shape::TYPE_SPHERE => "PRIM_TYPE_SPHERE",
        shape::TYPE_TORUS => "PRIM_TYPE_TORUS",
        shape::TYPE_TUBE => "PRIM_TYPE_TUBE",
        shape::TYPE_RING => "PRIM_TYPE_RING",
        shape::TYPE_SCULPT => "PRIM_TYPE_SCULPT",
        _ => {
            if opensim_flavour {
                ctx.refuse_to_owner(format!(
                    "{}: Error running rule #{}: arg #0 - unsupported parameter",
                    reader.function,
                    reader.number.saturating_add(1),
                ));
                ctx.sleep(1.0);
            } else {
                ctx.refuse_to_owner(format!(
                    "{} error running rule #{}: unknown rule.",
                    reader.function, reader.number,
                ));
            }
            return Err(Stop::Refused);
        }
    };
    reader.rename(named);
    if kind == shape::TYPE_SCULPT {
        reader.need(2)?;
        let map = reader.name_or_key(ctx, Some("map"))?;
        let sculpt_kind = reader.integer(ctx, Some("type"))?;
        let Some(map) = texture_named(ctx, &map).filter(|key| key != NULL_KEY) else {
            // Second Life drops the sculpt and keeps the shape under it, a
            // torus-like circle (`prim/params`: `sculpt-not-a-key` reads
            // `PRIM_TYPE_TORUS` after a sculpt); OpenSim changes nothing.
            if !opensim_flavour && prim.shape.sculpt.is_some() {
                let mut shape = prim.shape.clone();
                shape.sculpt = None;
                set_shape(prim, shape);
                changes.raise(CHANGED_SHAPE);
            }
            return Ok(());
        };
        let made = shape::sculpted(&prim.shape, map, sculpt_kind, opensim_flavour);
        set_shape(prim, made);
        changes.raise(CHANGED_SHAPE);
        return Ok(());
    }
    let values = match kind {
        shape::TYPE_SPHERE => 5,
        shape::TYPE_TORUS | shape::TYPE_TUBE | shape::TYPE_RING => 11,
        _ => 6,
    };
    reader.need(values)?;
    let zero = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut asked = Asked {
        kind,
        hole: reader.integer(ctx, Some("hole shape"))?,
        cut: reader.vector(ctx, Some("cut"))?,
        hollow: reader.float(ctx, Some("hollow"))?,
        twist: reader.vector(ctx, Some("twist"))?,
        scale: Vector {
            x: 1.0,
            y: 1.0,
            z: 0.0,
        },
        shear: zero.clone(),
        profile_cut: Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        },
        taper: zero,
        revolutions: 1.0,
        radius_offset: 0.0,
        skew: 0.0,
    };
    match kind {
        shape::TYPE_SPHERE => asked.profile_cut = reader.vector(ctx, Some("dimple"))?,
        shape::TYPE_TORUS | shape::TYPE_TUBE | shape::TYPE_RING => {
            asked.scale = reader.vector(ctx, Some("hole size"))?;
            asked.shear = reader.vector(ctx, Some("top shear"))?;
            asked.profile_cut = reader.vector(ctx, Some("profile cut"))?;
            asked.taper = reader.vector(ctx, Some("taper"))?;
            asked.revolutions = reader.float(ctx, Some("revolutions"))?;
            asked.radius_offset = reader.float(ctx, Some("radius offset"))?;
            asked.skew = reader.float(ctx, Some("skew"))?;
        }
        _ => {
            asked.scale = reader.vector(ctx, Some("taper"))?;
            asked.shear = reader.vector(ctx, Some("top shear"))?;
        }
    }
    if !opensim_flavour && !shape::hole_is_known(asked.hole) {
        reader.warn(ctx, "Invalid hole type.");
    }
    if let Some(made) = shape::shaped(&prim.shape, &asked, opensim_flavour) {
        set_shape(prim, made);
        changes.raise(CHANGED_SHAPE);
    }
    Ok(())
}

/// Writes a move of `target` to `to`, which is its local position: an
/// unworn root's region position, a child's offset. `capped` moves at most
/// 10 m from where it is. OpenSim raises its `CHANGED_POSITION` for a move.
fn move_to(
    prim: &mut Prim,
    to: &Vector,
    capped: bool,
    opensim_flavour: bool,
    changes: &mut Changes,
) {
    let goes = if capped {
        within_reach(&prim.local_position, to)
    } else {
        to.clone()
    };
    if goes != prim.local_position && opensim_flavour {
        changes.raise(CHANGED_POSITION);
    }
    prim.local_position = goes;
}

/// Reads one glTF override value, `""` clearing it.
///
/// # Errors
///
/// [`Stop::Short`] at the end of the list.
fn read_override(reader: &mut Reader<'_>) -> Result<Option<Element>, Stop> {
    let value = match reader.peek().ok_or(Stop::Short)? {
        Element::String(text) if text.is_empty() => None,
        other => Some(other.clone()),
    };
    reader.skip()?;
    Ok(value)
}

/// A glTF override as a float, if it is one.
fn float_override(value: Option<Element>) -> Option<f32> {
    match value? {
        Element::Float(float) => Some(float),
        Element::Integer(integer) => Some(crate::num::int_to_float(integer)),
        _ => None,
    }
}

/// A glTF override as a vector, if it is one.
fn vector_override(value: Option<Element>) -> Option<Vector> {
    match value? {
        Element::Vector(vector) => Some(vector),
        _ => None,
    }
}

/// A glTF override as an integer, if it is one.
fn integer_override(value: Option<Element>) -> Option<i32> {
    match value? {
        Element::Integer(integer) => Some(integer),
        _ => None,
    }
}

/// Reads a glTF map override: texture, repeats, offsets and rotation, each
/// `""` to clear it; [`None`] when the texture is cleared.
///
/// # Errors
///
/// [`Stop::Short`] at the end of the list.
fn read_gltf_map(reader: &mut Reader<'_>) -> Result<Option<Map>, Stop> {
    let texture = read_override(reader)?;
    let repeats = vector_override(read_override(reader)?);
    let offsets = vector_override(read_override(reader)?);
    let rotation = float_override(read_override(reader)?);
    Ok(texture.map(|texture| Map {
        texture: crate::cast::element_to_string(&texture),
        repeats: repeats.unwrap_or(Vector {
            x: 1.0,
            y: 1.0,
            z: 0.0,
        }),
        offsets: offsets.unwrap_or(Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        rotation: rotation.unwrap_or(0.0),
    }))
}

/// Applies one rule `code`, its values next in `reader`, to `prim`.
///
/// # Errors
///
/// [`Stop`] when the rule is short of values or refused: nothing after it
/// runs.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per rule of the PRIM_* language, each a few lines"
)]
fn rule(
    ctx: &mut ScriptCtx<'_>,
    reader: &mut Reader<'_>,
    code: i32,
    target: &Target,
    prim: &mut Prim,
    changes: &mut Changes,
) -> Result<(), Stop> {
    let opensim_flavour = ctx.flavour().is_opensim();
    match code {
        POSITION | POS_LOCAL => {
            reader.need(1)?;
            let to = reader.vector(ctx, Some("position"))?;
            if target.child {
                if code == POSITION && !opensim_flavour {
                    return Ok(());
                }
                move_to(prim, &to, false, opensim_flavour, changes);
            } else {
                move_to(prim, &to, true, opensim_flavour, changes);
            }
        }
        SIZE => {
            reader.need(1)?;
            let scale = reader.vector(ctx, Some("size"))?;
            set_scale(prim, &scale, opensim_flavour, changes);
        }
        ROTATION => {
            reader.need(1)?;
            let turn = normalised(&reader.rotation(ctx, Some("rotation"))?);
            // A child's is the root's turned by it, as on both grids
            // (`LSL_Api` sets `root * rot`, OpenMetaverse's product, which
            // is LSL's `rot * root`).
            prim.local_rotation = if target.child {
                crate::ops::compose(&turn, &prim.root_rotation)
            } else {
                turn
            };
        }
        ROT_LOCAL => {
            reader.need(1)?;
            prim.local_rotation = normalised(&reader.rotation(ctx, Some("rotation"))?);
        }
        TYPE => {
            reader.need(3)?;
            read_type(ctx, reader, prim, changes)?;
        }
        TEXTURE => {
            reader.need(5)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let (given, repeats, offsets, rotation) = read_map(ctx, reader)?;
            let texture =
                texture_named(ctx, &given).filter(|key| !(opensim_flavour && key == NULL_KEY));
            each_face(prim, face, opensim_flavour, changes, |found| {
                let current = found.texture.texture.clone();
                found.texture = placed_map(
                    texture.clone().unwrap_or(current),
                    &repeats,
                    &offsets,
                    rotation,
                    opensim_flavour,
                );
            });
        }
        COLOR => {
            reader.need(3)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let colour = reader.vector(ctx, Some("color"))?;
            let alpha = reader.float(ctx, Some("alpha"))?;
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    set_face_colour(found, &colour, opensim_flavour, true, changes);
                    set_face_alpha(found, alpha, opensim_flavour, changes);
                }
            }
        }
        BUMP_SHINY => {
            reader.need(3)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let shiny = reader.integer(ctx, Some("shiny"))?;
            let bump = reader.integer(ctx, Some("bump"))?;
            let (shiny, bump) = if opensim_flavour {
                (
                    if (0..=3).contains(&shiny) { shiny } else { 0 },
                    bump & 0x1f,
                )
            } else {
                (
                    in_bounds(ctx, reader, shiny, 0, 3)?,
                    in_bounds(ctx, reader, bump, 0, 17)?,
                )
            };
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.shiny = shiny;
                found.bump = bump;
            });
        }
        FULLBRIGHT => {
            reader.need(2)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let value = reader.integer(ctx, Some("fullbright"))?;
            let bright = if opensim_flavour {
                value != 0
            } else {
                in_bounds(ctx, reader, value, 0, 1)? == 1
            };
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.fullbright = bright;
            });
        }
        FLEXIBLE => {
            reader.need(7)?;
            let on = reader.integer(ctx, Some("flexible"))? != 0;
            let softness = reader.integer(ctx, Some("softness"))?;
            let gravity = reader.float(ctx, Some("gravity"))?;
            let friction = reader.float(ctx, Some("friction"))?;
            let wind = reader.float(ctx, Some("wind"))?;
            let tension = reader.float(ctx, Some("tension"))?;
            let force = reader.vector(ctx, Some("force"))?;
            set_flexible(
                prim,
                on,
                softness,
                [gravity, friction, wind, tension],
                force,
                opensim_flavour,
            );
        }
        TEXGEN => {
            reader.need(2)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let value = reader.integer(ctx, Some("texgen"))?;
            let mode = if opensim_flavour {
                if value == 1 { 1 } else { 0 }
            } else {
                in_bounds(ctx, reader, value, 0, 1)?
            };
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.texgen = mode;
            });
        }
        POINT_LIGHT => {
            reader.need(5)?;
            let on = reader.integer(ctx, Some("light"))? != 0;
            let colour = reader.vector(ctx, Some("color"))?;
            let intensity = reader.float(ctx, Some("intensity"))?;
            let radius = reader.float(ctx, Some("radius"))?;
            let falloff = reader.float(ctx, Some("falloff"))?;
            set_light(
                prim,
                on,
                &colour,
                [intensity, radius, falloff],
                opensim_flavour,
            );
        }
        GLOW => {
            reader.need(2)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let glow = reader.float(ctx, Some("glow"))?;
            let glow = if opensim_flavour {
                opensim_glow(glow)
            } else {
                in_unit(ctx, reader, glow)?
            };
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.glow = glow;
            });
        }
        TEXT => {
            reader.need(3)?;
            let text = reader.string(ctx, Some("text"))?;
            let colour = reader.vector(ctx, Some("color"))?;
            let alpha = reader.float(ctx, Some("alpha"))?;
            set_text(prim, &text, &colour, unit(alpha), opensim_flavour, true);
        }
        NAME => {
            reader.need(1)?;
            let name = reader.string(ctx, None)?;
            prim.name = kept_name(&name, opensim_flavour);
        }
        DESC => {
            reader.need(1)?;
            let description = reader.string(ctx, None)?;
            prim.description = kept_description(&description, opensim_flavour);
        }
        PHYSICS_SHAPE_TYPE => {
            reader.need(1)?;
            prim.physics_shape = reader.integer(ctx, Some("physics shape type"))?;
        }
        MATERIAL => {
            reader.need(1)?;
            let material = reader.integer(ctx, Some("material"))?;
            if opensim_flavour {
                if (0..=7).contains(&material) {
                    prim.material = material;
                }
            } else {
                prim.material = in_bounds(ctx, reader, material, 0, 7)?;
            }
        }
        PHYSICS | TEMP_ON_REZ | PHANTOM => {
            reader.need(1)?;
            let on = if opensim_flavour {
                reader.flag_text()?
            } else {
                reader.integer(ctx, None)? != 0
            };
            match code {
                PHYSICS => prim.physics = on,
                TEMP_ON_REZ => prim.temp_on_rez = on,
                _ => prim.phantom = on,
            }
        }
        OMEGA => {
            reader.need(3)?;
            let axis = reader.vector(ctx, Some("axis"))?;
            let spin = reader.float(ctx, Some("spinrate"))?;
            let gain = reader.float(ctx, Some("gain"))?;
            set_omega(prim, axis, spin, gain, opensim_flavour);
        }
        SLICE => {
            reader.need(1)?;
            let slice = reader.vector(ctx, Some("slice"))?;
            let sliced = shape::sliced(&prim.shape, &slice, opensim_flavour);
            set_shape(prim, sliced);
        }
        SPECULAR => {
            reader.need(8)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let (given, repeats, offsets, rotation) = read_map(ctx, reader)?;
            let colour = reader.vector(ctx, Some("color"))?;
            let glossiness = reader.integer(ctx, Some("glossiness"))?;
            let environment = reader.integer(ctx, Some("environment"))?;
            let texture = texture_named(ctx, &given).unwrap_or_else(|| NULL_KEY.to_owned());
            let quantise = if opensim_flavour {
                byte_rounded
            } else {
                byte_truncated
            };
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.specular.map = Map {
                    texture: texture.clone(),
                    repeats: repeats.clone(),
                    offsets: offsets.clone(),
                    rotation,
                };
                found.specular.color = Vector {
                    x: quantise(colour.x),
                    y: quantise(colour.y),
                    z: quantise(colour.z),
                };
                found.specular.glossiness = glossiness.clamp(0, 255);
                found.specular.environment = environment.clamp(0, 255);
            });
        }
        NORMAL => {
            reader.need(5)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let (given, repeats, offsets, rotation) = read_map(ctx, reader)?;
            let texture = texture_named(ctx, &given).unwrap_or_else(|| NULL_KEY.to_owned());
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.normal = Map {
                    texture: texture.clone(),
                    repeats: repeats.clone(),
                    offsets: offsets.clone(),
                    rotation,
                };
            });
        }
        ALPHA_MODE => {
            reader.need(3)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let mode = reader.integer(ctx, Some("alpha mode"))?;
            if opensim_flavour && !(0..=3).contains(&mode) {
                reader.error(ctx, "must be 0 to 3");
                return Err(Stop::Refused);
            }
            let cutoff = reader.integer(ctx, Some("mask cutoff"))?;
            if opensim_flavour && !(0..=255).contains(&cutoff) {
                reader.error(ctx, "must be 0 to 255");
                return Err(Stop::Refused);
            }
            let (mode, cutoff) = (mode.clamp(0, 3), cutoff.clamp(0, 255));
            each_face(prim, face, opensim_flavour, changes, |found| {
                found.alpha_mode = mode;
                found.mask_cutoff = cutoff;
            });
        }
        ALLOW_UNSIT => {
            reader.need(1)?;
            prim.allow_unsit = reader.integer(ctx, None)? != 0;
        }
        SCRIPTED_SIT_ONLY => {
            reader.need(1)?;
            prim.scripted_sit_only = reader.integer(ctx, None)? != 0;
        }
        SIT_TARGET => {
            reader.need(3)?;
            let active = reader.integer(ctx, Some("active"))?;
            let mut offset = reader.vector(ctx, Some("offset"))?;
            let rotation = reader.rotation(ctx, Some("rotation"))?;
            set_sit_target(ctx, target.link, active, &mut offset, rotation);
        }
        CLICK_ACTION => {
            reader.need(1)?;
            let action = reader.integer(ctx, None)?;
            if (0..=9).contains(&action) {
                prim.click_action = action;
            }
        }
        REFLECTION_PROBE => {
            reader.need(4)?;
            prim.reflection_probe.on = reader.integer(ctx, None)? != 0;
            prim.reflection_probe.ambiance = reader.float(ctx, Some("ambiance"))?;
            prim.reflection_probe.clip = reader.float(ctx, Some("clip distance"))?;
            prim.reflection_probe.flags = reader.integer(ctx, Some("flags"))?;
        }
        RENDER_MATERIAL => {
            reader.need(2)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let given = reader.name_or_key(ctx, Some("material"))?;
            let material = if dotnet::is_uuid(&given) {
                given.to_ascii_lowercase()
            } else {
                let caller = ctx.caller();
                ctx.host()
                    .inventory_key(caller, &given, MATERIAL_ASSET)
                    .unwrap_or_else(|| NULL_KEY.to_owned())
            };
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    found.render_material.clone_from(&material);
                    changes.raise(CHANGED_RENDER_MATERIAL);
                }
            }
        }
        SIT_FLAGS => {
            reader.need(1)?;
            let flags = reader.integer(ctx, Some("flags"))?;
            prim.allow_unsit = flags & SIT_FLAG_ALLOW_UNSIT != 0;
            prim.scripted_sit_only = flags & SIT_FLAG_SCRIPTED_ONLY != 0;
            prim.sit_flags = flags & SIT_FLAGS_KEPT;
        }
        GLTF_BASE_COLOR => {
            reader.need(10)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let map = read_gltf_map(reader)?;
            let colour = vector_override(read_override(reader)?);
            let alpha = float_override(read_override(reader)?);
            let mode = integer_override(read_override(reader)?);
            let cutoff = float_override(read_override(reader)?);
            let sided = integer_override(read_override(reader)?);
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    found.gltf.base_color_map.clone_from(&map);
                    found.gltf.base_color.clone_from(&colour);
                    found.gltf.base_alpha = alpha;
                    found.gltf.alpha_mode = mode;
                    found.gltf.alpha_cutoff = cutoff;
                    found.gltf.double_sided = sided.map(|value| value != 0);
                }
            }
        }
        GLTF_NORMAL => {
            reader.need(5)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let map = read_gltf_map(reader)?;
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    found.gltf.normal_map.clone_from(&map);
                }
            }
        }
        GLTF_METALLIC_ROUGHNESS => {
            reader.need(7)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let map = read_gltf_map(reader)?;
            let metallic = float_override(read_override(reader)?);
            let roughness = float_override(read_override(reader)?);
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    found.gltf.metallic_roughness_map.clone_from(&map);
                    found.gltf.metallic = metallic;
                    found.gltf.roughness = roughness;
                }
            }
        }
        GLTF_EMISSIVE => {
            reader.need(6)?;
            let face = reader.integer(ctx, Some("face number"))?;
            let map = read_gltf_map(reader)?;
            let tint = vector_override(read_override(reader)?);
            for index in faces_set(prim, face, opensim_flavour) {
                if let Some(found) = prim.faces.get_mut(index) {
                    found.gltf.emissive_map.clone_from(&map);
                    found.gltf.emissive.clone_from(&tint);
                }
            }
        }
        DAMAGE => {
            reader.need(2)?;
            let damage = reader.float(ctx, Some("damage"))?;
            let kind = reader.integer(ctx, Some("damage type"))?;
            prim.damage = (damage, kind);
        }
        HEALTH => {
            reader.need(1)?;
            prim.health = reader.float(ctx, Some("health"))?;
        }
        COLLISION_SOUND => {
            reader.need(2)?;
            let given = reader.name_or_key(ctx, Some("sound"))?;
            let volume = reader.float(ctx, Some("volume"))?;
            let sound = asset_named(ctx, &given, INVENTORY_SOUND);
            prim.sound.collision = sound.map(|sound| (sound, unit(volume)));
        }
        PROJECTOR => {
            // Second Life takes a projector without a word and keeps
            // nothing a get could read back (`errors/flood-window`'s `P`).
            reader.need(4)?;
            reader.name_or_key(ctx, Some("texture"))?;
            reader.float(ctx, Some("fov"))?;
            reader.float(ctx, Some("focus"))?;
            reader.float(ctx, Some("ambiance"))?;
        }
        _ => {
            unknown(ctx, reader);
            return Err(Stop::Refused);
        }
    }
    Ok(())
}

/// `INVENTORY_MATERIAL`.
const MATERIAL_ASSET: i32 = 57;

/// Writes a flexible path: on Second Life clamped and in the tenths its
/// parameters travel in, everything read as 0 while it is off; on OpenSim
/// as given, on or off (`prim/params`). It makes a straight path flexible,
/// and a flexible one straight again.
pub(crate) fn set_flexible(
    prim: &mut Prim,
    on: bool,
    softness: i32,
    values: [f32; 4],
    force: Vector,
    opensim_flavour: bool,
) {
    let [gravity, friction, wind, tension] = values;
    let tenths =
        |value: f32| crate::num::int_to_float(crate::num::float_to_int(value * 10.0)) / 10.0;
    let flexible = &mut prim.flexible;
    flexible.on = on;
    if opensim_flavour {
        flexible.softness = softness;
        flexible.gravity = gravity;
        flexible.friction = friction;
        flexible.wind = wind;
        flexible.tension = tension;
        flexible.force = force;
    } else if on {
        flexible.softness = softness.clamp(0, 3);
        flexible.gravity = tenths(gravity.clamp(-10.0, 10.0) + 10.0) - 10.0;
        flexible.friction = tenths(friction.clamp(0.0, 10.0));
        flexible.wind = tenths(wind.clamp(0.0, 10.0));
        flexible.tension = tenths(tension.clamp(0.0, 10.0));
        flexible.force = force;
    }
    if on && prim.shape.path_curve == path::LINE {
        prim.shape.path_curve = path::FLEXIBLE;
    } else if !on && prim.shape.path_curve == path::FLEXIBLE {
        prim.shape.path_curve = path::LINE;
    }
}

/// Writes a point light: on Second Life clamped with the colour and the
/// intensity in rounded bytes; on OpenSim with the colour as given and the
/// radius and falloff no less than 0.1 and 0.01 — and a light turned off
/// keeps the values it had there (`prim/params`).
pub(crate) fn set_light(
    prim: &mut Prim,
    on: bool,
    colour: &Vector,
    values: [f32; 3],
    opensim_flavour: bool,
) {
    let [intensity, radius, falloff] = values;
    let light = &mut prim.light;
    light.on = on;
    if opensim_flavour {
        if on {
            light.color = Vector {
                x: unit(colour.x),
                y: unit(colour.y),
                z: unit(colour.z),
            };
            light.intensity = unit(intensity);
            light.radius = radius.clamp(0.1, 20.0);
            light.falloff = falloff.clamp(0.01, 2.0);
        }
    } else if on {
        light.color = Vector {
            x: byte_rounded(colour.x),
            y: byte_rounded(colour.y),
            z: byte_rounded(colour.z),
        };
        light.intensity = byte_rounded(intensity);
        light.radius = radius.clamp(0.0, 20.0);
        light.falloff = falloff.clamp(0.0, 2.0);
    }
}

/// Writes hover text as the grid keeps it: a colour clamped (OpenSim's in
/// truncated bytes) and an alpha clamped (OpenSim's read back inverted,
/// [`opensim_text_alpha`]); a text past 254 bytes cut on OpenSim, and on
/// Second Life refused by `PRIM_TEXT` (`refuse_long`) but cut by
/// `llSetText` (`prim/params`, `prim/faces`).
pub(crate) fn set_text(
    prim: &mut Prim,
    text: &str,
    colour: &Vector,
    alpha: f32,
    opensim_flavour: bool,
    refuse_long: bool,
) {
    if !opensim_flavour && refuse_long && text.len() > TEXT_BYTES {
        return;
    }
    prim.text.text = cut_to_bytes(text, TEXT_BYTES);
    if opensim_flavour {
        prim.text.color = opensim_colour(colour);
        prim.text.alpha = opensim_text_alpha(alpha);
    } else {
        prim.text.color = Vector {
            x: unit(colour.x),
            y: unit(colour.y),
            z: unit(colour.z),
        };
        prim.text.alpha = unit(alpha);
    }
}

/// Writes a spin: OpenSim reads back a gain of 1 for any it was given but
/// 0 (`prim/placement`: 0.5 reads 1, a stopped spin's 0 reads 0).
pub(crate) fn set_omega(
    prim: &mut Prim,
    axis: Vector,
    spin: f32,
    gain: f32,
    opensim_flavour: bool,
) {
    prim.omega.axis = axis;
    prim.omega.spin = spin;
    prim.omega.gain = if opensim_flavour && gain != 0.0 {
        1.0
    } else {
        gain
    };
}

/// Writes the sit target of the prim numbered `link` through the host's
/// seat: set when `active` is 1 (on OpenSim a zero target with the
/// identity turn is nudged a hundred-thousandth up, so it is one at all),
/// cleared when it is 0.
fn set_sit_target(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    active: i32,
    offset: &mut Vector,
    rotation: Rotation,
) {
    let opensim_flavour = ctx.flavour().is_opensim();
    let caller = ctx.caller();
    let Some(seat) = ctx.host().seat(caller, link) else {
        return;
    };
    if active == 1 || (!opensim_flavour && active != 0) {
        if opensim_flavour
            && offset.x == 0.0
            && offset.y == 0.0
            && offset.z == 0.0
            && rotation.s.total_cmp(&1.0).is_eq()
        {
            offset.z = 1e-5;
        }
        seat.target = Some(SitTarget {
            offset: offset.clone(),
            rotation,
        });
    } else if active == 0 {
        seat.target = None;
    }
}

/// Writes the media of each face `face` names: [`None`] clears it. Raises
/// `CHANGED_MEDIA` (`prim/changed`).
pub(crate) fn set_media(prim: &mut Prim, face: i32, media: Option<&Media>, changes: &mut Changes) {
    let faces = faces_set(prim, face, true);
    for index in faces {
        if let Some(slot) = prim.media.get_mut(index) {
            *slot = media.cloned();
            changes.raise(super::CHANGED_MEDIA);
        }
    }
    if media.is_some() {
        prim.has_media_list = true;
    }
}
