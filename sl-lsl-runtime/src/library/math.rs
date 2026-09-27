//! Library tranche: numbers (`server-lsl-lib-math-rotations`).

use crate::library::CallError;

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
}
