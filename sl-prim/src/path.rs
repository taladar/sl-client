//! The **extrusion path** the profile ring is swept along.
//!
//! Where [`profile`](crate::profile) builds the 2D cross-section, this module
//! builds the 3D curve that drags it through space: a sequence of
//! [`PathPoint`]s, each carrying a position, a per-step X/Y scale, a rotation,
//! and a sweep parameter (`tex_t`). A straight [`PathCurve::Line`] gives a box /
//! cylinder / prism; a [`PathCurve::Circle`] gives a torus / tube / ring; and
//! [`PathCurve::Circle2`] is the sphere path.
//!
//! It is a faithful, idiomatic re-implementation of Firestorm
//! `indra/llmath/llvolume.cpp` — `LLPath::generate` and `LLPath::genNGon` —
//! reworked to the workspace's restriction lints (no indexing, no `as` casts
//! outside the bounded numeric helpers, no panics). Every path parameter is
//! applied here: twist (begin/end), taper, shear, radius offset, skew,
//! revolutions, per-step scale (taper flip), and the path begin/end cut. The
//! later `volume` phase sweeps the [`profile`](crate::profile) ring along this
//! path and assembles per-face geometry.
//!
//! A [`PathPoint`]'s rotation is stored as a quaternion in the reference
//! viewer's convention (`twist * qang`, applied to a point as `p * rot`); the
//! sweep first scales a profile point by the point's [`PathPoint::scale`], then
//! rotates it by [`PathPoint::rotate`], then translates it by
//! [`PathPoint::position`] — exactly Firestorm's `LLVolumeFace::createSide`.

use crate::PrimLod;
use crate::shape::{PathCurve, PrimShape};
use core::f32::consts::{FRAC_1_SQRT_2, PI, TAU};

/// The base side count of a full circular path at detail `1.0` (Firestorm
/// `MIN_DETAIL_FACES`); a curved path uses roughly `MIN_DETAIL_FACES * detail`
/// steps (before the revolutions and twist adjustments).
const MIN_DETAIL_FACES: f32 = 6.0;

/// The twist-driven detail factor (Firestorm's `3.5f`): extra path steps scale
/// with the twist magnitude times this, times `detail - 0.5`.
const TWIST_DETAIL: f32 = 3.5;

/// The detail offset subtracted before the twist-detail multiply (Firestorm's
/// `detail - 0.5f`).
const DETAIL_OFFSET: f32 = 0.5;

/// The default path radius when the ring has eight or more sides (Firestorm's
/// initial `radius_start = 0.5f`).
const DEFAULT_RADIUS: f32 = 0.5;

/// The `genNGon` radius table (Firestorm `tableScale`), indexed by the side
/// count `0..8`; it compensates a low-side ring's radius. Eight-or-more-sided
/// rings use [`DEFAULT_RADIUS`].
const RADIUS_TABLE: [f32; 8] = [1.0, 1.0, 1.0, 0.5, FRAC_1_SQRT_2, 0.53, 0.525, 0.5];

/// The tolerance above which skew, taper spread, or radius spread makes a swept
/// ring **open** (Firestorm's `0.001f`).
const OPEN_EPSILON: f32 = 0.001;

/// The span below which a swept ring is **open** (cut): `end - begin < 1.0`.
const CLOSED_SPAN: f32 = 1.0;

/// The half-offset applied to a line path's Z (`t - 0.5`, centring the sweep on
/// the origin) and to the skew term (`* 0.5`).
const HALF: f32 = 0.5;

/// The alternating X coordinate a `Circle2` (sphere) path snaps its points to
/// (Firestorm's `toggle` of `±0.5`), collapsing the circle onto two planes.
const SPHERE_TOGGLE: f32 = 0.5;

/// One point along the extrusion path: a frame the profile ring is placed into.
///
/// The sweep transforms each profile point `p` (in the prim-local X/Y plane) to
/// world space as `rotate(scale ⊙ p) + position`, matching Firestorm's
/// `LLVolumeFace::createSide` (`scale_mat * rot`, then `+ offset`). The rotation
/// is a quaternion in the reference viewer's `p * rot` convention.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Path*` names read clearly"
)]
pub struct PathPoint {
    /// The frame origin in the prim's local, right-handed Z-up space.
    pub position: [f32; 3],
    /// The per-step X/Y scale applied to a profile point before rotation
    /// (Firestorm's `mScale` X/Y; its Z is always `0`).
    pub scale: [f32; 2],
    /// The frame rotation as a quaternion `(x, y, z, w)`, in the reference
    /// viewer's row-vector (`p * rot`) convention.
    pub rotation: [f32; 4],
    /// The sweep parameter (Firestorm's `mTexT`): the volume sweep reads it as
    /// the vertical (V) texture coordinate.
    pub tex_t: f32,
}

