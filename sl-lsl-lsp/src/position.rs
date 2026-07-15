//! Converting `sl-lsl`'s **byte spans** into LSP **positions**.
//!
//! Every node the parser builds carries a `Range<usize>` of *byte* offsets into
//! the source (see [`sl_lsl::ast`]). The Language Server Protocol speaks in
//! `(line, character)` [`Position`]s instead, and — the subtle part — the unit
//! of `character` is **not bytes**: it depends on the *position encoding* the
//! client and server negotiate at initialisation. The default every client must
//! support is UTF-16 (a character column counts UTF-16 code units, a legacy of
//! the protocol's JavaScript origins); a 3.17 client may also offer UTF-8 (byte
//! columns, the cheap case for us) or UTF-32 (Unicode scalar values).
//!
//! This module owns that conversion: a [`LineIndex`] precomputes each line's
//! start byte offset once, and [`LineIndex::position`] turns any byte offset
//! into the [`Position`] for a given [`PositionEncoding`]. A wrong encoding here
//! is not a crash but a silently misplaced squiggle or rename, so the counting
//! is done explicitly per encoding rather than assuming bytes == columns.

use lsp_types::{Position, PositionEncodingKind, Range};

/// The unit in which an LSP `character` column is counted — the negotiated
/// **position encoding**. UTF-16 is the protocol default every client supports;
/// UTF-8 (byte columns) and UTF-32 (scalar-value columns) are opt-in under LSP
/// 3.17's `general.positionEncodings` capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "`PositionEncoding` names the protocol's own concept (a position encoding) and is \
              the established public type re-exported at the crate root; `position::Encoding` \
              would read worse at the call sites"
)]
pub enum PositionEncoding {
    /// Columns count UTF-8 bytes — the offset the parser already uses, so the
    /// conversion is a subtraction. Offered only when the client advertises it.
    Utf8,
    /// Columns count UTF-16 code units — the protocol's mandatory default, and
    /// the fallback when the client offers nothing else.
    #[default]
    Utf16,
    /// Columns count Unicode scalar values (`char`s).
    Utf32,
}

impl PositionEncoding {
    /// The [`PositionEncodingKind`] to advertise back to the client for this
    /// encoding.
    #[must_use]
    pub const fn to_kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
            Self::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    /// Pick the encoding to use given the encodings a client advertises in its
    /// `general.positionEncodings` capability, preferring **UTF-8** (a byte
    /// column is what the parser already produces, so no re-counting) then
    /// **UTF-32**, and falling back to the mandatory **UTF-16** when the client
    /// offers none of them (or the capability is absent entirely).
    #[must_use]
    pub fn negotiate(client_encodings: Option<&[PositionEncodingKind]>) -> Self {
        let Some(encodings) = client_encodings else {
            return Self::Utf16;
        };
        if encodings.contains(&PositionEncodingKind::UTF8) {
            Self::Utf8
        } else if encodings.contains(&PositionEncodingKind::UTF32) {
            Self::Utf32
        } else {
            Self::Utf16
        }
    }
}

/// A precomputed map from **byte offset** to **line** for one document's text,
/// holding the byte offset at which each line begins (line 0 always starts at
/// byte 0). Built once per document edit; the per-lookup work is a binary search
/// plus a column count over the one line the offset falls on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// The byte offset at which each line starts, in ascending order. Always
    /// begins with `0`; a trailing newline yields one more entry than there are
    /// non-empty lines, which is what an offset at end-of-file needs.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build the index for `text`, recording the start of line 0 and the byte
    /// just past every `\n`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0_usize];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                // The next line starts one byte past the newline. `offset` is a
                // valid index, so `offset + 1` cannot overflow `usize`.
                line_starts.push(offset.saturating_add(1));
            }
        }
        Self { line_starts }
    }

    /// The zero-based line number the byte `offset` falls on: the index of the
    /// last line start that is `<= offset`.
    #[must_use]
    fn line_of(&self, offset: usize) -> usize {
        // `partition_point` gives the count of starts `<= offset`; subtracting
        // one yields that line's index. The vector always holds `0` first, and
        // every offset is `>= 0`, so the count is at least one and the
        // saturating subtraction never underflows.
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// The [`Position`] of byte `offset` within `text` under `encoding`.
    ///
    /// `text` must be the same string the index was built from; the line is
    /// found from the precomputed starts and the column is counted from that
    /// line's start to `offset` in the encoding's unit.
    #[must_use]
    pub fn position(&self, text: &str, offset: usize, encoding: PositionEncoding) -> Position {
        let line = self.line_of(offset);
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        // The slice from the line start to the target offset; `get` yields
        // `None` (an empty column) rather than panicking if either bound is off
        // the end or not a char boundary.
        let prefix = text.get(line_start..offset).unwrap_or("");
        let character = column_of(prefix, encoding);
        Position {
            line: clamp_u32(line),
            character: clamp_u32(character),
        }
    }

    /// The LSP [`Range`] spanning the byte range `span` within `text`.
    #[must_use]
    pub fn range(
        &self,
        text: &str,
        span: core::ops::Range<usize>,
        encoding: PositionEncoding,
    ) -> Range {
        Range {
            start: self.position(text, span.start, encoding),
            end: self.position(text, span.end, encoding),
        }
    }

    /// The **byte offset** within `text` of an LSP [`Position`] under `encoding` —
    /// the inverse of [`position`](Self::position), which every cursor-driven
    /// request (hover, go-to-definition, completion) needs to turn the client's
    /// `(line, character)` back into the byte offset the parse tree is spanned in.
    ///
    /// `text` must be the string the index was built from. A position past the
    /// end of its line clamps to the line's end (excluding the trailing newline),
    /// and a line past the end of the document clamps to `text.len()`, so an
    /// out-of-range cursor never panics — it resolves to the nearest valid byte.
    #[must_use]
    pub fn offset_at(&self, text: &str, position: Position, encoding: PositionEncoding) -> usize {
        let line = usize::try_from(position.line).unwrap_or(usize::MAX);
        let Some(&line_start) = self.line_starts.get(line) else {
            return text.len();
        };
        let line_end = match self.line_starts.get(line.saturating_add(1)) {
            Some(&start) => start,
            None => text.len(),
        };
        let line_text = text.get(line_start..line_end).unwrap_or("");
        let target = usize::try_from(position.character).unwrap_or(usize::MAX);
        let mut column = 0_usize;
        for (byte_offset, ch) in line_text.char_indices() {
            if column >= target {
                return line_start.saturating_add(byte_offset);
            }
            // A newline is the line's terminator, not an addressable column; stop
            // at it so a character past the line's text clamps to the line's end.
            if ch == '\n' {
                return line_start.saturating_add(byte_offset);
            }
            column = column.saturating_add(match encoding {
                PositionEncoding::Utf8 => ch.len_utf8(),
                PositionEncoding::Utf16 => ch.len_utf16(),
                PositionEncoding::Utf32 => 1,
            });
        }
        line_end
    }
}

