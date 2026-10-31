//! Library tranche: numbers (`server-lsl-lib-math-rotations`).
//!
//! Second Life's answers are PyOptimizer's (`lslbasefuncs.py`), held to
//! `math/functions`, `math/rotation-bits` and `errors/math` on aditi: the
//! double result of the float argument, rounded once to a float, with
//! Linden's answers at the corners (`llLog(0)` is 0, `llTan(1e30)` hands the
//! argument back, `llRound` adds the half in float). OpenSim's are
//! `LSL_Api.cs`'s: `System.Math` in double, rounded to the value model's
//! float only as the result is stored.

use sl_types::lsl::Vector;

use crate::library::{BuiltinId, CallError, Exception, PureCtx};
use crate::num::{double_to_long, float_to_int, integral_to_long, to_f32, wide_to_double};
use crate::vm::ScriptCtx;

/// The x87 "indefinite" NaN, sign bit set, that Second Life's library answers
/// where the result is undefined (`llSqrt(-1)`, `llSin(INFINITY)`;
/// PyOptimizer's `Indet`). It prints as `NaN` like any other.
const INDETERMINATE: f32 = f32::from_bits(0xFFC0_0000);

/// `llAbs`: the absolute value, where `llAbs(-2147483648)` is
/// `-2147483648` — the magnitude does not fit, so it wraps back (PyOptimizer
/// `llAbs`; `Math.Abs` guarded the same way on OpenSim).
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_abs<C>(_ctx: &mut C, value: i32) -> Result<i32, CallError> {
    Ok(value.wrapping_abs())
}

/// `llFabs`: the absolute value of a float. On Second Life `-0.0` and a
/// negative NaN keep their sign (PyOptimizer `llFabs`: "llFabs(-0.0) is
/// -0.0; llFabs(-nan) is -nan"; `math/functions` prints `-0.000000`), which
/// `f32::abs` would clear; OpenSim's `Math.Abs` clears it (`0.000000`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_fabs<C: PureCtx>(ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(
        if !ctx.flavour().is_opensim() && (value == 0.0 || value.is_nan()) {
            value
        } else {
            value.abs()
        },
    )
}

/// `llFloor`: the greatest integer not above the argument; NaN, an infinity
/// and anything past the integer range are `-2147483648` on every grid
/// (`math/functions`: Second Life's range check, OpenSim's `(int)` of the
/// double on x86).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_floor<C>(_ctx: &mut C, value: f32) -> Result<i32, CallError> {
    Ok(float_to_int(value.floor()))
}

/// `llCeil`: the least integer not below the argument, out of range as
/// [`ll_floor`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ceil<C>(_ctx: &mut C, value: f32) -> Result<i32, CallError> {
    Ok(float_to_int(value.ceil()))
}

/// `llRound`: on Second Life `floor(x + 0.5)` with the sum taken in float,
/// so a half rounds **up** (−2.5 → −2) and `0.49999997` rounds to 1; on
/// OpenSim `Math.Round(x, MidpointRounding.AwayFromZero)` in double (−2.5 →
/// −3, `0.49999997` → 0). Out of range as [`ll_floor`] (`math/functions`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_round<C: PureCtx>(ctx: &mut C, value: f32) -> Result<i32, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        // An integral float's rounding is itself, and below 2^23 the
        // rounding of a float is a float: `f32::round` is the double's.
        float_to_int(value.round())
    } else {
        float_to_int((value + 0.5).floor())
    })
}

/// `llSqrt`: the square root; of a negative number NaN on every grid (no
/// math error under Mono, `errors/math` case 8). `llSqrt(-0.0)` is `-0.0`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sqrt<C: PureCtx>(ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(if value < 0.0 && !ctx.flavour().is_opensim() {
        INDETERMINATE
    } else {
        to_f32(f64::from(value).sqrt())
    })
}

/// `llPow`: `base` to the power `exponent`. Second Life's corners are
/// PyOptimizer's: `0^0` and anything to the 0 is 1, a NaN operand NaN,
/// `0` to a negative power infinite (negative for `-0` to an odd one), `±1`
/// to an infinite power NaN, a negative base to a fractional power NaN, and
/// never `-0.0`; a result past the float range is `Infinity`. OpenSim's
/// `Math.Pow` is IEEE's, in double (`math/functions`, `errors/math` case 13).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_pow<C: PureCtx>(ctx: &mut C, base: f32, exponent: f32) -> Result<f32, CallError> {
    let (b, e) = (f64::from(base), f64::from(exponent));
    if ctx.flavour().is_opensim() {
        return Ok(to_f32(b.powf(e)));
    }
    if base.is_nan() || exponent.is_nan() {
        return Ok(f32::NAN);
    }
    if exponent == 0.0 {
        return Ok(1.0);
    }
    if base == 0.0 {
        if exponent < 0.0 {
            let odd = e.fract() == 0.0 && (e / 2.0).fract() != 0.0;
            return Ok(if base.is_sign_negative() && odd && exponent.is_finite() {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            });
        }
    } else if base.abs().to_bits() == 1.0_f32.to_bits() && exponent.is_infinite() {
        return Ok(f32::NAN);
    }
    if base < 0.0 && e.fract() != 0.0 && exponent.is_finite() {
        return Ok(INDETERMINATE);
    }
    let result = to_f32(b.powf(e));
    Ok(if result == 0.0 { 0.0 } else { result })
}