impl PathPoint {
    /// Rotate a prim-local vector by this frame's rotation, in the reference
    /// viewer's `v * rot` convention (Firestorm `LLVector3 operator*(vec,
    /// quat)`). The sweep applies this to a scaled profile point.
    #[must_use]
    pub fn rotate(&self, vector: [f32; 3]) -> [f32; 3] {
        rotate_vector(self.rotation, vector)
    }

    /// Place a 2D profile point into this frame: scale it by
    /// [`Self::scale`], rotate it by [`Self::rotate`], then translate it by
    /// [`Self::position`] — the full per-vertex sweep transform.
    #[must_use]
    pub fn place(&self, profile_point: [f32; 2]) -> [f32; 3] {
        let [px, py] = profile_point;
        let [sx, sy] = self.scale;
        let rotated = self.rotate([px * sx, py * sy, 0.0]);
        let [rx, ry, rz] = rotated;
        let [ox, oy, oz] = self.position;
        [rx + ox, ry + oy, rz + oz]
    }
}

/// A fully generated extrusion path: its ordered frames plus the open flag the
/// sweep needs to decide whether to close the path loop and add end caps.
#[derive(Clone, Debug, Default)]
pub struct Path {
    /// The ordered path frames, from the begin cut to the end cut.
    pub points: Vec<PathPoint>,
    /// Whether the path is open (a straight line, a cut / skewed / tapered
    /// sweep) rather than a closed loop (a full torus or sphere).
    pub open: bool,
    /// The path frame count (`points.len()`).
    pub total: usize,
}

impl Path {
    /// Whether the path is open (not a closed loop).
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The number of path frames.
    #[must_use]
    pub const fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Generate the extrusion path for `shape` at level of detail `lod`.
    ///
    /// `split` raises the minimum straight-line frame count to `split + 2`
    /// (Firestorm's per-edge split, used to reduce interpolation error under
    /// twist / taper); pass `0` for the un-split minimum of two frames.
    ///
    /// This mirrors `LLPath::generate`: it dispatches on the path curve, builds
    /// a straight line, a `genNGon` circle, or the sphere `Circle2` path, and
    /// finally forces the path open when its begin and end twist differ.
    #[must_use]
    pub fn generate(shape: &PrimShape, lod: PrimLod, split: u32) -> Self {
        let detail = lod.detail();
        let split = usize_from_u32(split);

        let mut builder = Builder::new(shape);
        match shape.path_curve {
            // A flexible path is tessellated as a straight line (softbody flex
            // is a non-goal), matching the crate's `PathCurve` documentation.
            PathCurve::Line | PathCurve::Flexible => builder.build_line(detail, split),
            PathCurve::Circle => builder.build_circle(detail),
            PathCurve::Circle2 => builder.build_circle2(detail),
        }

        // The final override in `LLPath::generate`: any twist along the path
        // opens the loop.
        if (shape.twist_begin - shape.twist_end).abs() > f32::EPSILON {
            builder.path.open = true;
        }

        builder.path.total = builder.path.points.len();
        builder.path
    }
}

/// The mutable state while a [`Path`] is being generated — the counterpart of
/// Firestorm's in-progress `LLPath`. It carries the (dequantized) shape
/// parameters alongside the growing [`Path`].
struct Builder<'shape> {
    /// The dequantized shape driving the path.
    shape: &'shape PrimShape,
    /// The path being assembled.
    path: Path,
}

impl<'shape> Builder<'shape> {
    /// A fresh builder over `shape`, starting from an empty, open path.
    const fn new(shape: &'shape PrimShape) -> Self {
        Self {
            shape,
            path: Path {
                points: Vec::new(),
                open: true,
                total: 0,
            },
        }
    }

