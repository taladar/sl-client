//! `PRIM_TYPE` and `PRIM_SLICE` against a prim's volume parameters, each
//! grid's way, and how many faces a shape has (`llGetNumberOfSides`).
//!
//! Second Life keeps the parameters as floats and clamps them as Linden
//! Lab's `LLVolumeParams` setters do (`llvolume.cpp`) — but with the
//! server's older limits, hollow at most 0.95 and a hole at least 0.05
//! across, which the viewer's copy has since relaxed: the `prim/params` rows
//! (book chapter `gridspec/lsl.md`, § `PRIM_*` round trips) are those
//! setters to the digit, the profile cut truncated to five decimals
//! included. OpenSim converts each value to the byte or the 1/50000th it
//! travels in (`LSL_Api.SetPrimitiveShapeParams`), so a value it reads back
//! is that byte's.

use sl_types::lsl::Vector;

use crate::value::Element;
use crate::vm::prim::{Sculpt, Shape, path, profile};

/// `PRIM_TYPE_BOX`.
pub(crate) const TYPE_BOX: i32 = 0;
/// `PRIM_TYPE_CYLINDER`.
pub(crate) const TYPE_CYLINDER: i32 = 1;
/// `PRIM_TYPE_PRISM`.
pub(crate) const TYPE_PRISM: i32 = 2;
/// `PRIM_TYPE_SPHERE`.
pub(crate) const TYPE_SPHERE: i32 = 3;
/// `PRIM_TYPE_TORUS`.
pub(crate) const TYPE_TORUS: i32 = 4;
/// `PRIM_TYPE_TUBE`.
pub(crate) const TYPE_TUBE: i32 = 5;
/// `PRIM_TYPE_RING`.
pub(crate) const TYPE_RING: i32 = 6;
/// `PRIM_TYPE_SCULPT`.
pub(crate) const TYPE_SCULPT: i32 = 7;

/// The hole shapes a script may give (`PRIM_HOLE_*`), the high nibble of
/// the profile curve.
const HOLES: [i32; 4] = [0x00, 0x10, 0x20, 0x30];
/// `PRIM_HOLE_SQUARE`.
const HOLE_SQUARE: u8 = 0x20;

/// `PRIM_SCULPT_TYPE_MASK`: the sculpt type without its flags.
const SCULPT_TYPE_MASK: i32 = 0x07;
/// `PRIM_SCULPT_TYPE_SPHERE`.
const SCULPT_SPHERE: i32 = 1;
/// `PRIM_SCULPT_TYPE_MESH`.
const SCULPT_MESH: i32 = 5;
/// `PRIM_SCULPT_FLAG_INVERT | PRIM_SCULPT_FLAG_MIRROR`.
const SCULPT_FLAGS: i32 = 0xc0;

/// How close a cut's begin may come to its end (`MIN_CUT_DELTA`).
const MIN_CUT_DELTA: f32 = 0.02;

/// The shape a `PRIM_TYPE` rule asked for, its values as given.
#[derive(Debug, Clone)]
pub(crate) struct Asked {
    /// `PRIM_TYPE_*`.
    pub(crate) kind: i32,
    /// The hole shape, `PRIM_HOLE_*`.
    pub(crate) hole: i32,
    /// The cut.
    pub(crate) cut: Vector,
    /// The hollow.
    pub(crate) hollow: f32,
    /// The twist.
    pub(crate) twist: Vector,
    /// A straight path's top size; a circular path's hole size.
    pub(crate) scale: Vector,
    /// The top shear.
    pub(crate) shear: Vector,
    /// A sphere's dimple; a circular path's profile cut.
    pub(crate) profile_cut: Vector,
    /// A circular path's taper.
    pub(crate) taper: Vector,
    /// The revolutions.
    pub(crate) revolutions: f32,
    /// The radius offset.
    pub(crate) radius_offset: f32,
    /// The skew.
    pub(crate) skew: f32,
}

