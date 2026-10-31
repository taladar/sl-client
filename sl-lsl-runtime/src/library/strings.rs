//! Library tranche: strings (`server-lsl-lib-strings-lists`) — length,
//! cutting, searching, trimming, case, replacing, characters and `llHash`.
//!
//! Second Life's answers are PyOptimizer's (`lslbasefuncs.py`), the tested
//! oracle the task names, checked against aditi's transcripts of
//! `strings/functions`, `strings/unicode`, `strings/newer`, `errors/bounds`
//! and `errors/bounds-newer`; OpenSim's are ported from `LSL_Api.cs` and held
//! to the local grid's transcripts of the same probes. Where they part —
//! code points against UTF-16 units above all — the book chapter
//! `gridspec/lsl.md` has the measured table.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::library::range::{os_delete_string, os_get, py, signed, sl_delete, sl_get};
use crate::library::text::{Text, until_nul};
use crate::library::{BuiltinId, CallError, Exception, PureCtx};

/// `llStringLength`: the characters in the string — code points on Second
/// Life, UTF-16 units on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_string_length<C: PureCtx>(ctx: &mut C, text: String) -> Result<i32, CallError> {
    Ok(Text::new(ctx.flavour(), &text).length())
}

/// `llGetSubString`: the characters `start..=end`, each counted from the end
/// when negative, or everything outside the range when the start comes
/// after the end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_sub_string<C: PureCtx>(
    ctx: &mut C,
    text: String,
    start: i32,
    end: i32,
) -> Result<String, CallError> {
    let text = Text::new(ctx.flavour(), &text);
    let cut = if ctx.flavour().is_opensim() {
        os_get(text.units(), start, end)
    } else {
        sl_get(text.units(), start, end)
    };
    Ok(text.string(&cut))
}

/// `llDeleteSubString`: the string without the characters `start..=end`, or
/// with only those between the two when the start comes after the end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_delete_sub_string<C: PureCtx>(
    ctx: &mut C,
    text: String,
    start: i32,
    end: i32,
) -> Result<String, CallError> {
    let text = Text::new(ctx.flavour(), &text);
    let cut = if ctx.flavour().is_opensim() {
        os_delete_string(text.units(), start, end)
    } else {
        sl_delete(text.units(), start, end)
    };
    Ok(text.string(&cut))
}

/// `llInsertString`: `insert` put into `text` before the character at
/// `index`.
///
/// Second Life reads a negative index as the start and an index past the
/// end as the end (`errors/bounds`: 99, −5 and −1 into `abcdef` append,
/// prepend and prepend). OpenSim counts a negative index from the end and
/// keeps a surrogate pair whole, but an index at or past the end throws
/// (`LSL_Api.llInsertString` reads the character there before it checks).
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for an index at or past the end.
pub fn ll_insert_string<C: PureCtx>(
    ctx: &mut C,
    text: String,
    index: i32,
    insert: String,
) -> Result<String, CallError> {
    let flavour = ctx.flavour();
    let target = Text::new(flavour, &text);
    let inserted = Text::new(flavour, &insert);
    let units = target.units();
    if !flavour.is_opensim() {
        let at = py(i64::from(index.max(0)), units.len());
        let mut joined = units.get(..at).unwrap_or_default().to_vec();
        joined.extend_from_slice(inserted.units());
        joined.extend_from_slice(units.get(at..).unwrap_or_default());
        return Ok(target.string(&joined));
    }
    let length = signed(units.len());
    let is_low_surrogate = |at: i64| {
        usize::try_from(at)
            .ok()
            .and_then(|at| units.get(at))
            .is_some_and(|unit| (0xDC00..=0xDFFF).contains(unit))
    };
    let mut at = i64::from(index);
    if at < 0 {
        at = at.saturating_add(length);
        if at > 0 && is_low_surrogate(at) {
            at = at.saturating_sub(1);
        }
        if at < 0 {
            let mut joined = insert;
            joined.push_str(&text);
            return Ok(joined);
        }
    } else {
        if at >= length {
            return Err(CallError::Exception {
                id: BuiltinId::LlInsertString,
                exception: Exception::IndexOutOfRange,
            });
        }
        if is_low_surrogate(at) {
            at = at.saturating_add(1);
        }
    }
    let at = usize::try_from(at).unwrap_or(0).min(units.len());
    let mut joined = units.get(..at).unwrap_or_default().to_vec();
    joined.extend_from_slice(inserted.units());
    joined.extend_from_slice(units.get(at..).unwrap_or_default());
    Ok(target.string(&joined))
}

