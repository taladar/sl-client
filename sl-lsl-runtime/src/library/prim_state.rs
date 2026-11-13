//! Library tranche: reading and writing the prim (`server-lsl-lib-prim-state`)
//! — its placement, its faces through the older per-face calls, its
//! identity and the `PRIM_*` rule lists. The effects (sound, particles,
//! texture animation, media) are [`super::effects`].
//!
//! Every function reads the prim from the host as a [`Prim`], changes it
//! with the grid's rules (`prim_params`) and hands the whole of it
//! back with the `changed` it raises. What the grids measured is the book
//! chapter `gridspec/lsl.md`, § Reading and writing the prim.

use sl_types::lsl::{Rotation, Vector};

use crate::library::prim_params::shape::faces_of;
use crate::library::prim_params::{
    self, ALL_SIDES, Changes, LINK_THIS, faces_set, opensim_offset, opensim_rotation, set_targets,
};
use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY, ZERO_VECTOR};
use crate::vm::ScriptCtx;
use crate::vm::prim::{Face, ObjectDetails, Prim};

use prim_params::set::{
    kept_description, kept_name, normalised, set_face_alpha, set_face_colour, set_omega, set_scale,
    set_text, texture_named, textured,
};

/// The caller's own prim, as the host lends it.
pub(crate) fn own(ctx: &mut ScriptCtx<'_>) -> Option<Prim> {
    let key = prim_params::own_key(ctx)?;
    let caller = ctx.caller();
    ctx.host().prim(caller, &key)
}

/// Hands `prim` back to the host with the `changed` in `changes`.
pub(crate) fn write(ctx: &mut ScriptCtx<'_>, prim: &Prim, changes: &Changes) {
    let events = changes.events(ctx.flavour().is_opensim());
    ctx.set_prim(prim, &events);
}

/// Changes the caller's own prim with `change`, then hands it back.
pub(crate) fn change_own(
    ctx: &mut ScriptCtx<'_>,
    change: impl FnOnce(&mut ScriptCtx<'_>, &mut Prim, &mut Changes),
) {
    let Some(mut prim) = own(ctx) else {
        return;
    };
    let mut changes = Changes::default();
    change(ctx, &mut prim, &mut changes);
    write(ctx, &prim, &changes);
}

/// Changes each prim the link argument `link` names with `change`, then
/// hands each back.
pub(crate) fn change_linked(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    mut change: impl FnMut(&mut ScriptCtx<'_>, &mut Prim, &mut Changes),
) {
    for key in set_targets(ctx, link) {
        let caller = ctx.caller();
        let Some(mut prim) = ctx.host().prim(caller, &key) else {
            continue;
        };
        let mut changes = Changes::default();
        change(ctx, &mut prim, &mut changes);
        write(ctx, &prim, &changes);
    }
}

/// Whether the caller's own prim is a child of a linkset.
fn own_is_child(ctx: &mut ScriptCtx<'_>) -> bool {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    links.this >= 2
}

/// `llGetRot`: the prim's rotation in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_rot(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(own(ctx).map_or(crate::value::ZERO_ROTATION, |prim| prim.rotation))
}

/// `llSetRot`: turns the prim to `rotation`, normalised (`prim/placement`:
/// `<0,0,2,2>` reads `<0,0,0.7071,0.7071>`, the zero rotation the
/// identity) — a root in the region, a child as `PRIM_ROTATION` turns one,
/// to the root's rotation turned by it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_rot(ctx: &mut ScriptCtx<'_>, rotation: Rotation) -> Result<(), CallError> {
    let child = own_is_child(ctx);
    change_own(ctx, |_ctx, prim, _changes| {
        let turn = normalised(&rotation);
        prim.local_rotation = if child {
            crate::ops::compose(&turn, &prim.root_rotation)
        } else {
            turn
        };
    });
    Ok(())
}

/// `llGetLocalRot`: the prim's rotation from its parent — a root's in the
/// region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_local_rot(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(own(ctx).map_or(crate::value::ZERO_ROTATION, |prim| prim.local_rotation))
}

/// `llSetLocalRot`: turns the prim to `rotation` from its parent,
/// normalised (`prim/placement`: of a root, its rotation in the region).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_local_rot(ctx: &mut ScriptCtx<'_>, rotation: Rotation) -> Result<(), CallError> {
    change_own(ctx, |_ctx, prim, _changes| {
        prim.local_rotation = normalised(&rotation);
    });
    Ok(())
}

/// `llGetRootRotation`: the rotation of the prim's linkset root in the
/// region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_root_rotation(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(own(ctx).map_or(crate::value::ZERO_ROTATION, |prim| prim.root_rotation))
}

/// `llGetScale`: the prim's size.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_scale(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(own(ctx).map_or(ZERO_VECTOR, |prim| prim.scale))
}

/// `llSetScale`: resizes the prim, each axis within the grid's limits —
/// 0.01 to 64 m on Second Life, 0.001 to 256 m on OpenSim — raising
/// `CHANGED_SCALE` (on Second Life only for another size; `prim/placement`,
/// `prim/changed`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_scale(ctx: &mut ScriptCtx<'_>, size: Vector) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_own(ctx, |_ctx, prim, changes| {
        set_scale(prim, &size, opensim_flavour, changes);
    });
    Ok(())
}

