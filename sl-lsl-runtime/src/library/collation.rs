//! .NET's culture-sensitive string search as OpenSim runs it: Mono's
//! managed collator (`Mono.Globalization.Unicode.SimpleCollator`) under the
//! en-US culture OpenSim sets on every thread (`OpenSim.Framework.Culture`).
//!
//! `String.IndexOf(string)` with no comparison named, and every
//! `StringComparison.CurrentCulture…` search, is this, not a search unit by
//! unit: `llSubStringIndex`, the delimiters of `llParseString2List`, and the
//! OSSL searches `osStringIndexOf`, `osStringLastIndexOf`,
//! `osStringStartsWith` and `osStringEndsWith`. It finds `ae` in `æ` and
//! `fi` in `ﬁ`, ignores what the table gives no weight (U+0000, the
//! noncharacters, unassigned code points), folds a combining accent into the
//! letter before it, and with `IgnoreCase` lowers both sides by en-US's
//! `TextInfo.ToLower` first — so `ẞ` finds `ß`, but `ß` never finds `SS`.
//! It answers nothing a search unit by unit would explain, and several
//! things that are Mono's own bugs (book chapter `gridspec/lsl.md`, § OSSL
//! string search); this module ports the code, bugs included, rather than
//! describing what it does.
//!
//! What is ported is what en-US reaches. Its collator has the invariant
//! culture's tailoring: 35 contractions, each one code unit read as its
//! replacement, no contraction of several units, none to a sort key, no
//! diacritic remapping and no French accent order (`mono/dump_collation.cs`
//! refuses to write the tables if that ever changes). So the collator's
//! "unsafe" units, its sort-key contractions and the previous sort key they
//! leave behind (`Context.PrevSortKey`) never come into play, and are left
//! out. Everything else — the extenders and special weights of kana, the
//! quick paths, the per-search cache of ASCII units that always or never
//! match — is here, because each changes an answer.
//!
//! Positions are .NET's: `i32` indices into UTF-16 code units, where the
//! collator itself counts down to `-1`.

mod tables;

use tables::{CATEGORY, LEVEL2, LEVEL3, TABLES, Tables};

/// Whether a search folds case: `CompareOptions.None` or `IgnoreCase`
/// (`StringComparison.CurrentCulture` or `CurrentCultureIgnoreCase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Case {
    /// Case matters.
    Sensitive,
    /// Both sides lowered by en-US `TextInfo.ToLower` first.
    Ignored,
}

/// The `ArgumentOutOfRangeException` .NET throws at a start or count that
/// does not fit the string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutOfRange;

/// `String.IndexOf(value, startIndex, count, comparison)`: where `value`
/// first starts within `count` units from `start`, or `-1`.
///
/// # Errors
///
/// [`OutOfRange`] where .NET throws: `start` outside the string, or `count`
/// negative or reaching past its end.
pub(crate) fn index_of(
    source: &[u16],
    value: &[u16],
    start: i32,
    count: i32,
    case: Case,
) -> Result<i32, OutOfRange> {
    let length = len(source);
    if start < 0 || start > length || count < 0 || start > length.saturating_sub(count) {
        return Err(OutOfRange);
    }
    // `CompareInfo.IndexOf`.
    if source.is_empty() {
        return Ok(if value.is_empty() { 0 } else { -1 });
    }
    Ok(Collator::new(case).index_of(source, value, start, count))
}

/// `String.LastIndexOf(value, startIndex, count, comparison)`: where `value`
/// last starts within the `count` units that end at `start`, or `-1`. The
/// start is where the search **ends**, as .NET reads it.
///
/// # Errors
///
/// [`OutOfRange`] where .NET throws: `start` outside the string, or `count`
/// negative or reaching back past its beginning.
pub(crate) fn last_index_of(
    source: &[u16],
    value: &[u16],
    start: i32,
    count: i32,
    case: Case,
) -> Result<i32, OutOfRange> {
    let length = len(source);
    if length == 0 && (start == -1 || start == 0) {
        return Ok(if value.is_empty() { 0 } else { -1 });
    }
    if start < 0 || start > length {
        return Err(OutOfRange);
    }
    let (start, count) = if start == length {
        (
            start.saturating_sub(1),
            if count > 0 {
                count.saturating_sub(1)
            } else {
                count
            },
        )
    } else {
        (start, count)
    };
    if count < 0 || start.saturating_sub(count).saturating_add(1) < 0 {
        return Err(OutOfRange);
    }
    if value.is_empty() {
        return Ok(start);
    }
    Ok(Collator::new(case).last_index_of(source, value, start, count))
}

/// `String.StartsWith(value, ignoreCase, culture)` through
/// `CompareInfo.IsPrefix`: the empty value starts every string, nothing else
/// the empty string.
pub(crate) fn starts_with(source: &[u16], value: &[u16], case: Case) -> bool {
    if value.is_empty() {
        return true;
    }
    if source.is_empty() {
        return false;
    }
    Collator::new(case).is_prefix(source, value, 0, len(source), true)
}

/// `String.EndsWith(value, ignoreCase, culture)` through
/// `CompareInfo.IsSuffix`, which Mono answers by searching for the last
/// match and comparing from there to the end.
pub(crate) fn ends_with(source: &[u16], value: &[u16], case: Case) -> bool {
    if value.is_empty() {
        return true;
    }
    if source.is_empty() {
        return false;
    }
    let length = len(source);
    let last = Collator::new(case).last_index_of(source, value, length.saturating_sub(1), length);
    last >= 0 && Collator::new(case).compare(source, last, length.saturating_sub(last), value) == 0
}

