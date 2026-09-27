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

/// How many ticks of length `step` a suspension of `seconds` lasts: the
/// duration rounded to whole microseconds first — so an `f32` argument such as
/// `0.2`, which is a hair above a fifth, is not taken for the next tick —
/// then rounded **up** to whole ticks, since a script must not wake before its
/// time. Not positive, or NaN, is no suspension; a step of zero counts as one
/// microsecond.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is rounded, positive and clamped below 2^64 first"
)]
pub(crate) fn ticks_for(seconds: f64, step: core::time::Duration) -> u64 {
    /// `2^64` as a float: the first microsecond count that does not fit.
    const LIMIT: f64 = 18_446_744_073_709_551_616.0;
    if seconds.is_nan() || seconds <= 0.0 {
        return 0;
    }
    let micros = (seconds * 1_000_000.0).round();
    let micros = if micros >= LIMIT {
        u64::MAX
    } else {
        micros as u64
    };
    let step = u64::try_from(step.as_micros()).unwrap_or(u64::MAX).max(1);
    micros.div_ceil(step)
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_suspension_rounds_up_to_whole_ticks_and_not_past_them() {
        let step = Duration::from_millis(100);
        assert_eq!(ticks_for(2.0, step), 20);
        // `0.2_f32` is 0.20000000298…: still two ticks, not three.
        assert_eq!(ticks_for(f64::from(0.2_f32), step), 2);
        assert_eq!(ticks_for(0.25, step), 3);
        assert_eq!(ticks_for(0.000_001, step), 1);
        assert_eq!(ticks_for(0.0, step), 0);
        assert_eq!(ticks_for(-1.0, step), 0);
        assert_eq!(ticks_for(f64::NAN, step), 0);
        assert_eq!(ticks_for(f64::INFINITY, step), u64::MAX.div_ceil(100_000));
    }

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