/// `llLog`: the natural logarithm. On Second Life 0 for zero, a negative
/// number, NaN and `-Infinity` (`errors/math` case 11); OpenSim's `Math.Log`
/// gives `-Infinity` for zero and NaN for a negative number.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_log<C: PureCtx>(ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(logarithm(ctx, value, f64::ln))
}

/// `llLog10`: the decimal logarithm, at the corners as [`ll_log`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_log10<C: PureCtx>(ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(logarithm(ctx, value, f64::log10))
}

/// A logarithm as each grid takes it: Second Life answers 0 where the
/// logarithm is not a finite number or not defined.
fn logarithm<C: PureCtx>(ctx: &C, value: f32, curve: fn(f64) -> f64) -> f32 {
    if !ctx.flavour().is_opensim() && (value.is_nan() || value <= 0.0) {
        return 0.0;
    }
    to_f32(curve(f64::from(value)))
}

/// The argument of Second Life's sine, cosine and tangent: an integral float
/// is reduced modulo 2π the way the x87's `fsin` reduces it, with π's 66-bit
/// approximation (PyOptimizer `reduce`: `t * 2^64 mod 115904311329233965478`
/// over `2^64`); any other float is taken as it is. The caller passes a
/// finite float below `2^63` in magnitude.
fn reduce(value: f32) -> f64 {
    /// `2π` times `2^64`, as the x87 holds it.
    const TWO_PI_SCALED: i128 = 115_904_311_329_233_965_478;
    /// `2^64`.
    const SCALE: f64 = 18_446_744_073_709_551_616.0;
    if value.fract() != 0.0 {
        return f64::from(value);
    }
    let scaled = i128::from(integral_to_long(value)).wrapping_shl(64);
    wide_to_double(scaled.rem_euclid(TWO_PI_SCALED)) / SCALE
}

/// A trigonometric function as each grid takes it: on Second Life an
/// infinity gives the indeterminate NaN, a float past `2^63` in magnitude is
/// handed back as it is (`llTan(1e30)` is `1e30`, `math/functions`), and the
/// rest is reduced as the x87 reduces it; OpenSim's `System.Math` in double.
fn trigonometric<C: PureCtx>(ctx: &C, value: f32, curve: fn(f64) -> f64) -> f32 {
    /// `2^63`, past which the x87 does not reduce its argument.
    const UNREDUCED: f32 = 9_223_372_036_854_775_808.0;
    if ctx.flavour().is_opensim() {
        return to_f32(curve(f64::from(value)));
    }
    if value.is_infinite() {
        return INDETERMINATE;
    }
    if value.is_nan() || value.abs() >= UNREDUCED {
        return value;
    }
    to_f32(curve(reduce(value)))
}

/// `llSin`: the sine of an angle in radians.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sin<C: PureCtx>(ctx: &mut C, angle: f32) -> Result<f32, CallError> {
    Ok(trigonometric(ctx, angle, f64::sin))
}

/// `llCos`: the cosine of an angle in radians.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_cos<C: PureCtx>(ctx: &mut C, angle: f32) -> Result<f32, CallError> {
    Ok(trigonometric(ctx, angle, f64::cos))
}

/// `llTan`: the tangent of an angle in radians (`llTan(PI_BY_TWO)` is
/// −22877332, which `(string)` writes `-22877330.000000`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_tan<C: PureCtx>(ctx: &mut C, angle: f32) -> Result<f32, CallError> {
    Ok(trigonometric(ctx, angle, f64::tan))
}

/// `llAsin`: the arcsine, NaN outside `[-1, 1]` on every grid (`errors/math`
/// case 12).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_asin<C>(_ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(to_f32(f64::from(value).asin()))
}

/// `llAcos`: the arccosine, NaN outside `[-1, 1]` as [`ll_asin`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_acos<C>(_ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(to_f32(f64::from(value).acos()))
}