/// The length of `text` as .NET's `int`.
fn len(text: &[u16]) -> i32 {
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

/// The code unit of `text` at `at`, as the collator's `int`. Every caller
/// keeps `at` inside the text, as Mono's code does (it would throw an
/// `IndexOutOfRangeException` otherwise); outside, it reads as U+0000.
fn unit(text: &[u16], at: i32) -> i32 {
    usize::try_from(at)
        .ok()
        .and_then(|at| text.get(at))
        .map_or(0, |unit| i32::from(*unit))
}

/// A code unit as the tables index it; a negative one (the collator's `-1`
/// for "none") as U+FFFF, which is what C#'s `(char) -1` is.
fn code(unit: i32) -> u32 {
    u32::try_from(unit).unwrap_or(0xFFFF)
}

/// `SimpleCollator.ExtenderType`: whether a code unit repeats the one
/// before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Extender {
    /// No extender.
    None,
    /// Repeats the previous kana.
    Simple,
    /// Repeats it voiced.
    Voiced,
    /// The prolonged sound mark, which repeats its vowel.
    Conditional,
    /// U+3005, which Mono weighs as a level-2 difference of 5.
    Buggy,
}

/// The four weights of one collation element, Mono's 4-byte sort-key
/// buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Key {
    /// The category (`sortkey [0]`).
    category: u8,
    /// The primary weight (`[1]`).
    level1: u8,
    /// The diacritic weight (`[2]`), summed with the marks that follow.
    level2: u8,
    /// The case and width weight (`[3]`).
    level3: u8,
}

impl Key {
    /// Adds a following diacritic's weight, as Mono does: a key without one
    /// starts from 2, and the byte wraps.
    const fn add_diacritic(&mut self, weight: u8) {
        if self.level2 == 0 {
            self.level2 = 2;
        }
        self.level2 = self.level2.wrapping_add(weight);
    }
}

/// One search's state, Mono's `Context`: the options, the cache of ASCII
/// units already found to match or not, the two scratch keys every
/// comparison writes into (and leaves stale where it does not), and the
/// last unit an extender would repeat.
#[derive(Debug)]
struct Collator {
    /// Whether case is folded.
    ignore_case: bool,
    /// Whether this search keeps the ASCII cache (`IndexOf` and
    /// `LastIndexOf` do; a bare prefix test or comparison does not).
    cached: bool,
    /// ASCII units that matched the head of the search
    /// (`AlwaysMatchFlags`), one bit each.
    always: u128,
    /// ASCII units that did not (`NeverMatchFlags`).
    never: u128,
    /// `Buffer1`.
    one: Key,
    /// `Buffer2`.
    two: Key,
    /// `PrevCode`: the last unit with a weight of its own, which an
    /// extender repeats; `-1` for none.
    prev_code: i32,
}

/// What [`Collator::compare_internal`] found.
#[derive(Debug, Clone, Copy)]
struct Compared {
    /// Negative, zero or positive, as the first string sorts before, with or
    /// after the second; `-1` for any difference when breaking up early.
    result: i32,
    /// Whether the second string was used up with no difference.
    target_consumed: bool,
}

/// Where a comparison resumes after a contraction's replacement
/// (`SimpleCollator.Escape`).
#[derive(Debug, Clone, Copy)]
struct Escape<'text> {
    /// The text the replacement interrupted.
    source: &'text [u16],
    /// Its start.
    start: i32,
    /// Where to go on in it.
    index: i32,
    /// Its end.
    end: i32,
    /// Its quick-check position.
    optional: i32,
}

/// One side of a comparison: the text it is reading, its range, the
/// quick-check position and an escape back out of a replacement.
#[derive(Debug, Clone, Copy)]
struct Side<'text> {
    /// The text.
    text: &'text [u16],
    /// `start1` / `start2`.
    start: i32,
    /// `idx1` / `idx2`.
    at: i32,
    /// `end1` / `end2`.
    end: i32,
    /// `quickCheckPos1` / `2`.
    quick: i32,
    /// `escape1` / `2`.
    escape: Option<Escape<'text>>,
}

impl<'text> Side<'text> {
    /// Steps into `replacement`, to come back after the unit at `cur`.
    fn enter(&mut self, replacement: &'text [u16], cur: i32) {
        self.escape = Some(Escape {
            source: self.text,
            start: self.start,
            index: cur.saturating_add(1),
            end: self.end,
            optional: self.quick,
        });
        self.text = replacement;
        self.at = 0;
        self.start = 0;
        self.end = len(replacement);
        self.quick = 0;
    }

    /// Steps back out of a replacement; `false` when not in one.
    const fn leave(&mut self) -> bool {
        let Some(escape) = self.escape.take() else {
            return false;
        };
        self.text = escape.source;
        self.start = escape.start;
        self.at = escape.index;
        self.end = escape.end;
        self.quick = escape.optional;
        true
    }
}

/// The table lookups (`MSCompatUnicodeTable`, and the culture's
/// contractions) on the code units the collator passes around.
trait Weights {
    /// `Category`.
    fn category(&self, unit: i32) -> u8;
    /// `Level1`.
    fn level1(&self, unit: i32) -> u8;
    /// `SimpleCollator.Level2`.
    fn level2(&self, unit: i32, extender: Extender) -> u8;
    /// `Level3`.
    fn level3(&self, unit: i32) -> u8;
    /// `IsIgnorable(cp, flag)`.
    fn is_ignorable_with(&self, unit: i32, flag: u8) -> bool;
    /// `SimpleCollator.IsIgnorable`.
    fn is_ignorable(&self, unit: i32) -> bool;
    /// `IsIgnorableNonSpacing`.
    fn is_ignorable_non_spacing(&self, unit: i32) -> bool;
    /// `GetContraction(s, start, end)`.
    fn contraction(&self, text: &[u16], start: i32, end: i32) -> Option<&Vec<u16>>;
    /// `GetTailContraction(s, start, end)`.
    fn tail_contraction(&self, text: &[u16], start: i32, end: i32) -> Option<&[u16]>;
}

impl Weights for Tables {
    /// `Category`.
    fn category(&self, unit: i32) -> u8 {
        Self::lookup(self.category, &CATEGORY, code(unit))
    }

    /// `Level1`.
    fn level1(&self, unit: i32) -> u8 {
        Self::lookup(self.level1, &CATEGORY, code(unit))
    }

    /// `SimpleCollator.Level2`: the table's, but 5 for U+3005 and 0 for the
    /// prolonged sound mark.
    fn level2(&self, unit: i32, extender: Extender) -> u8 {
        match extender {
            Extender::Buggy => 5,
            Extender::Conditional => 0,
            Extender::None | Extender::Simple | Extender::Voiced => {
                Self::lookup(self.level2, &LEVEL2, code(unit))
            }
        }
    }