    /// Build a straight-line path (Firestorm's `LL_PCODE_PATH_LINE` branch). The
    /// frame count grows with the begin/end twist spread; each frame lerps the
    /// shear, the (taper-flipped) begin/end scale, and a Z twist between the cut
    /// endpoints. A line path is always open.
    fn build_line(&mut self, detail: f32, split: usize) {
        let shape = self.shape;
        let twist_spread = (shape.twist_begin - shape.twist_end).abs();
        let extra = floor_to_usize(twist_spread * TWIST_DETAIL * (detail - DETAIL_OFFSET));
        let np = extra.saturating_add(2).max(split.saturating_add(2));
        let steps = np.saturating_sub(1).max(1);
        let step = 1.0 / usize_to_f32(steps);

        let (begin_scale, end_scale) = self.line_scales();
        for i in 0..np {
            let t = usize_to_f32(i) * step;
            // Position along the cut, centred on the origin in Z.
            let along = lerp(shape.path_begin, shape.path_end, t);
            let position = [
                lerp(0.0, shape.path_shear_x, along),
                lerp(0.0, shape.path_shear_y, along),
                along - HALF,
            ];
            // Twist rotates the frame about Z; a line's twist is `pi * twist`.
            let twist_angle = lerp(PI * shape.twist_begin, PI * shape.twist_end, along);
            let scale = [
                lerp(begin_scale[0], end_scale[0], along),
                lerp(begin_scale[1], end_scale[1], along),
            ];
            self.path.points.push(PathPoint {
                position,
                scale,
                rotation: quat_axis_z(twist_angle),
                tex_t: along,
            });
        }
        self.path.open = true;
    }

    /// Build a circular path (Firestorm's `LL_PCODE_PATH_CIRCLE` branch, the
    /// torus / tube / ring). The step count grows with detail, twist, and the
    /// number of revolutions; a positive count runs [`Self::gen_ngon`].
    fn build_circle(&mut self, detail: f32) {
        let shape = self.shape;
        let twist_mag = (shape.twist_begin - shape.twist_end).abs();
        let base = (MIN_DETAIL_FACES * detail
            + twist_mag * TWIST_DETAIL * (detail - DETAIL_OFFSET))
            .floor();
        let sides = floor_to_usize(base * shape.revolutions);
        if sides > 0 {
            self.gen_ngon(sides);
        }
    }

    /// Build the sphere path (Firestorm's `LL_PCODE_PATH_CIRCLE2` branch): a
    /// `genNGon` circle whose points are then snapped onto two `±0.5` planes,
    /// giving the profile something to sweep between the poles. A full,
    /// full-width sphere path closes its loop.
    fn build_circle2(&mut self, detail: f32) {
        let sides = floor_to_usize(MIN_DETAIL_FACES * detail);
        self.gen_ngon(sides);

        // Snap each frame's X to an alternating ±0.5 (Firestorm's `toggle`).
        let mut toggle = SPHERE_TOGGLE;
        for point in &mut self.path.points {
            if let Some(x) = point.position.first_mut() {
                *x = toggle;
            }
            toggle = -toggle;
        }
    }

