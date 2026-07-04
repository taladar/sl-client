//! The 2D **profile ring** swept along the extrusion path.
//!
//! A prim's cross-section — the shape the path drags through space — is a closed
//! (or, when cut, open) 2D ring of points in the prim's local X/Y plane. This
//! module builds that ring for the five profile curves (square, the three
//! triangles, circle, half-circle), honouring the profile begin/end **cut** and
//! the **hollow** inner cutout, and records the semantic **face ranges** that
//! name which slice of the ring becomes which drawable face.
//!
//! It is a faithful, idiomatic re-implementation of Firestorm
//! `indra/llmath/llvolume.cpp` — `LLProfile::genNGon`, `LLProfile::addHole`,
//! `LLProfile::addCap`, and `LLProfile::generate` — reworked to the workspace's
//! restriction lints (no indexing, no `as` casts outside the bounded numeric
//! helpers, no panics). The later `volume` phase sweeps this ring along the
//! `path` ring and assembles per-face geometry.
//!
//! Each [`ProfilePoint`] carries its 2D position plus the sweep parameter `u`
//! (Firestorm's profile-point `.z`), which the volume sweep reads directly as
//! the horizontal (U) texture coordinate. Each [`ProfileFace`] carries the
//! `[index, index + count)` slice of the ring it spans, its U scale, whether it
//! is a flat two-point edge, whether it is an end **cap**, and its
//! [`ProfileFaceId`] — the Linden `LL_FACE_*` bit flag naming the face.

use crate::PrimLod;
use crate::shape::{HoleType, PrimShape, ProfileCurve};
use core::f32::consts::{FRAC_1_SQRT_2, TAU};

/// The base side count of a full circular profile at detail `1.0` (Firestorm
/// `MIN_DETAIL_FACES`); a round profile's ring uses `MIN_DETAIL_FACES * detail`
/// sides. Mirrors [`crate::MIN_DETAIL_FACES`] as the `f32` the tessellation
/// multiplies.
const MIN_DETAIL_FACES: f32 = 6.0;

/// The profile-point `t`-fraction below which the first fractional ring point is
/// treated as lying exactly on an edge and skipped (Firestorm `0.9999`).
const EDGE_EPSILON: f32 = 0.9999;

/// The profile-point `t`-fraction above which the trailing fractional ring point
/// is emitted (Firestorm `0.0001`).
const FRACTION_EPSILON: f32 = 0.0001;

/// The span threshold below which a swept ring is considered **open** (cut):
/// `(end - begin) * ang_scale < 0.99` (Firestorm).
const OPEN_THRESHOLD: f32 = 0.99;

/// The span threshold above which an open ring is additionally **concave**:
/// `(end - begin) * ang_scale > 0.5` (Firestorm).
const CONCAVE_THRESHOLD: f32 = 0.5;

/// The four-sided (square) profile side count.
const SQUARE_SIDES: f32 = 4.0;

/// The three-sided (triangle) profile side count.
const TRIANGLE_SIDES: f32 = 3.0;

/// The square profile's angular offset (Firestorm `-0.375`), rotating the four
/// corners so the box's faces align to the axes.
const SQUARE_OFFSET: f32 = -0.375;

/// The half-circle profile's angular offset (Firestorm `0.5`).
const HALF_CIRCLE_OFFSET: f32 = 0.5;

/// The half-circle profile's angular scale (Firestorm `0.5`): it sweeps only
/// half the full circle.
const HALF_CIRCLE_ANG_SCALE: f32 = 0.5;

/// The default (full) angular scale — a whole revolution.
const FULL_ANG_SCALE: f32 = 1.0;

/// The `genNGon` scale table (Firestorm `tableScale`), indexed by the total side
/// count `0..8`; it compensates a low-side ring to roughly fill the unit
/// bounding box. Eight-or-more-sided rings use the default `0.5` scale.
const SCALE_TABLE: [f32; 8] = [1.0, 1.0, 1.0, 0.5, FRAC_1_SQRT_2, 0.53, 0.525, 0.5];

/// The default per-ring scale used when the total side count is eight or more
/// (Firestorm's initial `scale = 0.5f`).
const DEFAULT_SCALE: f32 = 0.5;