/// The prims of the caller's linkset, root first.
fn linkset(ctx: &mut ScriptCtx<'_>) -> Vec<Prim> {
    let caller = ctx.caller();
    let keys: Vec<String> = ctx
        .host()
        .links(caller)
        .prims
        .iter()
        .map(|prim| prim.key.clone())
        .collect();
    keys.iter()
        .filter_map(|key| ctx.host().prim(caller, key))
        .collect()
}

/// The size limits of the grid: least and greatest axis.
const fn size_limits(opensim_flavour: bool) -> (f32, f32) {
    if opensim_flavour {
        (0.001, 256.0)
    } else {
        (0.01, 64.0)
    }
}

/// The least and greatest factors the linkset may be scaled by, each prim's
/// axes kept within the grid's sizes.
fn scale_factors(prims: &[Prim], opensim_flavour: bool) -> (f32, f32) {
    let (least, greatest) = size_limits(opensim_flavour);
    prims.iter().fold((0.0_f32, f32::MAX), |(low, high), prim| {
        let smallest = prim.scale.x.min(prim.scale.y).min(prim.scale.z);
        let largest = prim.scale.x.max(prim.scale.y).max(prim.scale.z);
        (low.max(least / smallest), high.min(greatest / largest))
    })
}

/// `llGetMinScaleFactor`: the least factor `llScaleByFactor` takes, the
/// linkset's smallest axis kept at the grid's least size.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_min_scale_factor(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    Ok(scale_factors(&linkset(ctx), opensim_flavour).0)
}

/// `llGetMaxScaleFactor`: the greatest factor `llScaleByFactor` takes.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_max_scale_factor(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    Ok(scale_factors(&linkset(ctx), opensim_flavour).1)
}

/// `llScaleByFactor`: scales the whole linkset by `factor`, each prim's size
/// and each child's offset; `FALSE` and nothing scaled for a factor outside
/// [`ll_get_min_scale_factor`] to [`ll_get_max_scale_factor`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_scale_by_factor(ctx: &mut ScriptCtx<'_>, factor: f32) -> Result<i32, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let prims = linkset(ctx);
    let (low, high) = scale_factors(&prims, opensim_flavour);
    if !(low..=high).contains(&factor) || factor <= 0.0 {
        return Ok(0);
    }
    for (index, mut prim) in prims.into_iter().enumerate() {
        let mut changes = Changes::default();
        let scaled = Vector {
            x: prim.scale.x * factor,
            y: prim.scale.y * factor,
            z: prim.scale.z * factor,
        };
        set_scale(&mut prim, &scaled, opensim_flavour, &mut changes);
        if index > 0 {
            prim.local_position = Vector {
                x: prim.local_position.x * factor,
                y: prim.local_position.y * factor,
                z: prim.local_position.z * factor,
            };
        }
        write(ctx, &prim, &changes);
    }
    Ok(1)
}

/// The corners of a box of half-size `half`, turned by `turn` and moved to
/// `at`.
fn corners(half: &Vector, turn: &Rotation, at: &Vector) -> Vec<Vector> {
    let signs = [-1.0_f32, 1.0];
    let mut points = Vec::new();
    for x in signs {
        for y in signs {
            for z in signs {
                let corner = Vector {
                    x: x * half.x,
                    y: y * half.y,
                    z: z * half.z,
                };
                let turned = crate::ops::rotate(&corner, turn);
                points.push(Vector {
                    x: turned.x + at.x,
                    y: turned.y + at.y,
                    z: turned.z + at.z,
                });
            }
        }
    }
    points
}

/// The least extent of a prim along any axis of its bounding box on Second
/// Life, in metres: a prim 0.01 or 0.04 m thin measures 0.05, one 0.1 m its
/// own (`prim/placement-settle`, aditi 2026-11-08); OpenSim has none.
const SECOND_LIFE_LEAST_BOX: f32 = 0.05;