    /// `Level3`.
    fn level3(&self, unit: i32) -> u8 {
        Self::lookup(self.level3, &LEVEL3, code(unit))
    }

    /// `IsIgnorable(cp, flag)`: U+0000 always; for flag 1, every unassigned
    /// unit and the surrogates U+D880 … U+DB7F too; otherwise the table.
    fn is_ignorable_with(&self, unit: i32, flag: u8) -> bool {
        if unit == 0 {
            return true;
        }
        let unit = code(unit);
        if flag & 1 != 0 && (self.is_unassigned(unit) || (0xD880..0xDB80).contains(&unit)) {
            return true;
        }
        self.ignorable_flags(unit)
            .is_some_and(|flags| flags & flag != 0)
    }

    /// `SimpleCollator.IsIgnorable` under the options here, which is flag 1.
    fn is_ignorable(&self, unit: i32) -> bool {
        self.is_ignorable_with(unit, 1)
    }

    /// `IsIgnorableNonSpacing`.
    fn is_ignorable_non_spacing(&self, unit: i32) -> bool {
        self.is_ignorable_with(unit, 4)
    }

    /// The contraction of the unit at `start`, if it has one, as
    /// `GetContraction(s, start, end)` finds it: a one-unit contraction needs
    /// `end - start >= 1`, and some callers pass a length as `end`.
    fn contraction(&self, text: &[u16], start: i32, end: i32) -> Option<&Vec<u16>> {
        let wanted = unit(text, start);
        self.contractions
            .iter()
            .find(|(source, _)| i32::from(*source) >= wanted)
            .filter(|(source, _)| i32::from(*source) == wanted && end.saturating_sub(start) >= 1)
            .map(|(_, replacement)| replacement)
    }

    /// The contraction ending at `start`, searching backwards to `end`
    /// (`GetTailContraction`).
    fn tail_contraction(&self, text: &[u16], start: i32, end: i32) -> Option<&[u16]> {
        if start.saturating_sub(end) < 1 {
            return None;
        }
        let wanted = unit(text, start);
        self.contractions
            .iter()
            .find(|(source, _)| i32::from(*source) == wanted)
            .map(|(_, replacement)| replacement.as_slice())
    }
}

/// `HasSpecialWeight`: kana, which differ at a fourth level.
fn has_special_weight(unit: i32) -> bool {
    let unit = code(unit);
    if unit < 0x3041 {
        false
    } else if (0xFF66..0xFF9E).contains(&unit) {
        true
    } else if unit >= 0x3300 {
        false
    } else if unit < 0x309D {
        unit < 0x3099
    } else if unit < 0x3100 {
        unit != 0x30FB
    } else {
        (0x32D0..0x32FF).contains(&unit)
    }
}

/// `IsHalfWidthKana`.
fn is_half_width_kana(unit: i32) -> bool {
    (0xFF66..=0xFF9D).contains(&code(unit))
}

/// `IsHiragana`.
fn is_hiragana(unit: i32) -> bool {
    (0x3041..=0x3094).contains(&code(unit))
}

/// `IsJapaneseSmallLetter`.
fn is_japanese_small_letter(unit: i32) -> bool {
    let unit = code(unit);
    (0xFF67..=0xFF6F).contains(&unit)
        || matches!(
            unit,
            0x3041
                | 0x3043
                | 0x3045
                | 0x3047
                | 0x3049
                | 0x3063
                | 0x3083
                | 0x3085
                | 0x3087
                | 0x308E
                | 0x30A1
                | 0x30A3
                | 0x30A5
                | 0x30A7
                | 0x30A9
                | 0x30C3
                | 0x30E3
                | 0x30E5
                | 0x30E7
                | 0x30EE
                | 0x30F5
                | 0x30F6
        )
}

/// `GetExtenderType` (U+2015 is one only for Japanese).
const fn extender_type(unit: i32) -> Extender {
    match unit {
        0xFE7C | 0xFE7D | 0x3031 | 0x3032 | 0x309D | 0x30FD => Extender::Simple,
        0xFF9E | 0xFF9F | 0x309E | 0x30FE => Extender::Voiced,
        0xFF70 | 0x30FC => Extender::Conditional,
        0x3005 => Extender::Buggy,
        _ => Extender::None,
    }
}

/// `ToDashTypeValue`.
const fn dash_type(extender: Extender) -> u8 {
    match extender {
        Extender::None => 3,
        Extender::Conditional => 5,
        Extender::Simple | Extender::Voiced | Extender::Buggy => 4,
    }
}

/// `CompareFlagPair`.
const fn flag_pair(first: bool, second: bool) -> i32 {
    if first == second {
        0
    } else if first {
        1
    } else {
        -1
    }
}

/// The bit of an ASCII unit in a match cache, `None` past ASCII.
fn ascii_bit(unit: i32) -> Option<u128> {
    u32::try_from(unit)
        .ok()
        .filter(|unit| *unit < 128)
        .map(|unit| 1_u128 << unit)
}

impl Collator {
    /// A collator for one search.
    const fn new(case: Case) -> Self {
        Self {
            ignore_case: matches!(case, Case::Ignored),
            cached: false,
            always: 0,
            never: 0,
            one: Key {
                category: 0,
                level1: 0,
                level2: 0,
                level3: 0,
            },
            two: Key {
                category: 0,
                level1: 0,
                level2: 0,
                level3: 0,
            },
            prev_code: -1,
        }
    }

    /// `FilterOptions`: the unit lowered when case is ignored.
    fn filter(&self, unit: i32) -> i32 {
        if self.ignore_case {
            i32::try_from(TABLES.to_lower(code(unit))).unwrap_or(unit)
        } else {
            unit
        }
    }