/// The Linden semantic face identifier of a [`ProfileFace`] — one of the
/// `LL_FACE_*` bit flags (`llvolume.h`). It is a **bit flag**, not the
/// sequential texture-entry index; the `volume` phase maps the set of face
/// flags to sequential [`crate::PrimFaceId`]s.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Profile*` names read clearly"
)]
pub struct ProfileFaceId(u16);

impl ProfileFaceId {
    /// The path-begin cap face (`LL_FACE_PATH_BEGIN`, `0x0001`) — the top end
    /// cap of an open path.
    pub const PATH_BEGIN: Self = Self(0x0001);

    /// The path-end cap face (`LL_FACE_PATH_END`, `0x0002`) — the bottom end cap
    /// of an open path.
    pub const PATH_END: Self = Self(0x0002);

    /// The inner-side face (`LL_FACE_INNER_SIDE`, `0x0004`) — the wall of a
    /// hollow prim's cutout.
    pub const INNER_SIDE: Self = Self(0x0004);

    /// The profile-begin edge face (`LL_FACE_PROFILE_BEGIN`, `0x0008`) — one cut
    /// edge exposed when the profile ring is open.
    pub const PROFILE_BEGIN: Self = Self(0x0008);

    /// The profile-end edge face (`LL_FACE_PROFILE_END`, `0x0010`) — the other
    /// cut edge exposed when the profile ring is open.
    pub const PROFILE_END: Self = Self(0x0010);

    /// The first outer-side face (`LL_FACE_OUTER_SIDE_0`, `0x0020`); the box's
    /// four sides are this flag shifted by `0..4`.
    pub const OUTER_SIDE_0: Self = Self(0x0020);

    /// The outer-side face flag for side `index` (`LL_FACE_OUTER_SIDE_0 <<
    /// index`). An out-of-range shift (which the wire cannot actually produce)
    /// falls back to an empty flag rather than panicking.
    #[must_use]
    pub fn outer_side(index: u32) -> Self {
        Self(Self::OUTER_SIDE_0.0.checked_shl(index).unwrap_or(0))
    }

    /// The raw `LL_FACE_*` bit-flag value.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether this identifier has every bit of `other` set (a subset test over
    /// the face-flag bits).
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// One point of the 2D profile ring: its position in the prim-local X/Y plane
/// plus the sweep parameter `u` used as the horizontal texture coordinate.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Profile*` names read clearly"
)]
pub struct ProfilePoint {
    /// The profile position in the prim's local X/Y plane.
    pub position: [f32; 2],
    /// The sweep parameter (Firestorm's profile-point `.z`): the volume sweep
    /// reads it directly as the horizontal (U) texture coordinate.
    pub u: f32,
}

impl ProfilePoint {
    /// A ring point on the unit-circle at angle `ang` (scaled by `scale`) with
    /// sweep parameter `u`.
    fn on_ring(ang: f32, scale: f32, u: f32) -> Self {
        Self {
            position: [ang.cos() * scale, ang.sin() * scale],
            u,
        }
    }

    /// The linear interpolation from `self` to `other` by `fraction`.
    fn lerp(self, other: Self, fraction: f32) -> Self {
        let [ax, ay] = self.position;
        let [bx, by] = other.position;
        Self {
            position: [ax + (bx - ax) * fraction, ay + (by - ay) * fraction],
            u: self.u + (other.u - self.u) * fraction,
        }
    }

    /// This point with its position and sweep parameter uniformly scaled — the
    /// hollow-ring shrink (Firestorm `pt[i].mul(box_hollow)`).
    fn scaled(self, factor: f32) -> Self {
        let [x, y] = self.position;
        Self {
            position: [x * factor, y * factor],
            u: self.u * factor,
        }
    }

    /// This point with its sweep parameter (only) scaled — the square/triangle
    /// texture-coordinate stretch (Firestorm `mProfile[i].mul({1,1,n,1})`).
    fn with_u_scaled(self, factor: f32) -> Self {
        Self {
            position: self.position,
            u: self.u * factor,
        }
    }
}