/// `llSubStringIndex`: where `pattern` first starts in `text`, or `-1`. The
/// empty pattern is found at `0`, in the empty string too on Second Life;
/// OpenSim finds nothing in the empty string.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sub_string_index<C: PureCtx>(
    ctx: &mut C,
    text: String,
    pattern: String,
) -> Result<i32, CallError> {
    let flavour = ctx.flavour();
    if flavour.is_opensim() && text.is_empty() {
        return Ok(-1);
    }
    let text = Text::new(flavour, &text);
    let pattern = Text::new(flavour, &pattern);
    Ok(text
        .find_as_grid(pattern.units(), 0)
        .map_or(-1, |at| i32::try_from(at).unwrap_or(i32::MAX)))
}

/// `STRING_TRIM_HEAD`.
const TRIM_HEAD: i32 = 1;

/// `STRING_TRIM_TAIL`.
const TRIM_TAIL: i32 = 2;

/// `llStringTrim`: the string without its leading (`STRING_TRIM_HEAD`),
/// trailing (`STRING_TRIM_TAIL`) or both (`STRING_TRIM`) runs of white
/// space.
///
/// Second Life trims ASCII white space only — tab, line feed, vertical tab,
/// form feed, carriage return and space — and reads the mode as bits, so
/// mode 7 trims both ends. OpenSim trims all Unicode white space (.NET's
/// `Trim`) and answers any mode but the three with the string unchanged.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_string_trim<C: PureCtx>(
    ctx: &mut C,
    text: String,
    mode: i32,
) -> Result<String, CallError> {
    if ctx.flavour().is_opensim() {
        return Ok(match mode {
            TRIM_HEAD => text.trim_start().to_owned(),
            TRIM_TAIL => text.trim_end().to_owned(),
            3 => text.trim().to_owned(),
            _ => text,
        });
    }
    let ascii =
        |character: char| matches!(character, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ');
    let mut trimmed = text.as_str();
    if mode & TRIM_HEAD != 0 {
        trimmed = trimmed.trim_start_matches(ascii);
    }
    if mode & TRIM_TAIL != 0 {
        trimmed = trimmed.trim_end_matches(ascii);
    }
    Ok(trimmed.to_owned())
}

/// Second Life's case tables: `(upper, lower)` pairs, and the two one-way
/// mappings, from PyOptimizer's `__make_tables` — Mono's, which leaves
/// title case (`ǅ`), `İ` and `ß` alone.
static CASE: LazyLock<(HashMap<char, char>, HashMap<char, char>)> = LazyLock::new(|| {
    let mut to_lower = HashMap::new();
    let mut to_upper = HashMap::new();
    let mut pair = |upper: u32, lower: u32| {
        if let (Some(upper), Some(lower)) = (char::from_u32(upper), char::from_u32(lower)) {
            let _old = to_lower.insert(upper, lower);
            let _old = to_upper.insert(lower, upper);
        }
    };
    let offset = |from: u32, to: u32, by: u32, skip: &[u32], pair: &mut dyn FnMut(u32, u32)| {
        for code in (from..=to).filter(|code| !skip.contains(code)) {
            pair(code, code.saturating_add(by));
        }
    };
    let alternating = |from: u32, to: u32, skip: &[u32], pair: &mut dyn FnMut(u32, u32)| {
        for code in (from..=to).step_by(2).filter(|code| !skip.contains(code)) {
            pair(code, code.saturating_add(1));
        }
    };
    let back = |from: u32, to: u32, by: u32, pair: &mut dyn FnMut(u32, u32)| {
        for code in from..=to {
            pair(code, code.saturating_sub(by));
        }
    };
    offset(65, 90, 32, &[], &mut pair);
    offset(192, 222, 32, &[215], &mut pair);
    alternating(256, 311, &[304], &mut pair);
    alternating(313, 328, &[], &mut pair);
    alternating(330, 375, &[], &mut pair);
    pair(376, 255);
    alternating(377, 382, &[], &mut pair);
    pair(385, 595);
    alternating(386, 389, &[], &mut pair);
    for (upper, lower) in [
        (390, 596),
        (391, 392),
        (393, 598),
        (394, 599),
        (395, 396),
        (398, 477),
        (399, 601),
        (400, 603),
        (401, 402),
        (403, 608),
        (404, 611),
        (406, 617),
        (407, 616),
        (408, 409),
        (412, 623),
        (413, 626),
        (415, 629),
        (416, 417),
        (418, 419),
        (420, 421),
        (423, 424),
        (425, 643),
        (428, 429),
        (430, 648),
        (431, 432),
        (433, 650),
        (434, 651),
        (435, 436),
        (437, 438),
        (439, 658),
        (440, 441),
        (444, 445),
        (452, 454),
        (455, 457),
        (458, 460),
    ] {
        pair(upper, lower);
    }
    alternating(461, 476, &[], &mut pair);
    alternating(478, 495, &[], &mut pair);
    pair(497, 499);
    pair(500, 501);
    alternating(506, 535, &[], &mut pair);
    for (upper, lower) in [
        (902, 940),
        (904, 941),
        (905, 942),
        (906, 943),
        (908, 972),
        (910, 973),
        (911, 974),
    ] {
        pair(upper, lower);
    }
    offset(913, 939, 32, &[930], &mut pair);
    alternating(994, 1007, &[], &mut pair);
    offset(1025, 1039, 80, &[1037], &mut pair);
    offset(1040, 1071, 32, &[], &mut pair);
    alternating(1120, 1153, &[], &mut pair);
    alternating(1168, 1215, &[], &mut pair);
    for (upper, lower) in [(1217, 1218), (1219, 1220), (1223, 1224), (1227, 1228)] {
        pair(upper, lower);
    }
    alternating(1232, 1269, &[1260], &mut pair);
    pair(1272, 1273);
    offset(1329, 1366, 48, &[], &mut pair);
    alternating(7680, 7829, &[], &mut pair);
    alternating(7840, 7929, &[], &mut pair);
    back(7944, 7951, 8, &mut pair);
    back(7960, 7965, 8, &mut pair);
    back(7976, 7983, 8, &mut pair);
    back(7992, 7999, 8, &mut pair);
    back(8008, 8013, 8, &mut pair);
    for (upper, lower) in [(8025, 8017), (8027, 8019), (8029, 8021), (8031, 8023)] {
        pair(upper, lower);
    }
    back(8040, 8047, 8, &mut pair);
    for (upper, lower) in [(8120, 8112), (8121, 8113), (8122, 8048), (8123, 8049)] {
        pair(upper, lower);
    }
    back(8136, 8139, 86, &mut pair);
    for (upper, lower) in [
        (8152, 8144),
        (8153, 8145),
        (8154, 8054),
        (8155, 8055),
        (8168, 8160),
        (8169, 8161),
        (8170, 8058),
        (8171, 8059),
        (8172, 8165),
        (8184, 8056),
        (8185, 8057),
        (8186, 8060),
        (8187, 8061),
    ] {
        pair(upper, lower);
    }
    offset(8544, 8559, 16, &[], &mut pair);
    offset(9398, 9423, 26, &[], &mut pair);
    offset(65313, 65338, 32, &[], &mut pair);
    // One way only: final sigma to capital sigma, Georgian capitals to
    // their small letters.
    if let (Some(sigma), Some(capital)) = (char::from_u32(962), char::from_u32(931)) {
        let _old = to_upper.insert(sigma, capital);
    }
    for code in 4256..=4293_u32 {
        if let (Some(capital), Some(small)) = (
            char::from_u32(code),
            char::from_u32(code.saturating_add(48)),
        ) {
            let _old = to_lower.insert(capital, small);
        }
    }
    (to_upper, to_lower)
});

/// `llToUpper`.
///
/// Second Life maps by Mono's table (`aäßéÿǅ` is `AÄßÉŸǅ`); OpenSim by
/// .NET's `ToUpper`, which maps title case too (`ǅ` → `Ǆ`) and keeps a
/// character whose capital is two (`ß`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_to_upper<C: PureCtx>(ctx: &mut C, text: String) -> Result<String, CallError> {
    let text = until_nul(text);
    if ctx.flavour().is_opensim() {
        return Ok(text
            .chars()
            .map(|character| {
                let mut upper = character.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(single), None) => single,
                    _ => character,
                }
            })
            .collect());
    }
    Ok(text
        .chars()
        .map(|character| CASE.0.get(&character).copied().unwrap_or(character))
        .collect())
}

