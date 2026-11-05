//! Answering a `PRIM_*` rule list (`llGetPrimitiveParams` and its family).
//!
//! Each rule appends its values in the order asked, once per asking, for
//! each prim of the run in link order; `PRIM_LINK_TARGET` moves on to the
//! prims it names. Where the grids part (`prim/params-rules`): an unknown
//! code empties Second Life's whole answer with `Param 0 error, unknown
//! parameter`, and OpenSim skips it; a face past the prim's answers nothing
//! for that rule on Second Life and the default face on OpenSim, which
//! throws for a face it cannot index at all.

use sl_types::lsl::{Rotation, Vector};

use super::args::Reader;
use super::shape;
use super::{
    ALLOW_UNSIT, ALPHA_MODE, BUMP_SHINY, CLICK_ACTION, COLLISION_SOUND, COLOR, DAMAGE, DESC,
    FLEXIBLE, FULLBRIGHT, FacesRead, GLOW, GLTF_BASE_COLOR, GLTF_EMISSIVE, GLTF_METALLIC_ROUGHNESS,
    GLTF_NORMAL, HEALTH, LINK_TARGET, MATERIAL, NAME, NORMAL, OMEGA, PHANTOM, PHYSICS,
    PHYSICS_SHAPE_TYPE, POINT_LIGHT, POS_LOCAL, POSITION, REFLECTION_PROBE, RENDER_MATERIAL,
    ROT_LOCAL, ROTATION, SCRIPTED_SIT_ONLY, SIT_FLAGS, SIT_TARGET, SIZE, SLICE, SPECULAR,
    TEMP_ON_REZ, TEXGEN, TEXT, TEXTURE, TYPE, faces_get, opensim_has, second_life_has, unit,
};
use crate::library::{CallError, Exception};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::prim::{Face, Map, Prim};

/// `SIT_FLAG_SIT_TARGET`.
const SIT_FLAG_SIT_TARGET: i32 = 0x01;
/// `SIT_FLAG_ALLOW_UNSIT`.
const SIT_FLAG_ALLOW_UNSIT: i32 = 0x02;
/// `SIT_FLAG_SCRIPTED_ONLY`.
const SIT_FLAG_SCRIPTED_ONLY: i32 = 0x04;

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// How a run of a get ended.
enum Ended {
    /// At the end of the list.
    End,
    /// At a `PRIM_LINK_TARGET` naming the next run's prims.
    Link(i32),
    /// At a rule short of values: nothing more is answered.
    Short,
    /// At an unknown code on Second Life: the whole answer is nothing.
    Unknown(usize),
    /// At a code that is no integer on Second Life: the whole answer is
    /// nothing, without a word (`prim/params-rules`, `code-as-string`).
    NotACode,
}

/// `llGetPrimitiveParams`: the caller's own prim's answer to `rules`.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws (a face it cannot index).
pub(crate) fn get_params(
    ctx: &mut ScriptCtx<'_>,
    function: &'static str,
    rules: &[Element],
) -> Result<Vec<Element>, CallError> {
    let keys = super::own_key(ctx).into_iter().collect::<Vec<_>>();
    answer(ctx, function, &keys, rules)
}

/// `llGetLinkPrimitiveParams`: the answer to `rules` of the prims the link
/// argument `link` names ([`super::get_targets`]).
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws.
pub(crate) fn link_get_params(
    ctx: &mut ScriptCtx<'_>,
    function: &'static str,
    link: i32,
    rules: &[Element],
) -> Result<Vec<Element>, CallError> {
    let keys = super::get_targets(ctx, link);
    answer(ctx, function, &keys, rules)
}

