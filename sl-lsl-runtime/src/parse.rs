//! The lenient string parsers behind `(integer)`, `(float)`, `(vector)` and
//! `(rotation)` of a string.
//!
//! None of them fails: each reads the longest number-shaped prefix it can and
//! gives up on the rest, and a string with no such prefix is zero. The rules
//! are PyOptimizer's `InternalTypecast` and its `int_re` / `float_re` /
//! `vfloat_re` patterns, which were measured against Second Life.

use sl_types::lsl::{Rotation, Vector};

use crate::num::to_f32;
use crate::value::{ZERO_ROTATION, ZERO_VECTOR};

/// Whitespace as the reference's parsers skip it: the six ASCII characters of
/// C's `isspace` (so vertical tab and form feed too, and no Unicode spaces).
const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// `bytes` with the leading [`is_space`] run removed.
fn skip_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !is_space(*byte))
        .unwrap_or(bytes.len());
    bytes.get(start..).unwrap_or_default()
}

/// The length of the leading run of bytes matching `accept`.
fn run(bytes: &[u8], accept: impl Fn(u8) -> bool) -> usize {
    bytes
        .iter()
        .position(|byte| !accept(*byte))
        .unwrap_or(bytes.len())
}

/// `bytes` from `offset`, or empty past the end.
fn from(bytes: &[u8], offset: usize) -> &[u8] {
    bytes.get(offset..).unwrap_or_default()
}

/// Whether `bytes` starts with `prefix`, ignoring ASCII case.
fn starts_with_ignore_case(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The value of an ASCII hex digit.
fn hex_value(byte: u8) -> Option<u64> {
    char::from(byte).to_digit(16).map(u64::from)
}

/// `(integer)text`.
///
/// Either `0x` (or `0X`) and hex digits at the very start — no whitespace or
/// sign before it — or optional whitespace, an optional sign and decimal
/// digits; the rest of the string is ignored (`(integer)"3.14e+0a"` is `3`,
/// `(integer)"0XA3.14e+0a"` is `163`). The number read is kept to 32 bits,
/// but one whose magnitude exceeds `4294967295` is `-1` instead
/// (`(integer)"4294967296"` and `(integer)"3333333333333"` are both `-1`,
/// `(integer)"4294967295"` is `-1` by wrapping and `(integer)"-4294967295"`
/// is `1`).
#[must_use]
pub fn integer(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let (negative, digits, radix) = if starts_with_ignore_case(bytes, b"0x")
        && from(bytes, 2).first().is_some_and(u8::is_ascii_hexdigit)
    {
        (false, from(bytes, 2), 16)
    } else {
        let rest = skip_space(bytes);
        let (negative, rest) = match rest.first() {
            Some(b'-') => (true, from(rest, 1)),
            Some(b'+') => (false, from(rest, 1)),
            _ => (false, rest),
        };
        (negative, rest, 10)
    };
    let length = run(digits, |byte| char::from(byte).is_digit(radix));
    if length == 0 {
        return 0;
    }
    // Accumulate while the magnitude can still land in range; anything past
    // 2^32 - 1 is the `-1` case whatever follows.
    let mut magnitude = 0_u64;
    for byte in digits.iter().take(length) {
        let digit = char::from(*byte).to_digit(radix).map_or(0, u64::from);
        magnitude = magnitude
            .saturating_mul(u64::from(radix))
            .saturating_add(digit);
        if magnitude > u64::from(u32::MAX) {
            return -1;
        }
    }
    // Two's-complement wrap of the 32-bit pattern.
    let low = u32::try_from(magnitude).unwrap_or(u32::MAX);
    let wrapped = low.cast_signed();
    if negative {
        wrapped.wrapping_neg()
    } else {
        wrapped
    }
}

/// Which spelling of infinity a float parser accepts.
#[derive(Clone, Copy)]
enum Infinity {
    /// `(float)`: `inf`, which also reads the first three letters of
    /// `infinity`.
    Scalar,
    /// A vector or rotation component: `infinity`, or `inf` when *not*
    /// followed by an `i` — so `<1,1,info` reads infinity and `<1,1,infix`
    /// reads nothing.
    Component,
}

/// Read a float from the start of `bytes`: optional whitespace, optional
/// sign, then a hex float (`0x1.8p3`), a decimal float (`1.5e3`, `.5`, `5.`),
/// an infinity or `nan`. Returns the value and the bytes consumed, or
/// [`None`] when no float starts here.
fn read_float(bytes: &[u8], infinity: Infinity) -> Option<(f32, usize)> {
    let lead = bytes.len().saturating_sub(skip_space(bytes).len());
    let mut at = lead;
    let negative = match bytes.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };
    let body = from(bytes, at);
    let (magnitude, length) = read_hex_float(body)
        .or_else(|| read_decimal_float(body))
        .or_else(|| read_special(body, infinity))?;
    let value = if negative { -magnitude } else { magnitude };
    Some((value, at.saturating_add(length)))
}