    /// Generate an `sides`-sided circular path, from the begin cut to the end
    /// cut, appending its frames (Firestorm `LLPath::genNGon`, always called
    /// with the default offset / end-scale / twist-scale of `0` / `1` / `1`).
    /// Sets [`Path::open`] from the cut span, skew, taper spread, and radius
    /// spread.
    fn gen_ngon(&mut self, sides: usize) {
        let shape = self.shape;
        let sides_f = usize_to_f32(sides);
        let step = 1.0 / sides_f;

        let skew = shape.skew;
        let skew_mag = skew.abs();
        let hole_x = shape.path_scale_x * (1.0 - skew_mag);
        let hole_y = shape.path_scale_y;

        let (taper_x_begin, taper_x_end) = taper_span(shape.taper_x);
        let (taper_y_begin, taper_y_end) = taper_span(shape.taper_y);

        // The base radius, hole-scaled, then offset toward the start or end.
        let base_radius = radius_for_sides(sides) * (1.0 - hole_y);
        let (radius_start, radius_end) = radius_span(base_radius, shape.radius_offset);

        self.path.open = shape.path_end - shape.path_begin < CLOSED_SPAN
            || skew_mag > OPEN_EPSILON
            || (taper_x_end - taper_x_begin).abs() > OPEN_EPSILON
            || (taper_y_end - taper_y_begin).abs() > OPEN_EPSILON
            || (radius_end - radius_start).abs() > OPEN_EPSILON;

        let frame = |t: f32| -> PathPoint {
            let ang = TAU * shape.revolutions * t;
            let radius = lerp(radius_start, radius_end, t);
            let s = ang.sin() * radius;
            let c = ang.cos() * radius;
            let position = [
                lerp(0.0, shape.path_shear_x, s) + lerp(-skew, skew, t) * HALF,
                c + lerp(0.0, shape.path_shear_y, s),
                s,
            ];
            let scale = [
                hole_x * lerp(taper_x_begin, taper_x_end, t),
                hole_y * lerp(taper_y_begin, taper_y_end, t),
            ];
            // Twist (about Z) composed with the ring angle (about X): `p * rot`
            // applies the twist first, then the ring rotation.
            let twist_angle = lerp(shape.twist_begin, shape.twist_end, t) * TAU - PI;
            let rotation = quat_mul(quat_axis_z(twist_angle), quat_axis_x(ang));
            PathPoint {
                position,
                scale,
                rotation,
                tex_t: t,
            }
        };

        // The begin cut, then every whole step up to the end, then the end cut.
        self.path.points.push(frame(shape.path_begin));
        // Snap the next step to a quantized parameter so the begin cut does not
        // shift the interior points.
        let snapped = floor_to_usize((shape.path_begin + step) * sides_f);
        let mut t = usize_to_f32(snapped) / sides_f;
        while t < shape.path_end {
            self.path.points.push(frame(t));
            t += step;
        }
        self.path.points.push(frame(shape.path_end));
    }

    /// The line path's begin/end X/Y scale (Firestorm `getBeginScale` /
    /// `getEndScale`): a top size above `1` tapers the begin, below `1` tapers
    /// the end.
    fn line_scales(&self) -> ([f32; 2], [f32; 2]) {
        let (sx, sy) = (self.shape.path_scale_x, self.shape.path_scale_y);
        let begin = [
            if sx > 1.0 { 2.0 - sx } else { 1.0 },
            if sy > 1.0 { 2.0 - sy } else { 1.0 },
        ];
        let end = [
            if sx < 1.0 { sx } else { 1.0 },
            if sy < 1.0 { sy } else { 1.0 },
        ];
        (begin, end)
    }
}

/// The taper begin/end pair for one axis (Firestorm's taper flip): a taper that
/// would push the end above `1` flips onto the begin instead.
fn taper_span(taper: f32) -> (f32, f32) {
    let end = 1.0 - taper;
    if end > 1.0 {
        (2.0 - end, 1.0)
    } else {
        (1.0, end)
    }
}

/// The start/end radius pair for a radius offset (Firestorm's radius offset): a
/// negative offset shrinks the start radius, a positive one the end radius.
fn radius_span(base: f32, offset: f32) -> (f32, f32) {
    if offset < 0.0 {
        (base * (1.0 + offset), base)
    } else {
        (base, base * (1.0 - offset))
    }
}

/// The `genNGon` base radius for a ring of `sides` sides: the [`RADIUS_TABLE`]
/// entry for a small side count, else [`DEFAULT_RADIUS`].
fn radius_for_sides(sides: usize) -> f32 {
    RADIUS_TABLE.get(sides).copied().unwrap_or(DEFAULT_RADIUS)
}

/// The quaternion `(x, y, z, w)` for a rotation of `angle` radians about the Z
/// axis (Firestorm `setQuat(angle, 0, 0, 1)`).
pub(crate) fn quat_axis_z(angle: f32) -> [f32; 4] {
    let half = angle * 0.5;
    [0.0, 0.0, half.sin(), half.cos()]
}

/// The quaternion `(x, y, z, w)` for a rotation of `angle` radians about the X
/// axis (Firestorm `setQuat(angle, 1, 0, 0)`).
fn quat_axis_x(angle: f32) -> [f32; 4] {
    let half = angle * 0.5;
    [half.sin(), 0.0, 0.0, half.cos()]
}

