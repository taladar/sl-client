//! How LSL prints a float, a vector and a rotation — the text content parses
//! back out, so every digit is observable.

use core::fmt::Write as _;

use sl_types::lsl::{Rotation, Vector};

/// Decimals of `(string)float`, and of a vector or rotation component inside
/// a list being cast to a string.
pub const FLOAT_DECIMALS: u32 = 6;

/// Decimals of a vector or rotation component in `(string)vector` /
/// `(string)rotation` — one fewer than anywhere else, so `(string)<1,2,3>` is
/// `<1.00000, 2.00000, 3.00000>` while `(string)[<1,2,3>]` has six.
pub const VECTOR_DECIMALS: u32 = 5;

/// `10^exponent` for the seven-digit mantissa arithmetic below.
const fn pow10(exponent: u32) -> u64 {
    let mut result = 1_u64;
    let mut remaining = exponent;
    while remaining > 0 {
        result = result.wrapping_mul(10);
        remaining = remaining.wrapping_sub(1);
    }
    result
}

/// `count` ASCII zeros.
fn zeros(count: i32) -> String {
    "0".repeat(usize::try_from(count).unwrap_or(0))
}

/// Print a float the way Second Life's Mono runtime does, with `decimals`
/// digits after the point (PyOptimizer's `f2s`).
///
/// Mono prints **seven significant digits**, then pads with zeros: so
/// `(string)123456789.0` is `123456800.000000` and `(string)1e28` is
/// `9999999000000000000000000000.000000`. The seven digits come from a
/// round-to-nearest-even conversion, and the cut to `decimals` places then
/// rounds again, half away from zero — which is how `0.0000014999995` prints
/// as `0.000002`. A value that rounds away entirely prints as zero *without*
/// a sign (`0.000000` for `-0.0000004`), while negative zero itself keeps
/// its sign (`-0.000000`). Infinities are `Infinity` / `-Infinity` and every
/// NaN is `NaN`.
#[must_use]
pub fn float(value: f32, decimals: u32) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if value.is_nan() {
        return "NaN".to_owned();
    }
    let places = usize::try_from(decimals).unwrap_or(usize::MAX);
    if value == 0.0 {
        return format!("{value:.places$}");
    }
    let dp = i32::try_from(decimals).unwrap_or(i32::MAX);

    // The first rounding: seven significant digits, round-to-nearest-even.
    let scientific = format!("{:.6e}", f64::from(value));
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let mut exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |rest| ("-", rest));
    // Eight digits: a leading zero (room for a carry) and the seven.
    let mut digits = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .fold(0_u64, |acc, digit| {
            acc.wrapping_mul(10)
                .wrapping_add(u64::from(digit.wrapping_sub(b'0')))
        });
    let leading = digits.checked_div(pow10(6)).unwrap_or(0);

    let cut = dp.saturating_add(1);
    if exponent < cut.saturating_neg() || (exponent == cut.saturating_neg() && leading < 5) {
        return format!("0.{}", zeros(dp));
    }

    // The second rounding, at the first digit the cut drops: index `drop` of
    // the eight, whose place value is `10^(7 - drop)`.
    let drop = dp.saturating_add(exponent).saturating_add(2);
    if let Ok(drop) = u32::try_from(drop)
        && drop <= 7
    {
        let place = pow10(7_u32.saturating_sub(drop));
        if digits.checked_div(place).unwrap_or(0) % 10 >= 5 {
            digits = (digits.checked_div(place).unwrap_or(0) / 10)
                .wrapping_add(1)
                .wrapping_mul(place)
                .wrapping_mul(10);
        }
    }
    // A carry into the leading zero adds a digit before the point.
    let seven = if digits >= pow10(7) {
        exponent = exponent.saturating_add(1);
        digits / 10
    } else {
        digits
    };
    let seven = format!("{seven:07}");

    let body = if exponent >= 6 {
        format!("{seven}{}.{}", zeros(exponent.saturating_sub(6)), zeros(dp))
    } else if exponent < 0 {
        format!(
            "0.{}{seven}",
            zeros(exponent.saturating_neg().saturating_sub(1))
        )
    } else {
        let split = usize::try_from(exponent.saturating_add(1)).unwrap_or(0);
        let (whole, fraction) = seven.split_at(split.min(seven.len()));
        format!(
            "{whole}.{fraction}{}",
            zeros(exponent.saturating_sub(6).saturating_add(dp))
        )
    };
    let point = body.find('.').unwrap_or(body.len());
    let end = point
        .saturating_add(1)
        .saturating_add(places)
        .min(body.len());
    format!("{sign}{}", body.get(..end).unwrap_or(&body))
}

/// `(string)vector` (`decimals` [`VECTOR_DECIMALS`]) or a vector in a list
/// (`decimals` [`FLOAT_DECIMALS`]): `<x, y, z>` with a comma *and a space*.
#[must_use]
pub fn vector(vector: &Vector, decimals: u32) -> String {
    format_components(&[vector.x, vector.y, vector.z], decimals)
}

/// `(string)rotation`: `<x, y, z, s>`, the decimals as for a vector.
#[must_use]
pub fn rotation(rotation: &Rotation, decimals: u32) -> String {
    format_components(&[rotation.x, rotation.y, rotation.z, rotation.s], decimals)
}