/// `llGetBoundingBox`: the box around the object one of whose prims is
/// keyed `object`, as its lower and upper corners from its root in the
/// root's frame — for a lone prim the half sizes of its scale, whatever its
/// rotation (`prim/placement`). An avatar's is its size around its centre;
/// a key nothing has answers an empty list.
///
/// Second Life measures each prim at the scale it had when its box last
/// caught up, at most once a second ([`ScriptCtx::box_scale`]), and no axis
/// under 0.05 m (`SECOND_LIFE_LEAST_BOX`); OpenSim at the scale it has,
/// however thin (`prim/placement-settle`, `prim/box-beat`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_bounding_box(
    ctx: &mut ScriptCtx<'_>,
    object: Key,
) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let keys = ctx.host().linkset_of(&object.0);
    if keys.is_empty() {
        let Some(details) = ctx.host().object_details(caller, &object.0) else {
            return Ok(Vec::new());
        };
        let half = Vector {
            x: details.scale.x / 2.0,
            y: details.scale.y / 2.0,
            z: details.scale.z / 2.0,
        };
        return Ok(vec![
            Element::Vector(Vector {
                x: -half.x,
                y: -half.y,
                z: -half.z,
            }),
            Element::Vector(half),
        ]);
    }
    let prims: Vec<Prim> = keys
        .iter()
        .filter_map(|key| ctx.host().prim(caller, key))
        .collect();
    let mut low = Vector {
        x: f32::MAX,
        y: f32::MAX,
        z: f32::MAX,
    };
    let mut high = Vector {
        x: f32::MIN,
        y: f32::MIN,
        z: f32::MIN,
    };
    let least = if ctx.flavour().is_opensim() {
        0.0
    } else {
        SECOND_LIFE_LEAST_BOX
    };
    for (index, prim) in prims.iter().enumerate() {
        let scale = ctx.box_scale(prim);
        let half = Vector {
            x: scale.x.max(least) / 2.0,
            y: scale.y.max(least) / 2.0,
            z: scale.z.max(least) / 2.0,
        };
        let (turn, at) = if index == 0 {
            (crate::value::ZERO_ROTATION, ZERO_VECTOR)
        } else {
            (prim.local_rotation.clone(), prim.local_position.clone())
        };
        for point in corners(&half, &turn, &at) {
            low = Vector {
                x: low.x.min(point.x),
                y: low.y.min(point.y),
                z: low.z.min(point.z),
            };
            high = Vector {
                x: high.x.max(point.x),
                y: high.y.max(point.y),
                z: high.z.max(point.z),
            };
        }
    }
    if prims.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Element::Vector(low), Element::Vector(high)])
}

/// `llGetGeometricCenter`: the mean of the linkset's prims' positions from
/// the root, in the root's frame — zero for a lone prim (`prim/placement`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_geometric_center(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let prims = linkset(ctx);
    let count = crate::num::int_to_float(i32::try_from(prims.len()).unwrap_or(i32::MAX));
    if prims.len() < 2 {
        return Ok(ZERO_VECTOR);
    }
    let sum = prims.iter().skip(1).fold(ZERO_VECTOR, |sum, prim| Vector {
        x: sum.x + prim.local_position.x,
        y: sum.y + prim.local_position.y,
        z: sum.z + prim.local_position.z,
    });
    Ok(Vector {
        x: sum.x / count,
        y: sum.y / count,
        z: sum.z / count,
    })
}

/// `llTargetOmega`: spins the prim about `axis` at `spinrate` radians a
/// second. The spin is the viewer's: the region keeps the parameters, which
/// `PRIM_OMEGA` reads back, and never the angle (`prim/placement`: `llGetRot`
/// does not move).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_target_omega(
    ctx: &mut ScriptCtx<'_>,
    axis: Vector,
    spinrate: f32,
    gain: f32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_own(ctx, |_ctx, prim, _changes| {
        set_omega(prim, axis, spinrate, gain, opensim_flavour);
    });
    Ok(())
}

/// `llGetOmega`: the prim's spin, its axis scaled by its rate.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_omega(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(own(ctx).map_or(ZERO_VECTOR, |prim| Vector {
        x: prim.omega.axis.x * prim.omega.spin,
        y: prim.omega.axis.y * prim.omega.spin,
        z: prim.omega.axis.z * prim.omega.spin,
    }))
}

/// `llGetVel`: the object's velocity, as `OBJECT_VELOCITY` reads it, zero
/// for one that is not physical however it was rezzed (`inventory/rez`'s
/// child, rezzed at 5 m/s, reads zero on aditi and both OpenSim engines).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_vel(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    let object = ctx.host().whereabouts(caller).object;
    Ok(ctx
        .host()
        .object_details(caller, &object)
        .map_or(ZERO_VECTOR, |details| details.velocity))
}

/// The highest a prim may be put, in metres (`prim/placement`: 4000 m
/// moves, 5000 m does not).
const CEILING_M: f32 = 4096.0;

/// `llSetRegionPos`: moves the object so its root stands at `position`, in
/// one move however far; `TRUE` when it got there, `FALSE` when the ground
/// stopped it short or it did not move — past 4096 m up, or outside the
/// region (`prim/placement`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_region_pos(ctx: &mut ScriptCtx<'_>, position: Vector) -> Result<i32, CallError> {
    if position.z > CEILING_M || !position.z.is_finite() {
        return Ok(0);
    }
    let caller = ctx.caller();
    let ended = ctx.host().set_region_position(caller, position.clone());
    Ok(i32::from(ended.is_some_and(|at| at == position)))
}

/// `llSetColor`: the colour of face `face`. Second Life keeps a colour past
/// its range as given for `llGetColor` (`PRIM_COLOR` reads it clamped) and
/// takes −2 as `ALL_SIDES`; OpenSim clamps it into a byte and sets nothing
/// for −2 (`prim/faces`). `CHANGED_COLOR` on every set on Second Life, for
/// another colour on OpenSim (`prim/changed`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_color(ctx: &mut ScriptCtx<'_>, color: Vector, face: i32) -> Result<(), CallError> {
    ll_set_link_color(ctx, LINK_THIS, color, face)
}

/// `llSetLinkColor`: [`ll_set_color`] for the prims `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_color(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    color: Vector,
    face: i32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_linked(ctx, link, |_ctx, prim, changes| {
        for index in faces_set(prim, face, opensim_flavour) {
            if let Some(found) = prim.faces.get_mut(index) {
                set_face_colour(found, &color, opensim_flavour, false, changes);
            }
        }
    });
    Ok(())
}