/// The profile and path of a parametric `PRIM_TYPE_*`.
const fn curves(kind: i32) -> Option<(u8, u8)> {
    match kind {
        TYPE_BOX => Some((profile::SQUARE, path::LINE)),
        TYPE_CYLINDER => Some((profile::CIRCLE, path::LINE)),
        TYPE_PRISM => Some((profile::EQUALTRI, path::LINE)),
        TYPE_SPHERE => Some((profile::CIRCLE_HALF, path::CIRCLE)),
        TYPE_TORUS => Some((profile::CIRCLE, path::CIRCLE)),
        TYPE_TUBE => Some((profile::SQUARE, path::CIRCLE)),
        TYPE_RING => Some((profile::EQUALTRI, path::CIRCLE)),
        _ => None,
    }
}

/// Whether `hole` is a hole shape a script may give.
pub(crate) fn hole_is_known(hole: i32) -> bool {
    HOLES.contains(&hole)
}

/// The `PRIM_TYPE_*` of a shape, as both grids read it back
/// (`SceneObjectPart.GetPrimType`).
#[must_use]
pub(crate) const fn kind_of(shape: &Shape) -> i32 {
    if shape.sculpt.is_some() {
        return TYPE_SCULPT;
    }
    let straight = matches!(shape.path_curve, path::LINE | path::FLEXIBLE);
    let circular = shape.path_curve == path::CIRCLE;
    match shape.profile_curve & 0x07 {
        profile::SQUARE if circular => TYPE_TUBE,
        profile::CIRCLE if straight => TYPE_CYLINDER,
        profile::CIRCLE if circular => TYPE_TORUS,
        profile::CIRCLE_HALF if circular || shape.path_curve == path::CIRCLE2 => TYPE_SPHERE,
        profile::EQUALTRI if straight => TYPE_PRISM,
        profile::EQUALTRI if circular => TYPE_RING,
        _ => TYPE_BOX,
    }
}

/// `value` clamped to `[min, max]`.
const fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.clamp(min, max)
}

/// LL's `LLProfileParams` truncation of a profile value to five decimals.
fn five_decimals(value: f32) -> f32 {
    crate::num::int_to_float(crate::num::float_to_int(value * 100_000.0)) / 100_000.0
}

/// A cut as `LLVolumeParams::setBeginAndEnd*` makes it: both ends in range,
/// the begin at least [`MIN_CUT_DELTA`] below the end.
fn ll_cut(begin: f32, end: f32) -> (f32, f32) {
    let mut begin = clamp(begin, 0.0, 1.0 - MIN_CUT_DELTA);
    let mut end = end;
    if (0.0149..MIN_CUT_DELTA).contains(&end) {
        end = MIN_CUT_DELTA;
    }
    end = clamp(end, MIN_CUT_DELTA, 1.0);
    begin = clamp(begin, 0.0, end - MIN_CUT_DELTA);
    (begin, end)
}

/// A profile cut as Second Life stores it ([`ll_cut`], truncated).
fn ll_profile_cut(begin: f32, end: f32) -> (f32, f32) {
    let (begin, end) = ll_cut(begin, end);
    let begin = if begin >= 1.0 {
        0.0
    } else {
        five_decimals(begin)
    };
    let end = if end <= 0.0 { 1.0 } else { five_decimals(end) };
    (begin, end)
}

/// Second Life's hollow limit for a profile and hole (the server's: 0.95,
/// and 0.7 for a square hole in a round or triangular profile).
const fn ll_hollow_max(profile_curve: u8) -> f32 {
    let hole = profile_curve & profile::HOLE_MASK;
    let shape = profile_curve & profile::MASK;
    if hole == HOLE_SQUARE
        && matches!(
            shape,
            profile::CIRCLE | profile::CIRCLE_HALF | profile::EQUALTRI
        )
    {
        0.7
    } else {
        0.95
    }
}