    /// `FilterExtender`: the vowel a prolonged sound mark repeats after
    /// `unit`; any other extender repeats `unit` itself.
    fn filter_extender(unit: i32, extender: Extender) -> i32 {
        if extender != Extender::Conditional || !has_special_weight(unit) {
            return unit;
        }
        let half = is_half_width_kana(unit);
        let katakana = !is_hiragana(unit);
        let pick = |half_width: i32, kata: i32, hira: i32| {
            if half {
                half_width
            } else if katakana {
                kata
            } else {
                hira
            }
        };
        match TABLES.level1(unit) & 7 {
            2 => pick(0xFF71, 0x30A2, 0x3042),
            3 => pick(0xFF72, 0x30A4, 0x3044),
            4 => pick(0xFF73, 0x30A6, 0x3046),
            5 => pick(0xFF74, 0x30A8, 0x3048),
            6 => pick(0xFF75, 0x30AA, 0x304A),
            _ => unit,
        }
    }

    /// `Compare`: the sign of how `first[start..start+length]` sorts against
    /// `second`.
    fn compare(&mut self, first: &[u16], start: i32, length: i32, second: &[u16]) -> i32 {
        self.compare_internal(first, start, length, second, 0, len(second), true, false)
            .result
            .signum()
    }

    /// `IsPrefix(s, target, start, length, skipHeadingExtenders, ctx)`.
    fn is_prefix(
        &mut self,
        text: &[u16],
        target: &[u16],
        start: i32,
        length: i32,
        skip_heading_extenders: bool,
    ) -> bool {
        self.compare_internal(
            text,
            start,
            length,
            target,
            0,
            len(target),
            skip_heading_extenders,
            true,
        )
        .target_consumed
    }