/// `llSetAlpha`: the alpha of face `face`, clamped; `CHANGED_COLOR` on both
/// grids, the same alpha again included (`prim/faces`, `prim/changed`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_alpha(ctx: &mut ScriptCtx<'_>, alpha: f32, face: i32) -> Result<(), CallError> {
    ll_set_link_alpha(ctx, LINK_THIS, alpha, face)
}

/// `llSetLinkAlpha`: [`ll_set_alpha`] for the prims `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_alpha(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    alpha: f32,
    face: i32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_linked(ctx, link, |_ctx, prim, changes| {
        for index in faces_set(prim, face, opensim_flavour) {
            if let Some(found) = prim.faces.get_mut(index) {
                set_face_alpha(found, alpha, opensim_flavour, changes);
            }
        }
    });
    Ok(())
}

/// Changes face `face` of each prim `link` names with `change`, raising
/// OpenSim's `CHANGED_TEXTURE` where a face changed.
fn change_faces(ctx: &mut ScriptCtx<'_>, link: i32, face: i32, change: impl Fn(&mut Face)) {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_linked(ctx, link, |_ctx, prim, changes| {
        for index in faces_set(prim, face, opensim_flavour) {
            if let Some(found) = prim.faces.get_mut(index) {
                let before = found.clone();
                change(found);
                textured(&before, found, opensim_flavour, changes);
            }
        }
    });
}

/// `llSetTexture`: the texture of face `face`, by key or by the name of a
/// texture in the prim. One that names nothing is not applied: on Second
/// Life with `Could not find texture '…'.`, `NULL_KEY` and `""` included;
/// on OpenSim silently, though `NULL_KEY` is applied there (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_texture(
    ctx: &mut ScriptCtx<'_>,
    texture: String,
    face: i32,
) -> Result<(), CallError> {
    ll_set_link_texture(ctx, LINK_THIS, texture, face)
}

/// `llSetLinkTexture`: [`ll_set_texture`] for the prims `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_texture(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    texture: String,
    face: i32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let found = texture_named(ctx, &texture);
    let found = match found {
        Some(key) if opensim_flavour || key != NULL_KEY => key,
        _ => {
            // Second Life's complaint costs the script a second, on top of
            // the call's forced delay (`errors/flood-paths`' `cost`).
            if !opensim_flavour {
                ctx.refuse(format!("Could not find texture '{texture}'."));
                ctx.error_pause();
            }
            return Ok(());
        }
    };
    change_faces(ctx, link, face, |face| {
        face.texture.texture.clone_from(&found);
    });
    Ok(())
}

/// `llScaleTexture`: the repeats of face `face`, kept as given on both grids
/// (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_scale_texture(
    ctx: &mut ScriptCtx<'_>,
    u: f32,
    v: f32,
    face: i32,
) -> Result<(), CallError> {
    change_faces(ctx, LINK_THIS, face, |face| {
        face.texture.repeats = Vector { x: u, y: v, z: 0.0 };
    });
    Ok(())
}

/// `llOffsetTexture`: the offsets of face `face`, kept on Second Life and
/// within `±1` on OpenSim (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_offset_texture(
    ctx: &mut ScriptCtx<'_>,
    u: f32,
    v: f32,
    face: i32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_faces(ctx, LINK_THIS, face, |face| {
        face.texture.offsets = if opensim_flavour {
            Vector {
                x: opensim_offset(u),
                y: opensim_offset(v),
                z: 0.0,
            }
        } else {
            Vector { x: u, y: v, z: 0.0 }
        };
    });
    Ok(())
}

/// `llRotateTexture`: the texture rotation of face `face`, kept on Second
/// Life and modulo 2π on OpenSim (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rotate_texture(ctx: &mut ScriptCtx<'_>, angle: f32, face: i32) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_faces(ctx, LINK_THIS, face, |face| {
        face.texture.rotation = if opensim_flavour {
            opensim_rotation(angle)
        } else {
            angle
        };
    });
    Ok(())
}

/// The face `face` of the caller's prim for a per-face getter: the face, or
/// [`None`] past the prim's faces.
fn face_of(ctx: &mut ScriptCtx<'_>, face: i32) -> Option<Face> {
    let prim = own(ctx)?;
    let index = if face == ALL_SIDES {
        0
    } else {
        usize::try_from(face).ok()?
    };
    prim.faces.get(index).cloned()
}

/// Whether `face` reads every face: `ALL_SIDES`, and on Second Life any
/// negative face (`prim/faces`: −2 reads as `ALL_SIDES` there).
const fn reads_all(ctx: &ScriptCtx<'_>, face: i32) -> bool {
    face == ALL_SIDES || (face < 0 && !ctx.flavour().is_opensim())
}

