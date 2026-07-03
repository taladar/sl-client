//! The dequantized, float **prim shape** that drives tessellation.
//!
//! An `ObjectUpdate` carries a prim's path/profile parameters as the simulator's
//! quantized integers ([`sl_proto::PrimShapeParams`]). Tessellation, however,
//! works in floats — cut fractions in `[0, 1]`, twist in revolutions, a taper
//! ratio, and so on. [`PrimShape`] is that dequantized form, and
//! [`PrimShape::from_params`] converts one to the other using exactly the
//! reference viewer's constants (Firestorm `LLVolumeMessage::unpackPathParams` /
//! `unpackProfileParams`, quanta in `llvolume.h`).
//!
//! The curve bytes are split into typed enums: the profile byte's low nibble is
//! a [`ProfileCurve`] and its high nibble a [`HoleType`]; the path byte is a
//! [`PathCurve`].

use sl_proto::PrimShapeParams;

/// The cut / hollow quantum (`CUT_QUANTA` / `HOLLOW_QUANTA`, `0.00002` =
/// `1 / 50000`): begin / end / hollow wire integers are multiples of it.
const CUT_QUANTA: f32 = 0.000_02;

/// The scale / twist / radius-offset / skew quantum (`SCALE_QUANTA`, `0.01`).
const SCALE_QUANTA: f32 = 0.01;

/// The shear quantum (`SHEAR_QUANTA`, `0.01`).
const SHEAR_QUANTA: f32 = 0.01;

/// The taper quantum (`TAPER_QUANTA`, `0.01`).
const TAPER_QUANTA: f32 = 0.01;

/// The revolutions quantum (`REV_QUANTA`, `0.015`); revolutions dequantize to
/// `raw * REV_QUANTA + 1.0`, spanning `[1, 4]`.
const REV_QUANTA: f32 = 0.015;

/// The wire integer denoting a fully open path/profile end (`50000`): `path_end`
/// dequantizes to `(PATH_END_MAX - raw) * CUT_QUANTA`.
const PATH_END_MAX: f32 = 50000.0;

/// The 2D **profile** curve swept along the path — the low nibble of the profile
/// byte (`LL_PCODE_PROFILE_*`, mask `0x0f`). Determines the cross-section shape.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum ProfileCurve {
    /// A circle (`LL_PCODE_PROFILE_CIRCLE`, `0x00`) — a cylinder / sphere / cone
    /// cross-section.
    #[default]
    Circle,
    /// A square (`LL_PCODE_PROFILE_SQUARE`, `0x01`) — the default box.
    Square,
    /// An isosceles triangle (`LL_PCODE_PROFILE_ISOTRI`, `0x02`).
    IsoTriangle,
    /// An equilateral triangle (`LL_PCODE_PROFILE_EQUALTRI`, `0x03`).
    EqualTriangle,
    /// A right triangle (`LL_PCODE_PROFILE_RIGHTTRI`, `0x04`).
    RightTriangle,
    /// A half-circle (`LL_PCODE_PROFILE_CIRCLE_HALF`, `0x05`).
    HalfCircle,
}

impl ProfileCurve {
    /// The profile from the low nibble of a profile byte (`byte & 0x0f`);
    /// unknown values fall back to [`ProfileCurve::Square`], matching the
    /// viewer's tolerance of out-of-range curves.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        match byte & 0x0f {
            0x00 => Self::Circle,
            0x02 => Self::IsoTriangle,
            0x03 => Self::EqualTriangle,
            0x04 => Self::RightTriangle,
            0x05 => Self::HalfCircle,
            // 0x01 and any unknown value.
            _square => Self::Square,
        }
    }

    /// The low-nibble byte value for this profile.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Circle => 0x00,
            Self::Square => 0x01,
            Self::IsoTriangle => 0x02,
            Self::EqualTriangle => 0x03,
            Self::RightTriangle => 0x04,
            Self::HalfCircle => 0x05,
        }
    }

    /// Whether this profile is round (circle / half-circle) rather than
    /// straight-sided; a round profile's ring side count follows
    /// [`PrimLod::circle_sides`](crate::PrimLod::circle_sides).
    #[must_use]
    pub const fn is_round(self) -> bool {
        matches!(self, Self::Circle | Self::HalfCircle)
    }
}