/// The answer to `rules` of the prims keyed `keys`, then of the prims each
/// `PRIM_LINK_TARGET` names.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws.
pub(crate) fn answer(
    ctx: &mut ScriptCtx<'_>,
    function: &'static str,
    keys: &[String],
    rules: &[Element],
) -> Result<Vec<Element>, CallError> {
    let mut reader = Reader::new(ctx, function, rules);
    let mut out = Vec::new();
    let mut keys = keys.to_vec();
    loop {
        if keys.is_empty() {
            return Ok(out);
        }
        let start = reader.at;
        let mut ended = Ended::End;
        for key in &keys {
            reader.at = start;
            let caller = ctx.caller();
            let Some(prim) = ctx.host().prim(caller, key) else {
                continue;
            };
            ended = run(ctx, &mut reader, &prim, &mut out)?;
        }
        match ended {
            Ended::End | Ended::Short => return Ok(out),
            Ended::NotACode => return Ok(Vec::new()),
            Ended::Unknown(index) => {
                ctx.refuse(format!(
                    "{function} error: Param {index} error, unknown parameter"
                ));
                return Ok(Vec::new());
            }
            Ended::Link(link) => keys = super::get_targets(ctx, link),
        }
    }
}

/// Answers rules from where `reader` is for `prim` into `out`, until the
/// list ends or a `PRIM_LINK_TARGET` names other prims.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws.
fn run(
    ctx: &mut ScriptCtx<'_>,
    reader: &mut Reader<'_>,
    prim: &Prim,
    out: &mut Vec<Element>,
) -> Result<Ended, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    while !reader.done() {
        let index = reader.at;
        let Some(code) = reader.code() else {
            if opensim_flavour {
                continue;
            }
            return Ok(Ended::NotACode);
        };
        let known = if opensim_flavour {
            opensim_has(code)
        } else {
            second_life_has(code)
        };
        if !known {
            if opensim_flavour {
                continue;
            }
            return Ok(Ended::Unknown(index));
        }
        if code == LINK_TARGET {
            return Ok(match reader.peek() {
                Some(Element::Integer(link)) => {
                    let link = *link;
                    let _skipped = reader.skip();
                    Ended::Link(link)
                }
                _ => Ended::Short,
            });
        }
        let per_face = matches!(
            code,
            TEXTURE
                | COLOR
                | BUMP_SHINY
                | FULLBRIGHT
                | TEXGEN
                | GLOW
                | SPECULAR
                | NORMAL
                | ALPHA_MODE
                | RENDER_MATERIAL
                | GLTF_BASE_COLOR
                | GLTF_NORMAL
                | GLTF_METALLIC_ROUGHNESS
                | GLTF_EMISSIVE
        );
        if per_face {
            let face = match reader.peek() {
                Some(Element::Integer(face)) => *face,
                Some(Element::String(text) | Element::Key(text)) if opensim_flavour => {
                    crate::parse::integer(text)
                }
                Some(_) => {
                    let _skipped = reader.skip();
                    continue;
                }
                None => return Ok(Ended::Short),
            };
            let _skipped = reader.skip();
            match faces_get(prim, face, opensim_flavour) {
                FacesRead::Thrown(face) => {
                    return Err(CallError::Exception {
                        id: crate::library::BuiltinId::LlGetPrimitiveParams,
                        exception: Exception::FaceOutOfRange(face),
                    });
                }
                FacesRead::Faces(faces) => {
                    for found in &faces {
                        out.extend(face_rule(code, found));
                    }
                }
            }
            continue;
        }
        out.extend(prim_rule(ctx, code, prim, opensim_flavour));
    }
    Ok(Ended::End)
}

/// A map's texture, repeats, offsets and rotation as a get lists them.
fn map_values(map: &Map) -> [Element; 4] {
    [
        Element::String(map.texture.clone()),
        Element::Vector(map.repeats.clone()),
        Element::Vector(map.offsets.clone()),
        Element::Float(map.rotation),
    ]
}

/// A glTF map override as a get lists it: an empty texture and the default
/// placement where the face keeps its material's.
fn gltf_map_values(map: Option<&Map>) -> [Element; 4] {
    map.map_or_else(
        || {
            [
                Element::String(String::new()),
                Element::Vector(Vector {
                    x: 1.0,
                    y: 1.0,
                    z: 0.0,
                }),
                Element::Vector(ZERO),
                Element::Float(0.0),
            ]
        },
        map_values,
    )
}

/// An override as a get lists it: the value, or `""` where there is none.
fn or_empty(value: Option<Element>) -> Element {
    value.unwrap_or_else(|| Element::String(String::new()))
}