/// `llToLower`.
///
/// Second Life maps by Mono's table (`AÄÉŸǅİ` is `aäéÿǅİ`); OpenSim by .NET's
/// `ToLower`, which maps title case (`ǅ` → `ǆ`) and `İ` to a plain `i`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_to_lower<C: PureCtx>(ctx: &mut C, text: String) -> Result<String, CallError> {
    let text = until_nul(text);
    if ctx.flavour().is_opensim() {
        return Ok(text
            .chars()
            .map(|character| character.to_lowercase().next().unwrap_or(character))
            .collect());
    }
    Ok(text
        .chars()
        .map(|character| CASE.1.get(&character).copied().unwrap_or(character))
        .collect())
}

/// `llReplaceSubString`: `text` with matches of `pattern` replaced, left to
/// right and never overlapping — every one for a `count` of 0, the first
/// `count`, or the last `-count` searched from the right. An empty pattern
/// replaces nothing (`strings/newer`, PyOptimizer `llReplaceSubString`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_replace_sub_string<C: PureCtx>(
    ctx: &mut C,
    text: String,
    pattern: String,
    replacement: String,
    count: i32,
) -> Result<String, CallError> {
    let flavour = ctx.flavour();
    if pattern.is_empty() {
        return Ok(text);
    }
    let pattern = Text::new(flavour, &pattern);
    let replacement = Text::new(flavour, &replacement);
    let mut source = Text::new(flavour, &text);
    let width = pattern.len();
    if count >= 0 {
        let limit = if count == 0 {
            usize::MAX
        } else {
            usize::try_from(count).unwrap_or(usize::MAX)
        };
        let mut out = Vec::new();
        let mut from = 0_usize;
        let mut done = 0_usize;
        while done < limit {
            let Some(at) = source.find(pattern.units(), from) else {
                break;
            };
            out.extend_from_slice(source.units().get(from..at).unwrap_or_default());
            out.extend_from_slice(replacement.units());
            from = at.saturating_add(width);
            done = done.saturating_add(1);
        }
        out.extend_from_slice(source.units().get(from..).unwrap_or_default());
        return Ok(source.string(&out));
    }
    let mut remaining = count;
    let mut before = source.len();
    while remaining < 0 {
        let Some(at) = source.rfind(pattern.units(), before) else {
            break;
        };
        let mut out = source.units().get(..at).unwrap_or_default().to_vec();
        out.extend_from_slice(replacement.units());
        out.extend_from_slice(
            source
                .units()
                .get(at.saturating_add(width)..)
                .unwrap_or_default(),
        );
        source = Text::new(flavour, &source.string(&out));
        before = at;
        remaining = remaining.saturating_add(1);
    }
    Ok(source.string(source.units()))
}

