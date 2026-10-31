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

/// `(long)double` as .NET on x86-64 converts it (`cvttsd2si`): truncated
/// toward zero, and anything outside the `i64` range — infinities and NaN
/// included — `i64::MIN`, never saturated. OpenSim's `llModPow` goes through
/// it: `(long)Math.Pow(7, 1000000007)` is `i64::MIN`, so its remainder by
/// `1000000007` is `-291172004` (`math/functions` on both OpenSim engines).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the range check above the cast makes the truncation exact"
)]
pub(crate) fn double_to_long(value: f64) -> i64 {
    /// `2^63`, the first magnitude that does not fit.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if (-LIMIT..LIMIT).contains(&value) {
        value as i64
    } else {
        i64::MIN
    }
}

/// An integral `f32` below `2^63` in magnitude as the integer it is.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the caller passes an integral float the `i64` range holds"
)]
pub(crate) const fn integral_to_long(value: f32) -> i64 {
    value as i64
}

/// An integer as the nearest `f64` (round half to even), as Python's
/// `int` → `float` does.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "rounding to the nearest double is the conversion meant"
)]
pub(crate) const fn wide_to_double(value: i128) -> f64 {
    value as f64
}

/// `a + b` and its rounding error: `hi + lo` is exactly `a + b` (Knuth's
/// two-sum).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let hi = a + b;
    let b_part = hi - a;
    let lo = (a - (hi - b_part)) + (b - b_part);
    (hi, lo)
}

/// The correctly rounded sum of `values` — Python's `math.fsum`, which
/// PyOptimizer's library uses for every sum of squares (`llVecMag`,
/// `llVecNorm`, `llRot2Angle` …): Shewchuk's exact partials, then rounded
/// once, half to even. A sum with an infinity or NaN in it is the plain sum.
#[expect(
    clippy::float_cmp,
    reason = "the half-way test asks whether a sum is exact, which is exact equality"
)]
pub(crate) fn fsum(values: &[f64]) -> f64 {
    if values.iter().any(|value| !value.is_finite()) {
        return values.iter().sum();
    }
    let mut partials: Vec<f64> = Vec::new();
    for value in values {
        let mut carried = *value;
        let mut kept = Vec::with_capacity(partials.len().saturating_add(1));
        for partial in &partials {
            let (hi, lo) = two_sum(carried, *partial);
            if lo != 0.0 {
                kept.push(lo);
            }
            carried = hi;
        }
        kept.push(carried);
        partials = kept;
    }
    let Some(mut hi) = partials.pop() else {
        return 0.0;
    };
    let mut lo = 0.0;
    while let Some(next) = partials.pop() {
        let x = hi;
        hi = x + next;
        lo = next - (hi - x);
        if lo != 0.0 {
            break;
        }
    }
    // Half-way between two doubles: the partials below decide which.
    if let Some(below) = partials.last()
        && ((lo < 0.0 && *below < 0.0) || (lo > 0.0 && *below > 0.0))
    {
        let doubled = lo * 2.0;
        let x = hi + doubled;
        if doubled == x - hi {
            hi = x;
        }
    }
    hi
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
    fn fsum_rounds_the_exact_sum_once() {
        assert_eq!(fsum(&[]).to_bits(), 0.0_f64.to_bits());
        assert_eq!(fsum(&[0.1, 0.2, 0.3]).to_bits(), 0.6_f64.to_bits());
        // Plain addition loses the 1 entirely.
        assert_eq!(fsum(&[1e100, 1.0, -1e100]).to_bits(), 1.0_f64.to_bits());
        // Python: math.fsum([1, 1e-16, 1e-16]) == 1.0000000000000002.
        assert_eq!(
            fsum(&[1.0, 1e-16, 1e-16]).to_bits(),
            1.000_000_000_000_000_2_f64.to_bits()
        );
        assert!(fsum(&[f64::INFINITY, 1.0]).is_infinite());
    }

    #[test]
    fn double_to_long_sends_the_out_of_range_to_min() {
        assert_eq!(double_to_long(f64::INFINITY), i64::MIN);
        assert_eq!(double_to_long(f64::NAN), i64::MIN);
        assert_eq!(double_to_long(-1.9), -1);
        assert_eq!(double_to_long(9.3e18), i64::MIN);
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
