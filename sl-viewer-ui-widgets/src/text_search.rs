//! The **buffer arithmetic** an editor needs and a text field does not: finding
//! a string forwards and backwards, replacing every match, and turning a line
//! and column into a byte offset and back.
//!
//! Pure functions over a `&str`, so the find/replace floater, go-to-line and a
//! compile error's jump are all tested here without a frame being run. Every
//! offset is a byte offset on a character boundary, which is what
//! `parley::PlainEditor` selects by.
//!
//! # Case
//!
//! A case-insensitive match compares the two strings' **lower-case mappings**
//! character by character, and never ends half-way through a character whose
//! lower case is longer than one code point: `İ` lower-cases to `i` plus a
//! combining dot, so a needle of `i` alone does not match it, because a match
//! must end on a character boundary of the haystack. Lower-casing both sides is
//! the reference's approach too (`LLTextEditor::selectNext`).
//!
//! Reference (Firestorm, read-only): `llfloatersearchreplace`,
//! `lltexteditor` (`selectNext`, `replaceTextAll`).

use core::ops::Range;

/// What to look for, and how strictly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchQuery<'a> {
    /// The text to find. An empty needle finds nothing.
    pub needle: &'a str,
    /// Whether `a` and `A` are different characters.
    pub case_sensitive: bool,
}

impl<'a> SearchQuery<'a> {
    /// A query for `needle`, case-sensitive or not.
    #[must_use]
    pub const fn new(needle: &'a str, case_sensitive: bool) -> Self {
        Self {
            needle,
            case_sensitive,
        }
    }

    /// Where a match of this query that starts at `start` ends, or `None` when
    /// none starts there. `start` must be a character boundary of `haystack`.
    fn match_at(&self, haystack: &str, start: usize) -> Option<usize> {
        let rest = haystack.get(start..)?;
        if self.case_sensitive {
            return rest
                .starts_with(self.needle)
                .then(|| start.saturating_add(self.needle.len()));
        }
        let mut needle = self.needle.chars().flat_map(char::to_lowercase).peekable();
        for (offset, character) in rest.char_indices() {
            for lower in character.to_lowercase() {
                match needle.next() {
                    Some(wanted) if wanted == lower => {}
                    // A mismatch, or a needle that ran out part-way through this
                    // character's lower case — a match must end on a boundary.
                    Some(_) | None => return None,
                }
            }
            if needle.peek().is_none() {
                return Some(
                    start
                        .saturating_add(offset)
                        .saturating_add(character.len_utf8()),
                );
            }
        }
        None
    }
}

/// Every match of `query` in `haystack`, in order and not overlapping — what
/// Replace All replaces and what the editor washes while the find window is up.
#[must_use]
pub fn find_all(haystack: &str, query: SearchQuery<'_>) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if query.needle.is_empty() {
        return found;
    }
    let mut from = 0;
    while let Some(range) = first_match_from(haystack, query, from) {
        from = range.end;
        found.push(range);
    }
    found
}

/// The first match of `query` that starts at or after byte `from`.
fn first_match_from(haystack: &str, query: SearchQuery<'_>, from: usize) -> Option<Range<usize>> {
    let tail = haystack.get(from..)?;
    tail.char_indices()
        .map(|(offset, _character)| from.saturating_add(offset))
        .find_map(|start| query.match_at(haystack, start).map(|end| start..end))
}

/// The last match of `query` that ends at or before byte `before`.
fn last_match_before(
    haystack: &str,
    query: SearchQuery<'_>,
    before: usize,
) -> Option<Range<usize>> {
    let head = haystack.get(..before)?;
    head.char_indices().rev().find_map(|(start, _character)| {
        query
            .match_at(haystack, start)
            .filter(|end| *end <= before)
            .map(|end| start..end)
    })
}

/// The next match of `query` at or after byte `from`, **wrapping** to the start
/// of the buffer when there is none below it — Find Next, which the reference
/// wraps (`selectNext`'s `wrap`).
#[must_use]
pub fn find_next(haystack: &str, query: SearchQuery<'_>, from: usize) -> Option<Range<usize>> {
    if query.needle.is_empty() {
        return None;
    }
    let from = floor_char_boundary(haystack, from);
    first_match_from(haystack, query, from).or_else(|| first_match_from(haystack, query, 0))
}

/// The previous match of `query` ending at or before byte `before`, wrapping to
/// the end of the buffer when there is none above it — Find Previous.
#[must_use]
pub fn find_previous(
    haystack: &str,
    query: SearchQuery<'_>,
    before: usize,
) -> Option<Range<usize>> {
    if query.needle.is_empty() {
        return None;
    }
    let before = floor_char_boundary(haystack, before);
    last_match_before(haystack, query, before)
        .or_else(|| last_match_before(haystack, query, haystack.len()))
}

/// `haystack` with every match of `query` replaced by `replacement`, and how
/// many there were. Matches are found in the original text, so a replacement
/// that contains the needle is not replaced again.
#[must_use]
pub fn replace_all(haystack: &str, query: SearchQuery<'_>, replacement: &str) -> (String, usize) {
    let matches = find_all(haystack, query);
    let mut out = String::with_capacity(haystack.len());
    let mut copied = 0;
    for found in &matches {
        out.push_str(haystack.get(copied..found.start).unwrap_or_default());
        out.push_str(replacement);
        copied = found.end;
    }
    out.push_str(haystack.get(copied..).unwrap_or_default());
    (out, matches.len())
}

/// The largest character boundary of `text` at or below `index`, clamped to its
/// length — so an offset that went stale under an edit still names a place.
#[must_use]
pub fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