/// `llChar`: the character with the code point `code`; `""` for 0 and
/// U+FFFD for anything that is no character.
///
/// Second Life also answers U+FFFD for the noncharacters U+FFFE and U+FFFF,
/// which OpenSim hands back (`strings/unicode`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_char<C: PureCtx>(ctx: &mut C, code: i32) -> Result<String, CallError> {
    if code == 0 {
        return Ok(String::new());
    }
    let noncharacter = !ctx.flavour().is_opensim() && matches!(code, 0xFFFE | 0xFFFF);
    Ok(u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .filter(|_| !noncharacter)
        .unwrap_or('\u{FFFD}')
        .to_string())
}

/// `llOrd`: the code point of the character at `index`, counted from the end
/// when negative, or `0` outside the string.
///
/// Second Life counts code points. OpenSim counts UTF-16 units and reads a
/// surrogate pair from either half — the first half reads the pair forwards,
/// the second backwards (`llOrd("a😀b", 2)` is 128512 there, 98 here).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ord<C: PureCtx>(ctx: &mut C, text: String, index: i32) -> Result<i32, CallError> {
    let flavour = ctx.flavour();
    let units = Text::new(flavour, &text);
    let units = units.units();
    let length = signed(units.len());
    let mut at = i64::from(index);
    if at < 0 {
        at = at.saturating_add(length);
    }
    let unit = |at: i64| {
        usize::try_from(at)
            .ok()
            .and_then(|at| units.get(at))
            .copied()
    };
    let Some(code) = unit(at) else {
        return Ok(0);
    };
    if !flavour.is_opensim() {
        return Ok(i32::try_from(code).unwrap_or(0));
    }
    let answer = match code {
        0xDC00..=0xDFFF => match unit(at.saturating_sub(1)) {
            Some(high @ 0xD800..=0xDBFF) => pair(high, code),
            _ => 0,
        },
        0xD800..=0xDBFF => match unit(at.saturating_add(1)) {
            Some(low @ 0xDC00..=0xDFFF) => pair(code, low),
            _ => 0,
        },
        other => other,
    };
    Ok(i32::try_from(answer).unwrap_or(0))
}