/// The column of the end of `prefix` (the text from a line's start up to the
/// target offset), counted in `encoding`'s unit.
#[must_use]
fn column_of(prefix: &str, encoding: PositionEncoding) -> usize {
    match encoding {
        PositionEncoding::Utf8 => prefix.len(),
        PositionEncoding::Utf16 => prefix.chars().map(char::len_utf16).sum(),
        PositionEncoding::Utf32 => prefix.chars().count(),
    }
}

/// Narrow a `usize` line/column count to the `u32` the LSP [`Position`] uses,
/// saturating at [`u32::MAX`] rather than wrapping — a document with more than
/// four billion lines is not a real editing session, and a saturated value is a
/// harmless clamp to the document end.
#[must_use]
fn clamp_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{LineIndex, PositionEncoding};
    use lsp_types::{Position, PositionEncodingKind};

    /// An ASCII document counts identical columns in every encoding: byte,
    /// UTF-16 and UTF-32 units all coincide for ASCII.
    #[test]
    fn ascii_positions_agree_across_encodings() {
        let text = "integer x;\ndefault {}\n";
        let index = LineIndex::new(text);
        // The `x` is at byte 8 on line 0.
        let offset = 8;
        for encoding in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            assert_eq!(
                index.position(text, offset, encoding),
                Position {
                    line: 0,
                    character: 8
                }
            );
        }
        // The `default` keyword starts line 1 at column 0.
        assert_eq!(
            index.position(text, 11, PositionEncoding::Utf16),
            Position {
                line: 1,
                character: 0
            }
        );
    }

    /// A non-ASCII character makes the encodings diverge: a `😀` (a single
    /// scalar value, two UTF-16 code units, four UTF-8 bytes) shifts the column
    /// of the text after it differently per encoding.
    #[test]
    fn multibyte_columns_differ_by_encoding() {
        // `"😀"` in an LSL string, then `;` — the `;` sits after the emoji.
        let text = "string s = \"😀\";";
        let index = LineIndex::new(text);
        // Byte offset of the closing `"`: 11 bytes of `string s = "`, plus 4
        // bytes for the emoji, is byte 15; the `;` follows at 16.
        let semicolon = 16;
        assert_eq!(
            index.position(text, semicolon, PositionEncoding::Utf8),
            Position {
                line: 0,
                character: 16
            }
        );
        // UTF-16: the emoji is two units, so four bytes count as two columns —
        // the `;` is two columns earlier than its byte offset.
        assert_eq!(
            index.position(text, semicolon, PositionEncoding::Utf16),
            Position {
                line: 0,
                character: 14
            }
        );
        // UTF-32: the emoji is one scalar value, so the `;` is three columns
        // earlier than its byte offset.
        assert_eq!(
            index.position(text, semicolon, PositionEncoding::Utf32),
            Position {
                line: 0,
                character: 13
            }
        );
    }

    /// An offset at end-of-file (past a trailing newline) resolves to the start
    /// of the final empty line rather than panicking.
    #[test]
    fn end_of_file_offset_is_last_line() {
        let text = "default {}\n";
        let index = LineIndex::new(text);
        assert_eq!(
            index.position(text, text.len(), PositionEncoding::Utf16),
            Position {
                line: 1,
                character: 0
            }
        );
    }

    /// Encoding negotiation prefers UTF-8, then UTF-32, and falls back to UTF-16
    /// when the client advertises neither or nothing at all.
    #[test]
    fn negotiation_prefers_utf8_then_utf32_then_utf16() {
        assert_eq!(
            PositionEncoding::negotiate(Some(&[
                PositionEncodingKind::UTF16,
                PositionEncodingKind::UTF8,
            ])),
            PositionEncoding::Utf8
        );
        assert_eq!(
            PositionEncoding::negotiate(Some(&[
                PositionEncodingKind::UTF16,
                PositionEncodingKind::UTF32,
            ])),
            PositionEncoding::Utf32
        );
        assert_eq!(
            PositionEncoding::negotiate(Some(&[PositionEncodingKind::UTF16])),
            PositionEncoding::Utf16
        );
        assert_eq!(PositionEncoding::negotiate(None), PositionEncoding::Utf16);
    }
}