/// `llGetColor`: the colour of face `face`; for `ALL_SIDES` the **mean** of
/// the faces' colours. Past the prim's faces Second Life reads white,
/// OpenSim black (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_color(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<Vector, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let missing = if opensim_flavour {
        ZERO_VECTOR
    } else {
        Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }
    };
    if reads_all(ctx, face) {
        let Some(prim) = own(ctx) else {
            return Ok(missing);
        };
        let count = crate::num::int_to_float(i32::try_from(prim.faces.len()).unwrap_or(1).max(1));
        let sum = prim.faces.iter().fold(ZERO_VECTOR, |sum, face| Vector {
            x: sum.x + face.color.x,
            y: sum.y + face.color.y,
            z: sum.z + face.color.z,
        });
        return Ok(Vector {
            x: sum.x / count,
            y: sum.y / count,
            z: sum.z / count,
        });
    }
    Ok(face_of(ctx, face).map_or(missing, |found| found.color))
}

/// `llGetAlpha`: the alpha of face `face`; for `ALL_SIDES` the **sum** of
/// the faces' alphas (6 for an opaque cube). Past the prim's faces Second
/// Life reads 1, OpenSim 0 (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_alpha(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<f32, CallError> {
    let missing = if ctx.flavour().is_opensim() { 0.0 } else { 1.0 };
    if reads_all(ctx, face) {
        return Ok(own(ctx).map_or(missing, |prim| {
            prim.faces.iter().map(|face| face.alpha).sum()
        }));
    }
    Ok(face_of(ctx, face).map_or(missing, |found| found.alpha))
}

/// `llGetTexture`: the texture of face `face` (face 0's for `ALL_SIDES`);
/// `NULL_KEY` past the prim's faces (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_texture(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<String, CallError> {
    Ok(face_of(ctx, face).map_or_else(|| NULL_KEY.to_owned(), |found| found.texture.texture))
}

/// `llGetTextureOffset`: the offsets of face `face`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_texture_offset(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<Vector, CallError> {
    Ok(face_of(ctx, face).map_or(ZERO_VECTOR, |found| found.texture.offsets))
}

/// `llGetTextureScale`: the repeats of face `face`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_texture_scale(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<Vector, CallError> {
    Ok(face_of(ctx, face).map_or(ZERO_VECTOR, |found| found.texture.repeats))
}

/// `llGetTextureRot`: the texture rotation of face `face`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_texture_rot(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<f32, CallError> {
    Ok(face_of(ctx, face).map_or(0.0, |found| found.texture.rotation))
}

/// `llSetText`: the prim's hover text, cut to 254 bytes on both grids, its
/// colour clamped (a byte on OpenSim). Second Life clamps the alpha; OpenSim
/// stores it inverted, so 2 reads 0 and −1 reads 1 (`prim/faces`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_text(
    ctx: &mut ScriptCtx<'_>,
    text: String,
    color: Vector,
    alpha: f32,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let alpha = if opensim_flavour {
        alpha
    } else {
        prim_params::unit(alpha)
    };
    change_own(ctx, |_ctx, prim, _changes| {
        set_text(prim, &text, &color, alpha, opensim_flavour, false);
    });
    Ok(())
}

/// `llGetNumberOfSides`: how many faces the prim has (`prim/faces`: a box 6,
/// hollow 7, a sphere 1, a sculpt 1).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_number_of_sides(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(own(ctx).map_or(0, |prim| faces_of(&prim.shape)))
}

/// `llGetLinkNumberOfSides`: how many faces the prim numbered `link` has, 0
/// for a number naming none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_number_of_sides(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let Some(crate::vm::comms::Linked::Prim(found)) = links.numbered(link) else {
        return Ok(0);
    };
    let key = found.key.clone();
    Ok(ctx
        .host()
        .prim(caller, &key)
        .map_or(0, |prim| faces_of(&prim.shape)))
}

/// `llSetPrimitiveParams`: applies the rule list `rules` to the prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    prim_params::set_params(ctx, "llSetPrimitiveParams", LINK_THIS, &rules);
    Ok(())
}

/// `llSetLinkPrimitiveParams`: applies `rules` to the prims `link` names;
/// the table's 0.2 s forced delay.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    prim_params::set_params(ctx, "llSetLinkPrimitiveParams", link, &rules);
    Ok(())
}

/// `llSetLinkPrimitiveParamsFast`: [`ll_set_link_primitive_params`] without
/// the delay.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_primitive_params_fast(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    prim_params::set_params(ctx, "llSetLinkPrimitiveParamsFast", link, &rules);
    Ok(())
}

/// `llGetPrimitiveParams`: the prim's answer to the rule list `params`.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws: a face it cannot index.
pub fn ll_get_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    prim_params::get_params(ctx, "llGetPrimitiveParams", &params)
}

/// `llGetLinkPrimitiveParams`: the answer to `params` of the prims `link`
/// names — on Second Life a single prim only (`prim/params-rules`,
/// `prim/link`).
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws.
pub fn ll_get_link_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    prim_params::link_get_params(ctx, "llGetLinkPrimitiveParams", link, &params)
}

/// `llGetKey`: the prim's key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_key(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(Key(
        prim_params::own_key(ctx).unwrap_or_else(|| NULL_KEY.to_owned())
    ))
}

/// `llGetCreator`: the key of the prim's creator (`prim/identity`: a prim the
/// owner rezzed names the owner).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_creator(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(Key(
        own(ctx).map_or_else(|| NULL_KEY.to_owned(), |prim| prim.creator)
    ))
}

/// `llGetObjectDesc`: the prim's description.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_desc(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(own(ctx).map(|prim| prim.description).unwrap_or_default())
}