    /// `CompareInternal`: compares the two ranges level by level — primary
    /// weights at once, diacritics, case and kana as tie-breaks carried to
    /// the end — returning at the first primary difference, or at any
    /// difference when `immediate_breakup`.
    #[expect(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "a line-for-line port of Mono's CompareInternal, kept in its shape so it can be \
                  read against the original"
    )]
    fn compare_internal(
        &mut self,
        first: &[u16],
        first_start: i32,
        first_length: i32,
        second: &[u16],
        second_start: i32,
        second_length: i32,
        skip_heading_extenders: bool,
        immediate_breakup: bool,
    ) -> Compared {
        let tables = &*TABLES;
        let mut one = Side {
            text: first,
            start: first_start,
            at: first_start,
            end: first_start.saturating_add(first_length),
            quick: 0,
            escape: None,
        };
        let mut two = Side {
            text: second,
            start: second_start,
            at: second_start,
            end: second_start.saturating_add(second_length),
            quick: 0,
            escape: None,
        };
        let different = |result: i32| Compared {
            result,
            target_consumed: false,
        };
        let mut prev2_code = -1_i32;
        let mut final_result = 0_i32;
        let mut current_level = 5_u8;
        let mut lv5_at1 = -1_i32;
        let mut lv5_at2 = -1_i32;
        let mut lv5_value1 = 0_i32;
        let mut lv5_value2 = 0_i32;

        if skip_heading_extenders {
            for side in [&mut one, &mut two] {
                while side.at < side.end
                    && extender_type(unit(side.text, side.at)) != Extender::None
                {
                    side.at = side.at.saturating_add(1);
                }
            }
        }
        let mut ext1 = Extender::None;
        let mut ext2 = Extender::None;
        one.quick = one.at;
        two.quick = two.at;

        loop {
            for side in [&mut one, &mut two] {
                while side.at < side.end && tables.is_ignorable(unit(side.text, side.at)) {
                    side.at = side.at.saturating_add(1);
                }
            }
            if one.at >= one.end {
                if one.leave() {
                    continue;
                }
                break;
            }
            if two.at >= two.end {
                if two.leave() {
                    continue;
                }
                break;
            }

            // The quick check: run over identical units, then back to a
            // unit that is no diacritic.
            if one.quick < one.at && two.quick < two.at {
                while one.at < one.end && two.at < two.end {
                    let (mine, theirs) = (unit(one.text, one.at), unit(two.text, two.at));
                    if mine != theirs {
                        break;
                    }
                    one.at = one.at.saturating_add(1);
                    two.at = two.at.saturating_add(1);
                }
                if one.at == one.end || two.at == two.end {
                    continue;
                }
                for side in [&mut one, &mut two] {
                    let backward_end = side.quick;
                    side.quick = side.at;
                    side.at = side.at.saturating_sub(1);
                    while side.at > backward_end && tables.category(unit(side.text, side.at)) == 1 {
                        side.at = side.at.saturating_sub(1);
                    }
                    // Mono steps back further over "unsafe" units, which are
                    // part of a contraction of several; en-US has none.
                }
            }

            let cur1 = one.at;
            let cur2 = two.at;
            let mut i1 = self.filter(unit(one.text, one.at));
            let mut i2 = self.filter(unit(two.text, two.at));
            let mut special1 = false;
            let mut special2 = false;

            // An extender repeats the previous unit; with none before it
            // there is nothing to extend (a previous sort key, Mono's other
            // source, only a sort-key contraction leaves).
            ext1 = extender_type(i1);
            if ext1 != Extender::None {
                if self.prev_code < 0 {
                    one.at = one.at.saturating_add(1);
                    continue;
                }
                i1 = Self::filter_extender(self.prev_code, ext1);
            }
            ext2 = extender_type(i2);
            if ext2 != Extender::None {
                if prev2_code < 0 {
                    two.at = two.at.saturating_add(1);
                    continue;
                }
                i2 = Self::filter_extender(prev2_code, ext2);
            }

            let cat1 = tables.category(i1);
            let cat2 = tables.category(i2);

            // Special (variable) weights compare at the fifth level, by
            // where they stand and then by value.
            if cat1 == 6 {
                if current_level == 5 {
                    lv5_at1 = one.escape.map_or_else(
                        || cur1.saturating_sub(one.start),
                        |escape| escape.index.saturating_sub(escape.start),
                    );
                    // `Level1 (i1) << 8 + Uni.Level3 (i1)`: C# reads it as a
                    // shift by 8 plus the third level.
                    lv5_value1 = i32::from(tables.level1(i1))
                        .wrapping_shl(8_u32.saturating_add(u32::from(tables.level3(i1))));
                }
                self.prev_code = i1;
                one.at = one.at.saturating_add(1);
            }
            if cat2 == 6 {
                if current_level == 5 {
                    lv5_at2 = two.escape.map_or_else(
                        || cur2.saturating_sub(two.start),
                        |escape| escape.index.saturating_sub(escape.start),
                    );
                    lv5_value2 = i32::from(tables.level1(i2))
                        .wrapping_shl(8_u32.saturating_add(u32::from(tables.level3(i2))));
                }
                prev2_code = i2;
                two.at = two.at.saturating_add(1);
            }
            if cat1 == 6 || cat2 == 6 {
                if current_level == 5 {
                    if lv5_value1 == lv5_value2 {
                        lv5_at1 = -1;
                        lv5_at2 = -1;
                        lv5_value1 = 0;
                        lv5_value2 = 0;
                    } else {
                        current_level = 4;
                    }
                }
                continue;
            }

            let ct1 = if ext1 == Extender::None {
                tables.contraction(one.text, one.at, one.end)
            } else {
                None
            };
            if let Some(replacement) = ct1 {
                // A replacement's units are no contraction's, so this is
                // never reached from inside one.
                if one.escape.is_none() {
                    one.enter(replacement, cur1);
                    continue;
                }
            } else {
                self.one.category = cat1;
                self.one.level1 = tables.level1(i1);
                if current_level > 1 {
                    self.one.level2 = tables.level2(i1, ext1);
                }
                if current_level > 2 {
                    self.one.level3 = tables.level3(i1);
                }
                if current_level > 3 {
                    special1 = has_special_weight(i1);
                }
                if cat1 > 1 {
                    self.prev_code = i1;
                }
            }

            let ct2 = if ext2 == Extender::None {
                tables.contraction(two.text, two.at, two.end)
            } else {
                None
            };
            if let Some(replacement) = ct2 {
                two.at = two.at.saturating_add(1);
                if two.escape.is_none() {
                    two.enter(replacement, cur2);
                    continue;
                }
            } else {
                self.two.category = cat2;
                self.two.level1 = tables.level1(i2);
                if current_level > 1 {
                    self.two.level2 = tables.level2(i2, ext2);
                }
                if current_level > 2 {
                    self.two.level3 = tables.level3(i2);
                }
                if current_level > 3 {
                    special2 = has_special_weight(i2);
                }
                if cat2 > 1 {
                    prev2_code = i2;
                }
                two.at = two.at.saturating_add(1);
            }

            one.at = one.at.saturating_add(1);

            // The diacritics that follow each side fold into its key.
            while one.at < one.end {
                let mark = unit(one.text, one.at);
                if tables.category(mark) != 1 {
                    break;
                }
                self.one.add_diacritic(tables.level2(mark, Extender::None));
                one.at = one.at.saturating_add(1);
            }
            while two.at < two.end {
                let mark = unit(two.text, two.at);
                if tables.category(mark) != 1 {
                    break;
                }
                self.two.add_diacritic(tables.level2(mark, Extender::None));
                two.at = two.at.saturating_add(1);
            }

            let primary = i32::from(self.one.category).saturating_sub(i32::from(self.two.category));
            let primary = if primary == 0 {
                i32::from(self.one.level1).saturating_sub(i32::from(self.two.level1))
            } else {
                primary
            };
            if primary != 0 {
                return different(primary);
            }
            if current_level == 1 {
                continue;
            }
            let secondary = i32::from(self.one.level2).saturating_sub(i32::from(self.two.level2));
            if secondary != 0 {
                final_result = secondary;
                if immediate_breakup {
                    return different(-1);
                }
                current_level = 1;
                continue;
            }
            if current_level == 2 {
                continue;
            }
            let tertiary = i32::from(self.one.level3).saturating_sub(i32::from(self.two.level3));
            if tertiary != 0 {
                final_result = tertiary;
                if immediate_breakup {
                    return different(-1);
                }
                current_level = 2;
                continue;
            }
            if current_level == 3 {
                continue;
            }
            if special1 != special2 {
                if immediate_breakup {
                    return different(-1);
                }
                final_result = if special1 { 1 } else { -1 };
                current_level = 3;
                continue;
            }
            if special1 {
                let mut kana =
                    flag_pair(!is_japanese_small_letter(i1), !is_japanese_small_letter(i2));
                if kana == 0 {
                    kana = i32::from(dash_type(ext1)).saturating_sub(i32::from(dash_type(ext2)));
                }
                if kana == 0 {
                    kana = flag_pair(is_hiragana(i1), is_hiragana(i2));
                }
                if kana == 0 {
                    kana = flag_pair(!is_half_width_kana(i1), !is_half_width_kana(i2));
                }
                if kana != 0 {
                    if immediate_breakup {
                        return different(-1);
                    }
                    final_result = kana;
                    current_level = 3;
                }
            }
        }

        // Only differences below the first level: look for a diacritic
        // difference in the marks left over.
        if final_result != 0 && current_level > 2 {
            while one.at < one.end && two.at < two.end {
                let mark1 = unit(one.text, one.at);
                let mark2 = unit(two.text, two.at);
                if !tables.is_ignorable_non_spacing(mark1)
                    || !tables.is_ignorable_non_spacing(mark2)
                {
                    break;
                }
                final_result = i32::from(tables.level2(self.filter(mark1), ext1))
                    .saturating_sub(i32::from(tables.level2(self.filter(mark2), ext2)));
                if final_result != 0 {
                    break;
                }
                one.at = one.at.saturating_add(1);
                two.at = two.at.saturating_add(1);
                ext1 = Extender::None;
                ext2 = Extender::None;
            }
        }
        if current_level == 1 && final_result != 0 {
            for side in [&mut one, &mut two] {
                while side.at < side.end
                    && tables.is_ignorable_non_spacing(unit(side.text, side.at))
                {
                    side.at = side.at.saturating_add(1);
                }
            }
        }
        if final_result == 0 {
            final_result = if lv5_at1 < 0 && lv5_at2 >= 0 {
                -1
            } else if lv5_at2 < 0 && lv5_at1 >= 0 {
                1
            } else {
                let by_place = lv5_at1.saturating_sub(lv5_at2);
                if by_place == 0 {
                    lv5_value1.saturating_sub(lv5_value2)
                } else {
                    by_place
                }
            };
        }
        let target_consumed = final_result == 0 && two.at == two.end;
        let result = if one.at != one.end {
            1
        } else if two.at == two.end {
            final_result
        } else {
            -1
        };
        Compared {
            result,
            target_consumed,
        }
    }

    /// The public `IndexOf(s, target, start, length, opt)`: an ASCII-only
    /// search unit by unit when case matters, the collator otherwise.
    fn index_of(&mut self, text: &[u16], target: &[u16], start: i32, length: i32) -> i32 {
        if !self.ignore_case
            && let Some(found) = quick_index_of(text, target, start, length)
        {
            return found;
        }
        self.cached = true;
        let mut key = Key::default();
        self.index_of_inner(text, target, start, length, &mut key)
    }

    /// The public `LastIndexOf(s, target, start, length, opt)`, which has no
    /// quick path.
    fn last_index_of(&mut self, text: &[u16], target: &[u16], start: i32, length: i32) -> i32 {
        self.cached = true;
        let mut key = Key::default();
        self.last_index_of_inner(text, target, start, length, &mut key)
    }

    /// The target's head as a search key: where its first weighted unit is,
    /// that unit's contraction if it has one, and otherwise its key — with
    /// the diacritics after it folded in — its filtered unit and whether it
    /// has no fourth level.
    fn head(&self, target: &[u16], key: &mut Key) -> Head<'static> {
        let tables = &*TABLES;
        let length = len(target);
        let mut tidx = 0_i32;
        while tidx < length && tables.is_ignorable(unit(target, tidx)) {
            tidx = tidx.saturating_add(1);
        }
        if tidx == length {
            return Head::Ignorable;
        }
        // Mono passes the remaining length where the end belongs.
        if let Some(replacement) = tables.contraction(target, tidx, length.saturating_sub(tidx)) {
            return Head::Replaced(replacement);
        }
        let ti = self.filter(unit(target, tidx));
        *key = Key {
            category: tables.category(ti),
            level1: tables.level1(ti),
            level2: tables.level2(ti, Extender::None),
            level3: tables.level3(ti),
        };
        let no_lv4 = !has_special_weight(ti);
        tidx = tidx.saturating_add(1);
        while tidx < length {
            let mark = unit(target, tidx);
            if tables.category(mark) != 1 {
                break;
            }
            key.add_diacritic(tables.level2(mark, Extender::None));
            tidx = tidx.saturating_add(1);
        }
        Head::Unit { ti, no_lv4 }
    }

    /// `IndexOf(s, target, start, length, targetSortKey, ctx)`: find the
    /// target's head, then test the whole target as a prefix there; on a
    /// miss go on one unit later.
    fn index_of_inner(
        &mut self,
        text: &[u16],
        target: &[u16],
        start: i32,
        length: i32,
        key: &mut Key,
    ) -> i32 {
        let head = self.head(target, key);
        let (mut start, mut length) = (start, length);
        loop {
            let found = match head {
                Head::Ignorable => {
                    return if target.contains(&0) {
                        index_of_ordinal(text, target, start, length)
                    } else {
                        start
                    };
                }
                Head::Replaced(replacement) => {
                    self.index_of_inner(text, replacement, start, length, key)
                }
                Head::Unit { ti, no_lv4 } => {
                    self.index_of_sort_key(text, start, length, ti, *key, no_lv4)
                }
            };
            if found < 0 {
                return -1;
            }
            length = length.saturating_sub(found.saturating_sub(start));
            start = found;
            if self.is_prefix(text, target, start, length, false) {
                return found;
            }
            // Past the unit there, a contraction's one unit or any other.
            start = start.saturating_add(1);
            length = length.saturating_sub(1);
            if length <= 0 {
                return -1;
            }
        }
    }

    /// `LastIndexOf(s, target, start, length, targetSortKey, ctx)`: the same
    /// backwards from `start`, the match then moved past ignorable units.
    fn last_index_of_inner(
        &mut self,
        text: &[u16],
        target: &[u16],
        start: i32,
        length: i32,
        key: &mut Key,
    ) -> i32 {
        let tables = &*TABLES;
        let org_start = start;
        let head = self.head(target, key);
        let (mut start, mut length) = (start, length);
        loop {
            let found = match head {
                Head::Ignorable => {
                    return if target.contains(&0) {
                        last_index_of_ordinal(text, target, start, length)
                    } else {
                        start
                    };
                }
                Head::Replaced(replacement) => {
                    self.last_index_of_inner(text, replacement, start, length, key)
                }
                Head::Unit { ti, no_lv4 } => {
                    self.last_index_of_sort_key(text, start, org_start, length, ti, *key, no_lv4)
                }
            };
            if found < 0 {
                return -1;
            }
            length = length.saturating_sub(start.saturating_sub(found));
            start = found;
            let rest = org_start.saturating_sub(found).saturating_add(1);
            if self.is_prefix(text, target, found, rest, false) {
                let mut at = found;
                while at < org_start && tables.is_ignorable(unit(text, at)) {
                    at = at.saturating_add(1);
                }
                return at;
            }
            start = start.saturating_sub(1);
            length = length.saturating_sub(1);
            if length <= 0 {
                return -1;
            }
        }
    }

    /// `IndexOfSortKey`: the first unit from `start` whose key matches.
    fn index_of_sort_key(
        &mut self,
        text: &[u16],
        start: i32,
        length: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
    ) -> i32 {
        let end = start.saturating_add(length);
        let mut at = start;
        while at < end {
            let cur = at;
            if self.matches_forward(text, &mut at, end, ti, key, no_lv4) {
                return cur;
            }
        }
        -1
    }

    /// `LastIndexOfSortKey`: the last unit at or before `start` whose key
    /// matches.
    #[expect(
        clippy::too_many_arguments,
        reason = "Mono's LastIndexOfSortKey, argument for argument"
    )]
    fn last_index_of_sort_key(
        &mut self,
        text: &[u16],
        start: i32,
        org_start: i32,
        length: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
    ) -> i32 {
        let end = start.saturating_sub(length);
        let mut at = start;
        while at > end {
            let cur = at;
            if self.matches_backward(text, &mut at, end, org_start, ti, key, no_lv4) {
                return cur;
            }
        }
        -1
    }

    /// The ASCII cache's answer for `unit`, if it has one.
    const fn cached_answer(&self, bit: Option<u128>) -> Option<bool> {
        match bit {
            Some(bit) if self.cached && self.always & bit != 0 => Some(true),
            Some(bit) if self.cached && self.never & bit != 0 => Some(false),
            _ => None,
        }
    }

    /// Records in the ASCII cache what a plain unit (no contraction, no
    /// extender) answered.
    const fn remember(&mut self, bit: Option<u128>, plain: bool, matched: bool) {
        if let (true, true, Some(bit)) = (self.cached, plain, bit) {
            if matched {
                self.always |= bit;
            } else {
                self.never |= bit;
            }
        }
    }

    /// `MatchesForward`: whether the element at `at` matches the key,
    /// moving `at` past it (but not past a cached match).
    fn matches_forward(
        &mut self,
        text: &[u16],
        at: &mut i32,
        end: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
    ) -> bool {
        let si = unit(text, *at);
        let bit = ascii_bit(si);
        match self.cached_answer(bit) {
            Some(true) => return true,
            Some(false) => {
                *at = at.saturating_add(1);
                return false;
            }
            None => {}
        }
        let extender = extender_type(si);
        let (matched, contraction) =
            self.matches_forward_core(text, at, end, ti, key, no_lv4, extender);
        self.remember(bit, !contraction && extender == Extender::None, matched);
        matched
    }

    /// `MatchesForwardCore`; the second answer is whether the element was a
    /// contraction.
    #[expect(
        clippy::too_many_arguments,
        reason = "Mono's MatchesForwardCore, argument for argument"
    )]
    fn matches_forward_core(
        &mut self,
        text: &[u16],
        at: &mut i32,
        end: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
        extender: Extender,
    ) -> (bool, bool) {
        let tables = &*TABLES;
        let mut si = -1_i32;
        let contraction = if extender == Extender::None {
            tables.contraction(text, *at, end)
        } else if self.prev_code < 0 {
            *at = at.saturating_add(1);
            return (false, false);
        } else {
            si = Self::filter_extender(self.prev_code, extender);
            None
        };
        if let Some(replacement) = contraction {
            *at = at.saturating_add(1);
            if !no_lv4 {
                return (false, true);
            }
            // Mono's documented lameness: a contraction matches when the
            // first element of its replacement does.
            let mut inner = 0_i32;
            let matched =
                self.matches_forward(replacement, &mut inner, len(replacement), ti, key, no_lv4);
            return (matched, true);
        }
        if si < 0 {
            si = self.filter(unit(text, *at));
        }
        *at = at.saturating_add(1);
        let mut no_match = false;
        self.one.category = tables.category(si);
        if key.category == self.one.category {
            self.one.level1 = tables.level1(si);
        } else {
            no_match = true;
        }
        if key.level1 == self.one.level1 {
            self.one.level2 = tables.level2(si, extender);
        } else {
            no_match = true;
        }
        if no_match {
            while *at < end && tables.category(unit(text, *at)) == 1 {
                *at = at.saturating_add(1);
            }
            return (false, false);
        }
        self.one.level3 = tables.level3(si);
        if self.one.category != 1 {
            self.prev_code = si;
        }
        while *at < end {
            let mark = unit(text, *at);
            if tables.category(mark) != 1 {
                break;
            }
            self.one.add_diacritic(tables.level2(mark, Extender::None));
            *at = at.saturating_add(1);
        }
        (
            matches_primitive(self.one, si, extender, key, ti, no_lv4),
            false,
        )
    }

    /// `MatchesBackward`: [`Self::matches_forward`] going back.
    #[expect(
        clippy::too_many_arguments,
        reason = "Mono's MatchesBackward, argument for argument"
    )]
    fn matches_backward(
        &mut self,
        text: &[u16],
        at: &mut i32,
        end: i32,
        org_start: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
    ) -> bool {
        let si = unit(text, *at);
        let bit = ascii_bit(si);
        match self.cached_answer(bit) {
            Some(true) => return true,
            Some(false) => {
                *at = at.saturating_sub(1);
                return false;
            }
            None => {}
        }
        let extender = extender_type(si);
        let (matched, contraction) =
            self.matches_backward_core(text, at, end, org_start, ti, key, no_lv4, extender);
        self.remember(bit, !contraction && extender == Extender::None, matched);
        matched
    }

    /// `MatchesBackwardCore`; the second answer is whether the element was a
    /// contraction.
    #[expect(
        clippy::too_many_arguments,
        reason = "Mono's MatchesBackwardCore, argument for argument"
    )]
    fn matches_backward_core(
        &mut self,
        text: &[u16],
        at: &mut i32,
        end: i32,
        org_start: i32,
        ti: i32,
        key: Key,
        no_lv4: bool,
        extender: Extender,
    ) -> (bool, bool) {
        let tables = &*TABLES;
        let cur = *at;
        let si;
        if extender != Extender::None {
            // An extender takes the next primary unit back — which, as Mono
            // starts the walk at the extender itself, is usually the
            // extender.
            let mut diacritical = 0_u8;
            let mut back = *at;
            loop {
                if back < 0 {
                    return (false, false);
                }
                let raw = unit(text, back);
                if tables.is_ignorable(raw) {
                    back = back.saturating_sub(1);
                    continue;
                }
                let filtered = self.filter(raw);
                let category = tables.category(filtered);
                if category == 1 {
                    diacritical = tables.level2(filtered, Extender::None);
                    back = back.saturating_sub(1);
                    continue;
                }
                si = Self::filter_extender(filtered, extender);
                self.one.category = category;
                self.one.level1 = tables.level1(si);
                self.one.level2 = tables.level2(si, extender);
                self.one.level3 = tables.level3(si);
                if extender != Extender::Conditional && diacritical != 0 {
                    self.one.level2 = if self.one.level2 == 0 {
                        diacritical.wrapping_add(2)
                    } else {
                        diacritical
                    };
                }
                break;
            }
            *at = at.saturating_sub(1);
        } else if let Some(replacement) = tables.tail_contraction(text, *at, end) {
            *at = at.saturating_sub(1);
            if !no_lv4 {
                return (false, true);
            }
            let last = len(replacement).saturating_sub(1);
            let found = self.last_index_of_sort_key(
                replacement,
                last,
                last,
                len(replacement),
                ti,
                key,
                no_lv4,
            );
            return (found >= 0, true);
        } else {
            si = self.filter(unit(text, *at));
            *at = at.saturating_sub(1);
            let mut no_match = false;
            self.one.category = tables.category(si);
            if self.one.category == key.category {
                self.one.level1 = tables.level1(si);
            } else {
                no_match = true;
            }
            if self.one.level1 == key.level1 {
                self.one.level2 = tables.level2(si, extender);
            } else {
                no_match = true;
            }
            if no_match {
                return (false, false);
            }
            self.one.level3 = tables.level3(si);
            if self.one.category != 1 {
                self.prev_code = si;
            }
            let mut mark_at = cur.saturating_add(1);
            while mark_at < org_start {
                let mark = unit(text, mark_at);
                if tables.category(mark) != 1 {
                    break;
                }
                self.one.add_diacritic(tables.level2(mark, Extender::None));
                mark_at = mark_at.saturating_add(1);
            }
        }
        (
            matches_primitive(self.one, si, extender, key, ti, no_lv4),
            false,
        )
    }
}

