//! Text as each grid counts it.
//!
//! Second Life counts a string in **code points**; OpenSim, whose strings are
//! .NET's, in **UTF-16 units** — so a character outside the Basic
//! Multilingual Plane (an emoji, `𝄞`) is one character there and two here,
//! and OpenSim can cut a surrogate pair in half, which then reads back as
//! U+FFFD (book chapter `gridspec/lsl.md`, § Strings, `strings/unicode`).
//! Every function that indexes, counts or cuts text goes through [`Text`].

use crate::vm::EngineFlavour;

/// A string as a sequence of its grid's units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Text {
    /// The units: code points, or UTF-16 code units.
    units: Vec<u32>,
    /// Whether the units are UTF-16 code units (OpenSim) rather than code
    /// points (Second Life).
    utf16: bool,
}

impl Text {
    /// `text` in the units `flavour` counts.
    pub(crate) fn new(flavour: EngineFlavour, text: &str) -> Self {
        let utf16 = flavour.is_opensim();
        let units = if utf16 {
            text.encode_utf16().map(u32::from).collect()
        } else {
            text.chars().map(u32::from).collect()
        };
        Self { units, utf16 }
    }

    /// The units.
    pub(crate) fn units(&self) -> &[u32] {
        &self.units
    }

    /// How many units there are.
    pub(crate) const fn len(&self) -> usize {
        self.units.len()
    }

    /// The length as LSL's integer.
    pub(crate) fn length(&self) -> i32 {
        i32::try_from(self.units.len()).unwrap_or(i32::MAX)
    }

    /// `units` of this kind as a string: a code point that is no character,
    /// and on OpenSim a surrogate without its other half, as U+FFFD.
    pub(crate) fn string(&self, units: &[u32]) -> String {
        decode(units, self.utf16)
    }

    /// The first index at or after `from` where `pattern` starts.
    pub(crate) fn find(&self, pattern: &[u32], from: usize) -> Option<usize> {
        if pattern.is_empty() {
            return (from <= self.units.len()).then_some(from);
        }
        self.units
            .get(from..)?
            .windows(pattern.len())
            .position(|window| window == pattern)
            .map(|at| at.saturating_add(from))
    }

    /// [`Self::find`] as the grid's string search finds: on Second Life the
    /// same, on OpenSim .NET's culture-sensitive `String.IndexOf`, which
    /// `llSubStringIndex` and the string parsers call.
    ///
    /// That search **ignores** the characters its collation gives no weight —
    /// among them the noncharacters U+FDD0 … U+FDEF, which hold the `JSON_*`
    /// sentinels — in the pattern and in the text alike. So a pattern of
    /// nothing else is found at `from` whenever `from` is inside the text
    /// (`llSubStringIndex("abc", JSON_INVALID)` is 0, and
    /// `llParseStringKeepNulls("abc", [JSON_INVALID], [])` splits before
    /// every character, dropping it: `data/json`'s `sentinel-search` records),
    /// and never at its end. Which other characters .NET ignores is Mono's
    /// collation table and not modelled; the block is the one measured.
    pub(crate) fn find_as_grid(&self, pattern: &[u32], from: usize) -> Option<usize> {
        if !self.utf16 {
            return self.find(pattern, from);
        }
        let wanted: Vec<u32> = pattern
            .iter()
            .copied()
            .filter(|unit| !is_ignorable(*unit))
            .collect();
        if wanted.is_empty() {
            if pattern.is_empty() {
                return self.find(pattern, from);
            }
            return (from < self.units.len()).then_some(from);
        }
        (from..self.units.len()).find(|start| {
            let mut rest = self
                .units
                .get(*start..)
                .unwrap_or_default()
                .iter()
                .filter(|unit| !is_ignorable(**unit));
            self.units
                .get(*start)
                .is_some_and(|first| !is_ignorable(*first))
                && wanted.iter().all(|unit| rest.next() == Some(unit))
        })
    }