/// The **hole** (inner cutout) curve of a hollow prim — the high nibble of the
/// profile byte (`LL_PCODE_HOLE_*`, mask `0xf0`). Only meaningful when the prim
/// has non-zero hollow; [`HoleType::Same`] reuses the outer profile's shape.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum HoleType {
    /// Same as the outer profile (`LL_PCODE_HOLE_SAME`, `0x00`).
    #[default]
    Same,
    /// A circular hole (`LL_PCODE_HOLE_CIRCLE`, `0x10`).
    Circle,
    /// A square hole (`LL_PCODE_HOLE_SQUARE`, `0x20`).
    Square,
    /// A triangular hole (`LL_PCODE_HOLE_TRIANGLE`, `0x30`).
    Triangle,
}

impl HoleType {
    /// The hole type from the high nibble of a profile byte (`byte & 0xf0`);
    /// unknown values fall back to [`HoleType::Same`].
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        match byte & 0xf0 {
            0x10 => Self::Circle,
            0x20 => Self::Square,
            0x30 => Self::Triangle,
            // 0x00 and any unknown value.
            _same => Self::Same,
        }
    }

    /// The high-nibble byte value for this hole type.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Same => 0x00,
            Self::Circle => 0x10,
            Self::Square => 0x20,
            Self::Triangle => 0x30,
        }
    }
}

/// The **path** the profile is extruded along (`LL_PCODE_PATH_*`). A straight
/// [`PathCurve::Line`] gives a box / cylinder / prism; a [`PathCurve::Circle`]
/// gives a torus / ring / tube; [`PathCurve::Circle2`] is the sphere path.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PathCurve {
    /// A straight line (`LL_PCODE_PATH_LINE`, `0x10`).
    #[default]
    Line,
    /// A circle (`LL_PCODE_PATH_CIRCLE`, `0x20`) — the torus / tube / ring path.
    Circle,
    /// The second circle path (`LL_PCODE_PATH_CIRCLE2`, `0x30`) — the sphere
    /// path.
    Circle2,
    /// A flexible ("flexi") path (`LL_PCODE_PATH_FLEXIBLE`, `0x80`); tessellated
    /// as a straight line here (softbody flex is a non-goal).
    Flexible,
}

impl PathCurve {
    /// The path from a path byte; unknown values fall back to
    /// [`PathCurve::Line`], matching the viewer's tolerance of out-of-range
    /// curves.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        match byte {
            0x20 => Self::Circle,
            0x30 => Self::Circle2,
            0x80 => Self::Flexible,
            // 0x10 and any unknown value.
            _line => Self::Line,
        }
    }

    /// The path byte value for this curve.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Line => 0x10,
            Self::Circle => 0x20,
            Self::Circle2 => 0x30,
            Self::Flexible => 0x80,
        }
    }

    /// Whether this path is curved (a circle sweep) rather than a straight line;
    /// a curved path's step count follows
    /// [`PrimLod::circle_sides`](crate::PrimLod::circle_sides).
    #[must_use]
    pub const fn is_curved(self) -> bool {
        matches!(self, Self::Circle | Self::Circle2)
    }
}

/// A prim's dequantized, float shape: the tessellation input derived from the
/// wire [`sl_proto::PrimShapeParams`] via [`PrimShape::from_params`]. All cut
/// fields are fractions in `[0, 1]`; twist is in revolutions; taper / shear /
/// scale are ratios; revolutions span `[1, 4]`.
#[derive(Clone, Copy, PartialEq, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `PrimShape` reads clearly"
)]
pub struct PrimShape {
    /// The extrusion path curve.
    pub path_curve: PathCurve,
    /// The swept profile curve (the profile byte's low nibble).
    pub profile_curve: ProfileCurve,
    /// The inner hole curve for a hollow prim (the profile byte's high nibble).
    pub hole_type: HoleType,
    /// The path cut start fraction, `[0, 1]`.
    pub path_begin: f32,
    /// The path cut end fraction, `[0, 1]`.
    pub path_end: f32,
    /// The path top-size (taper-to-here) X ratio; `1.0` is full size.
    pub path_scale_x: f32,
    /// The path top-size Y ratio; `1.0` is full size.
    pub path_scale_y: f32,
    /// The path shear X, roughly `[-0.5, 0.5]`.
    pub path_shear_x: f32,
    /// The path shear Y, roughly `[-0.5, 0.5]`.
    pub path_shear_y: f32,
    /// The twist at the path start, in revolutions (`[-1, 1]`).
    pub twist_begin: f32,
    /// The twist at the path end, in revolutions (`[-1, 1]`).
    pub twist_end: f32,
    /// The path radius offset (torus hole size), roughly `[-1, 1]`.
    pub radius_offset: f32,
    /// The path taper X, `[-1, 1]`.
    pub taper_x: f32,
    /// The path taper Y, `[-1, 1]`.
    pub taper_y: f32,
    /// The number of path revolutions, `[1, 4]`.
    pub revolutions: f32,
    /// The path skew, `[-1, 1]`.
    pub skew: f32,
    /// The profile cut start fraction, `[0, 1]`.
    pub profile_begin: f32,
    /// The profile cut end fraction, `[0, 1]`.
    pub profile_end: f32,
    /// The profile hollow fraction, `[0, 1]` (`0.0` is solid).
    pub hollow: f32,
}

