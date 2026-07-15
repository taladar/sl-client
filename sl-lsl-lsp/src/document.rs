//! The in-memory **document store** — one open text buffer, its precomputed
//! line index and its parse tree, kept in sync with the editor.
//!
//! LSP is a *stateful* protocol: the client streams the buffer's lifecycle
//! (`textDocument/didOpen` → `didChange`* → `didClose`) and every later request
//! (`documentSymbol`, and — in the `viewer-lsl-lsp-diagnostics-nav` task — hover,
//! definition, completion) is answered against the server's own copy, never by
//! re-reading a file. This module is that copy: a [`Document`] owns the current
//! text, so a request handler holds a consistent snapshot even as edits arrive.
//!
//! Each edit **re-parses** eagerly. The `sl-lsl` parser is error-tolerant and
//! cheap (a single scan with no grid round-trip), so re-parsing the whole buffer
//! on every keystroke is simpler and more robust than incremental reparse, and
//! the resulting [`Parse`] — tree plus recovered errors — is exactly what the
//! symbol and (later) diagnostic handlers read.
//!
//! Synchronisation is **full-text**: the server advertises
//! [`TextDocumentSyncKind::FULL`](lsp_types::TextDocumentSyncKind::FULL), so a
//! `didChange` carries the entire new buffer and [`Document::update`] simply
//! swaps it in. Incremental sync (byte-ranged edits) is a later optimisation the
//! parser's speed does not yet justify.

use lsp_types::Uri;
use sl_lsl::{Parse, parse};

use crate::position::{LineIndex, PositionEncoding};

/// One open document: its URI, the editor's version counter, the current text,
/// the [`LineIndex`] over that text and the [`Parse`] of it.
///
/// The three derived fields ([`line_index`](Self::line_index),
/// [`parse`](Self::parse)) are always kept consistent with
/// [`text`](Self::text): they are recomputed together whenever the text changes,
/// so a handler never sees a line index or tree that disagrees with the buffer.
#[derive(Debug, Clone)]
pub struct Document {
    /// The document's URI, as the client sent it.
    uri: Uri,
    /// The editor's monotonically increasing version for this document. Carried
    /// so a future diagnostics push can tag its report with the version it was
    /// computed against (the LSP guard against a stale squiggle).
    version: i32,
    /// The current full text of the buffer.
    text: String,
    /// The line index over [`text`](Self::text), for byte-span → LSP-position
    /// conversion.
    line_index: LineIndex,
    /// The error-tolerant parse of [`text`](Self::text): the syntax tree plus any
    /// recovered parse errors.
    parse: Parse,
}

impl Document {
    /// Open a document from a `didOpen`: store its text and compute the line
    /// index and parse tree.
    #[must_use]
    pub fn open(uri: Uri, version: i32, text: String) -> Self {
        let line_index = LineIndex::new(&text);
        let parse = parse(&text);
        Self {
            uri,
            version,
            text,
            line_index,
            parse,
        }
    }

    /// Replace the whole buffer from a full-text `didChange`, recomputing the
    /// line index and parse tree to match.
    pub fn update(&mut self, version: i32, text: String) {
        self.version = version;
        self.line_index = LineIndex::new(&text);
        self.parse = parse(&text);
        self.text = text;
    }

    /// The document's URI.
    #[must_use]
    pub const fn uri(&self) -> &Uri {
        &self.uri
    }

    /// The editor's current version for this document.
    #[must_use]
    pub const fn version(&self) -> i32 {
        self.version
    }

    /// The current full text of the buffer.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The error-tolerant parse of the current text.
    #[must_use]
    pub const fn parse(&self) -> &Parse {
        &self.parse
    }

    /// The line index over the current text.
    #[must_use]
    pub const fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    /// The LSP range for a byte `span` in this document under `encoding` — the
    /// convenience that pairs [`text`](Self::text) with
    /// [`line_index`](Self::line_index) so a caller cannot mismatch them.
    #[must_use]
    pub fn range(
        &self,
        span: core::ops::Range<usize>,
        encoding: PositionEncoding,
    ) -> lsp_types::Range {
        self.line_index.range(&self.text, span, encoding)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use core::str::FromStr as _;

    use super::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{Position, Uri};

    /// Build a `file:///test.lsl` URI for the tests, surfacing a parse failure
    /// as an `Err` rather than a panic (the workspace denies `unwrap`).
    fn test_uri() -> Result<Uri, String> {
        Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())
    }

    /// Opening a document parses it and lets a caller map a node's byte span to
    /// an LSP range.
    #[test]
    fn open_parses_and_maps_spans() -> Result<(), String> {
        let doc = Document::open(test_uri()?, 1, "integer x;\n".to_owned());
        assert_eq!(doc.version(), 1);
        assert_eq!(doc.parse().errors, vec![]);
        // The whole `integer x` global spans bytes 0..9.
        let range = doc.range(0..9, PositionEncoding::Utf16);
        assert_eq!(
            range.start,
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            range.end,
            Position {
                line: 0,
                character: 9
            }
        );
        Ok(())
    }

    /// An update swaps the text, bumps the version and re-parses.
    #[test]
    fn update_reparses() -> Result<(), String> {
        let mut doc = Document::open(test_uri()?, 1, "integer x;\n".to_owned());
        doc.update(2, "float y;\nfloat z;\n".to_owned());
        assert_eq!(doc.version(), 2);
        assert_eq!(doc.text(), "float y;\nfloat z;\n");
        assert_eq!(doc.parse().script.globals.len(), 2);
        Ok(())
    }
}