    /// The last index before `before` where `pattern` starts and ends at or
    /// before `before`.
    pub(crate) fn rfind(&self, pattern: &[u32], before: usize) -> Option<usize> {
        self.units
            .get(..before.min(self.units.len()))?
            .windows(pattern.len().max(1))
            .rposition(|window| window == pattern)
    }
}

/// Whether .NET's culture-sensitive comparison ignores the UTF-16 unit
/// `unit`: the noncharacters U+FDD0 … U+FDEF (see [`Text::find_as_grid`]).
const fn is_ignorable(unit: u32) -> bool {
    matches!(unit, 0xFDD0..=0xFDEF)
}

/// Units as a string; see [`Text::string`].
fn decode(units: &[u32], utf16: bool) -> String {
    if utf16 {
        let halves: Vec<u16> = units
            .iter()
            .map(|unit| u16::try_from(*unit).unwrap_or(0xFFFD))
            .collect();
        char::decode_utf16(halves)
            .map(|decoded| decoded.unwrap_or('\u{FFFD}'))
            .collect()
    } else {
        units
            .iter()
            .map(|unit| char::from_u32(*unit).unwrap_or('\u{FFFD}'))
            .collect()
    }
}

/// `text` cut at its first NUL (PyOptimizer's `zstr`): the reference's
/// strings end there.
pub(crate) use crate::cast::until_nul;

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn second_life_counts_code_points_and_opensim_utf16_units() {
        let sl = Text::new(EngineFlavour::SecondLife, "a𝄞b😀");
        let os = Text::new(EngineFlavour::XEngine, "a𝄞b😀");
        assert_eq!((sl.len(), os.len()), (4, 6));
        assert_eq!(sl.string(sl.units().get(1..2).unwrap_or_default()), "𝄞");
        // Half a surrogate pair reads back as U+FFFD.
        assert_eq!(
            os.string(os.units().get(1..2).unwrap_or_default()),
            "\u{FFFD}"
        );
        assert_eq!(os.string(os.units().get(1..3).unwrap_or_default()), "𝄞");
    }

    #[test]
    fn finds_forwards_and_backwards() {
        let text = Text::new(EngineFlavour::SecondLife, "aaa-b-aaa");
        let a = [u32::from('a')];
        assert_eq!(text.find(&a, 0), Some(0));
        assert_eq!(text.find(&a, 3), Some(6));
        assert_eq!(text.find(&[], 9), Some(9));
        assert_eq!(text.find(&[], 10), None);
        assert_eq!(text.rfind(&a, 9), Some(8));
        assert_eq!(text.rfind(&a, 6), Some(2));
        assert_eq!(text.rfind(&a, 0), None);
    }

    /// OpenSim's search skips the sentinels in pattern and text; Second
    /// Life's does not (`data/json`'s `sentinel-search`).
    #[test]
    fn opensims_search_ignores_the_sentinels() {
        let invalid = [0xFDD0_u32];
        let sl = Text::new(EngineFlavour::SecondLife, "abc");
        let os = Text::new(EngineFlavour::YEngine, "abc");
        assert_eq!(sl.find_as_grid(&invalid, 0), None);
        assert_eq!(os.find_as_grid(&invalid, 0), Some(0));
        assert_eq!(os.find_as_grid(&invalid, 2), Some(2));
        assert_eq!(os.find_as_grid(&invalid, 3), None);
        let marked = Text::new(EngineFlavour::YEngine, "x\u{FDD0}bc");
        let b = [u32::from('b')];
        assert_eq!(marked.find_as_grid(&b, 0), marked.find(&b, 0));
        let spaced = Text::new(EngineFlavour::YEngine, "a\u{FDD0}b");
        let ab = [u32::from('a'), 0xFDD1, u32::from('b')];
        assert_eq!(spaced.find_as_grid(&ab, 0), Some(0));
        let after = Text::new(EngineFlavour::SecondLife, "a\u{FDD0}");
        assert_eq!(after.find_as_grid(&invalid, 0), Some(1));
    }
}
