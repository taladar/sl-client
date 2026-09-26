//! The three numeric conversions the value model needs and the workspace's
//! lints forbid spelling as a bare `as`: each is the LSL rule, stated once.

/// Round an `f64` to the nearest `f32` — how every LSL float result reaches
/// its single-precision value (PyOptimizer's `F32`). Out-of-range magnitudes
/// become infinities, tiny ones subnormals or zero, as IEEE rounding does.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "rounding to single precision is the LSL float's defining rule"
)]
pub(crate) const fn to_f32(value: f64) -> f32 {
    value as f32
}

/// `(float)integer`: the nearest `f32`, so integers beyond 2^24 lose their
/// low bits (`(float)16777217 == 16777216.0`).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "an LSL integer promotes to the nearest single-precision float"
)]
pub(crate) const fn int_to_float(value: i32) -> f32 {
    value as f32
}

/// `(integer)float`: truncate toward zero; a value outside the `i32` range —
/// and a NaN — is `-2147483648` (PyOptimizer's `InternalTypecast`), never a
/// saturated `2147483647`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the range check above the cast makes the truncation exact"
)]
pub(crate) fn float_to_int(value: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&f64::from(value)) {
        value as i32
    } else {
        i32::MIN
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn float_to_int_truncates_and_sends_the_out_of_range_to_min() {
        // casts.lsl: `(integer)3333333333333.` is `-2147483648`.
        assert_eq!(float_to_int(3_333_333_333_333.0), i32::MIN);
        assert_eq!(float_to_int(2_147_483_648.0), i32::MIN);
        assert_eq!(float_to_int(-2_147_483_648.0), i32::MIN);
        assert_eq!(float_to_int(f32::NAN), i32::MIN);
        assert_eq!(float_to_int(f32::INFINITY), i32::MIN);
        assert_eq!(float_to_int(-1.9), -1);
        assert_eq!(float_to_int(1.9), 1);
        assert_eq!(float_to_int(2_147_483_520.0), 2_147_483_520);
    }

    #[test]
    fn int_to_float_rounds_to_nearest() {
        assert_eq!(
            int_to_float(0x100_0001).to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            int_to_float(i32::MAX).to_bits(),
            2_147_483_648.0_f32.to_bits()
        );
    }
}