/// `<a, b, …>` over [`float`] of each component.
fn format_components(components: &[f32], decimals: u32) -> String {
    let mut text = String::from("<");
    for (index, component) in components.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        let _written = write!(text, "{}", float(*component, decimals));
    }
    text.push('>');
    text
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A float from its IEEE bits, for inputs the expression suite writes as
    /// hex floats.
    const fn bits(raw: u32) -> f32 {
        f32::from_bits(raw)
    }

    #[test]
    fn floats_print_seven_significant_digits_like_mono() {
        // PyOptimizer unit_tests/expr.suite/casts.lsl → casts.out.
        let cases: [(f32, &str); 11] = [
            (-0.5e-6, "-0.000001"),
            (0.5e-6, "0.000001"),
            (-123_456_789.0, "-123456800.000000"),
            (-123_456_784.0, "-123456800.000000"),
            (-123_456_740.0, "-123456700.000000"),
            (-12_345.674, "-12345.670000"),
            (-1.234_567_4, "-1.234567"),
            (-1.234_567_5, "-1.234568"),
            (f32::INFINITY, "Infinity"),
            (f32::NEG_INFINITY, "-Infinity"),
            (f32::NAN, "NaN"),
        ];
        for (value, expected) in cases {
            assert_eq!(float(value, FLOAT_DECIMALS), expected, "{value:e}");
        }
    }

    #[test]
    fn a_value_that_rounds_away_prints_unsigned_zero() {
        // casts.lsl: (string)((float)"-0x1.0C6F78p-21") → "0.000000"; the
        // 0x1.0C6F7A variant rounds up to "-0.000001".
        assert_eq!(float(bits(0xB506_37BC), FLOAT_DECIMALS), "0.000000");
        assert_eq!(float(bits(0x3506_37BC), FLOAT_DECIMALS), "0.000000");
        assert_eq!(float(bits(0xB506_37BD), FLOAT_DECIMALS), "-0.000001");
        // nan-fcast-vcast-minus0.lsl: `-0.` keeps its sign.
        assert_eq!(float(-0.0, FLOAT_DECIMALS), "-0.000000");
    }

    #[test]
    fn powers_of_ten_print_as_mono_does() {
        // casts.lsl `(string)1e-7` … `(string)1e38`, each literal parsed to
        // f32 first; note `1e28`, whose nearest f32 prints as 9999999…
        let expected = [
            "0.000000",
            "0.000001",
            "0.000010",
            "0.000100",
            "0.001000",
            "0.010000",
            "0.100000",
            "1.000000",
            "10.000000",
            "100.000000",
            "1000.000000",
            "10000.000000",
            "100000.000000",
            "1000000.000000",
            "10000000.000000",
            "100000000.000000",
            "1000000000.000000",
            "10000000000.000000",
            "100000000000.000000",
            "1000000000000.000000",
            "10000000000000.000000",
            "100000000000000.000000",
            "1000000000000000.000000",
            "10000000000000000.000000",
            "100000000000000000.000000",
            "1000000000000000000.000000",
            "10000000000000000000.000000",
            "100000000000000000000.000000",
            "1000000000000000000000.000000",
            "10000000000000000000000.000000",
            "100000000000000000000000.000000",
            "1000000000000000000000000.000000",
            "10000000000000000000000000.000000",
            "100000000000000000000000000.000000",
            "1000000000000000000000000000.000000",
            "9999999000000000000000000000.000000",
            "100000000000000000000000000000.000000",
            "1000000000000000000000000000000.000000",
            "10000000000000000000000000000000.000000",
            "100000000000000000000000000000000.000000",
            "1000000000000000000000000000000000.000000",
            "10000000000000000000000000000000000.000000",
            "100000000000000000000000000000000000.000000",
            "1000000000000000000000000000000000000.000000",
            "10000000000000000000000000000000000000.000000",
            "100000000000000000000000000000000000000.000000",
        ];
        for (power, text) in (-7_i32..=38).zip(expected) {
            let value: f32 = format!("1e{power}").parse().unwrap_or(f32::NAN);
            assert_eq!(float(value, FLOAT_DECIMALS), text, "1e{power}");
        }
    }

    #[test]
    fn vectors_print_five_decimals_and_six_in_a_list() {
        // casts.lsl → casts.out.
        let cast = |x: f32, y: f32, z: f32| vector(&Vector { x, y, z }, VECTOR_DECIMALS);
        assert_eq!(
            cast(0.5e-5, -0.5e-5, bits(0xB6A7_C5AC)),
            "<0.00001, -0.00001, -0.00001>"
        );
        assert_eq!(
            cast(bits(0x36A7_C5AC), bits(0xB6A7_C5AB), bits(0x36A7_C5AB)),
            "<0.00001, 0.00000, 0.00000>"
        );
        assert_eq!(
            cast(12_345_675.0, 12_345_674.0, 9.999_999),
            "<12345680.00000, 12345670.00000, 10.00000>"
        );
        assert_eq!(
            rotation(
                &Rotation {
                    x: -123_456_740.0,
                    y: -12_345.674,
                    z: -1.234_567_4,
                    s: -1.234_564,
                },
                VECTOR_DECIMALS
            ),
            "<-123456700.00000, -12345.67000, -1.23457, -1.23456>"
        );
        assert_eq!(
            rotation(
                &Rotation {
                    x: 1_234_567.5,
                    y: 1_234_567.4,
                    z: 123_456.75,
                    s: 123_456.74,
                },
                VECTOR_DECIMALS
            ),
            "<1234568.00000, 1234567.00000, 123456.80000, 123456.70000>"
        );
        assert_eq!(
            vector(
                &Vector {
                    x: -123_456_750.0,
                    y: -12_345.675,
                    z: -12_345.676,
                },
                FLOAT_DECIMALS
            ),
            "<-123456800.000000, -12345.670000, -12345.680000>"
        );
        assert_eq!(
            vector(
                &Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                VECTOR_DECIMALS
            ),
            "<0.00000, 0.00000, 0.00000>"
        );
    }
}
