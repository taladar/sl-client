//! The .NET parsers OpenSim's library leans on, so a function ported from
//! `LSL_Api.cs` reads its argument as OpenSim does.

/// .NET's white space for `NumberStyles.AllowLeadingWhite` /
/// `AllowTrailingWhite`: tab, line feed, vertical tab, form feed, carriage
/// return and space.
const fn is_number_space(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ')
}

/// `Double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture)`:
/// optional white space, an optional sign, digits with an optional point
/// (at least one digit somewhere), an optional exponent, optional white
/// space — and nothing else; or the invariant culture's `Infinity`,
/// `-Infinity` and `NaN`. No hexadecimal, no thousands separators.
pub(crate) fn parse_double(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_number_space);
    match trimmed {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        "NaN" => return Some(f64::NAN),
        _ => {}
    }
    let unsigned = trimmed
        .strip_prefix(['+', '-'])
        .unwrap_or(trimmed)
        .as_bytes();
    let (mantissa, exponent) = match unsigned.iter().position(|byte| matches!(byte, b'e' | b'E')) {
        Some(at) => (
            unsigned.get(..at).unwrap_or_default(),
            Some(unsigned.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (unsigned, None),
    };
    let mut points = 0_usize;
    let mut digits = 0_usize;
    for byte in mantissa {
        match byte {
            b'.' => points = points.saturating_add(1),
            b'0'..=b'9' => digits = digits.saturating_add(1),
            _ => return None,
        }
    }
    if digits == 0 || points > 1 {
        return None;
    }
    if let Some(exponent) = exponent {
        let exponent_digits = exponent.strip_prefix(b"+").unwrap_or(exponent);
        let exponent_digits = exponent_digits
            .strip_prefix(b"-")
            .unwrap_or(exponent_digits);
        if exponent_digits.is_empty() || !exponent_digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    trimmed.parse().ok()
}

/// `UUID.TryParse` (OpenMetaverse, through `Guid.TryParse`): 32 hex digits,
/// bare or dashed `8-4-4-4-12`, the dashed form optionally in braces or
/// parentheses. OpenSim's library takes a string for a key whenever this
/// accepts it (`llGetListEntryType` answers `TYPE_KEY` for one).
pub(crate) fn is_uuid(text: &str) -> bool {
    let inner = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .or_else(|| {
            text.strip_prefix('(')
                .and_then(|rest| rest.strip_suffix(')'))
        });
    let dashed = |candidate: &str| {
        candidate.len() == 36
            && candidate.char_indices().all(|(index, character)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    character == '-'
                } else {
                    character.is_ascii_hexdigit()
                }
            })
    };
    match inner {
        Some(inner) => dashed(inner),
        None => dashed(text) || (text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn doubles_as_the_invariant_culture_reads_them() {
        let cases: [(&str, Option<f64>); 14] = [
            ("3", Some(3.0)),
            (" 2.5 ", Some(2.5)),
            ("-.5e1", Some(-5.0)),
            ("5.", Some(5.0)),
            ("+4", Some(4.0)),
            ("1e3", Some(1000.0)),
            ("Infinity", Some(f64::INFINITY)),
            ("-Infinity", Some(f64::NEG_INFINITY)),
            ("", None),
            (".", None),
            ("0x10", None),
            ("1,5", None),
            ("<1, 2, 3>", None),
            ("inf", None),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_double(text), expected, "{text:?}");
        }
        assert!(parse_double("NaN").is_some_and(f64::is_nan));
    }

    #[test]
    fn uuids_in_the_forms_guid_accepts() {
        assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_uuid("5748DECC-F629-461C-9A36-A35A221FE21F"));
        assert!(is_uuid("00000000000000000000000000000001"));
        assert!(is_uuid("{00000000-0000-0000-0000-000000000001}"));
        assert!(!is_uuid(" 00000000-0000-0000-0000-000000000001"));
        assert!(!is_uuid("00000000-0000-0000-0000-00000000000"));
        assert!(!is_uuid("b"));
    }
}