/// `LLVolumeParams::setRatio`: a straight path's top size within `[0, 2]`,
/// a circular path's hole within the server's `[0.05, 1] × [0.05, 0.5]`.
const fn ll_ratio(shape: &Shape, x: f32, y: f32) -> [f32; 2] {
    let hole = shape.path_curve == path::CIRCLE
        && shape.profile_curve & profile::MASK != profile::CIRCLE_HALF;
    if hole {
        [clamp(x, 0.05, 1.0), clamp(y, 0.05, 0.5)]
    } else {
        [clamp(x, 0.0, 2.0), clamp(y, 0.0, 2.0)]
    }
}

/// `LLVolumeParams::setRadiusOffset`: zero off a torus-like path, and
/// within what the hole and the taper leave.
fn ll_radius_offset(shape: &Shape, offset: f32) -> f32 {
    if shape.profile_curve & profile::MASK == profile::CIRCLE_HALF
        || shape.path_curve != path::CIRCLE
    {
        return 0.0;
    }
    let [_, taper_y] = shape.taper;
    let [_, hole_y] = shape.scale;
    let hole_y_mag = hole_y.abs();
    let taper_y_mag = if (offset > 0.0 && taper_y < 0.0) || (offset < 0.0 && taper_y > 0.0) {
        0.0
    } else {
        taper_y.abs()
    };
    let max = 1.0 - hole_y_mag * (1.0 - taper_y_mag) / (1.0 - hole_y_mag);
    if max - offset.abs() < 0.0 {
        if offset < 0.0 { -max } else { max }
    } else {
        offset
    }
}

/// `LLVolumeParams::setSkew`: within `[−0.95, 0.95]` and no smaller than
/// the revolutions and hole allow.
fn ll_skew(shape: &Shape, skew: f32) -> f32 {
    let skew = clamp(skew, -0.95, 0.95);
    let [scale_x, _] = shape.scale;
    let min = if (shape.revolutions - 1.0).abs() < 0.001 {
        0.0
    } else {
        1.0 - 1.0 / shape.revolutions.mul_add(scale_x, 1.0)
    };
    if skew.abs() - min < 0.0 {
        if skew < 0.0 { -min } else { min }
    } else {
        skew
    }
}

/// `value` as the double OpenSim holds for it: OpenSim's vectors keep the
/// double a literal parsed to, where ours keep an `f32`, so the `f32` is
/// widened through its shortest decimal form (0.2 stays 0.2, not
/// 0.20000000298) and the 1/50000ths come out as OpenSim's do.
fn as_written(value: f32) -> f64 {
    value
        .to_string()
        .parse()
        .unwrap_or_else(|_| f64::from(value))
}

/// OpenSim's `(ushort)(50000 * value)` of a double, as its 1/50000ths.
fn os_quanta(value: f64) -> f32 {
    let quanta = crate::num::double_to_long(50_000.0 * value).clamp(0, 0xFFFF);
    crate::num::int_to_float(i32::try_from(quanta).unwrap_or(0))
}

/// OpenSim's `(ushort)(50000 * value)`, read back. OpenSim reckons in
/// doubles, so a begin of `0.51 - 0.02` stays 24500 quanta where an `f32`
/// product falls to 24499.
fn os_cut_begin(value: f64) -> f32 {
    os_quanta(value) / 50_000.0
}

/// OpenSim's `(ushort)(50000 * (1 - value))`, read back as the end.
fn os_cut_end(value: f64) -> f32 {
    1.0 - os_quanta(1.0 - value) / 50_000.0
}

/// OpenSim's rounding of `100 * value` to a byte, away from zero, read back
/// as hundredths.
fn os_hundredths(value: f32) -> f32 {
    let scaled = 100.0 * value;
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    crate::num::int_to_float(crate::num::float_to_int(rounded)) / 100.0
}