/// What a per-face rule answers for one face.
fn face_rule(code: i32, face: &Face) -> Vec<Element> {
    match code {
        TEXTURE => map_values(&face.texture).to_vec(),
        COLOR => {
            // Second Life's `llSetColor` keeps a colour past its range for
            // `llGetColor`; the rule reads it clamped (`prim/faces`).
            let colour = Vector {
                x: unit(face.color.x),
                y: unit(face.color.y),
                z: unit(face.color.z),
            };
            vec![Element::Vector(colour), Element::Float(face.alpha)]
        }
        BUMP_SHINY => vec![Element::Integer(face.shiny), Element::Integer(face.bump)],
        FULLBRIGHT => vec![Element::Integer(i32::from(face.fullbright))],
        TEXGEN => vec![Element::Integer(face.texgen)],
        GLOW => vec![Element::Float(face.glow)],
        NORMAL => map_values(&face.normal).to_vec(),
        SPECULAR => {
            let mut values = map_values(&face.specular.map).to_vec();
            values.extend([
                Element::Vector(face.specular.color.clone()),
                Element::Integer(face.specular.glossiness),
                Element::Integer(face.specular.environment),
            ]);
            values
        }
        ALPHA_MODE => vec![
            Element::Integer(face.alpha_mode),
            Element::Integer(face.mask_cutoff),
        ],
        RENDER_MATERIAL => vec![Element::String(face.render_material.clone())],
        GLTF_BASE_COLOR => {
            let gltf = &face.gltf;
            let mut values = gltf_map_values(gltf.base_color_map.as_ref()).to_vec();
            values.extend([
                or_empty(gltf.base_color.clone().map(Element::Vector)),
                or_empty(gltf.base_alpha.map(Element::Float)),
                or_empty(gltf.alpha_mode.map(Element::Integer)),
                or_empty(gltf.alpha_cutoff.map(Element::Float)),
                or_empty(
                    gltf.double_sided
                        .map(|sided| Element::Integer(i32::from(sided))),
                ),
            ]);
            values
        }
        GLTF_NORMAL => gltf_map_values(face.gltf.normal_map.as_ref()).to_vec(),
        GLTF_METALLIC_ROUGHNESS => {
            let gltf = &face.gltf;
            let mut values = gltf_map_values(gltf.metallic_roughness_map.as_ref()).to_vec();
            values.extend([
                or_empty(gltf.metallic.map(Element::Float)),
                or_empty(gltf.roughness.map(Element::Float)),
            ]);
            values
        }
        GLTF_EMISSIVE => {
            let gltf = &face.gltf;
            let mut values = gltf_map_values(gltf.emissive_map.as_ref()).to_vec();
            values.push(or_empty(gltf.emissive.clone().map(Element::Vector)));
            values
        }
        _ => Vec::new(),
    }
}