/// `0x` followed by hex digits with an optional fraction (at least one digit
/// somewhere) and an optional binary exponent `p±n`.
fn read_hex_float(bytes: &[u8]) -> Option<(f32, usize)> {
    if !starts_with_ignore_case(bytes, b"0x") {
        return None;
    }
    let mut at = 2_usize;
    let whole = run(from(bytes, at), |byte| byte.is_ascii_hexdigit());
    let mut mantissa = 0_u64;
    let mut scale = 0_i32;
    // Keep 60 bits of mantissa; later digits only move the exponent (whole
    // part) or vanish (fraction) — far below f32 precision either way.
    let mut push = |digit: u64, in_fraction: bool| {
        if mantissa < (1_u64 << 56) {
            mantissa = mantissa.wrapping_mul(16).wrapping_add(digit);
            if in_fraction {
                scale = scale.saturating_sub(4);
            }
        } else if !in_fraction {
            scale = scale.saturating_add(4);
        }
    };
    for byte in from(bytes, at).iter().take(whole) {
        push(hex_value(*byte).unwrap_or(0), false);
    }
    at = at.saturating_add(whole);
    let mut fraction = 0_usize;
    if from(bytes, at).first() == Some(&b'.') {
        fraction = run(from(bytes, at.saturating_add(1)), |byte| {
            byte.is_ascii_hexdigit()
        });
        if whole > 0 || fraction > 0 {
            for byte in from(bytes, at.saturating_add(1)).iter().take(fraction) {
                push(hex_value(*byte).unwrap_or(0), true);
            }
            at = at.saturating_add(1).saturating_add(fraction);
        }
    }
    if whole == 0 && fraction == 0 {
        return None;
    }
    if let Some((exponent, length)) = read_exponent(from(bytes, at), b'p') {
        scale = scale.saturating_add(exponent);
        at = at.saturating_add(length);
    }
    Some((to_f32(scale_by_power_of_two(mantissa, scale)), at))
}