/// What a search looks for first (see [`Collator::head`]).
#[derive(Debug, Clone, Copy)]
enum Head<'table> {
    /// The target has no weighted unit at all.
    Ignorable,
    /// Its first weighted unit is a contraction: search for the
    /// replacement (a `Vec` rather than a slice, to keep this thin).
    Replaced(&'table Vec<u16>),
    /// Its first weighted unit, filtered, and whether that has no fourth
    /// level; the key is in the caller's buffer.
    Unit {
        /// The unit.
        ti: i32,
        /// Whether it has no special (kana) weight.
        no_lv4: bool,
    },
}

/// `MatchesPrimitive`: the source key against the target's, then the kana
/// properties where the target has a fourth level.
fn matches_primitive(
    source: Key,
    si: i32,
    extender: Extender,
    target: Key,
    ti: i32,
    no_lv4: bool,
) -> bool {
    if source != target {
        return false;
    }
    if no_lv4 {
        return si < 0 || !has_special_weight(si);
    }
    if extender == Extender::Conditional {
        return false;
    }
    is_japanese_small_letter(si) == is_japanese_small_letter(ti)
        && dash_type(extender) == dash_type(Extender::None)
        && is_hiragana(si) == is_hiragana(ti)
        && is_half_width_kana(si) == is_half_width_kana(ti)
}