/// OpenSim's cut clamp (`SetPrimitiveBlockShapeParams`): both ends in
/// `[0, 1]` and the begin pulled down to 0.02 below the end.
fn os_cut(begin: f32, end: f32) -> (f32, f32) {
    let mut begin = as_written(clamp(begin, 0.0, 1.0));
    let mut end = as_written(clamp(end, 0.0, 1.0));
    let delta = f64::from(MIN_CUT_DELTA);
    if end - begin < delta {
        begin = end - delta;
        if begin < 0.0 {
            begin = 0.0;
            end = delta;
        }
    }
    (os_cut_begin(begin), os_cut_end(end))
}

/// The shape a `PRIM_TYPE` rule makes of `current` on Second Life, or
/// [`None`] for a type Second Life does not have.
fn second_life(current: &Shape, asked: &Asked) -> Option<Shape> {
    let (profile_shape, path_curve) = curves(asked.kind)?;
    let hole = u8::try_from(asked.hole)
        .ok()
        .filter(|_| hole_is_known(asked.hole))
        .unwrap_or(0);
    let mut shape = Shape {
        profile_curve: profile_shape | hole,
        path_curve,
        sculpt: None,
        ..current.clone()
    };
    let circular = path_curve == path::CIRCLE;
    let (cut_begin, cut_end) = (asked.cut.x, asked.cut.y);
    if circular {
        let (begin, end) = ll_cut(cut_begin, cut_end);
        shape.path_begin = begin;
        shape.path_end = end;
        let (begin, end) = ll_profile_cut(asked.profile_cut.x, asked.profile_cut.y);
        shape.profile_begin = begin;
        shape.profile_end = end;
    } else {
        let (begin, end) = ll_profile_cut(cut_begin, cut_end);
        shape.profile_begin = begin;
        shape.profile_end = end;
    }
    shape.hollow = five_decimals(clamp(asked.hollow, 0.0, ll_hollow_max(shape.profile_curve)));
    shape.twist_begin = clamp(asked.twist.x, -1.0, 1.0);
    shape.twist_end = clamp(asked.twist.y, -1.0, 1.0);
    if asked.kind == TYPE_SPHERE {
        shape.scale = [1.0, 1.0];
        shape.shear = [0.0, 0.0];
    } else {
        shape.scale = ll_ratio(&shape, asked.scale.x, asked.scale.y);
        shape.shear = [
            clamp(asked.shear.x, -0.5, 0.5),
            clamp(asked.shear.y, -0.5, 0.5),
        ];
    }
    if circular && asked.kind != TYPE_SPHERE {
        shape.taper = [
            clamp(asked.taper.x, -1.0, 1.0),
            clamp(asked.taper.y, -1.0, 1.0),
        ];
        shape.revolutions = clamp(asked.revolutions, 1.0, 4.0);
        shape.radius_offset = ll_radius_offset(&shape, asked.radius_offset);
        shape.skew = ll_skew(&shape, asked.skew);
    } else {
        shape.taper = [0.0, 0.0];
        shape.revolutions = 1.0;
        shape.radius_offset = 0.0;
        shape.skew = 0.0;
    }
    Some(shape)
}