/// `llSetObjectName`: the prim's name — on Second Life cut to 63 bytes, and
/// a pipe, a newline and every byte of a character outside ASCII a `?`;
/// kept whole on OpenSim (`prim/identity`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_object_name(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_own(ctx, |_ctx, prim, _changes| {
        prim.name = kept_name(&name, opensim_flavour);
    });
    Ok(())
}

/// `llSetObjectDesc`: the prim's description, cut to 127 bytes, and on
/// Second Life cleaned as a name is (`prim/identity`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_object_desc(ctx: &mut ScriptCtx<'_>, description: String) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_own(ctx, |_ctx, prim, _changes| {
        prim.description = kept_description(&description, opensim_flavour);
    });
    Ok(())
}

/// `llSetClickAction`: what a left click on the prim does. Second Life
/// ignores an action past `CLICK_ACTION_IGNORE` (9) and below 0; OpenSim
/// keeps any, as the byte it stores (−1 reads 255) (`prim/identity`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_click_action(ctx: &mut ScriptCtx<'_>, action: i32) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_own(ctx, |_ctx, prim, _changes| {
        if opensim_flavour {
            prim.click_action = action.rem_euclid(256);
        } else if (0..=9).contains(&action) {
            prim.click_action = action;
        }
    });
    Ok(())
}

/// `llSetLinkSitFlags`: the sit flags of the prims `link` names, as
/// `PRIM_SIT_FLAGS` sets them.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_sit_flags(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    flags: i32,
) -> Result<(), CallError> {
    prim_params::set_params(
        ctx,
        "llSetLinkSitFlags",
        link,
        &[
            Element::Integer(prim_params::SIT_FLAGS),
            Element::Integer(flags),
        ],
    );
    Ok(())
}

/// `llGetLinkSitFlags`: the sit flags of the prim numbered `link`, as
/// `PRIM_SIT_FLAGS` reads them; 0 for a number naming none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_sit_flags(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<i32, CallError> {
    let answer = prim_params::link_get_params(
        ctx,
        "llGetLinkSitFlags",
        link,
        &[Element::Integer(prim_params::SIT_FLAGS)],
    )?;
    Ok(match answer.first() {
        Some(Element::Integer(flags)) => *flags,
        _ => 0,
    })
}

/// `llSetRenderMaterial`: the glTF render material of face `face`, by key or
/// by the name of a material in the prim; raises `CHANGED_RENDER_MATERIAL`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_render_material(
    ctx: &mut ScriptCtx<'_>,
    material: String,
    face: i32,
) -> Result<(), CallError> {
    ll_set_link_render_material(ctx, LINK_THIS, material, face)
}

/// `llSetLinkRenderMaterial`: [`ll_set_render_material`] for the prims
/// `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_render_material(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    material: String,
    face: i32,
) -> Result<(), CallError> {
    prim_params::set_params(
        ctx,
        "llSetLinkRenderMaterial",
        link,
        &[
            Element::Integer(prim_params::RENDER_MATERIAL),
            Element::Integer(face),
            Element::String(material),
        ],
    );
    Ok(())
}

/// `llGetRenderMaterial`: the render material of face `face`, `NULL_KEY` for
/// none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_render_material(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<String, CallError> {
    Ok(face_of(ctx, face).map_or_else(|| NULL_KEY.to_owned(), |found| found.render_material))
}

/// `llIsLinkGLTFMaterial`: whether face `face` of the prim numbered `link`
/// carries a glTF render material.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_is_link_gltf_material(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    face: i32,
) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let Some(crate::vm::comms::Linked::Prim(found)) = links.numbered(link) else {
        return Ok(0);
    };
    let key = found.key.clone();
    let Some(prim) = ctx.host().prim(caller, &key) else {
        return Ok(0);
    };
    let index = usize::try_from(face).unwrap_or(0);
    Ok(i32::from(
        prim.faces
            .get(index)
            .is_some_and(|found| found.render_material != NULL_KEY),
    ))
}

/// `OVERRIDE_GLTF_*`: a [`ll_set_link_gltf_overrides`] parameter's code.
mod overrides {
    /// `OVERRIDE_GLTF_BASE_COLOR_FACTOR`.
    pub(super) const BASE_COLOR: i32 = 1;
    /// `OVERRIDE_GLTF_BASE_ALPHA`.
    pub(super) const BASE_ALPHA: i32 = 2;
    /// `OVERRIDE_GLTF_BASE_ALPHA_MODE`.
    pub(super) const ALPHA_MODE: i32 = 3;
    /// `OVERRIDE_GLTF_BASE_ALPHA_MASK`.
    pub(super) const ALPHA_MASK: i32 = 4;
    /// `OVERRIDE_GLTF_BASE_DOUBLE_SIDED`.
    pub(super) const DOUBLE_SIDED: i32 = 5;
    /// `OVERRIDE_GLTF_METALLIC_FACTOR`.
    pub(super) const METALLIC: i32 = 6;
    /// `OVERRIDE_GLTF_ROUGHNESS_FACTOR`.
    pub(super) const ROUGHNESS: i32 = 7;
    /// `OVERRIDE_GLTF_EMISSIVE_FACTOR`.
    pub(super) const EMISSIVE: i32 = 8;
}

