//! Library tranche: numbers (`server-lsl-lib-math-rotations`).

use sl_types::lsl::Vector;

use crate::library::CallError;
use crate::num::to_f32;

/// `llAbs`: the absolute value, where `llAbs(-2147483648)` is
/// `-2147483648` — the magnitude does not fit, so it wraps back (PyOptimizer
/// `llAbs`).
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_abs<C>(_ctx: &mut C, value: i32) -> Result<i32, CallError> {
    Ok(value.wrapping_abs())
}

/// `llFabs`: the absolute value of a float — except that `-0.0` and a
/// negative NaN keep their sign (PyOptimizer `llFabs`: "llFabs(-0.0) is
/// -0.0; llFabs(-nan) is -nan"), which `f32::abs` would clear.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_fabs<C>(_ctx: &mut C, value: f32) -> Result<f32, CallError> {
    Ok(if value == 0.0 || value.is_nan() {
        value
    } else {
        value.abs()
    })
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

    #[test]
    fn abs_wraps_at_the_minimum() {
        assert_eq!(ll_abs(&mut (), -5), Ok(5));
        assert_eq!(ll_abs(&mut (), i32::MIN), Ok(i32::MIN));
    }

    #[test]
    fn fabs_keeps_the_sign_of_zero_and_nan() {
        let bits = |value: f32| ll_fabs(&mut (), value).map(f32::to_bits);
        assert_eq!(bits(-2.5), Ok(2.5_f32.to_bits()));
        assert_eq!(bits(-0.0), Ok((-0.0_f32).to_bits()));
        assert_eq!(bits(-f32::NAN), Ok((-f32::NAN).to_bits()));
        assert_eq!(bits(f32::NEG_INFINITY), Ok(f32::INFINITY.to_bits()));
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