/// The shape a `PRIM_TYPE` rule makes of `current` on OpenSim
/// (`LSL_Api.SetPrimitiveShapeParams`), or [`None`] for a type OpenSim
/// does not have.
fn opensim(current: &Shape, asked: &Asked) -> Option<Shape> {
    let (profile_shape, path_curve) = curves(asked.kind)?;
    let hole = u8::try_from(asked.hole)
        .ok()
        .filter(|_| hole_is_known(asked.hole))
        .unwrap_or(0);
    let mut shape = Shape {
        profile_curve: profile_shape | hole,
        path_curve,
        sculpt: None,
        ..current.clone()
    };
    let (begin, end) = os_cut(asked.cut.x, asked.cut.y);
    let hollow_max = if profile_shape != profile::SQUARE && hole == HOLE_SQUARE {
        0.70
    } else {
        0.99
    };
    shape.hollow = os_cut_begin(as_written(clamp(asked.hollow, 0.0, hollow_max)));
    shape.twist_begin = os_hundredths(clamp(asked.twist.x, -1.0, 1.0));
    shape.twist_end = os_hundredths(clamp(asked.twist.y, -1.0, 1.0));
    let circular = path_curve == path::CIRCLE;
    if circular {
        shape.path_begin = begin;
        shape.path_end = end;
    } else {
        shape.profile_begin = begin;
        shape.profile_end = end;
    }
    match asked.kind {
        TYPE_SPHERE => {
            shape.scale = [1.0, 1.0];
            let (begin, end) = os_cut(asked.profile_cut.x, asked.profile_cut.y);
            shape.profile_begin = begin;
            shape.profile_end = end;
        }
        TYPE_TORUS | TYPE_TUBE | TYPE_RING => {
            shape.scale = [
                2.0 - os_hundredths(2.0 - clamp(asked.scale.x, 0.01, 1.0)),
                2.0 - os_hundredths(2.0 - clamp(asked.scale.y, 0.01, 0.5)),
            ];
            shape.shear = [
                os_hundredths(clamp(asked.shear.x, -0.5, 0.5)),
                os_hundredths(clamp(asked.shear.y, -0.5, 0.5)),
            ];
            let (begin, end) = os_cut(asked.profile_cut.x, asked.profile_cut.y);
            shape.profile_begin = begin;
            shape.profile_end = end;
            shape.taper = [
                os_hundredths(clamp(asked.taper.x, -1.0, 1.0)),
                os_hundredths(clamp(asked.taper.y, -1.0, 1.0)),
            ];
            let revolutions = clamp(asked.revolutions, 1.0, 4.0);
            let quanta = crate::num::float_to_int(66.666_67_f32.mul_add(revolutions - 1.0, 0.5));
            shape.revolutions = crate::num::int_to_float(quanta).mul_add(0.015, 1.0);
            shape.radius_offset = os_hundredths(clamp(asked.radius_offset, 0.0, 1.0));
            shape.skew = os_hundredths(clamp(asked.skew, -0.95, 0.95));
        }
        _ => {
            shape.scale = [
                2.0 - os_hundredths(2.0 - clamp(asked.scale.x, 0.0, 2.0)),
                2.0 - os_hundredths(2.0 - clamp(asked.scale.y, 0.0, 2.0)),
            ];
            shape.shear = [
                os_hundredths(clamp(asked.shear.x, -0.5, 0.5)),
                os_hundredths(clamp(asked.shear.y, -0.5, 0.5)),
            ];
        }
    }
    if !circular || asked.kind == TYPE_SPHERE {
        shape.taper = [0.0, 0.0];
        shape.revolutions = 1.0;
        shape.radius_offset = 0.0;
        shape.skew = 0.0;
    }
    Some(shape)
}

/// The shape a parametric `PRIM_TYPE` rule makes of `current`, the way the
/// imitated grid makes it; [`None`] for a type that has no such shape.
pub(crate) fn shaped(current: &Shape, asked: &Asked, opensim_flavour: bool) -> Option<Shape> {
    if opensim_flavour {
        opensim(current, asked)
    } else {
        second_life(current, asked)
    }
}

/// The shape `PRIM_TYPE_SCULPT` makes of `current` with the map `map` and
/// the sculpt type `kind`: Second Life keeps a type it has no shape for,
/// OpenSim makes it a sphere (`kind | PRIM_SCULPT_TYPE_SPHERE`).
pub(crate) fn sculpted(current: &Shape, map: String, kind: i32, opensim_flavour: bool) -> Shape {
    let flags = kind & SCULPT_FLAGS;
    let known = [1, 2, 3, 4].iter().any(|base| kind == base | flags);
    let kind = if opensim_flavour && !known {
        kind | SCULPT_SPHERE
    } else {
        kind
    };
    Shape {
        profile_curve: profile::CIRCLE,
        path_curve: path::CIRCLE,
        scale: [1.0, 0.5],
        sculpt: Some(Sculpt { map, kind }),
        ..current.clone()
    }
}