/// What a rule about the whole prim answers.
fn prim_rule(
    ctx: &mut ScriptCtx<'_>,
    code: i32,
    prim: &Prim,
    opensim_flavour: bool,
) -> Vec<Element> {
    match code {
        TYPE => shape::type_list(&prim.shape, opensim_flavour),
        MATERIAL => vec![Element::Integer(prim.material)],
        PHYSICS => vec![Element::Integer(i32::from(prim.physics))],
        TEMP_ON_REZ => vec![Element::Integer(i32::from(prim.temp_on_rez))],
        PHANTOM => vec![Element::Integer(i32::from(prim.phantom))],
        POSITION => vec![Element::Vector(prim.position.clone())],
        POS_LOCAL => vec![Element::Vector(prim.local_position.clone())],
        SIZE => vec![Element::Vector(prim.scale.clone())],
        ROTATION => vec![Element::Rotation(prim.rotation.clone())],
        ROT_LOCAL => vec![Element::Rotation(prim.local_rotation.clone())],
        FLEXIBLE => {
            let flexible = &prim.flexible;
            if flexible.on || opensim_flavour {
                vec![
                    Element::Integer(i32::from(flexible.on)),
                    Element::Integer(flexible.softness),
                    Element::Float(flexible.gravity),
                    Element::Float(flexible.friction),
                    Element::Float(flexible.wind),
                    Element::Float(flexible.tension),
                    Element::Vector(flexible.force.clone()),
                ]
            } else {
                vec![
                    Element::Integer(0),
                    Element::Integer(0),
                    Element::Float(0.0),
                    Element::Float(0.0),
                    Element::Float(0.0),
                    Element::Float(0.0),
                    Element::Vector(ZERO),
                ]
            }
        }
        POINT_LIGHT => {
            let light = &prim.light;
            if light.on || opensim_flavour {
                vec![
                    Element::Integer(i32::from(light.on)),
                    Element::Vector(light.color.clone()),
                    Element::Float(light.intensity),
                    Element::Float(light.radius),
                    Element::Float(light.falloff),
                ]
            } else {
                vec![
                    Element::Integer(0),
                    Element::Vector(ZERO),
                    Element::Float(0.0),
                    Element::Float(0.0),
                    Element::Float(0.0),
                ]
            }
        }
        TEXT => vec![
            Element::String(prim.text.text.clone()),
            Element::Vector(prim.text.color.clone()),
            Element::Float(prim.text.alpha),
        ],
        NAME => vec![Element::String(prim.name.clone())],
        DESC => vec![Element::String(prim.description.clone())],
        PHYSICS_SHAPE_TYPE => vec![Element::Integer(prim.physics_shape)],
        OMEGA => vec![
            Element::Vector(prim.omega.axis.clone()),
            Element::Float(prim.omega.spin),
            Element::Float(prim.omega.gain),
        ],
        SLICE => vec![Element::Vector(shape::slice_of(&prim.shape))],
        ALLOW_UNSIT => vec![Element::Integer(i32::from(prim.allow_unsit))],
        SCRIPTED_SIT_ONLY => vec![Element::Integer(i32::from(prim.scripted_sit_only))],
        CLICK_ACTION => vec![Element::Integer(prim.click_action)],
        SIT_TARGET => {
            let target = sit_target(ctx, prim);
            match target {
                Some((offset, rotation)) => vec![
                    Element::Integer(1),
                    Element::Vector(offset),
                    Element::Rotation(rotation),
                ],
                None => vec![
                    Element::Integer(0),
                    Element::Vector(ZERO),
                    Element::Rotation(crate::value::ZERO_ROTATION),
                ],
            }
        }
        SIT_FLAGS => {
            let mut flags = prim.sit_flags;
            if sit_target(ctx, prim).is_some() {
                flags |= SIT_FLAG_SIT_TARGET;
            }
            if prim.allow_unsit {
                flags |= SIT_FLAG_ALLOW_UNSIT;
            }
            if prim.scripted_sit_only {
                flags |= SIT_FLAG_SCRIPTED_ONLY;
            }
            vec![Element::Integer(flags)]
        }
        REFLECTION_PROBE => {
            let probe = &prim.reflection_probe;
            vec![
                Element::Integer(i32::from(probe.on)),
                Element::Float(probe.ambiance),
                Element::Float(probe.clip),
                Element::Integer(probe.flags),
            ]
        }
        DAMAGE => vec![
            Element::Float(prim.damage.0),
            Element::Integer(prim.damage.1),
        ],
        HEALTH => vec![Element::Float(prim.health)],
        COLLISION_SOUND => match &prim.sound.collision {
            Some((sound, volume)) => vec![Element::String(sound.clone()), Element::Float(*volume)],
            None => vec![Element::String(NULL_KEY.to_owned()), Element::Float(0.0)],
        },
        _ => Vec::new(),
    }
}

/// The sit target of `prim`, which the host keeps with the prim's seat.
fn sit_target(ctx: &mut ScriptCtx<'_>, prim: &Prim) -> Option<(Vector, Rotation)> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let link = links
        .prims
        .iter()
        .find(|linked| linked.key == prim.key)
        .map(|linked| linked.link)?;
    ctx.host()
        .seat(caller, link)
        .and_then(|seat| seat.target.clone())
        .map(|target| (target.offset, target.rotation))
}
