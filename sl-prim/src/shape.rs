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

use crate::LodScaleBias;

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
    /// The test path (`LL_PCODE_PATH_TEST`, `0x40`): five frames curling the
    /// profile through the end twist about X. No build tool makes one, but the
    /// reference still generates it, so a prim that carries it renders.
    Test,
    /// A flexible ("flexi") path (`LL_PCODE_PATH_FLEXIBLE`, `0x80`); tessellated
    /// as a straight line here (softbody flex is a non-goal).
    Flexible,
}

/// The `LL_PCODE_PATH_FLEXIBLE` byte, which the reference recognises only by
/// an exact comparison.
const PATH_FLEXIBLE: u8 = 0x80;

/// The `LL_PCODE_PATH_LINE` byte, which a constrained path byte falls back to.
const PATH_LINE: u8 = 0x10;

/// The highest profile nibble the reference accepts (`LL_PCODE_PROFILE_MAX`).
const PROFILE_NIBBLE_MAX: u8 = 0x05;

/// The highest hole nibble the reference accepts, shifted down
/// (`LL_PCODE_HOLE_MAX`).
const HOLE_NIBBLE_MAX: u8 = 0x03;

/// The lowest and highest path nibbles the reference accepts, shifted down
/// (`LL_PCODE_PATH_MIN` / `LL_PCODE_PATH_MAX`).
const PATH_NIBBLE_RANGE: core::ops::RangeInclusive<u8> = 0x01..=0x08;

/// The `LL_PCODE_PROFILE_SQUARE` byte, which a bad profile nibble becomes.
const PROFILE_SQUARE: u8 = 0x01;

/// A received profile byte as the reference keeps it: `LLVolumeParams::setType`
/// (run on every unpack by `constrainVolumeParams`) turns a bad profile nibble
/// into a plain square, **hole bits and all**, and a bad hole nibble into no
/// hole. So `0x26` is a solid square and `0x41` a square with no hole.
const fn constrain_profile_byte(byte: u8) -> u8 {
    if byte & 0x0f > PROFILE_NIBBLE_MAX {
        PROFILE_SQUARE
    } else if byte >> 4 > HOLE_NIBBLE_MAX {
        byte & 0x0f
    } else {
        byte
    }
}

/// A received path byte as the reference keeps it: `LLVolumeParams::setType`
/// makes a path whose high nibble lies outside `1..=8` a plain line and keeps
/// every other byte whole, low bits included.
const fn constrain_path_byte(byte: u8) -> u8 {
    let nibble = byte >> 4;
    if nibble < *PATH_NIBBLE_RANGE.start() || nibble > *PATH_NIBBLE_RANGE.end() {
        PATH_LINE
    } else {
        byte
    }
}

/// Whether a prim's path byte was exactly its [`PathCurve`]'s own byte.
///
/// The generators decode the path by its high nibble, so `0x10`, `0x11` and
/// `0x50` all extrude along a line. A few shortcuts in the reference compare
/// the **whole** byte instead (`== LL_PCODE_PATH_LINE`): the edge split and
/// the level-of-detail bias in `LLVolume::generate`, the plain box cap in
/// `LLVolumeFace::createCap` and the half-circle pole normals in
/// `createSide`. Those fire only for an [`Exact`](Self::Exact) byte.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PathByteForm {
    /// The byte is the curve's canonical byte ([`PathCurve::to_byte`]).
    #[default]
    Exact,
    /// The byte decodes to the curve but is not its canonical byte: low bits
    /// are set, or the line is a nibble the generator only falls back to.
    Variant,
}