/// One drawable slice of the profile ring: the half-open `[index, index +
/// count)` range of [`Profile::points`] it spans, its U texture scale, whether
/// it is a flat two-point edge, whether it is an end **cap** (a whole-ring
/// polygon rather than a swept strip), and its [`ProfileFaceId`].
#[derive(Clone, Copy, PartialEq, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Profile*` names read clearly"
)]
pub struct ProfileFace {
    /// The first ring-point index this face spans.
    pub index: usize,
    /// The number of ring points this face spans.
    pub count: usize,
    /// The horizontal (U) texture-coordinate scale applied to this face.
    pub scale_u: f32,
    /// Whether this face is an end cap (top / bottom polygon of the whole ring)
    /// rather than a swept side strip.
    pub cap: bool,
    /// Whether this face is flat — a straight two-point edge (`count == 2`).
    pub flat: bool,
    /// The Linden semantic face identifier.
    pub face_id: ProfileFaceId,
}

/// A fully generated 2D profile ring: its ordered points and the semantic face
/// ranges over them, plus the open/concave flags and the outer/total point
/// counts the sweep needs.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// The ordered ring points, outer ring first then (for a hollow prim) the
    /// reversed, shrunk inner ring.
    pub points: Vec<ProfilePoint>,
    /// The semantic face ranges over [`Self::points`], in generation order.
    pub faces: Vec<ProfileFace>,
    /// Whether the ring is open (cut) rather than a closed loop.
    pub open: bool,
    /// Whether an open ring spans more than half the full sweep (concave).
    pub concave: bool,
    /// The number of outer-ring points (points before a hollow inner ring); for
    /// a solid prim this equals [`Self::total`].
    pub total_out: usize,
    /// The total ring-point count (`points.len()`).
    pub total: usize,
}

impl Profile {
    /// Whether the ring is open (cut).
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the ring is concave (an open ring spanning more than half sweep).
    #[must_use]
    pub const fn is_concave(&self) -> bool {
        self.concave
    }

    /// The number of ring points.
    #[must_use]
    pub const fn point_count(&self) -> usize {
        self.points.len()
    }

    /// The number of semantic faces.
    #[must_use]
    pub const fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Generate the profile ring for `shape` at level of detail `lod`.
    ///
    /// `path_open` is the extrusion path's open flag (a straight line is open, a
    /// full circle sweep is closed); an open path adds the two end **caps**.
    /// `split` tessellates each ring edge into `split + 1` segments (Firestorm's
    /// per-edge split, used to reduce interpolation error under twist / taper);
    /// pass `0` for un-split edges.
    ///
    /// This mirrors `LLProfile::generate`: it dispatches on the profile curve,
    /// runs `genNGon`, records the outer side faces, applies the square/triangle
    /// texture-coordinate stretch, adds the hollow inner ring, and finally adds
    /// the path caps and the open-ring profile edges.
    #[must_use]
    pub fn generate(shape: &PrimShape, lod: PrimLod, path_open: bool, split: u32) -> Self {
        let split = usize_from_u32(split);
        let detail = lod.detail();
        let mut builder = Builder::new(shape, split);

        match shape.profile_curve {
            ProfileCurve::Square => builder.build_square(shape, detail, path_open),
            ProfileCurve::IsoTriangle
            | ProfileCurve::EqualTriangle
            | ProfileCurve::RightTriangle => builder.build_triangle(shape, detail, path_open),
            ProfileCurve::Circle => builder.build_circle(shape, detail, path_open),
            ProfileCurve::HalfCircle => builder.build_half_circle(shape, detail, path_open),
        }

        builder.finish(path_open);
        builder.profile
    }
}

/// The mutable tessellation state while a [`Profile`] is being built — the
/// counterpart of Firestorm's in-progress `LLProfile`. It carries the profile
/// cut/hollow parameters (constant across the outer and inner `genNGon` runs)
/// and the split count alongside the growing [`Profile`].
struct Builder {
    /// The profile cut start fraction, `[0, 1]`.
    begin: f32,
    /// The profile cut end fraction, `[0, 1]`.
    end: f32,
    /// The profile hollow fraction, `[0, 1]` (`0` is solid).
    hollow: f32,
    /// The per-edge split count (Firestorm's `split`).
    split: usize,
    /// The profile being assembled.
    profile: Profile,
}