/// The code point of a surrogate pair.
const fn pair(high: u32, low: u32) -> u32 {
    high.saturating_sub(0xD800)
        .saturating_mul(0x400)
        .saturating_add(low.saturating_sub(0xDC00))
        .saturating_add(0x1_0000)
}

/// `llHash`: the 32-bit `sdbm`-style hash of the code points, `hash * 65599 +
/// code` wrapping — the same on every grid (`strings/unicode`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_hash<C>(_ctx: &mut C, text: String) -> Result<i32, CallError> {
    let hash = text.chars().fold(0_u32, |hash, character| {
        hash.wrapping_mul(65599).wrapping_add(u32::from(character))
    });
    Ok(i32::from_ne_bytes(hash.to_ne_bytes()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::{os, sl};

    #[test]
    fn length_and_cutting_count_code_points_on_second_life_and_units_on_opensim() {
        let astral = || "a𝄞b😀".to_owned();
        // strings/functions, strings/unicode.
        assert_eq!(ll_string_length(&mut sl(), astral()), Ok(4));
        assert_eq!(ll_string_length(&mut os(), astral()), Ok(6));
        assert_eq!(
            ll_get_sub_string(&mut sl(), astral(), 1, 1),
            Ok("𝄞".to_owned())
        );
        assert_eq!(
            ll_get_sub_string(&mut os(), astral(), 1, 1),
            Ok("\u{FFFD}".to_owned())
        );
        assert_eq!(
            ll_get_sub_string(&mut os(), astral(), 1, 2),
            Ok("𝄞".to_owned())
        );
        assert_eq!(
            ll_delete_sub_string(&mut sl(), astral(), 1, 1),
            Ok("ab😀".to_owned())
        );
        assert_eq!(
            ll_delete_sub_string(&mut os(), astral(), 1, 1),
            Ok("a\u{FFFD}b😀".to_owned())
        );
        assert_eq!(
            ll_sub_string_index(&mut sl(), astral(), "b".to_owned()),
            Ok(2)
        );
        assert_eq!(
            ll_sub_string_index(&mut os(), astral(), "b".to_owned()),
            Ok(3)
        );
    }

    #[test]
    fn the_empty_string_is_found_at_zero_on_second_life_only() {
        let index = |ctx: &mut crate::library::bare::Bare, text: &str, pattern: &str| {
            ll_sub_string_index(ctx, text.to_owned(), pattern.to_owned())
        };
        assert_eq!(index(&mut sl(), "", ""), Ok(0));
        assert_eq!(index(&mut os(), "", ""), Ok(-1));
        assert_eq!(index(&mut sl(), "abc", ""), Ok(0));
        assert_eq!(index(&mut os(), "abc", ""), Ok(0));
        assert_eq!(index(&mut sl(), "", "abc"), Ok(-1));
    }

    #[test]
    fn insert_string_clamps_on_second_life_and_throws_past_the_end_on_opensim() {
        let insert = |ctx: &mut crate::library::bare::Bare, at: i32| {
            ll_insert_string(ctx, "abcdef".to_owned(), at, "x".to_owned())
        };
        // errors/bounds: 99, -5, -1.
        assert_eq!(insert(&mut sl(), 99), Ok("abcdefx".to_owned()));
        assert_eq!(insert(&mut sl(), -5), Ok("xabcdef".to_owned()));
        assert_eq!(insert(&mut sl(), -1), Ok("xabcdef".to_owned()));
        assert_eq!(
            insert(&mut os(), 99),
            Err(CallError::Exception {
                id: BuiltinId::LlInsertString,
                exception: Exception::IndexOutOfRange,
            })
        );
        assert_eq!(insert(&mut os(), -1), Ok("abcdexf".to_owned()));
        assert_eq!(insert(&mut os(), -7), Ok("xabcdef".to_owned()));
        // strings/functions: an astral character goes in whole.
        assert_eq!(
            ll_insert_string(&mut sl(), "aé".to_owned(), 1, "😀".to_owned()),
            Ok("a😀é".to_owned())
        );
    }

    #[test]
    fn trim_removes_ascii_space_on_second_life_and_all_of_it_on_opensim() {
        let ws = || "\t \n\u{0B}\u{0C}\u{A0}\u{3000} a b \r\u{0B}".to_owned();
        assert_eq!(
            ll_string_trim(&mut sl(), ws(), 3),
            Ok("\u{A0}\u{3000} a b".to_owned())
        );
        assert_eq!(
            ll_string_trim(&mut sl(), ws(), 7),
            Ok("\u{A0}\u{3000} a b".to_owned())
        );
        assert_eq!(ll_string_trim(&mut os(), ws(), 3), Ok("a b".to_owned()));
        assert_eq!(ll_string_trim(&mut os(), ws(), 7), Ok(ws()));
        assert_eq!(
            ll_string_trim(&mut sl(), ws(), 1),
            Ok("\u{A0}\u{3000} a b \r\u{0B}".to_owned())
        );
    }

    #[test]
    fn case_follows_monos_table_on_second_life_and_dotnet_on_opensim() {
        assert_eq!(
            ll_to_upper(&mut sl(), "aäßéÿǅ".to_owned()),
            Ok("AÄßÉŸǅ".to_owned())
        );
        assert_eq!(
            ll_to_lower(&mut sl(), "AÄÉŸǅİ".to_owned()),
            Ok("aäéÿǅİ".to_owned())
        );
        assert_eq!(
            ll_to_upper(&mut os(), "aäßéÿǅ".to_owned()),
            Ok("AÄßÉŸǄ".to_owned())
        );
        assert_eq!(
            ll_to_lower(&mut os(), "AÄÉŸǅİ".to_owned()),
            Ok("aäéÿǆi".to_owned())
        );
        // Greek and Cyrillic round trips, and the one-way final sigma.
        assert_eq!(
            ll_to_upper(&mut sl(), "ωжς".to_owned()),
            Ok("ΩЖΣ".to_owned())
        );
        assert_eq!(
            ll_to_lower(&mut sl(), "ΩЖΣ".to_owned()),
            Ok("ωжσ".to_owned())
        );
    }

    #[test]
    fn replace_sub_string_from_either_end() {
        let replace = |count: i32| {
            ll_replace_sub_string(
                &mut sl(),
                "aaa-b-aaa".to_owned(),
                "a".to_owned(),
                "x".to_owned(),
                count,
            )
        };
        // strings/newer.
        assert_eq!(replace(0), Ok("xxx-b-xxx".to_owned()));
        assert_eq!(replace(2), Ok("xxa-b-aaa".to_owned()));
        assert_eq!(replace(-2), Ok("aaa-b-axx".to_owned()));
        let overlapping = |count: i32| {
            ll_replace_sub_string(
                &mut sl(),
                "aaa-b-aaa".to_owned(),
                "aa".to_owned(),
                "y".to_owned(),
                count,
            )
        };
        assert_eq!(overlapping(0), Ok("ya-b-ya".to_owned()));
        assert_eq!(overlapping(-1), Ok("aaa-b-ay".to_owned()));
        assert_eq!(
            ll_replace_sub_string(
                &mut sl(),
                "abc".to_owned(),
                String::new(),
                "x".to_owned(),
                0
            ),
            Ok("abc".to_owned())
        );
    }

    #[test]
    fn char_and_ord_per_grid() {
        let char_of = |ctx: &mut crate::library::bare::Bare, code: i32| ll_char(ctx, code);
        // errors/bounds, strings/unicode.
        assert_eq!(char_of(&mut sl(), 0), Ok(String::new()));
        assert_eq!(char_of(&mut sl(), -1), Ok("\u{FFFD}".to_owned()));
        assert_eq!(char_of(&mut sl(), 0x11_0000), Ok("\u{FFFD}".to_owned()));
        assert_eq!(char_of(&mut sl(), 0xD800), Ok("\u{FFFD}".to_owned()));
        assert_eq!(char_of(&mut sl(), 0xFFFF), Ok("\u{FFFD}".to_owned()));
        assert_eq!(char_of(&mut os(), 0xFFFF), Ok("\u{FFFF}".to_owned()));
        assert_eq!(char_of(&mut sl(), 0x1F600), Ok("😀".to_owned()));
        let ord =
            |ctx: &mut crate::library::bare::Bare, at: i32| ll_ord(ctx, "a😀b".to_owned(), at);
        assert_eq!(ord(&mut sl(), 1), Ok(0x1F600));
        assert_eq!(ord(&mut sl(), 2), Ok(98));
        assert_eq!(ord(&mut sl(), 3), Ok(0));
        assert_eq!(ord(&mut os(), 1), Ok(0x1F600));
        assert_eq!(ord(&mut os(), 2), Ok(0x1F600));
        assert_eq!(ord(&mut os(), 3), Ok(98));
        assert_eq!(ord(&mut sl(), -99), Ok(0));
    }

    #[test]
    fn hash_matches_the_transcripts() {
        // strings/unicode, every grid.
        let hash = |text: &str| ll_hash(&mut (), text.to_owned());
        assert_eq!(hash(""), Ok(0));
        assert_eq!(hash("a"), Ok(97));
        assert_eq!(hash("abc"), Ok(807_794_786));
        assert_eq!(hash("é"), Ok(233));
        assert_eq!(hash("😀"), Ok(128_512));
    }
}