impl PathCurve {
    /// The path from a path byte, decoded the way the reference viewer does:
    /// `LLPath::generate` switches on the high nibble (`byte & 0xf0`), so
    /// `0x21` is a circle, and every nibble it has no case for falls back to
    /// [`PathCurve::Line`]. A flexible path is the one exception — the
    /// reference tests `== LL_PCODE_PATH_FLEXIBLE` on the whole byte
    /// (`llvolume.cpp`, `llvovolume.cpp`), so only `0x80` itself is flexi and
    /// `0x81` is a line.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        if byte == PATH_FLEXIBLE {
            return Self::Flexible;
        }
        match byte & 0xf0 {
            0x20 => Self::Circle,
            0x30 => Self::Circle2,
            0x40 => Self::Test,
            // 0x10, and any nibble the reference's switch has no case for.
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
            Self::Test => 0x40,
            Self::Flexible => PATH_FLEXIBLE,
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
    /// Whether the path byte was exactly `path_curve`'s own byte, which the
    /// reference's whole-byte shortcuts need (see [`PathByteForm`]).
    pub path_byte_form: PathByteForm,
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
    ///
    /// The curve bytes are first constrained the way the reference constrains
    /// every unpacked shape (`LLVolumeParams::setType`): a bad profile nibble
    /// becomes a square with no hole, a bad hole nibble no hole, and a path
    /// nibble outside `1..=8` a line. The kinds are then decoded from those
    /// bytes, and [`path_byte_form`](Self::path_byte_form) records whether the
    /// path byte was exactly its curve's.
    #[must_use]
    pub fn from_params(params: &PrimShapeParams) -> Self {
        let path_byte = constrain_path_byte(params.path_curve);
        let profile_byte = constrain_profile_byte(params.profile_curve);
        let path_curve = PathCurve::from_byte(path_byte);
        let path_byte_form = if path_byte == path_curve.to_byte() {
            PathByteForm::Exact
        } else {
            PathByteForm::Variant
        };
        Self {
            path_curve,
            path_byte_form,
            profile_curve: ProfileCurve::from_byte(profile_byte),
            hole_type: HoleType::from_byte(profile_byte),
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

    /// Quantizes the float shape back into the wire
    /// [`sl_proto::PrimShapeParams`], the inverse of
    /// [`from_params`](Self::from_params) and a port of the reference viewer's
    /// `LLVolumeMessage::packPathParams` / `packProfileParams`.
    ///
    /// Rounding is to the nearest wire integer (the reference's `ll_round`, not
    /// a truncation — the comment above its own `pack_scale_x` records why:
    /// truncating `0.50 / 0.01` yields 49, not 50). The pass is lossy by
    /// construction, since the wire form has 0.01 or 0.00002 of resolution
    /// where the float has an `f32`'s; a value that came from
    /// [`from_params`](Self::from_params) survives the round trip exactly.
    ///
    /// Needed wherever a *float* shape has to be put on the wire or into a
    /// message: a simulator answering a rez, an object asset being turned back
    /// into an `ObjectUpdate`, or a viewer packing an edit.
    #[must_use]
    pub fn to_params(&self) -> PrimShapeParams {
        PrimShapeParams {
            path_curve: self.path_curve.to_byte(),
            profile_curve: self.profile_curve.to_byte() | self.hole_type.to_byte(),
            path_begin: round_to_u16(self.path_begin / CUT_QUANTA),
            path_end: PATH_END_WIRE.saturating_sub(round_to_u16(self.path_end / CUT_QUANTA)),
            path_scale_x: FULL_SCALE.saturating_sub(round_to_u8(self.path_scale_x / SCALE_QUANTA)),
            path_scale_y: FULL_SCALE.saturating_sub(round_to_u8(self.path_scale_y / SCALE_QUANTA)),
            path_shear_x: reinterpret_unsigned(round_to_i8(self.path_shear_x / SHEAR_QUANTA)),
            path_shear_y: reinterpret_unsigned(round_to_i8(self.path_shear_y / SHEAR_QUANTA)),
            path_twist: round_to_i8(self.twist_end / SCALE_QUANTA),
            path_twist_begin: round_to_i8(self.twist_begin / SCALE_QUANTA),
            path_radius_offset: round_to_i8(self.radius_offset / SCALE_QUANTA),
            path_taper_x: round_to_i8(self.taper_x / TAPER_QUANTA),
            path_taper_y: round_to_i8(self.taper_y / TAPER_QUANTA),
            path_revolutions: round_to_u8((self.revolutions - 1.0) / REV_QUANTA),
            path_skew: round_to_i8(self.skew / SCALE_QUANTA),
            profile_begin: round_to_u16(self.profile_begin / CUT_QUANTA),
            profile_end: PATH_END_WIRE.saturating_sub(round_to_u16(self.profile_end / CUT_QUANTA)),
            profile_hollow: round_to_u16(self.hollow / CUT_QUANTA),
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

    /// Whether the path byte is exactly `curve`'s byte — the reference's
    /// `getPathParams().getCurveType() == LL_PCODE_PATH_*`, as opposed to the
    /// generators' high-nibble decode.
    #[must_use]
    pub fn path_is_exactly(&self, curve: PathCurve) -> bool {
        self.path_curve == curve && self.path_byte_form == PathByteForm::Exact
    }

    /// Whether the profile byte is exactly `curve`'s byte — the reference's
    /// `getProfileParams().getCurveType() == LL_PCODE_PROFILE_*`, which
    /// compares the hole nibble too, so a square with a named hole (`0x21`) is
    /// not a square here.
    #[must_use]
    pub fn profile_is_exactly(&self, curve: ProfileCurve) -> bool {
        self.profile_curve == curve && self.hole_type == HoleType::Same
    }

    /// The level-of-detail scale bias the reference gives this shape
    /// (`LLVolume::generate`'s `mLODScaleBias`): a cylinder ignores its height,
    /// a circle path weighs every axis a little more, and everything else
    /// halves the scale. Both shape tests compare whole bytes.
    #[must_use]
    pub fn lod_scale_bias(&self) -> LodScaleBias {
        if self.path_is_exactly(PathCurve::Line) && self.profile_is_exactly(ProfileCurve::Circle) {
            LodScaleBias::CYLINDER
        } else if self.path_is_exactly(PathCurve::Circle) {
            LodScaleBias::CIRCLE_PATH
        } else {
            LodScaleBias::DEFAULT
        }
    }
}

/// Reinterprets a `u8` wire byte as the signed `i8` it semantically encodes
/// (the viewer's `U8_TO_F32` / `getS8Fast` reads of shear-like fields), without
/// an `as` cast.
const fn reinterpret_signed(byte: u8) -> i8 {
    i8::from_ne_bytes([byte])
}

/// Reinterprets a signed wire byte as the `u8` the wire types it (the inverse
/// of `reinterpret_signed`), without an `as` cast.
const fn reinterpret_unsigned(byte: i8) -> u8 {
    byte.cast_unsigned()
}

/// Clamps a dequantized fraction into `[0, 1]`, matching the viewer's clamping
/// of out-of-range cut / hollow values.
const fn clamp_unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// Rounds a quantum-count to the nearest `u16`, saturating at the ends of the
/// range — the reference's `(U16) ll_round(x)`, minus its undefined behaviour
/// for a value the type cannot hold.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is rounded and clamped into 0..=u16::MAX first, so the conversion is exact"
)]
const fn round_to_u16(value: f32) -> u16 {
    value.round().clamp(0.0, U16_MAX_AS_F32) as u16
}

/// Rounds a quantum-count to the nearest `u8`, saturating (`(U8) ll_round(x)`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is rounded and clamped into 0..=u8::MAX first, so the conversion is exact"
)]
const fn round_to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, U8_MAX_AS_F32) as u8
}