/// The byte offset where 1-based line `line` starts, or `None` past the last
/// line. A buffer always has line 1, empty or not; a trailing newline opens one
/// more (empty) line, as it does in every editor's gutter.
#[must_use]
pub fn line_start(text: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    text.match_indices('\n')
        .nth(line.saturating_sub(2))
        .map(|(newline, _)| newline.saturating_add(1))
}

/// The byte range of 1-based line `line`, without its newline.
#[must_use]
pub fn line_range(text: &str, line: usize) -> Option<Range<usize>> {
    let start = line_start(text, line)?;
    let end = text
        .get(start..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |length| start.saturating_add(length));
    Some(start..end)
}

/// How many lines `text` has — one more than its newlines.
#[must_use]
pub fn line_count(text: &str) -> usize {
    text.bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        .saturating_add(1)
}

/// The byte offset of 1-based `line` and 1-based `column`, the column counted
/// in **characters** — what a compile error names and what go-to-line types.
///
/// A line past the end lands on the last line, and a column past the line's end
/// on its end, the way the reference's go-to-line clamps rather than refusing:
/// a stale error row still takes the caret somewhere sensible.
#[must_use]
pub fn line_column_offset(text: &str, line: usize, column: usize) -> usize {
    let last = line_count(text);
    let range = line_range(text, line.clamp(1, last)).unwrap_or(0..0);
    let line_text = text.get(range.clone()).unwrap_or_default();
    let skip = column.saturating_sub(1);
    line_text
        .char_indices()
        .nth(skip)
        .map_or(range.end, |(offset, _character)| {
            range.start.saturating_add(offset)
        })
}

/// The 1-based line that byte `index` falls on.
#[must_use]
pub fn line_of(text: &str, index: usize) -> usize {
    let index = floor_char_boundary(text, index);
    text.get(..index).map_or(1, line_count)
}

#[cfg(test)]
mod tests {
    use super::{
        SearchQuery, find_all, find_next, find_previous, line_column_offset, line_count, line_of,
        line_range, line_start, replace_all,
    };
    use pretty_assertions::assert_eq;

    /// The script the tests search: three lines, with `llSay` twice and once in
    /// another case.
    const SCRIPT: &str = "llSay(0, \"a\");\nLLSAY(1, \"b\");\nllSay(2, \"c\");";

    /// A case-sensitive find skips the upper-case spelling; an insensitive one
    /// does not.
    #[test]
    fn case_decides_what_matches() {
        assert_eq!(
            find_all(SCRIPT, SearchQuery::new("llSay", true)),
            vec![0..5, 30..35]
        );
        assert_eq!(
            find_all(SCRIPT, SearchQuery::new("llsay", false)),
            vec![0..5, 15..20, 30..35]
        );
    }

    /// Find Next starts at the caret and wraps past the end; Find Previous ends
    /// before it and wraps past the start.
    #[test]
    fn next_and_previous_wrap() {
        let query = SearchQuery::new("llSay", true);
        assert_eq!(find_next(SCRIPT, query, 1), Some(30..35));
        assert_eq!(find_next(SCRIPT, query, 31), Some(0..5));
        assert_eq!(find_previous(SCRIPT, query, 30), Some(0..5));
        assert_eq!(find_previous(SCRIPT, query, 4), Some(30..35));
        assert_eq!(find_next(SCRIPT, SearchQuery::new("", true), 0), None);
        assert_eq!(find_next(SCRIPT, SearchQuery::new("absent", true), 0), None);
    }

    /// An insensitive match never ends inside a character whose lower case is
    /// longer than one code point, and does match across a multi-byte one.
    #[test]
    fn an_insensitive_match_ends_on_a_boundary() {
        // `İ` lower-cases to `i` plus a combining dot: `i` alone is not it.
        assert_eq!(
            find_all("İx", SearchQuery::new("i", false)),
            Vec::<core::ops::Range<usize>>::new()
        );
        assert_eq!(
            find_all("İx", SearchQuery::new("i\u{307}x", false)),
            vec![0..3]
        );
        assert_eq!(
            find_all("Grüße, GRÜSSE", SearchQuery::new("grü", false)),
            vec![0..4, 9..13]
        );
    }

    /// Replace All replaces what was there, not what it wrote.
    #[test]
    fn replace_all_does_not_rescan_its_own_output() {
        let (text, count) = replace_all("aaa", SearchQuery::new("a", true), "aa");
        assert_eq!((text.as_str(), count), ("aaaaaa", 3));
        let (text, count) = replace_all(SCRIPT, SearchQuery::new("llsay", false), "llOwnerSay");
        assert_eq!(count, 3);
        assert_eq!(
            text,
            "llOwnerSay(0, \"a\");\nllOwnerSay(1, \"b\");\nllOwnerSay(2, \"c\");"
        );
    }

    /// Lines are 1-based, a trailing newline opens a last empty line, and a
    /// column counts characters, clamping at the line's end.
    #[test]
    fn lines_and_columns() {
        let text = "ab\nüc\n";
        assert_eq!(line_count(text), 3);
        assert_eq!(line_start(text, 1), Some(0));
        assert_eq!(line_start(text, 2), Some(3));
        assert_eq!(line_start(text, 3), Some(7));
        assert_eq!(line_start(text, 4), None);
        assert_eq!(line_range(text, 2), Some(3..6));
        assert_eq!(line_column_offset(text, 2, 2), 5);
        assert_eq!(line_column_offset(text, 2, 99), 6);
        assert_eq!(line_column_offset(text, 99, 1), 7);
        assert_eq!(line_of(text, 0), 1);
        assert_eq!(line_of(text, 5), 2);
        assert_eq!(line_of(text, 7), 3);
    }
}