/// `mantissa × 2^scale` in `f64`, stepping the scale so neither the power
/// nor an intermediate overflows or flushes early.
fn scale_by_power_of_two(mantissa: u64, scale: i32) -> f64 {
    let high = u32::try_from(mantissa >> 32).unwrap_or(u32::MAX);
    let low = u32::try_from(mantissa & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    let mut value = f64::from(high) * 4_294_967_296.0 + f64::from(low);
    let mut remaining = scale;
    while remaining > 0 {
        let step = remaining.min(1000);
        value *= 2.0_f64.powi(step);
        remaining = remaining.saturating_sub(step);
    }
    while remaining < 0 {
        let step = remaining.max(-1000);
        value *= 2.0_f64.powi(step);
        remaining = remaining.saturating_sub(step);
    }
    value
}

/// An exponent marker (`e` or `p`, either case), an optional sign and at
/// least one decimal digit. Returns the exponent and the bytes consumed.
fn read_exponent(bytes: &[u8], marker: u8) -> Option<(i32, usize)> {
    if !bytes.first()?.eq_ignore_ascii_case(&marker) {
        return None;
    }
    let (negative, sign) = match bytes.get(1) {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let digits_at = 1_usize.saturating_add(sign);
    let length = run(from(bytes, digits_at), |byte| byte.is_ascii_digit());
    if length == 0 {
        return None;
    }
    let magnitude = from(bytes, digits_at)
        .iter()
        .take(length)
        .fold(0_i32, |acc, byte| {
            acc.saturating_mul(10)
                .saturating_add(i32::from(byte.wrapping_sub(b'0')))
        });
    let exponent = if negative {
        magnitude.saturating_neg()
    } else {
        magnitude
    };
    Some((exponent, digits_at.saturating_add(length)))
}

/// Decimal digits with an optional fraction (at least one digit somewhere)
/// and an optional exponent `e±n`. The text is read to an `f64` and rounded
/// to `f32` from there, as the reference does.
fn read_decimal_float(bytes: &[u8]) -> Option<(f32, usize)> {
    let whole = run(bytes, |byte| byte.is_ascii_digit());
    let mut at = whole;
    if from(bytes, at).first() == Some(&b'.') {
        let fraction = run(from(bytes, at.saturating_add(1)), |byte| {
            byte.is_ascii_digit()
        });
        if whole > 0 || fraction > 0 {
            at = at.saturating_add(1).saturating_add(fraction);
        }
    }
    if at == 0 {
        return None;
    }
    if let Some((_, length)) = read_exponent(from(bytes, at), b'e') {
        at = at.saturating_add(length);
    }
    let text = core::str::from_utf8(bytes.get(..at)?).ok()?;
    let value: f64 = text.parse().ok()?;
    Some((to_f32(value), at))
}

/// An infinity (spelled per `infinity`) or `nan`, ignoring case. `nan` is a
/// positive NaN; the caller's sign makes `-nan` a negative one, which is
/// what Second Life reads it as since 2022.
fn read_special(bytes: &[u8], infinity: Infinity) -> Option<(f32, usize)> {
    if starts_with_ignore_case(bytes, b"nan") {
        return Some((f32::NAN, 3));
    }
    match infinity {
        Infinity::Scalar if starts_with_ignore_case(bytes, b"inf") => Some((f32::INFINITY, 3)),
        Infinity::Component if starts_with_ignore_case(bytes, b"infinity") => {
            Some((f32::INFINITY, 8))
        }
        Infinity::Component
            if starts_with_ignore_case(bytes, b"inf")
                && !bytes
                    .get(3)
                    .is_some_and(|byte| byte.eq_ignore_ascii_case(&b'i')) =>
        {
            Some((f32::INFINITY, 3))
        }
        Infinity::Scalar | Infinity::Component => None,
    }
}

/// `(float)text`: the float at the start of the string (after optional
/// whitespace), or `0.0` when there is none. `(float)"3.14e+0a"` is `3.14`,
/// `(float)"0x3.14p+0a"` is `3.078125`, `(float)"--3.14"` is `0.0`.
#[must_use]
pub fn float(text: &str) -> f32 {
    read_float(text.as_bytes(), Infinity::Scalar).map_or(0.0, |(value, _)| value)
}

/// Read `N` comma-separated components after a `<` that must be the very
/// first character. Each component may be preceded by whitespace but must be
/// followed *directly* by the comma — `"<1 , 2, 3>"` is not a vector — and
/// whatever follows the last one is ignored, a closing `>` included.
fn parse_components<const N: usize>(text: &str) -> Option<[f32; N]> {
    let mut rest = text.as_bytes().strip_prefix(b"<")?;
    let mut components = [0.0_f32; N];
    for (index, slot) in components.iter_mut().enumerate() {
        let (value, length) = read_float(rest, Infinity::Component)?;
        *slot = value;
        rest = from(rest, length);
        if index.saturating_add(1) < N {
            rest = rest.strip_prefix(b",")?;
        }
    }
    Some(components)
}

/// `(vector)text`: `<x, y, z` at the start of the string, or
/// [`ZERO_VECTOR`] when it does not parse (see `parse_components` for the
/// exact shape).
#[must_use]
pub fn vector(text: &str) -> Vector {
    parse_components::<3>(text).map_or(ZERO_VECTOR, |[x, y, z]| Vector { x, y, z })
}

/// `(rotation)text`: `<x, y, z, s` at the start of the string, or
/// [`ZERO_ROTATION`] when it does not parse.
#[must_use]
pub fn rotation(text: &str) -> Rotation {
    parse_components::<4>(text).map_or(ZERO_ROTATION, |[x, y, z, s]| Rotation { x, y, z, s })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn integers_read_the_longest_prefix() {
        // PyOptimizer unit_tests/expr.suite/casts.lsl → casts.out.
        let cases = [
            ("3.14e+0a", 3),
            ("a3.14e+0a", 0),
            ("0XA3.14e+0a", 163),
            ("0x1A", 26),
            ("  12abc", 12),
            ("abc", 0),
            ("3333333333333", -1),
            ("4124567890", -170_399_406),
            ("-4124567890", 170_399_406),
            ("-4294967296", -1),
            ("-4294967295", 1),
            ("-4294967294", 2),
            ("4294967294", -2),
            ("4294967295", -1),
            ("4294967296", -1),
            (" +12345 ", 12345),
            (" ++12345 ", 0),
            (" +-12345 ", 0),
            (" + 12345 ", 0),
            (" - 12345 ", 0),
            (" -+12345 ", 0),
            (" --12345 ", 0),
            (" -12345 ", -12345),
            // `int_re`: the hex form is anchored — no space or sign before it.
            (" 0x10", 0),
            ("-0x10", 0),
            ("0x", 0),
            ("0xg", 0),
            ("", 0),
        ];
        for (text, expected) in cases {
            assert_eq!(integer(text), expected, "{text:?}");
        }
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "3.14 is the oracle's own test literal, not an approximation of pi"
    )]
    fn floats_read_the_longest_prefix() {
        // casts.lsl and nan-fcast-vcast-minus0.lsl.
        let cases: [(&str, f32); 17] = [
            ("3.14e+0a", 3.14),
            ("+3.14e+0a", 3.14),
            ("++3.14e+0a", 0.0),
            ("-3.14e+0a", -3.14),
            ("--3.14e+0a", 0.0),
            ("0x3.14p+0a", 3.078_125),
            ("-0x13", -19.0),
            ("+0x13", 19.0),
            ("-0x13p1", -38.0),
            ("+0x13p1", 38.0),
            ("inf", f32::INFINITY),
            ("-inf", f32::NEG_INFINITY),
            ("+inf", f32::INFINITY),
            ("--inf", 0.0),
            ("+-inf", 0.0),
            ("1e", 1.0),
            (".", 0.0),
        ];
        for (text, expected) in cases {
            assert_eq!(float(text).to_bits(), expected.to_bits(), "{text:?}");
        }
    }

    #[test]
    fn nan_keeps_its_sign() {
        // nan-fcast-vcast-minus0.lsl: llList2CSV([(float)"nan"]) is "nan",
        // of (float)"-nan" and (float)"-nanometre" "-nan"; "--nan" is 0.
        assert!(float("nan").is_nan() && float("nan").is_sign_positive());
        assert!(float("-nan").is_nan() && float("-nan").is_sign_negative());
        assert!(float("-nanometre").is_sign_negative());
        assert_eq!(float("--nan").to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn subnormal_strings_round_once_through_double() {
        // casts.lsl: (float)"1.1754942e-38" × 2^126 × 2^24 is 16777214.
        let scaled = |text: &str| float(text) * 8.507_059e37 * 16_777_216.0;
        assert_eq!(
            scaled("1.1754944e-38").to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            scaled("1.1754943e-38").to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            scaled("1.1754942e-38").to_bits(),
            16_777_214.0_f32.to_bits()
        );
        assert_eq!(
            scaled("1.175494315789825834599e-38").to_bits(),
            16_777_216.0_f32.to_bits()
        );
    }

    /// A vector from three components.
    const fn vec3(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    #[test]
    fn vectors_parse_strictly_at_the_commas() {
        // casts.lsl → casts.out.
        let parsed = vec3(5.31, 7.13, 9.597_656);
        let inf = f32::INFINITY;
        let cases = [
            ("<5.31,7.13,0x9.99", parsed.clone()),
            ("<5.31, 7.13, 0x9.99>", parsed),
            ("<5.31 , 7.13 , 0x9.99>", ZERO_VECTOR),
            ("5.31, 7.13, 0x9.99>", ZERO_VECTOR),
            ("<5.31, a7.13, 0x9.99>", ZERO_VECTOR),
            ("<1,1,2+", vec3(1.0, 1.0, 2.0)),
            ("<1,1,2a", vec3(1.0, 1.0, 2.0)),
            ("<1,1,inf", vec3(1.0, 1.0, inf)),
            ("<1,1,info", vec3(1.0, 1.0, inf)),
            ("<1,1,infi", ZERO_VECTOR),
            ("<1,1,infix", ZERO_VECTOR),
            ("<1,1,infinite", ZERO_VECTOR),
            ("<1,1,iNfInItY", vec3(1.0, 1.0, inf)),
            ("<1,1,infinitys", vec3(1.0, 1.0, inf)),
            ("<1,1,infinities", ZERO_VECTOR),
            ("<inf,1,1>", vec3(inf, 1.0, 1.0)),
            ("<infinity,1,1>", vec3(inf, 1.0, 1.0)),
            ("<infini,1,1>", ZERO_VECTOR),
            ("<infinite,1,1>", ZERO_VECTOR),
            ("<info,1,1>", ZERO_VECTOR),
            ("<nano,nano,nano>", ZERO_VECTOR),
            (" <1,2,3>", ZERO_VECTOR),
        ];
        for (text, expected) in cases {
            assert_eq!(vector(text), expected, "{text:?}");
        }
    }

    #[test]
    fn vector_components_keep_nan_and_signed_zero() {
        // nan-fcast-vcast-minus0.lsl: (vector)"<nan,-0,-0.>" is
        // <NaN, -0., -0.>, and "<-nan,1,1>" starts with a negative NaN.
        let parsed = vector("<nan,-0,-0.>");
        assert!(parsed.x.is_nan() && parsed.x.is_sign_positive());
        assert_eq!(parsed.y.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(parsed.z.to_bits(), (-0.0_f32).to_bits());
        assert!(vector("<-nan,1,1>").x.is_sign_negative());
    }

    #[test]
    fn rotations_need_four_components() {
        let parsed = rotation("<-nan,nan,-nan,nan>");
        assert!(parsed.x.is_nan() && parsed.x.is_sign_negative());
        assert!(parsed.y.is_nan() && parsed.y.is_sign_positive());
        assert_eq!(rotation("<1,2,3>"), ZERO_ROTATION);
        assert_eq!(
            rotation("<1, 2, 3, 4>"),
            Rotation {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                s: 4.0,
            }
        );
    }
}