/// `QuickIndexOf`: the search unit by unit, as long as every unit it looks
/// at is ASCII other than U+0000; `None` once one is not, for the collator
/// to answer — and for an empty target or one longer than the range, where
/// Mono returns with its "unable" flag still set.
fn quick_index_of(text: &[u16], target: &[u16], start: i32, length: i32) -> Option<i32> {
    let target_length = len(target);
    if target_length == 0 || target_length > length {
        return None;
    }
    let plain = |unit: i32| unit != 0 && unit < 0x80;
    let mut tested_source = -1_i32;
    let mut tested_target = -1_i32;
    let end = start
        .saturating_add(length)
        .saturating_sub(target_length)
        .saturating_add(1);
    let mut at = start;
    while at < end {
        let mut matched = true;
        let mut offset = 0_i32;
        while offset < target_length {
            if tested_target < offset {
                if !plain(unit(target, offset)) {
                    return None;
                }
                tested_target = offset;
            }
            let here = at.saturating_add(offset);
            if tested_source < here {
                if !plain(unit(text, here)) {
                    return None;
                }
                tested_source = here;
            }
            if unit(text, here) != unit(target, offset) {
                matched = false;
                break;
            }
            offset = offset.saturating_add(1);
        }
        if matched {
            return Some(at);
        }
        at = at.saturating_add(1);
    }
    Some(-1)
}