/// A float override value.
const fn float_of(element: &Element) -> Option<f32> {
    match element {
        Element::Float(value) => Some(*value),
        _ => None,
    }
}

/// `llSetLinkGLTFOverrides`: sets the glTF overrides `params` names —
/// `OVERRIDE_GLTF_*` codes, each followed by its value — on face `face` of
/// the prims `link` names. A pair whose value has the wrong type is
/// skipped.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_gltf_overrides(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    face: i32,
    params: Vec<Element>,
) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    change_linked(ctx, link, |_ctx, prim, _changes| {
        for index in faces_set(prim, face, opensim_flavour) {
            let Some(found) = prim.faces.get_mut(index) else {
                continue;
            };
            let gltf = &mut found.gltf;
            for pair in params.chunks(2) {
                let [Element::Integer(code), value] = pair else {
                    continue;
                };
                let integer = match value {
                    Element::Integer(value) => Some(*value),
                    _ => None,
                };
                let float = float_of(value).or_else(|| integer.map(crate::num::int_to_float));
                let vector = match value {
                    Element::Vector(vector) => Some(vector.clone()),
                    _ => None,
                };
                match *code {
                    overrides::BASE_COLOR => gltf.base_color = vector,
                    overrides::BASE_ALPHA => gltf.base_alpha = float,
                    overrides::ALPHA_MODE => gltf.alpha_mode = integer,
                    overrides::ALPHA_MASK => gltf.alpha_cutoff = float,
                    overrides::DOUBLE_SIDED => gltf.double_sided = integer.map(|value| value != 0),
                    overrides::METALLIC => gltf.metallic = float,
                    overrides::ROUGHNESS => gltf.roughness = float,
                    overrides::EMISSIVE => gltf.emissive = vector,
                    _ => {}
                }
            }
        }
    });
    Ok(())
}

/// The answer to one `OBJECT_*` code of `llGetObjectDetails` for
/// `details`: one value, or five for the permission codes, or
/// `OBJECT_UNKNOWN_DETAIL` (−1) for a code the grid has no answer to —
/// OpenSim has none for codes 42 to 56 (`prim/identity`). `link` is the
/// object's link number as the caller sees it.
fn detail(details: &ObjectDetails, code: i32, opensim_flavour: bool, link: i32) -> Vec<Element> {
    let integer = |value: i32| vec![Element::Integer(value)];
    let float = |value: f32| vec![Element::Float(value)];
    let key = |value: &str| vec![Element::Key(value.to_owned())];
    let unknown = vec![Element::Integer(-1)];
    if opensim_flavour && (42..=56).contains(&code) {
        return unknown;
    }
    let (land, streaming, physics, server) = details.costs;
    let (physical, phantom, temporary) = details.flags;
    let masks = details
        .masks
        .iter()
        .map(|mask| Element::Integer(*mask))
        .collect::<Vec<_>>();
    match code {
        1 => vec![Element::String(details.name.clone())],
        2 => vec![Element::String(details.description.clone())],
        3 => vec![Element::Vector(details.position.clone())],
        4 => vec![Element::Rotation(details.rotation.clone())],
        5 => vec![Element::Vector(details.velocity.clone())],
        6 => key(&details.owner),
        7 => key(&details.group),
        8 => key(&details.creator),
        9 => integer(details.scripts.1),
        10 => integer(details.scripts.0),
        11 => integer(details.script_memory),
        12 | 17 | 50 | 51 => float(0.0),
        13 => integer(crate::num::float_to_int(land.ceil())),
        14 => float(server),
        15 => float(streaming),
        16 => float(physics),
        18 => key(&details.root),
        19 => integer(details.attached_point),
        20 | 24 | 34 | 35 | 37 | 39 | 40 | 55 | 56 => integer(match code {
            37 => details.selected.0,
            39 => details.animated,
            _ => 0,
        }),
        21 => integer(i32::from(physical)),
        22 => integer(i32::from(phantom)),
        23 => integer(i32::from(temporary)),
        25 => float(details.hover_height),
        26 => float(details.body_shape),
        27 => key(&details.last_owner),
        28 => integer(details.click_action),
        29 => vec![Element::Vector(details.omega.clone())],
        30 => integer(details.prim_count),
        31 => integer(details.inventory_count),
        32 => key(&details.rezzer),
        33 => vec![Element::String(String::new())],
        36 | 45 => vec![Element::String(details.rezzed.map_or_else(
            String::new,
            |micros| {
                // Second Life's six digits of the second, OpenSim's none
                // (`prim/placement-settle`'s `home` record on each grid).
                crate::library::calendar::timestamp(micros, if opensim_flavour { 0 } else { 6 })
            },
        ))],
        38 => integer(details.selected.1),
        41 => integer(if details.avatar { 1 } else { -1 }),
        42 => integer(details.material),
        43 => float(details.scale.x * details.scale.y * details.scale.z * 1000.0),
        44 => vec![Element::String(details.text.text.clone())],
        46 => integer(link),
        47 => vec![Element::Vector(details.scale.clone())],
        48 => vec![Element::Vector(details.text.color.clone())],
        49 => float(details.text.alpha),
        52 => integer(0),
        53 | 54 => masks,
        _ => unknown,
    }
}