/// What `PRIM_SLICE` makes of `current`: the path's cut for a straight
/// path, the profile's for a sphere, torus, tube or ring — on Second Life
/// as `LLVolumeParams` clamps it, on OpenSim as `UpdateSlice` does, which
/// swaps a reversed slice round.
pub(crate) fn sliced(current: &Shape, slice: &Vector, opensim_flavour: bool) -> Shape {
    let on_profile = matches!(
        kind_of(current),
        TYPE_SPHERE | TYPE_TORUS | TYPE_TUBE | TYPE_RING
    );
    let (begin, end) = if opensim_flavour {
        let (low, high) = if slice.y < slice.x {
            (slice.y, slice.x)
        } else {
            (slice.x, slice.y)
        };
        let delta = f64::from(MIN_CUT_DELTA);
        let mut end = as_written(clamp(high, 0.0, 1.0));
        let mut begin = as_written(clamp(low, 0.0, 1.0)).min(end - delta);
        if begin < delta && end < delta {
            begin = 0.0;
            end = delta;
        }
        (os_cut_begin(begin), os_cut_end(end))
    } else if on_profile {
        ll_profile_cut(slice.x, slice.y)
    } else {
        ll_cut(slice.x, slice.y)
    };
    let mut shape = current.clone();
    if on_profile {
        shape.profile_begin = begin;
        shape.profile_end = end;
    } else {
        shape.path_begin = begin;
        shape.path_end = end;
    }
    shape
}

/// `PRIM_SLICE` read back.
pub(crate) const fn slice_of(shape: &Shape) -> Vector {
    if matches!(
        kind_of(shape),
        TYPE_SPHERE | TYPE_TORUS | TYPE_TUBE | TYPE_RING
    ) {
        Vector {
            x: shape.profile_begin,
            y: shape.profile_end,
            z: 0.0,
        }
    } else {
        Vector {
            x: shape.path_begin,
            y: shape.path_end,
            z: 0.0,
        }
    }
}

/// A pair as a vector with a zero third component.
const fn pair(x: f32, y: f32) -> Vector {
    Vector { x, y, z: 0.0 }
}

/// What `llGetPrimitiveParams([PRIM_TYPE])` answers for `shape`. OpenSim
/// rounds the revolutions it reads back to two decimals.
pub(crate) fn type_list(shape: &Shape, opensim_flavour: bool) -> Vec<Element> {
    let kind = kind_of(shape);
    if let Some(sculpt) = &shape.sculpt {
        return vec![
            Element::Integer(TYPE_SCULPT),
            Element::String(sculpt.map.clone()),
            Element::Integer(sculpt.kind),
        ];
    }
    let hole = i32::from(shape.profile_curve & profile::HOLE_MASK);
    let circular = matches!(kind, TYPE_SPHERE | TYPE_TORUS | TYPE_TUBE | TYPE_RING);
    let cut = if circular {
        pair(shape.path_begin, shape.path_end)
    } else {
        pair(shape.profile_begin, shape.profile_end)
    };
    let [scale_x, scale_y] = shape.scale;
    let [shear_x, shear_y] = shape.shear;
    let [taper_x, taper_y] = shape.taper;
    let mut list = vec![
        Element::Integer(kind),
        Element::Integer(hole),
        Element::Vector(cut),
        Element::Float(shape.hollow),
        Element::Vector(pair(shape.twist_begin, shape.twist_end)),
    ];
    match kind {
        TYPE_SPHERE => {
            list.push(Element::Vector(pair(
                shape.profile_begin,
                shape.profile_end,
            )));
        }
        TYPE_TORUS | TYPE_TUBE | TYPE_RING => {
            let revolutions = if opensim_flavour {
                (shape.revolutions * 100.0).round() / 100.0
            } else {
                shape.revolutions
            };
            list.extend([
                Element::Vector(pair(scale_x, scale_y)),
                Element::Vector(pair(shear_x, shear_y)),
                Element::Vector(pair(shape.profile_begin, shape.profile_end)),
                Element::Vector(pair(taper_x, taper_y)),
                Element::Float(revolutions),
                Element::Float(shape.radius_offset),
                Element::Float(shape.skew),
            ]);
        }
        _ => {
            list.extend([
                Element::Vector(pair(scale_x, scale_y)),
                Element::Vector(pair(shear_x, shear_y)),
            ]);
        }
    }
    list
}