/// The reference viewer's quaternion product `a * b` (Firestorm
/// `LLQuaternion::operator*`), in `(x, y, z, w)` order. In its row-vector
/// convention, `v * (a * b) == (v * a) * b`, so `a` is applied before `b`.
pub(crate) fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        bw * ax + bx * aw + by * az - bz * ay,
        bw * ay + by * aw + bz * ax - bx * az,
        bw * az + bz * aw + bx * ay - by * ax,
        bw * aw - bx * ax - by * ay - bz * az,
    ]
}

/// Rotate `vector` by quaternion `rot` in the reference viewer's row-vector
/// convention (Firestorm `LLVector3 operator*(const LLVector3&, const
/// LLQuaternion&)`).
pub(crate) fn rotate_vector(rot: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let [qx, qy, qz, qw] = rot;
    let [vx, vy, vz] = vector;
    let rw = -qx * vx - qy * vy - qz * vz;
    let rx = qw * vx + qy * vz - qz * vy;
    let ry = qw * vy + qz * vx - qx * vz;
    let rz = qw * vz + qx * vy - qy * vx;
    [
        -rw * qx + rx * qw - ry * qz + rz * qy,
        -rw * qy + ry * qw - rz * qx + rx * qz,
        -rw * qz + rz * qw - rx * qy + ry * qx,
    ]
}

/// The linear interpolation `a + (b - a) * t`.
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Floor a small, non-negative path count to `usize`; a negative or non-finite
/// value (which the parameters cannot actually produce) maps to `0`.
fn floor_to_usize(value: f32) -> usize {
    if value.is_finite() && value >= 0.0 {
        floor_to_usize_unchecked(value)
    } else {
        0
    }
}

/// Floor a bounded, non-negative `f32` to `usize`. The path counts are small
/// (at most a few hundred), so the conversion is exact and cannot wrap.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is a small non-negative path count; its floor fits a usize exactly"
)]
const fn floor_to_usize_unchecked(value: f32) -> usize {
    value.floor() as usize
}

