//! The port held to Mono: the vendored tables' layout, a few answers worth
//! naming, and every case `mono/collation-cases.txt` recorded.

use pretty_assertions::assert_eq;

use super::tables::{CATEGORY, IGNORABLE, LEVEL2, LEVEL3, TABLES};
use super::{Case, OutOfRange, ends_with, index_of, last_index_of, starts_with};

/// The cases Mono answered, vendored.
const CASES: &str = include_str!("../../../mono/collation-cases.txt");

/// `text` as .NET's UTF-16.
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `String.IndexOf(value, comparison)`.
fn find(source: &str, value: &str, case: Case) -> i32 {
    let source = utf16(source);
    let value = utf16(value);
    let length = i32::try_from(source.len()).unwrap_or(i32::MAX);
    index_of(&source, &value, 0, length, case).unwrap_or(-2)
}

#[test]
fn core_table_sections_have_their_indexers_lengths() {
    assert_eq!(TABLES.ignorable.len(), IGNORABLE.total(), "ignorable flags");
    assert_eq!(TABLES.category.len(), CATEGORY.total(), "categories");
    assert_eq!(TABLES.level1.len(), CATEGORY.total(), "level 1");
    assert_eq!(TABLES.level2.len(), LEVEL2.total(), "level 2");
    assert_eq!(TABLES.level3.len(), LEVEL3.total(), "level 3");
}

#[test]
fn culture_data_is_en_us() {
    assert_eq!(TABLES.contractions.len(), 35, "the invariant tailoring");
    assert_eq!(TABLES.to_lower(u32::from('A')), u32::from('a'), "ASCII");
    assert_eq!(
        TABLES.to_lower(0x130),
        u32::from('i'),
        "İ lowers to i in en-US"
    );
    assert_eq!(TABLES.to_lower(0x1E9E), 0xDF, "ẞ lowers to ß");
    assert!(TABLES.is_unassigned(0x0378), "U+0378 is unassigned");
    assert!(!TABLES.is_unassigned(u32::from('a')), "a is assigned");
}

#[test]
fn expansions_ligatures_and_accents() {
    assert_eq!(find("æon", "aeon", Case::Sensitive), 0, "æ reads as ae");
    assert_eq!(
        find("xæ", "ae", Case::Sensitive),
        -1,
        "the ASCII quick path rules out every start but 0 by length"
    );
    assert_eq!(
        find("xæ", "ae", Case::Ignored),
        1,
        "which ignoring case skips"
    );
    assert_eq!(find("file", "ﬁle", Case::Sensitive), 0, "ﬁ reads as fi");
    assert_eq!(
        find("café", "cafe", Case::Sensitive),
        -1,
        "an accent is a difference"
    );
    assert_eq!(
        find("cafe\u{301}", "café", Case::Sensitive),
        0,
        "combining = precomposed"
    );
    assert_eq!(
        find("Hello World", "world", Case::Sensitive),
        -1,
        "case matters"
    );
    assert_eq!(
        find("Hello World", "world", Case::Ignored),
        6,
        "unless ignored"
    );
    assert_eq!(find("Strasse", "straße", Case::Ignored), 0, "ß reads as ss");
    assert_eq!(
        find("Straße", "STRASSE", Case::Ignored),
        -1,
        "but not the other way"
    );
    assert_eq!(
        find("a\u{FDD0}b", "ab", Case::Sensitive),
        0,
        "a noncharacter weighs nothing"
    );
    assert_eq!(
        find("a\u{AD}b", "ab", Case::Sensitive),
        -1,
        "a soft hyphen does"
    );
}

#[test]
fn last_index_of_reads_its_start_as_the_end() {
    let text = utf16("abc");
    let a = utf16("a");
    assert_eq!(
        last_index_of(&text, &a, 0, 1, Case::Sensitive),
        Ok(0),
        "abc, a, 0, 1"
    );
    assert_eq!(
        last_index_of(&text, &a, 0, 3, Case::Sensitive),
        Err(OutOfRange),
        "abc, a, 0, 3"
    );
    assert_eq!(
        last_index_of(&text, &a, 1, 2, Case::Sensitive),
        Ok(0),
        "abc, a, 1, 2"
    );
    assert!(starts_with(&text, &a, Case::Sensitive), "abc starts with a");
    assert!(
        !ends_with(&text, &a, Case::Sensitive),
        "abc does not end with a"
    );
}

/// One recorded case's units, `-` for none.
fn decode(word: &str) -> Vec<u16> {
    if word == "-" {
        return Vec::new();
    }
    word.split('.')
        .filter_map(|unit| u16::from_str_radix(unit, 16).ok())
        .collect()
}

/// A search's answer as the cases write it.
fn answer(result: Result<i32, OutOfRange>) -> String {
    result.map_or_else(|OutOfRange| "X".to_owned(), |found| found.to_string())
}

/// Every case of `cases` the port answers differently, as one line each.
fn mismatches(cases: &str) -> Vec<String> {
    let mut wrong = Vec::new();
    for line in cases.lines().filter(|line| !line.starts_with('#')) {
        let words: Vec<&str> = line.split(' ').collect();
        let [source, value, start, count, last, back, recorded @ ..] = words.as_slice() else {
            wrong.push(format!("unreadable: {line}"));
            continue;
        };
        let source = decode(source);
        let value = decode(value);
        let number = |word: &str| word.parse::<i32>().unwrap_or(-1);
        let length = i32::try_from(source.len()).unwrap_or(i32::MAX);
        let mut ours = Vec::new();
        for case in [Case::Sensitive, Case::Ignored] {
            ours.push(answer(index_of(&source, &value, 0, length, case)));
            ours.push(answer(last_index_of(
                &source,
                &value,
                length.saturating_sub(1),
                length,
                case,
            )));
            ours.push(
                if starts_with(&source, &value, case) {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            );
            ours.push(
                if ends_with(&source, &value, case) {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            );
            ours.push(answer(index_of(
                &source,
                &value,
                number(start),
                number(count),
                case,
            )));
            ours.push(answer(last_index_of(
                &source,
                &value,
                number(last),
                number(back),
                case,
            )));
        }
        if ours
            != recorded
                .iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        {
            wrong.push(format!("{line}\n  ours: {}", ours.join(" ")));
        }
    }
    wrong
}

#[test]
fn every_recorded_case_is_answered_as_mono_answered_it() {
    // `SL_LSL_COLLATION_CASES` names a larger set `dump_collation.cs` wrote,
    // for a run against more than the vendored sample.
    let cases = std::env::var("SL_LSL_COLLATION_CASES")
        .ok()
        .and_then(|path| fs_err::read_to_string(path).ok())
        .unwrap_or_else(|| CASES.to_owned());
    let wrong = mismatches(&cases);
    assert!(
        wrong.is_empty(),
        "{} cases differ from Mono, the first:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