/// `IndexOfOrdinal(s, target, start, length)`: unit by unit.
fn index_of_ordinal(text: &[u16], target: &[u16], start: i32, length: i32) -> i32 {
    let target_length = len(target);
    if target_length == 0 {
        return 0;
    }
    if target_length > length {
        return -1;
    }
    let end = start
        .saturating_add(length)
        .saturating_sub(target_length)
        .saturating_add(1);
    let mut at = start;
    while at < end {
        if (0..target_length)
            .all(|offset| unit(text, at.saturating_add(offset)) == unit(target, offset))
        {
            return at;
        }
        at = at.saturating_add(1);
    }
    -1
}

/// `LastIndexOfOrdinal(s, target, start, length)`: unit by unit, back from
/// `start`.
fn last_index_of_ordinal(text: &[u16], target: &[u16], start: i32, length: i32) -> i32 {
    let target_length = len(target);
    if target_length == 0 {
        return start;
    }
    if len(text) < target_length || target_length > length {
        return -1;
    }
    let end = start
        .saturating_sub(length)
        .saturating_add(target_length)
        .saturating_sub(1);
    let tail = unit(target, target_length.saturating_sub(1));
    let mut at = start;
    while at > end {
        if unit(text, at) != tail {
            at = at.saturating_sub(1);
            continue;
        }
        let from = at.saturating_sub(target_length).saturating_add(1);
        at = at.saturating_sub(1);
        if (0..target_length.saturating_sub(1))
            .all(|offset| unit(text, from.saturating_add(offset)) == unit(target, offset))
        {
            return from;
        }
    }
    -1
}

#[cfg(test)]
mod tests;