/// `llAtan2`: the angle of the point `(x, y)`, `y` first; `(0, 0)` is 0 and
/// the sign of a zero `y` picks ±π for a negative `x` (`math/functions`).
/// On Second Life two NaNs give a negative NaN only when both are negative
/// (PyOptimizer `llAtan2`, through its product rule), and one NaN is handed
/// back.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_atan2<C: PureCtx>(ctx: &mut C, y: f32, x: f32) -> Result<f32, CallError> {
    if !ctx.flavour().is_opensim() {
        if x.is_nan() && y.is_nan() {
            return Ok(if x.is_sign_negative() && y.is_sign_negative() {
                -f32::NAN
            } else {
                f32::NAN
            });
        }
        if x.is_nan() {
            return Ok(x);
        }
        if y.is_nan() {
            return Ok(y);
        }
    }
    Ok(to_f32(f64::from(y).atan2(f64::from(x))))
}

/// `llModPow`: `base` to the power `exponent`, modulo `modulus`. Second Life
/// reads all three as **unsigned** 32-bit integers and is exact, with 0 for a
/// modulus of 0 or 1 (`llModPow(-2, 3, 7)` is 1, `(7, 1000000007,
/// 1000000007)` is 7). OpenSim takes `Math.Pow` in a double, converts it to a
/// `long` as x86 does and takes the remainder (−1, −291172004), throwing
/// `DivideByZeroException` for a modulus of 0 (`math/functions`,
/// `errors/math` case 10). The forced second of sleep is the table's.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a modulus of 0, and for a modulus
/// of −1 when the power does not fit a `long`.
pub fn ll_mod_pow<C: PureCtx>(
    ctx: &mut C,
    base: i32,
    exponent: i32,
    modulus: i32,
) -> Result<i32, CallError> {
    let thrown = |exception| CallError::Exception {
        id: BuiltinId::LlModPow,
        exception,
    };
    if ctx.flavour().is_opensim() {
        let power = double_to_long(f64::from(base).powf(f64::from(exponent)));
        let remainder = match modulus {
            0 => return Err(thrown(Exception::DivideByZero)),
            -1 if power == i64::MIN => return Err(thrown(Exception::ArithmeticOverflow)),
            _ => power.checked_rem(i64::from(modulus)).unwrap_or(0),
        };
        // The remainder is smaller than the modulus, so it fits.
        return Ok(i32::try_from(remainder).unwrap_or(0));
    }
    let unsigned = |value: i32| u64::from(value.cast_unsigned());
    let modulus = unsigned(modulus);
    if modulus <= 1 {
        return Ok(0);
    }
    // Every factor is below the modulus, so below 2^32, and no product
    // overflows 64 bits.
    let reduce = |value: u64| value.checked_rem(modulus).unwrap_or(0);
    let mut product = reduce(unsigned(base));
    let mut exponent = unsigned(exponent);
    let mut result = 1_u64;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = reduce(result.wrapping_mul(product));
        }
        exponent = exponent.wrapping_shr(1);
        product = reduce(product.wrapping_mul(product));
    }
    Ok(u32::try_from(result).unwrap_or(0).cast_signed())
}

/// `x` rounded to a float toward zero: the float nearest `x` unless that is
/// further from zero than `x`, then the next float in.
fn truncate_to_float(x: f64) -> f32 {
    let nearest = to_f32(x);
    if f64::from(nearest).abs() > x.abs() && nearest.is_finite() {
        f32::from_bits(nearest.to_bits().saturating_sub(1))
    } else {
        nearest
    }
}

/// `llFrand`: a random float from 0 up to `magnitude`, never `magnitude`
/// itself — below zero for a negative magnitude, always 0 for a magnitude
/// of 0 (`math/functions`: 2000 draws of each, no draw of ±1 ever 0). The
/// draws come from the region's seeded source, 24 bits each, and the product
/// is truncated toward zero to a float so it cannot round up to the
/// magnitude. Second Life answers 0 for an infinite magnitude, a signed zero
/// for a magnitude below the smallest normal float and NaN for NaN
/// (PyOptimizer `llFrand`); OpenSim's `NextDouble() * mag` is taken as it
/// is.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_frand(ctx: &mut ScriptCtx<'_>, magnitude: f32) -> Result<f32, CallError> {
    /// `2^24`: the draw's scale.
    const SCALE: f64 = 16_777_216.0;
    if !ctx.flavour().is_opensim() {
        if magnitude.is_infinite() {
            return Ok(0.0);
        }
        if magnitude.abs() < f32::MIN_POSITIVE {
            return Ok(if magnitude < 0.0 { -0.0 } else { 0.0 });
        }
        if magnitude.is_nan() {
            return Ok(magnitude);
        }
    }
    let unit = f64::from(ctx.random_bits() >> 8_u32) / SCALE;
    Ok(truncate_to_float(unit * f64::from(magnitude)))
}