impl Builder {
    /// A fresh builder for `shape`'s cut/hollow parameters and `split`.
    fn new(shape: &PrimShape, split: usize) -> Self {
        Self {
            begin: shape.profile_begin,
            end: shape.profile_end,
            hollow: shape.hollow,
            split,
            profile: Profile::default(),
        }
    }

    /// Generate an `sides`-sided "circular" ring from `begin` to `end`, offset by
    /// `offset` revolutions and swept over `ang_scale` of a full turn, appending
    /// its points to the profile (Firestorm `LLProfile::genNGon`). Updates the
    /// open/concave flags and the total point count.
    fn gen_ngon(&mut self, sides: f32, offset: f32, ang_scale: f32) {
        let t_step = 1.0 / sides;
        let ang_step = TAU * t_step * ang_scale;

        let total_sides = round_to_i32(sides / ang_scale);
        let scale = ring_scale(total_sides);

        let t_first = (self.begin * sides).floor() / sides;

        // pt1 is the first point on the fractional face; pt2 the next point.
        let ang0 = TAU * (t_first * ang_scale + offset);
        let mut pt1 = ProfilePoint::on_ring(ang0, scale, t_first);
        let mut t = t_first + t_step;
        let mut ang = ang0 + ang_step;
        let pt2 = ProfilePoint::on_ring(ang, scale, t);

        // The first fractional point, unless it lies (almost) exactly on an edge.
        let first_fraction = (self.begin - t_first) * sides;
        if first_fraction < EDGE_EPSILON {
            self.profile.points.push(pt1.lerp(pt2, first_fraction));
        }

        // Every whole step of t up to end.
        while t < self.end {
            pt1 = ProfilePoint::on_ring(ang, scale, t);
            self.push_splits(pt1);
            self.profile.points.push(pt1);
            t += t_step;
            ang += ang_step;
        }

        // The trailing fractional point, unless it lands (almost) exactly on the
        // previous point.
        let last_pt2 = ProfilePoint::on_ring(ang, scale, t);
        let last_fraction = (self.end - (t - t_step)) * sides;
        if last_fraction > FRACTION_EPSILON {
            let new_pt = pt1.lerp(last_pt2, last_fraction);
            self.push_splits(new_pt);
            self.profile.points.push(new_pt);
        }

        // A short sweep leaves the ring open; a solid open ring gets a centre
        // point for its cut faces to pivot on.
        let span = (self.end - self.begin) * ang_scale;
        if span < OPEN_THRESHOLD {
            self.profile.concave = span > CONCAVE_THRESHOLD;
            self.profile.open = true;
            if self.hollow <= 0.0 {
                self.profile.points.push(ProfilePoint::default());
            }
        } else {
            self.profile.open = false;
            self.profile.concave = false;
        }

        self.profile.total = self.profile.points.len();
    }

    /// Insert the `split` interpolated edge points between the last pushed ring
    /// point and `next` (Firestorm's per-edge split); a no-op when there is no
    /// previous point or `split` is zero.
    fn push_splits(&mut self, next: ProfilePoint) {
        let Some(prev) = self.profile.points.last().copied() else {
            return;
        };
        let denom = usize_to_f32(self.split.saturating_add(1));
        for step in 1..=self.split {
            let fraction = usize_to_f32(step) / denom;
            self.profile.points.push(prev.lerp(next, fraction));
        }
    }

    /// Append an end cap face spanning the whole current ring (Firestorm
    /// `addCap`).
    fn add_cap(&mut self, face_id: ProfileFaceId) {
        self.profile.faces.push(ProfileFace {
            index: 0,
            count: self.profile.total,
            scale_u: 1.0,
            cap: true,
            flat: false,
            face_id,
        });
    }