/// How many quarters of a box's profile a cut takes off (`cut / 12500` of
/// its 1/50000ths).
fn quarters(fraction: f32) -> i32 {
    crate::num::float_to_int(fraction * 50_000.0) / 12_500
}

/// How many of a prism's or ring's three sides a cut takes off (one past a
/// third, two past two thirds).
fn thirds(fraction: f32) -> i32 {
    let quanta = crate::num::float_to_int(fraction * 50_000.0);
    i32::from(quanta >= 16_667).saturating_add(i32::from(quanta >= 33_333))
}

/// Whether a circular path's own cut, skew, taper, revolutions, radius
/// offset or twist give it end faces.
fn path_ends(shape: &Shape) -> bool {
    let [taper_x, taper_y] = shape.taper;
    shape.path_begin > 0.0
        || shape.path_end < 1.0
        || shape.skew != 0.0
        || taper_x != 0.0
        || taper_y != 0.0
        || shape.revolutions > 1.0
        || shape.radius_offset != 0.0
        || (shape.twist_begin - shape.twist_end).abs() > f32::EPSILON
}

/// How many faces `shape` has (`llGetNumberOfSides`), as both grids count
/// them (`prim/faces`; `SceneObjectPart.GetNumberOfSides`): a sculpt one, a
/// mesh eight.
#[must_use]
pub fn faces_of(shape: &Shape) -> i32 {
    if let Some(sculpt) = &shape.sculpt {
        return if sculpt.kind & SCULPT_TYPE_MASK == SCULPT_MESH {
            8
        } else {
            1
        };
    }
    let begin = shape.profile_begin;
    let end_cut = 1.0 - shape.profile_end;
    let cut = begin > 0.0 || end_cut > 0.0;
    let mut count = match kind_of(shape) {
        TYPE_PRISM => {
            if cut {
                5_i32
                    .saturating_sub(thirds(begin))
                    .saturating_sub(thirds(end_cut))
                    .saturating_add(2)
            } else {
                5
            }
        }
        TYPE_CYLINDER => {
            if cut {
                5
            } else {
                3
            }
        }
        TYPE_SPHERE => {
            let mut sides = 1_i32;
            if shape.path_begin > 0.0
                || shape.path_end < 1.0
                || shape.skew != 0.0
                || (shape.twist_begin - shape.twist_end).abs() > f32::EPSILON
            {
                sides = sides.saturating_add(2);
            }
            if cut || shape.hollow > 0.0 {
                sides = sides.saturating_add(2);
            }
            sides
        }
        TYPE_TORUS => 1_i32
            .saturating_add(if path_ends(shape) { 2 } else { 0 })
            .saturating_add(if cut { 2 } else { 0 }),
        TYPE_TUBE => {
            let mut sides = 4_i32.saturating_add(if path_ends(shape) { 2 } else { 0 });
            if cut {
                sides = sides
                    .saturating_sub(quarters(begin))
                    .saturating_sub(quarters(end_cut))
                    .saturating_add(2);
            }
            sides
        }
        TYPE_RING => {
            let mut sides = 3_i32.saturating_add(if path_ends(shape) { 2 } else { 0 });
            if cut {
                sides = sides
                    .saturating_sub(thirds(begin))
                    .saturating_sub(thirds(end_cut))
                    .saturating_add(2);
            }
            sides
        }
        _ => {
            if cut {
                6_i32
                    .saturating_sub(quarters(begin))
                    .saturating_sub(quarters(end_cut))
                    .saturating_add(2)
            } else {
                6
            }
        }
    };
    if shape.hollow > 0.0 {
        count = count.saturating_add(1);
    }
    count
}