impl PrimShape {
    /// Dequantizes the wire [`sl_proto::PrimShapeParams`] into a float shape,
    /// using exactly the reference viewer's constants and formulas (Firestorm
    /// `LLVolumeMessage::unpackPathParams` / `unpackProfileParams`).
    ///
    /// Note the shear bytes are semantically **signed** (`-0.5..0.5`, quanta
    /// `0.01`) even though the wire — and [`sl_proto::PrimShapeParams`] — types
    /// them `u8`; the viewer reads them as `S8`, so they are reinterpreted here.
    /// Cut and hollow fractions are clamped to `[0, 1]`.
    #[must_use]
    pub fn from_params(params: &PrimShapeParams) -> Self {
        Self {
            path_curve: PathCurve::from_byte(params.path_curve),
            profile_curve: ProfileCurve::from_byte(params.profile_curve),
            hole_type: HoleType::from_byte(params.profile_curve),
            path_begin: clamp_unit(f32::from(params.path_begin) * CUT_QUANTA),
            path_end: clamp_unit((PATH_END_MAX - f32::from(params.path_end)) * CUT_QUANTA),
            path_scale_x: (200.0 - f32::from(params.path_scale_x)) * SCALE_QUANTA,
            path_scale_y: (200.0 - f32::from(params.path_scale_y)) * SCALE_QUANTA,
            path_shear_x: f32::from(reinterpret_signed(params.path_shear_x)) * SHEAR_QUANTA,
            path_shear_y: f32::from(reinterpret_signed(params.path_shear_y)) * SHEAR_QUANTA,
            twist_begin: f32::from(params.path_twist_begin) * SCALE_QUANTA,
            twist_end: f32::from(params.path_twist) * SCALE_QUANTA,
            radius_offset: f32::from(params.path_radius_offset) * SCALE_QUANTA,
            taper_x: f32::from(params.path_taper_x) * TAPER_QUANTA,
            taper_y: f32::from(params.path_taper_y) * TAPER_QUANTA,
            revolutions: f32::from(params.path_revolutions) * REV_QUANTA + 1.0,
            skew: f32::from(params.path_skew) * SCALE_QUANTA,
            profile_begin: clamp_unit(f32::from(params.profile_begin) * CUT_QUANTA),
            profile_end: clamp_unit(1.0 - f32::from(params.profile_end) * CUT_QUANTA),
            hollow: clamp_unit(f32::from(params.profile_hollow) * CUT_QUANTA),
        }
    }

    /// Whether the prim is hollow (has a non-zero inner cutout).
    #[must_use]
    pub fn is_hollow(&self) -> bool {
        self.hollow > 0.0
    }

    /// Whether the profile ring is cut (its begin/end span less than the full
    /// `[0, 1]`), which opens the ring and adds cut-edge faces.
    #[must_use]
    pub fn is_profile_cut(&self) -> bool {
        self.profile_begin > 0.0 || self.profile_end < 1.0
    }

    /// Whether the extrusion path is cut (its begin/end span less than the full
    /// `[0, 1]`), which adds path-end caps partway along the sweep.
    #[must_use]
    pub fn is_path_cut(&self) -> bool {
        self.path_begin > 0.0 || self.path_end < 1.0
    }
}

/// Reinterprets a `u8` wire byte as the signed `i8` it semantically encodes
/// (the viewer's `U8_TO_F32` / `getS8Fast` reads of shear-like fields), without
/// an `as` cast.
const fn reinterpret_signed(byte: u8) -> i8 {
    i8::from_ne_bytes([byte])
}

