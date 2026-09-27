//! Literal text to values, as Linden's lexer reads it — LSL PyOptimizer's
//! lexer (`lslopt/lslparse.py`, `GetToken`) is the oracle.

use crate::num::to_f32;

/// An integer literal's value.
///
/// Decimal and `0x` hexadecimal both wrap into 32 bits, so `4294967295` and
/// `0xFFFFFFFF` are `-1` and `2147483648` is `-2147483648`. A literal too
/// long for 32 bits is `-1`: more than ten decimal digits (counting leading
/// zeros) or a decimal above `4294967295`, and more than eight significant
/// hex digits.
pub(super) fn integer(raw: &str) -> i32 {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        let digits = hex.trim_start_matches('0');
        if digits.len() > 8 {
            return -1;
        }
        return u32::from_str_radix(if digits.is_empty() { "0" } else { digits }, 16)
            .map_or(-1, u32::cast_signed);
    }
    if raw.len() > 10 {
        return -1;
    }
    raw.parse::<u64>()
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .map_or(-1, u32::cast_signed)
}

/// A float literal's value: the nearest `f32` to the decimal text, a
/// trailing `f` or `F` ignored. Out of range is an infinity.
pub(super) fn float(raw: &str) -> f32 {
    let text = raw.trim_end_matches(['f', 'F']);
    text.parse::<f64>().map_or(0.0, to_f32)
}

/// A string literal's value, from its raw text with the quotes.
///
/// `\n` is a line break, `\t` is **four spaces**, and a backslash before any
/// other character is that character. The legacy `L"…"` form keeps a
/// leading `"` in the value. Text from the first NUL on is dropped, as the
/// reference's strings are NUL-terminated.
pub(super) fn string(raw: &str) -> String {
    let (prefix, quoted) = match raw.strip_prefix('L') {
        Some(rest) => ("\"", rest),
        None => ("", raw),
    };
    let inner = quoted
        .strip_prefix('"')
        .map_or(quoted, |rest| rest.strip_suffix('"').unwrap_or(rest));
    let mut value = String::from(prefix);
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push_str("    "),
                Some(other) => value.push(other),
                None => {}
            }
        } else {
            value.push(character);
        }
    }
    match value.find('\0') {
        Some(end) => value.get(..end).unwrap_or_default().to_owned(),
        None => value,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn integers_wrap_and_overlong_ones_are_minus_one() {
        let cases = [
            ("0", 0),
            ("2147483647", i32::MAX),
            ("2147483648", i32::MIN),
            ("4294967295", -1),
            ("4294967296", -1),
            ("99999999999", -1),
            ("00000000001", -1),
            ("0x0", 0),
            ("0xFFFFFFFF", -1),
            ("0x80000000", i32::MIN),
            ("0x000000001A", 26),
            ("0x100000000", -1),
        ];
        for (raw, expected) in cases {
            assert_eq!(integer(raw), expected, "{raw}");
        }
    }

    #[test]
    fn floats_round_to_single_precision() {
        let cases = [
            ("1.5", 1.5_f32),
            ("1.", 1.0),
            (".25", 0.25),
            ("2e3", 2000.0),
            ("1.5f", 1.5),
            ("0.1", 0.1),
            ("1e39", f32::INFINITY),
        ];
        for (raw, expected) in cases {
            assert_eq!(float(raw).to_bits(), expected.to_bits(), "{raw}");
        }
    }

    #[test]
    fn strings_unescape_as_the_reference_does() {
        let cases = [
            (r#""plain""#, "plain"),
            (r#""a\nb""#, "a\nb"),
            (r#""a\tb""#, "a    b"),
            (r#""\"q\"""#, "\"q\""),
            (r#""\\""#, "\\"),
            (r#""\x""#, "x"),
            ("L\"legacy\"", "\"legacy"),
            ("\"line\nbreak\"", "line\nbreak"),
            ("\"cut\0here\"", "cut"),
        ];
        for (raw, expected) in cases {
            assert_eq!(string(raw), expected, "{raw}");
        }
    }
}