/// `llGetObjectDetails`: what `params` asks of the object or avatar keyed
/// `id`, code by code in the order asked. An empty list for `NULL_KEY`, a key
/// nothing in the region has, text that is no key, and no codes; −1
/// (`OBJECT_UNKNOWN_DETAIL`) in the place of a code the grid does not answer
/// (`prim/identity`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_details(
    ctx: &mut ScriptCtx<'_>,
    id: Key,
    params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    if params.is_empty() || id.0 == NULL_KEY || !crate::library::dotnet::is_uuid(&id.0) {
        return Ok(Vec::new());
    }
    let caller = ctx.caller();
    let Some(details) = ctx.host().object_details(caller, &id.0) else {
        return Ok(Vec::new());
    };
    let opensim_flavour = ctx.flavour().is_opensim();
    let links = ctx.host().links(caller);
    let link = links
        .prims
        .iter()
        .find(|prim| prim.key.eq_ignore_ascii_case(&id.0))
        .map_or(0, |prim| prim.link);
    Ok(params
        .iter()
        .flat_map(|code| match code {
            Element::Integer(code) => detail(&details, *code, opensim_flavour, link),
            _ => vec![Element::Integer(-1)],
        })
        .collect())
}

/// `STATUS_PHYSICS`.
const STATUS_PHYSICS: i32 = 1;
/// `STATUS_PHANTOM`.
const STATUS_PHANTOM: i32 = 16;
/// `STATUS_CAST_SHADOWS`.
const STATUS_CAST_SHADOWS: i32 = 512;
/// Every `STATUS_*` flag, `STATUS_PHYSICS` to `STATUS_BLOCK_GRAB_OBJECT`.
const STATUS_ALL: i32 = 0x7ff;

/// The prim's `STATUS_*` flags, physics and phantom included; a prim under
/// `llVolumeDetect` reads as phantom.
const fn status_of(prim: &Prim) -> i32 {
    let mut status = prim.status;
    if prim.physics {
        status |= STATUS_PHYSICS;
    }
    if prim.phantom || prim.volume_detect {
        status |= STATUS_PHANTOM;
    }
    status
}

/// `llSetStatus`: sets the `STATUS_*` flags in `mask` on the prim, or clears
/// them for a `value` of 0 (any other value sets). `STATUS_CAST_SHADOWS`
/// cannot be set on Second Life, and is set for good by either value on
/// OpenSim; a phantom clear leaves a prim under `llVolumeDetect` phantom
/// (`physics/status`, on every grid). A physical prim is moved by the
/// host's physics.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_status(ctx: &mut ScriptCtx<'_>, mask: i32, value: i32) -> Result<(), CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let on = value != 0;
    change_own(ctx, |_ctx, prim, _changes| {
        let mask = mask & STATUS_ALL;
        if mask & STATUS_PHYSICS != 0 {
            prim.physics = on;
        }
        if mask & STATUS_PHANTOM != 0 && (on || !prim.volume_detect) {
            prim.phantom = on;
        }
        let rest = mask & !(STATUS_PHYSICS | STATUS_PHANTOM | STATUS_CAST_SHADOWS);
        if on {
            prim.status |= rest;
        } else {
            prim.status &= !rest;
        }
        if mask & STATUS_CAST_SHADOWS != 0 && opensim_flavour {
            prim.status |= STATUS_CAST_SHADOWS;
        }
    });
    Ok(())
}

/// `llGetStatus`: whether the prim has the `STATUS_*` flags of `mask`. On
/// Second Life `TRUE` if it has any of them, and `FALSE` for a mask with a
/// bit no flag has (`-1`, `2048`); on OpenSim `FALSE` for any mask that is
/// not exactly one flag (`physics/status`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_status(ctx: &mut ScriptCtx<'_>, mask: i32) -> Result<i32, CallError> {
    let Some(prim) = own(ctx) else {
        return Ok(0);
    };
    let status = status_of(&prim);
    let known = mask != 0 && mask & !STATUS_ALL == 0;
    let asked = if ctx.flavour().is_opensim() {
        known && mask.count_ones() == 1
    } else {
        known
    };
    Ok(i32::from(asked && status & mask != 0))
}

/// `llVolumeDetect`: on, the prim reads as phantom, through `STATUS_PHANTOM`
/// and `PRIM_PHANTOM`, and a phantom clear leaves it so; off, it is not
/// phantom, even if it was before (`physics/status`, on every grid). The
/// host turns what passes through it into `collision_start` and
/// `collision_end`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_volume_detect(ctx: &mut ScriptCtx<'_>, detect: i32) -> Result<(), CallError> {
    change_own(ctx, |_ctx, prim, _changes| {
        prim.volume_detect = detect != 0;
        prim.phantom = prim.volume_detect;
    });
    Ok(())
}

/// `llSetBuoyancy`: the share of gravity the object no longer feels while it
/// is physical — 0 falls, 1 floats (`physics/targets` makes its prim
/// weightless so), more rises.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_buoyancy(ctx: &mut ScriptCtx<'_>, buoyancy: f32) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().set_buoyancy(caller, buoyancy);
    Ok(())
}