    /// Append a side face spanning `count` ring points from `index` (Firestorm
    /// `addFace`).
    fn add_face(
        &mut self,
        index: usize,
        count: usize,
        scale_u: f32,
        face_id: ProfileFaceId,
        flat: bool,
    ) {
        self.profile.faces.push(ProfileFace {
            index,
            count,
            scale_u,
            cap: false,
            flat,
            face_id,
        });
    }

    /// Add the hollow inner ring: generate a reversed, `box_hollow`-shrunk ring
    /// inside the outer one, record its inner-side face, and double every cap's
    /// point count so the caps span both rings (Firestorm `addHole`).
    fn add_hole(&mut self, flat: bool, sides: f32, offset: f32, box_hollow: f32, ang_scale: f32) {
        let outer_total = self.profile.total;
        self.gen_ngon(sides.floor(), offset, ang_scale);

        let inner_count = self.profile.total.saturating_sub(outer_total);
        self.add_face(
            outer_total,
            inner_count,
            0.0,
            ProfileFaceId::INNER_SIDE,
            flat,
        );

        // Shrink the inner ring toward the centre and reverse its winding.
        let shrunk: Vec<ProfilePoint> = self
            .profile
            .points
            .get(outer_total..)
            .unwrap_or(&[])
            .iter()
            .map(|point| point.scaled(box_hollow))
            .collect();
        for (offset_in_ring, point) in shrunk.iter().rev().enumerate() {
            if let Some(slot) = self
                .profile
                .points
                .get_mut(outer_total.saturating_add(offset_in_ring))
            {
                *slot = *point;
            }
        }

        self.profile.total_out = outer_total;

        for face in &mut self.profile.faces {
            if face.cap {
                face.count = face.count.saturating_mul(2);
            }
        }
    }

    /// Multiply the sweep parameter of every current ring point by `factor` — the
    /// square/triangle texture-coordinate stretch (Firestorm `mul({1,1,n,1})`).
    fn stretch_u(&mut self, factor: f32) {
        for point in &mut self.profile.points {
            *point = point.with_u_scaled(factor);
        }
    }

    /// Build a square profile ring with its (up to four) outer side faces and
    /// optional hollow (Firestorm's `LL_PCODE_PROFILE_SQUARE` branch).
    fn build_square(&mut self, shape: &PrimShape, detail: f32, path_open: bool) {
        self.gen_ngon(SQUARE_SIDES, SQUARE_OFFSET, FULL_ANG_SCALE);
        if path_open {
            self.add_cap(ProfileFaceId::PATH_BEGIN);
        }
        self.add_straight_sides(SQUARE_SIDES);
        self.stretch_u(SQUARE_SIDES);
        if self.hollow > 0.0 {
            match shape.hole_type {
                HoleType::Triangle => {
                    self.add_hole(true, 3.0, SQUARE_OFFSET, self.hollow, FULL_ANG_SCALE);
                }
                HoleType::Circle => {
                    self.add_hole(
                        false,
                        MIN_DETAIL_FACES * detail,
                        SQUARE_OFFSET,
                        self.hollow,
                        FULL_ANG_SCALE,
                    );
                }
                HoleType::Same | HoleType::Square => {
                    self.add_hole(true, 4.0, SQUARE_OFFSET, self.hollow, FULL_ANG_SCALE);
                }
            }
        }
        if path_open && let Some(first) = self.profile.faces.first_mut() {
            first.count = self.profile.total;
        }
    }

    /// Build a triangle profile ring with its (up to three) outer side faces and
    /// optional hollow (Firestorm's triangle branch). Swept triangles use half
    /// the hollow value because the triangle underfills its bounding box.
    fn build_triangle(&mut self, shape: &PrimShape, detail: f32, path_open: bool) {
        self.gen_ngon(TRIANGLE_SIDES, 0.0, FULL_ANG_SCALE);
        self.stretch_u(TRIANGLE_SIDES);
        if path_open {
            self.add_cap(ProfileFaceId::PATH_BEGIN);
        }
        self.add_straight_sides(TRIANGLE_SIDES);
        if self.hollow > 0.0 {
            let triangle_hollow = self.hollow / 2.0;
            match shape.hole_type {
                HoleType::Circle => {
                    self.add_hole(
                        false,
                        MIN_DETAIL_FACES * detail,
                        0.0,
                        triangle_hollow,
                        FULL_ANG_SCALE,
                    );
                }
                HoleType::Square => {
                    self.add_hole(true, 4.0, 0.0, triangle_hollow, FULL_ANG_SCALE);
                }
                HoleType::Same | HoleType::Triangle => {
                    self.add_hole(true, 3.0, 0.0, triangle_hollow, FULL_ANG_SCALE);
                }
            }
        }
    }