/// Widen a `u32` split count to `usize` (lossless on every supported target).
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Convert a small path count / step index to `f32`; the counts are tiny, so the
/// conversion is exact.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a tiny path count/step index that converts to f32 exactly"
)]
const fn usize_to_f32(value: usize) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::{Path, PathPoint, quat_axis_z, rotate_vector};
    use crate::PrimLod;
    use crate::shape::PrimShape;
    use core::f32::consts::PI;
    use pretty_assertions::assert_eq;
    use sl_proto::PrimShapeParams;

    /// The absolute tolerance for float comparisons in these tests.
    const EPSILON: f32 = 1.0e-4;

    /// Assert two floats are within [`EPSILON`].
    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{actual} differs from expected {expected}"
        );
    }

    /// The wire params for the viewer's default new prim (a unit box): line
    /// path, no twist / taper / shear / skew / cut.
    fn default_box_params() -> PrimShapeParams {
        PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x01,
            path_begin: 0,
            path_end: 0,
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
        }
    }

    #[test]
    fn default_line_path_has_two_centred_frames() {
        let shape = PrimShape::from_params(&default_box_params());
        let path = Path::generate(&shape, PrimLod::High, 0);
        // A twist-free line is two frames, at Z = -0.5 and +0.5.
        assert_eq!(path.point_count(), 2);
        assert!(path.is_open());
        let first = path.points.first().copied().unwrap_or_default();
        let last = path.points.last().copied().unwrap_or_default();
        assert_close(first.position[2], -0.5);
        assert_close(last.position[2], 0.5);
        // Full top size ⇒ unit scale at both ends.
        assert_close(first.scale[0], 1.0);
        assert_close(last.scale[1], 1.0);
        assert_close(first.tex_t, 0.0);
        assert_close(last.tex_t, 1.0);
    }

    #[test]
    fn split_raises_the_line_frame_count() {
        let shape = PrimShape::from_params(&default_box_params());
        let unsplit = Path::generate(&shape, PrimLod::High, 0);
        let split = Path::generate(&shape, PrimLod::High, 3);
        assert_eq!(unsplit.point_count(), 2);
        // A split of 3 forces at least split + 2 = 5 frames.
        assert_eq!(split.point_count(), 5);
    }

    #[test]
    fn twist_adds_line_frames_and_opens() {
        let mut params = default_box_params();
        // A full revolution of end twist (100 * 0.01 = 1.0).
        params.path_twist = 100;
        let shape = PrimShape::from_params(&params);
        let path = Path::generate(&shape, PrimLod::High, 0);
        assert!(path.point_count() > 2);
        assert!(path.is_open());
    }

    #[test]
    fn path_cut_shifts_the_line_endpoints() {
        let mut params = default_box_params();
        // Cut the path to [0.2, 0.8]: begin 10000 → 0.2, end 10000 → 0.8.
        params.path_begin = 10000;
        params.path_end = 10000;
        let shape = PrimShape::from_params(&params);
        assert!(shape.is_path_cut());
        let path = Path::generate(&shape, PrimLod::High, 0);
        let first = path.points.first().copied().unwrap_or_default();
        let last = path.points.last().copied().unwrap_or_default();
        // Z is `t - 0.5` over the cut span.
        assert_close(first.position[2], 0.2 - 0.5);
        assert_close(last.position[2], 0.8 - 0.5);
    }

    #[test]
    fn taper_scales_the_line_end() {
        let mut params = default_box_params();
        // Path top size 50 → scale_x = (200 - 50) * 0.01 = 1.5 (> 1 ⇒ taper
        // the begin: begin scale 0.5, end scale 1.0).
        params.path_scale_x = 50;
        let shape = PrimShape::from_params(&params);
        let path = Path::generate(&shape, PrimLod::High, 0);
        let first = path.points.first().copied().unwrap_or_default();
        let last = path.points.last().copied().unwrap_or_default();
        assert_close(first.scale[0], 0.5);
        assert_close(last.scale[0], 1.0);
    }

    #[test]
    fn circle_path_is_a_closed_ring() {
        let mut params = default_box_params();
        params.path_curve = 0x20;
        let shape = PrimShape::from_params(&params);
        let path = Path::generate(&shape, PrimLod::High, 0);
        // MIN_DETAIL_FACES * 4 = 24-ish frames plus the two cut frames.
        assert!(path.point_count() > 10);
        // A default (uncut, unskewed, untapered) torus path is closed.
        assert!(!path.is_open());
    }

    #[test]
    fn circle_path_frame_count_scales_with_detail() {
        let mut params = default_box_params();
        params.path_curve = 0x20;
        let shape = PrimShape::from_params(&params);
        let low = Path::generate(&shape, PrimLod::Lowest, 0);
        let high = Path::generate(&shape, PrimLod::High, 0);
        assert!(high.point_count() > low.point_count());
    }

    #[test]
    fn circle_path_cut_opens_the_ring() {
        let mut params = default_box_params();
        params.path_curve = 0x20;
        // Cut the path end to 0.5 (end wire 25000 → 0.5).
        params.path_end = 25000;
        let shape = PrimShape::from_params(&params);
        let path = Path::generate(&shape, PrimLod::High, 0);
        assert!(path.is_open());
    }

    #[test]
    fn sphere_path_snaps_to_two_planes_and_closes() {
        let mut params = default_box_params();
        params.path_curve = 0x30;
        let shape = PrimShape::from_params(&params);
        let path = Path::generate(&shape, PrimLod::High, 0);
        // Every frame's X is snapped to ±0.5.
        for point in &path.points {
            assert_close(point.position[0].abs(), 0.5);
        }
        // A full-width, uncut sphere path is closed.
        assert!(!path.is_open());
    }

    #[test]
    fn quat_axis_z_rotates_in_the_xy_plane() {
        // A quarter turn about Z maps +X to +Y.
        let rot = quat_axis_z(PI * 0.5);
        let rotated = rotate_vector(rot, [1.0, 0.0, 0.0]);
        assert_close(rotated[0], 0.0);
        assert_close(rotated[1], 1.0);
        assert_close(rotated[2], 0.0);
    }

    #[test]
    fn place_scales_rotates_and_translates() {
        let point = PathPoint {
            position: [1.0, 2.0, 3.0],
            scale: [2.0, 2.0],
            rotation: quat_axis_z(0.0),
            tex_t: 0.0,
        };
        // Identity rotation: the profile point is scaled then translated.
        let placed = point.place([1.0, 0.0]);
        assert_close(placed[0], 3.0);
        assert_close(placed[1], 2.0);
        assert_close(placed[2], 3.0);
    }
}