/// Rounds a quantum-count to the nearest `i8`, saturating (`(S8) ll_round(x)`).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped into i8::MIN..=i8::MAX first, so the conversion is \
              exact"
)]
const fn round_to_i8(value: f32) -> i8 {
    value.round().clamp(I8_MIN_AS_F32, I8_MAX_AS_F32) as i8
}

/// `u16::MAX` as the `f32` the rounding helpers clamp against.
const U16_MAX_AS_F32: f32 = 65_535.0;

/// `u8::MAX` as the `f32` the rounding helpers clamp against.
const U8_MAX_AS_F32: f32 = 255.0;

/// `i8::MIN` as the `f32` the rounding helpers clamp against.
const I8_MIN_AS_F32: f32 = -128.0;

/// `i8::MAX` as the `f32` the rounding helpers clamp against.
const I8_MAX_AS_F32: f32 = 127.0;

/// The wire integer for a fully open path/profile end, as the integer the
/// packers subtract from (`50000`, the integer twin of `PATH_END_MAX`).
const PATH_END_WIRE: u16 = 50_000;

/// The wire byte for a top size equal to the bottom (`200`), which the scale
/// packers subtract the quantized size from.
const FULL_SCALE: u8 = 200;

#[cfg(test)]
mod tests {
    use super::{HoleType, PathByteForm, PathCurve, PrimShape, ProfileCurve};
    use crate::LodScaleBias;
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
            PathCurve::Test,
            PathCurve::Flexible,
        ] {
            assert_eq!(PathCurve::from_byte(path.to_byte()), path);
        }
    }

    /// A path byte decodes by its high nibble, as the reference's
    /// `LLPath::generate` switch does: low bits are ignored, `0x40` is the
    /// test path and an uncovered nibble is a line — except flexi, which the
    /// reference matches on the whole byte.
    #[test]
    fn path_curve_decodes_the_high_nibble_but_flexi_exactly() {
        for (byte, expected) in [
            (0x00, PathCurve::Line),
            (0x10, PathCurve::Line),
            (0x1f, PathCurve::Line),
            (0x20, PathCurve::Circle),
            (0x21, PathCurve::Circle),
            (0x2f, PathCurve::Circle),
            (0x30, PathCurve::Circle2),
            (0x3a, PathCurve::Circle2),
            (0x40, PathCurve::Test),
            (0x45, PathCurve::Test),
            (0x50, PathCurve::Line),
            (0x70, PathCurve::Line),
            (0x80, PathCurve::Flexible),
            (0x81, PathCurve::Line),
            (0x90, PathCurve::Line),
            (0xf0, PathCurve::Line),
        ] {
            assert_eq!(PathCurve::from_byte(byte), expected, "byte {byte:#04x}");
        }
    }

    /// The received curve bytes are constrained as `LLVolumeParams::setType`
    /// constrains them before anything decodes them: a bad profile nibble is a
    /// square with no hole, a bad hole nibble no hole, and a path nibble outside
    /// `1..=8` an exact line. Whether the kept path byte was its curve's own is
    /// recorded.
    #[test]
    fn curve_bytes_are_constrained_like_set_type() {
        for (profile_byte, profile, hole) in [
            (0x21, ProfileCurve::Square, HoleType::Square),
            (0x26, ProfileCurve::Square, HoleType::Same),
            (0x3f, ProfileCurve::Square, HoleType::Same),
            (0x43, ProfileCurve::EqualTriangle, HoleType::Same),
            (0x15, ProfileCurve::HalfCircle, HoleType::Circle),
        ] {
            let shape = PrimShape::from_params(&PrimShapeParams {
                profile_curve: profile_byte,
                ..default_box_params()
            });
            assert_eq!(
                (shape.profile_curve, shape.hole_type),
                (profile, hole),
                "profile byte {profile_byte:#04x}"
            );
        }
        for (path_byte, curve, form) in [
            (0x10, PathCurve::Line, PathByteForm::Exact),
            (0x05, PathCurve::Line, PathByteForm::Exact),
            (0x95, PathCurve::Line, PathByteForm::Exact),
            (0x11, PathCurve::Line, PathByteForm::Variant),
            (0x50, PathCurve::Line, PathByteForm::Variant),
            (0x20, PathCurve::Circle, PathByteForm::Exact),
            (0x21, PathCurve::Circle, PathByteForm::Variant),
            (0x40, PathCurve::Test, PathByteForm::Exact),
            (0x80, PathCurve::Flexible, PathByteForm::Exact),
            (0x81, PathCurve::Line, PathByteForm::Variant),
        ] {
            let shape = PrimShape::from_params(&PrimShapeParams {
                path_curve: path_byte,
                ..default_box_params()
            });
            assert_eq!(
                (shape.path_curve, shape.path_byte_form),
                (curve, form),
                "path byte {path_byte:#04x}"
            );
        }
    }

    /// The exact-byte predicates compare what the reference's `==` compares:
    /// the hole nibble for a profile, the low bits for a path.
    #[test]
    fn exact_curve_predicates_compare_whole_bytes() {
        let shape = |profile_curve: u8, path_curve: u8| {
            PrimShape::from_params(&PrimShapeParams {
                profile_curve,
                path_curve,
                ..default_box_params()
            })
        };
        assert!(shape(0x01, 0x10).profile_is_exactly(ProfileCurve::Square));
        assert!(!shape(0x21, 0x10).profile_is_exactly(ProfileCurve::Square));
        assert!(shape(0x01, 0x10).path_is_exactly(PathCurve::Line));
        assert!(!shape(0x01, 0x11).path_is_exactly(PathCurve::Line));
        assert!(!shape(0x01, 0x10).path_is_exactly(PathCurve::Circle));
    }

    /// The level-of-detail bias follows `LLVolume::generate`: a cylinder by
    /// whole bytes ignores its height, an exact circle path weighs every axis
    /// by 0.6, and anything else — a named hole on the cylinder, low bits on
    /// the circle path, a box — halves the scale.
    #[test]
    fn lod_scale_bias_follows_the_whole_curve_bytes() {
        let bias = |profile_curve: u8, path_curve: u8| {
            PrimShape::from_params(&PrimShapeParams {
                profile_curve,
                path_curve,
                ..default_box_params()
            })
            .lod_scale_bias()
        };
        assert_eq!(bias(0x00, 0x10), LodScaleBias::CYLINDER);
        assert_eq!(bias(0x00, 0x20), LodScaleBias::CIRCLE_PATH);
        assert_eq!(bias(0x05, 0x20), LodScaleBias::CIRCLE_PATH);
        assert_eq!(bias(0x10, 0x10), LodScaleBias::DEFAULT);
        assert_eq!(bias(0x00, 0x11), LodScaleBias::DEFAULT);
        assert_eq!(bias(0x00, 0x21), LodScaleBias::DEFAULT);
        assert_eq!(bias(0x01, 0x10), LodScaleBias::DEFAULT);
        assert_eq!(bias(0x00, 0x30), LodScaleBias::DEFAULT);
    }
}