    /// Build a circle profile ring — one outer side face and an optional hollow
    /// (Firestorm's `LL_PCODE_PROFILE_CIRCLE` branch). A square hollow snaps the
    /// side count to a multiple of four so the corners line up.
    fn build_circle(&mut self, shape: &PrimShape, detail: f32, path_open: bool) {
        let mut circle_detail = MIN_DETAIL_FACES * detail;
        if self.hollow > 0.0 && shape.hole_type == HoleType::Square {
            circle_detail = (circle_detail / 4.0).ceil() * 4.0;
        }
        self.gen_ngon(circle_detail.floor(), 0.0, FULL_ANG_SCALE);
        if path_open {
            self.add_cap(ProfileFaceId::PATH_BEGIN);
        }
        self.add_round_outer_face();
        if self.hollow > 0.0 {
            match shape.hole_type {
                HoleType::Square => self.add_hole(true, 4.0, 0.0, self.hollow, FULL_ANG_SCALE),
                HoleType::Triangle => self.add_hole(true, 3.0, 0.0, self.hollow, FULL_ANG_SCALE),
                HoleType::Circle | HoleType::Same => {
                    self.add_hole(false, circle_detail, 0.0, self.hollow, FULL_ANG_SCALE);
                }
            }
        }
    }

    /// Build a half-circle profile ring (Firestorm's `LL_PCODE_PROFILE_CIRCLE_HALF`
    /// branch, the sphere cross-section). The side count is halved because it is
    /// only half a circle; a full, uncut, solid half-circle closes its ring.
    fn build_half_circle(&mut self, shape: &PrimShape, detail: f32, path_open: bool) {
        let mut circle_detail = MIN_DETAIL_FACES * detail * 0.5;
        if self.hollow > 0.0 && shape.hole_type == HoleType::Square {
            circle_detail = (circle_detail / 2.0).ceil() * 2.0;
        }
        self.gen_ngon(
            circle_detail.floor(),
            HALF_CIRCLE_OFFSET,
            HALF_CIRCLE_ANG_SCALE,
        );
        if path_open {
            self.add_cap(ProfileFaceId::PATH_BEGIN);
        }
        self.add_round_outer_face();
        if self.hollow > 0.0 {
            match shape.hole_type {
                HoleType::Square => {
                    self.add_hole(
                        true,
                        2.0,
                        HALF_CIRCLE_OFFSET,
                        self.hollow,
                        HALF_CIRCLE_ANG_SCALE,
                    );
                }
                HoleType::Triangle => {
                    self.add_hole(
                        true,
                        3.0,
                        HALF_CIRCLE_OFFSET,
                        self.hollow,
                        HALF_CIRCLE_ANG_SCALE,
                    );
                }
                HoleType::Circle | HoleType::Same => {
                    self.add_hole(
                        false,
                        circle_detail,
                        HALF_CIRCLE_OFFSET,
                        self.hollow,
                        HALF_CIRCLE_ANG_SCALE,
                    );
                }
            }
        }

        // Openness special-case for the sphere: a cut half-circle is open; a
        // full solid one closes by repeating its first point.
        if (self.end - self.begin) < 1.0 {
            self.profile.open = true;
        } else if self.hollow <= 0.0 {
            self.profile.open = false;
            if let Some(first) = self.profile.points.first().copied() {
                self.profile.points.push(first);
                self.profile.total = self.profile.points.len();
            }
        }
    }