/// Clamps a dequantized fraction into `[0, 1]`, matching the viewer's clamping
/// of out-of-range cut / hollow values.
const fn clamp_unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{HoleType, PathCurve, PrimShape, ProfileCurve};
    use pretty_assertions::assert_eq;
    use sl_proto::PrimShapeParams;

    /// The absolute tolerance for float comparisons in these tests.
    const EPSILON: f32 = 1.0e-4;

    /// Assert `actual` matches `expected` within [`EPSILON`], avoiding the
    /// exact-float comparison `assert_eq!` would perform.
    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{actual} differs from expected {expected}"
        );
    }

    /// The wire params for the viewer's default new prim (a unit box): square
    /// profile, line path, full top size, no cut / hollow / twist.
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
    fn default_box_dequantizes_to_a_solid_uncut_box() {
        let shape = PrimShape::from_params(&default_box_params());
        assert_eq!(shape.path_curve, PathCurve::Line);
        assert_eq!(shape.profile_curve, ProfileCurve::Square);
        assert_eq!(shape.hole_type, HoleType::Same);
        assert_close(shape.path_begin, 0.0);
        assert_close(shape.path_end, 1.0);
        assert_close(shape.profile_begin, 0.0);
        assert_close(shape.profile_end, 1.0);
        assert_close(shape.hollow, 0.0);
        // path_scale 100 → (200 - 100) * 0.01 = 1.0 (full top size).
        assert_close(shape.path_scale_x, 1.0);
        assert_close(shape.path_scale_y, 1.0);
        // revolutions 0 → 0 * 0.015 + 1 = 1.0.
        assert_close(shape.revolutions, 1.0);
        assert!(!shape.is_hollow());
        assert!(!shape.is_profile_cut());
        assert!(!shape.is_path_cut());
    }

    #[test]
    fn curve_bytes_split_into_profile_and_hole_nibbles() {
        let mut params = default_box_params();
        // Circle profile (0x00) with a circular hole (0x10) → hollow cylinder.
        params.profile_curve = 0x10;
        params.path_curve = 0x20;
        let shape = PrimShape::from_params(&params);
        assert_eq!(shape.profile_curve, ProfileCurve::Circle);
        assert_eq!(shape.hole_type, HoleType::Circle);
        assert_eq!(shape.path_curve, PathCurve::Circle);
        assert!(shape.profile_curve.is_round());
        assert!(shape.path_curve.is_curved());
    }

    #[test]
    fn cut_and_hollow_dequantize_and_flag() {
        let mut params = default_box_params();
        // Half a profile cut: begin at 0.25, end wire 25000 → 1 - 0.5 = 0.5.
        params.profile_begin = 12500;
        params.profile_end = 25000;
        params.profile_hollow = 25000;
        // Path cut: begin 10000 → 0.2, end wire 40000 → (50000-40000)*q = 0.2.
        params.path_begin = 10000;
        params.path_end = 40000;
        let shape = PrimShape::from_params(&params);
        assert_close(shape.profile_begin, 0.25);
        assert_close(shape.profile_end, 0.5);
        assert_close(shape.hollow, 0.5);
        assert_close(shape.path_begin, 0.2);
        assert_close(shape.path_end, 0.2);
        assert!(shape.is_hollow());
        assert!(shape.is_profile_cut());
        assert!(shape.is_path_cut());
    }

    #[test]
    fn signed_fields_dequantize_with_sign() {
        let mut params = default_box_params();
        // Shear is stored u8 but semantically i8: 0xF6 = -10 → -0.1.
        params.path_shear_x = 0xF6;
        params.path_twist = 50;
        params.path_taper_x = -50;
        params.path_revolutions = 100;
        let shape = PrimShape::from_params(&params);
        assert_close(shape.path_shear_x, -0.1);
        assert_close(shape.twist_end, 0.5);
        assert_close(shape.taper_x, -0.5);
        // 100 * 0.015 + 1 = 2.5 revolutions.
        assert_close(shape.revolutions, 2.5);
    }

    #[test]
    fn curve_bytes_round_trip() {
        for profile in [
            ProfileCurve::Circle,
            ProfileCurve::Square,
            ProfileCurve::IsoTriangle,
            ProfileCurve::EqualTriangle,
            ProfileCurve::RightTriangle,
            ProfileCurve::HalfCircle,
        ] {
            assert_eq!(ProfileCurve::from_byte(profile.to_byte()), profile);
        }
        for hole in [
            HoleType::Same,
            HoleType::Circle,
            HoleType::Square,
            HoleType::Triangle,
        ] {
            assert_eq!(HoleType::from_byte(hole.to_byte()), hole);
        }
        for path in [
            PathCurve::Line,
            PathCurve::Circle,
            PathCurve::Circle2,
            PathCurve::Flexible,
        ] {
            assert_eq!(PathCurve::from_byte(path.to_byte()), path);
        }
    }
}