/// Each component of `vector` through `curve`, in double, rounded to a float.
fn per_component(vector: &Vector, curve: impl Fn(f64) -> f64) -> Vector {
    Vector {
        x: to_f32(curve(f64::from(vector.x))),
        y: to_f32(curve(f64::from(vector.y))),
        z: to_f32(curve(f64::from(vector.z))),
    }
}

/// `llLinear2sRGB`: each component through the sRGB curve, unclamped — `2`
/// is 1.35326 and `-1` is −12.92 (`strings/newer` on aditi). The constants
/// are the floats Second Life computes with, and the power and the product
/// are each rounded to a float (PyOptimizer `llLinear2sRGB`). OpenSim does
/// not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linear2s_rgb<C>(_ctx: &mut C, color: Vector) -> Result<Vector, CallError> {
    let float = |value: f64| f64::from(to_f32(value));
    Ok(per_component(&color, |x| {
        if x <= f64::from(0.003_130_8_f32) {
            f64::from(12.92_f32) * x
        } else {
            float(f64::from(1.055_f32) * float(x.powf(f64::from(0.416_666_66_f32))))
                - f64::from(0.055_f32)
        }
    }))
}

/// `llsRGB2Linear`: each component through the inverse sRGB curve,
/// unclamped (`strings/newer` on aditi; PyOptimizer `llsRGB2Linear`, with
/// its float roundings). OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_s_rgb2_linear<C>(_ctx: &mut C, color: Vector) -> Result<Vector, CallError> {
    let float = |value: f64| f64::from(to_f32(value));
    Ok(per_component(&color, |x| {
        if x <= f64::from(0.040_45_f32) {
            x / f64::from(12.92_f32)
        } else {
            float(float(x + f64::from(0.055_f32)) / f64::from(1.055_f32)).powf(f64::from(2.4_f32))
        }
    }))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::{os, sl};

    #[test]
    fn abs_wraps_at_the_minimum() {
        assert_eq!(ll_abs(&mut (), -5), Ok(5));
        assert_eq!(ll_abs(&mut (), i32::MIN), Ok(i32::MIN));
    }

    #[test]
    fn fabs_keeps_the_sign_of_zero_and_nan() {
        let bits = |value: f32| ll_fabs(&mut sl(), value).map(f32::to_bits);
        assert_eq!(bits(-2.5), Ok(2.5_f32.to_bits()));
        assert_eq!(bits(-0.0), Ok((-0.0_f32).to_bits()));
        assert_eq!(bits(-f32::NAN), Ok((-f32::NAN).to_bits()));
        assert_eq!(bits(f32::NEG_INFINITY), Ok(f32::INFINITY.to_bits()));
        // OpenSim's `Math.Abs` clears the sign.
        assert_eq!(
            ll_fabs(&mut os(), -0.0).map(f32::to_bits),
            Ok(0.0_f32.to_bits())
        );
    }

    /// `math/rotation-bits`' `library` rows on aditi, bit for bit: the double
    /// result of the float argument, rounded once.
    #[test]
    fn the_scalar_functions_are_aditis_to_the_bit() -> Result<(), CallError> {
        // `<hi>p<exp>`: hi × 2^exp, as the probe prints a float.
        let float = |hi: i32, exponent: i32| to_f32(f64::from(hi) * 2.0_f64.powi(exponent));
        let ctx = &mut sl();
        let cases = [
            (ll_sqrt(ctx, 2.0)?, float(11_863_283, -23)),
            (ll_sin(ctx, 1.0)?, float(14_117_540, -24)),
            (ll_cos(ctx, 1.0)?, float(9_064_768, -24)),
            (ll_tan(ctx, 1.0)?, float(13_064_483, -23)),
            (ll_atan2(ctx, 1.0, 3.0)?, float(10_796_157, -25)),
            (ll_pow(ctx, 1.1, 2.5)?, float(10_645_637, -23)),
            (ll_log(ctx, 10.0)?, float(9_657_742, -22)),
            (ll_log10(ctx, 1_000_000.0)?, float(12_582_912, -21)),
            (ll_sin(ctx, 1_000_000.0)?, -float(11_743_833, -25)),
            (ll_asin(ctx, 0.1)?, float(13_444_244, -27)),
        ];
        for (index, (ours, aditi)) in cases.iter().enumerate() {
            assert_eq!(ours.to_bits(), aditi.to_bits(), "case {index}");
        }
        Ok(())
    }

    /// `math/functions` and `errors/math` on aditi and OpenSim (book
    /// `gridspec/lsl.md` § Numbers, § Run-time errors).
    #[test]
    fn the_corners_each_grid_answers() -> Result<(), CallError> {
        let (s, o) = (&mut sl(), &mut os());
        // A half rounds up on Second Life, away from zero on OpenSim.
        for (value, second_life, opensim) in [
            (-0.5, 0, -1),
            (-2.5, -2, -3),
            (2.5, 3, 3),
            (0.499_999_97, 1, 0),
            (2_147_483_648.0, i32::MIN, i32::MIN),
        ] {
            assert_eq!(ll_round(s, value)?, second_life, "{value}");
            assert_eq!(ll_round(o, value)?, opensim, "{value}");
        }
        assert_eq!(ll_floor(s, -1e10)?, i32::MIN);
        assert_eq!(ll_ceil(s, f32::NAN)?, i32::MIN);
        assert!(ll_sqrt(s, -1.0)?.is_nan());
        assert_eq!(ll_sqrt(s, -0.0)?.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(ll_log(s, 0.0)?.to_bits(), 0.0_f32.to_bits());
        assert_eq!(ll_log10(s, -1.0)?.to_bits(), 0.0_f32.to_bits());
        assert_eq!(ll_log(o, 0.0)?.to_bits(), f32::NEG_INFINITY.to_bits());
        assert!(ll_log10(o, -1.0)?.is_nan());
        assert_eq!(ll_pow(s, 0.0, 0.0)?.to_bits(), 1.0_f32.to_bits());
        assert!(ll_pow(s, -8.0, 1.0 / 3.0)?.is_nan());
        assert_eq!(ll_pow(s, 1e30, 2.0)?.to_bits(), f32::INFINITY.to_bits());
        assert_eq!(ll_pow(s, 0.0, -1.0)?.to_bits(), f32::INFINITY.to_bits());
        assert_eq!(ll_tan(s, 1e30)?.to_bits(), 1e30_f32.to_bits());
        assert_eq!(
            ll_atan2(s, -0.0, -1.0)?.to_bits(),
            (-core::f32::consts::PI).to_bits()
        );
        assert!(ll_acos(s, 2.0)?.is_nan());
        // Unsigned and exact on Second Life; a double cut to a long on
        // OpenSim, which divides by a zero modulus.
        for (args, second_life, opensim) in [
            ((-2, 3, 7), 1, -1),
            ((2, -1, 7), 1, 0),
            ((2, 3, -7), 8, 1),
            ((7, 1_000_000_007, 1_000_000_007), 7, -291_172_004),
            ((i32::MAX, i32::MAX, i32::MAX - 1), 1, -8),
            ((3, i32::MAX, 0x0001_0001), 21_846, -0x8001),
        ] {
            let (base, exponent, modulus) = args;
            assert_eq!(ll_mod_pow(s, base, exponent, modulus)?, second_life);
            assert_eq!(ll_mod_pow(o, base, exponent, modulus)?, opensim);
        }
        assert_eq!(ll_mod_pow(s, 2, 3, 0)?, 0);
        assert_eq!(
            ll_mod_pow(o, 2, 3, 0),
            Err(CallError::Exception {
                id: BuiltinId::LlModPow,
                exception: Exception::DivideByZero,
            })
        );
        Ok(())
    }

    #[test]
    fn the_srgb_curves_as_aditi_printed_them() {
        let v = |x: f32, y: f32, z: f32| Vector { x, y, z };
        let printed = |vector: &Vector| crate::format::vector(vector, 5);
        // strings/newer: `(string)` of each answer.
        assert_eq!(
            ll_linear2s_rgb(&mut (), v(0.5, 0.0, 1.0)).map(|v| printed(&v)),
            Ok("<0.73536, 0.00000, 1.00000>".to_owned())
        );
        assert_eq!(
            ll_linear2s_rgb(&mut (), v(2.0, -1.0, 0.003_130_8)).map(|v| printed(&v)),
            Ok("<1.35326, -12.92000, 0.04045>".to_owned())
        );
        assert_eq!(
            ll_s_rgb2_linear(&mut (), v(0.5, 0.0, 1.0)).map(|v| printed(&v)),
            Ok("<0.21404, 0.00000, 1.00000>".to_owned())
        );
        assert_eq!(
            ll_s_rgb2_linear(&mut (), v(2.0, -1.0, 0.040_45)).map(|v| printed(&v)),
            Ok("<4.95385, -0.07740, 0.00313>".to_owned())
        );
    }
}