    /// Add the per-side outer faces of a straight-sided (square / triangle)
    /// profile: one flat face per whole side spanned by the profile cut
    /// (Firestorm's `addFace` loop, `LL_FACE_OUTER_SIDE_0 << i`).
    fn add_straight_sides(&mut self, sides: f32) {
        let low = floor_to_i32(self.begin * sides);
        let high = floor_to_i32(self.end * sides + EDGE_EPSILON);
        for (face_num, side) in (low..high).enumerate() {
            let index = face_num.saturating_mul(self.split.saturating_add(1));
            let count = self.split.saturating_add(2);
            self.add_face(
                index,
                count,
                1.0,
                ProfileFaceId::outer_side(u32_from_i32(side)),
                true,
            );
        }
    }

    /// Add the single outer face of a round (circle / half-circle) profile — one
    /// point shorter when the ring is open and solid (Firestorm's circle-branch
    /// `addFace`).
    fn add_round_outer_face(&mut self) {
        let count = if self.profile.open && self.hollow <= 0.0 {
            self.profile.total.saturating_sub(1)
        } else {
            self.profile.total
        };
        self.add_face(0, count, 0.0, ProfileFaceId::OUTER_SIDE_0, false);
    }

    /// Add the path-end cap and the open-ring profile-edge faces once the curve
    /// branch has run (the tail of `LLProfile::generate`).
    fn finish(&mut self, path_open: bool) {
        if path_open {
            self.add_cap(ProfileFaceId::PATH_END);
        }
        if self.profile.open {
            let last = self.profile.total.saturating_sub(1);
            self.add_face(
                last,
                2,
                HALF_CIRCLE_OFFSET,
                ProfileFaceId::PROFILE_BEGIN,
                true,
            );
            let end_index = if self.hollow > 0.0 {
                self.profile.total_out.saturating_sub(1)
            } else {
                self.profile.total.saturating_sub(2)
            };
            self.add_face(
                end_index,
                2,
                HALF_CIRCLE_OFFSET,
                ProfileFaceId::PROFILE_END,
                true,
            );
        }
    }
}

/// The `genNGon` ring scale for a ring of `total_sides` sides: the
/// [`SCALE_TABLE`] entry for a small side count, else the [`DEFAULT_SCALE`].
fn ring_scale(total_sides: i32) -> f32 {
    usize::try_from(total_sides)
        .ok()
        .and_then(|index| SCALE_TABLE.get(index).copied())
        .unwrap_or(DEFAULT_SCALE)
}

/// Floor `value` to an `i32`. The profile side and cut indices are small,
/// non-negative counts (at most a few tens), so the conversion is exact and
/// cannot wrap.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "value is a small non-negative profile index; its floor fits an i32 exactly"
)]
const fn floor_to_i32(value: f32) -> i32 {
    value.floor() as i32
}

/// Round `value` to the nearest `i32`. The input is a small, non-negative side
/// count, so the conversion is exact and cannot wrap.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "value is a small non-negative side count; its nearest integer fits an i32 exactly"
)]
const fn round_to_i32(value: f32) -> i32 {
    value.round() as i32
}

/// Widen a small, non-negative side index to `u32` for a face-flag shift; a
/// negative index (which the wire cannot produce) maps to `0`.
fn u32_from_i32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// Widen a `u32` split count to `usize` (lossless on every supported target).
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Convert a small split/step count to `f32` for an interpolation fraction; the
/// counts are tiny, so the conversion is exact.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a tiny split/step count that converts to f32 exactly"
)]
const fn usize_to_f32(value: usize) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::{Profile, ProfileFaceId};
    use crate::PrimLod;
    use crate::shape::PrimShape;
    use pretty_assertions::assert_eq;
    use sl_proto::PrimShapeParams;

    /// The wire params for the viewer's default new prim (a unit box).
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

    /// Whether any face carries the given semantic identifier.
    fn has_face(profile: &Profile, id: ProfileFaceId) -> bool {
        profile.faces.iter().any(|face| face.face_id == id)
    }

    /// The number of outer-side faces (side `0..4`) a profile carries.
    fn outer_side_count(profile: &Profile) -> usize {
        (0..4)
            .filter(|&side| has_face(profile, ProfileFaceId::outer_side(side)))
            .count()
    }

    #[test]
    fn default_box_profile_has_four_sides_and_two_caps() {
        let shape = PrimShape::from_params(&default_box_params());
        // An open (line) path gives the two path caps.
        let profile = Profile::generate(&shape, PrimLod::High, true, 0);
        // Square ring closes with a repeated corner: five points.
        assert_eq!(profile.point_count(), 5);
        assert!(!profile.is_open());
        assert_eq!(outer_side_count(&profile), 4);
        assert!(has_face(&profile, ProfileFaceId::PATH_BEGIN));
        assert!(has_face(&profile, ProfileFaceId::PATH_END));
        // Solid, closed ring: no inner-side or profile-edge faces.
        assert!(!has_face(&profile, ProfileFaceId::INNER_SIDE));
        assert!(!has_face(&profile, ProfileFaceId::PROFILE_BEGIN));
        // Four sides + two caps.
        assert_eq!(profile.face_count(), 6);
    }

    #[test]
    fn closed_path_gives_no_caps() {
        let shape = PrimShape::from_params(&default_box_params());
        let profile = Profile::generate(&shape, PrimLod::High, false, 0);
        assert!(!has_face(&profile, ProfileFaceId::PATH_BEGIN));
        assert!(!has_face(&profile, ProfileFaceId::PATH_END));
        // Only the four outer sides remain.
        assert_eq!(profile.face_count(), 4);
    }

    #[test]
    fn circle_profile_ring_scales_with_detail() {
        let mut params = default_box_params();
        params.profile_curve = 0x00;
        let shape = PrimShape::from_params(&params);
        let low = Profile::generate(&shape, PrimLod::Lowest, true, 0);
        let high = Profile::generate(&shape, PrimLod::High, true, 0);
        // MIN_DETAIL_FACES * detail rounds to more points at higher detail.
        assert!(high.point_count() > low.point_count());
        // One round outer face plus two path caps.
        assert_eq!(outer_side_count(&high), 1);
        assert!(has_face(&high, ProfileFaceId::PATH_BEGIN));
        assert!(has_face(&high, ProfileFaceId::PATH_END));
    }

    #[test]
    fn hollow_box_adds_inner_ring_and_face() {
        let solid = Profile::generate(
            &PrimShape::from_params(&default_box_params()),
            PrimLod::High,
            true,
            0,
        );
        let mut params = default_box_params();
        // Half hollow, square hole (same as the outer square).
        params.profile_hollow = 25000;
        let hollow = Profile::generate(&PrimShape::from_params(&params), PrimLod::High, true, 0);
        assert!(has_face(&hollow, ProfileFaceId::INNER_SIDE));
        // Inner ring roughly doubles the outer point count.
        assert!(hollow.point_count() > solid.point_count());
        assert!(hollow.total_out > 0);
    }

    #[test]
    fn profile_cut_opens_the_ring_and_adds_edge_faces() {
        let mut params = default_box_params();
        // Cut the profile to a quarter: begin 0.25, end 0.5.
        params.profile_begin = 12500;
        params.profile_end = 25000;
        let shape = PrimShape::from_params(&params);
        assert!(shape.is_profile_cut());
        let profile = Profile::generate(&shape, PrimLod::High, true, 0);
        assert!(profile.is_open());
        assert!(has_face(&profile, ProfileFaceId::PROFILE_BEGIN));
        assert!(has_face(&profile, ProfileFaceId::PROFILE_END));
    }

    #[test]
    fn split_multiplies_edge_points() {
        let shape = PrimShape::from_params(&default_box_params());
        let unsplit = Profile::generate(&shape, PrimLod::High, true, 0);
        let split = Profile::generate(&shape, PrimLod::High, true, 2);
        assert!(split.point_count() > unsplit.point_count());
    }

    #[test]
    fn face_id_flags_are_distinct_and_shift() {
        assert_eq!(ProfileFaceId::outer_side(0), ProfileFaceId::OUTER_SIDE_0);
        assert_eq!(ProfileFaceId::outer_side(1).bits(), 0x0040);
        assert!(ProfileFaceId::outer_side(2).contains(ProfileFaceId::outer_side(2)));
        assert!(!ProfileFaceId::PATH_BEGIN.contains(ProfileFaceId::PATH_END));
    }
}
