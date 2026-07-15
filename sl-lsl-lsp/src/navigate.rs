//! Wiring the [`crate::navigation`] resolution pass to the LSP navigation
//! requests — `textDocument/definition`, `references`, `documentHighlight` and
//! `rename` — for a single [`Document`].
//!
//! Every request here resolves the whole document once and reads the same
//! occurrence list, so the four answers stay consistent: rename edits exactly
//! the spans find-references reports, go-to-definition lands on the declaration
//! highlight marks. This is the LSP-facing half; the scope rules and grouping
//! live in [`crate::navigation`], which knows nothing about [`Document`] or LSP
//! types.

use std::collections::HashMap;

use lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, TextEdit, Uri, WorkspaceEdit,
};

use crate::document::Document;
use crate::navigation::{
    Binding, Occurrence, declaration_of, occurrence_at, references_of, resolve,
};
use crate::position::PositionEncoding;

/// The definition location of the symbol at `position`: the declaration span of
/// a user symbol, or [`None`] for a library symbol (no in-document definition)
/// or a cursor not on a symbol.
#[must_use]
pub fn goto_definition(
    document: &Document,
    position: Position,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Option<Location> {
    let offset = document.offset(position, encoding);
    let occurrences = resolve(&document.parse().script, syntax);
    let target = occurrence_at(&occurrences, offset)?;
    let declaration = declaration_of(&occurrences, target)?;
    Some(Location {
        uri: document.uri().clone(),
        range: document.range(declaration.span.clone(), encoding),
    })
}

/// Every reference to the symbol at `position`, as locations in this document.
/// `include_declaration` follows the LSP request flag: when false, the
/// declaration occurrence of a user symbol is dropped. An empty list when the
/// cursor is not on a resolvable symbol.
#[must_use]
pub fn references(
    document: &Document,
    position: Position,
    include_declaration: bool,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Vec<Location> {
    let offset = document.offset(position, encoding);
    let occurrences = resolve(&document.parse().script, syntax);
    let Some(target) = occurrence_at(&occurrences, offset) else {
        return Vec::new();
    };
    references_of(&occurrences, target)
        .into_iter()
        .filter(|occ| include_declaration || !occ.is_declaration())
        .map(|occ| Location {
            uri: document.uri().clone(),
            range: document.range(occ.span.clone(), encoding),
        })
        .collect()
}

/// The document highlights for the symbol at `position` — the references
/// restricted to this document, each marked read or (for the declaration) text.
/// An empty list when the cursor is not on a resolvable symbol.
#[must_use]
pub fn document_highlights(
    document: &Document,
    position: Position,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Vec<DocumentHighlight> {
    let offset = document.offset(position, encoding);
    let occurrences = resolve(&document.parse().script, syntax);
    let Some(target) = occurrence_at(&occurrences, offset) else {
        return Vec::new();
    };
    references_of(&occurrences, target)
        .into_iter()
        .map(|occ| DocumentHighlight {
            range: document.range(occ.span.clone(), encoding),
            kind: Some(if occ.is_declaration() {
                DocumentHighlightKind::TEXT
            } else {
                DocumentHighlightKind::READ
            }),
        })
        .collect()
}

/// Why a rename was refused, surfaced to the client as the request's error
/// message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    /// The cursor is not on a renameable symbol (whitespace, a keyword, a
    /// literal, or an unresolved name).
    NotASymbol,
    /// The symbol is a grid library symbol the editor cannot rewrite (an `ll*`
    /// call, a constant, an event name).
    LibrarySymbol,
    /// The requested new name is not a valid LSL identifier.
    InvalidName {
        /// The rejected name.
        name: String,
    },
}

impl core::fmt::Display for RenameError {
    /// Render the refusal as the human-readable message the client shows.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotASymbol => write!(f, "no renameable symbol at the cursor"),
            Self::LibrarySymbol => write!(f, "cannot rename a grid library symbol"),
            Self::InvalidName { name } => {
                write!(f, "`{name}` is not a valid LSL identifier")
            }
        }
    }
}

impl core::error::Error for RenameError {}

/// A [`WorkspaceEdit`] renaming the symbol at `position` to `new_name`, or a
/// [`RenameError`] if the cursor is not on a user symbol or `new_name` is not a
/// valid identifier. The edit rewrites every occurrence — declaration and uses —
/// so the rename is complete and consistent with find-references.
///
/// # Errors
///
/// Returns [`RenameError`] when there is no renameable symbol at the cursor, the
/// symbol is a library symbol, or `new_name` is not a valid LSL identifier.
#[expect(
    clippy::mutable_key_type,
    reason = "the `WorkspaceEdit::changes` map is keyed by `lsp_types::Uri`, whose internal \
              buffer clippy flags as interior-mutable; the key is never mutated after insertion \
              and this is the protocol-mandated map type"
)]
pub fn rename(
    document: &Document,
    position: Position,
    new_name: &str,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Result<WorkspaceEdit, RenameError> {
    if !is_valid_identifier(new_name) {
        return Err(RenameError::InvalidName {
            name: new_name.to_owned(),
        });
    }
    let offset = document.offset(position, encoding);
    let occurrences = resolve(&document.parse().script, syntax);
    let target = occurrence_at(&occurrences, offset).ok_or(RenameError::NotASymbol)?;
    if matches!(target.binding, Binding::Library) {
        return Err(RenameError::LibrarySymbol);
    }
    let edits: Vec<TextEdit> = references_of(&occurrences, target)
        .into_iter()
        .map(|occ: &Occurrence| TextEdit {
            range: document.range(occ.span.clone(), encoding),
            new_text: new_name.to_owned(),
        })
        .collect();
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    let _absent = changes.insert(document.uri().clone(), edits);
    Ok(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

/// Whether `name` is a syntactically valid LSL identifier: a non-empty ASCII
/// letter-or-underscore start followed by letters, digits or underscores. LSL
/// forbids anything else in an identifier; this rejects an empty or symbol-laden
/// rename target before it can corrupt the buffer.
#[must_use]
fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::{RenameError, document_highlights, goto_definition, references, rename};
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{Position, Uri};
    use sl_lsl::{LslFunction, LslSyntax};

    /// A library with `llSay`.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        let _prev = syntax
            .functions
            .insert("llSay".to_owned(), LslFunction::default());
        syntax
    }

    /// Open `source`, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// The `(line, column)` position of the nth occurrence of `needle`.
    fn position_of(source: &str, needle: &str, nth: usize) -> Result<Position, String> {
        let mut search_from = 0_usize;
        let mut byte = None;
        for _ in 0..=nth {
            let found = source
                .get(search_from..)
                .and_then(|rest| rest.find(needle))
                .ok_or("needle not found")?;
            let absolute = search_from.saturating_add(found);
            byte = Some(absolute);
            search_from = absolute.saturating_add(needle.len());
        }
        let byte = byte.ok_or("needle not found")?;
        let before = source.get(..byte).ok_or("bad slice")?;
        let line = u32::try_from(before.matches('\n').count()).map_err(|err| err.to_string())?;
        let line_start = before
            .rfind('\n')
            .map_or(0, |index| index.saturating_add(1));
        let column =
            u32::try_from(byte.saturating_sub(line_start)).map_err(|err| err.to_string())?;
        Ok(Position {
            line,
            character: column,
        })
    }

    /// Go-to-definition on a use of a global jumps to its declaration.
    #[test]
    fn definition_jumps_to_declaration() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() { counter = 1; } }\n";
        let doc = open(source)?;
        // The `counter` use (second occurrence).
        let use_pos = position_of(source, "counter", 1)?;
        let location = goto_definition(&doc, use_pos, &library(), PositionEncoding::Utf16)
            .ok_or("no definition")?;
        // The declaration is on line 0.
        assert_eq!(location.range.start.line, 0);
        Ok(())
    }

    /// References with and without the declaration differ by one.
    #[test]
    fn references_honour_include_declaration() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() { counter = counter + 1; } }\n";
        let doc = open(source)?;
        let use_pos = position_of(source, "counter", 1)?;
        let with = references(&doc, use_pos, true, &library(), PositionEncoding::Utf16);
        let without = references(&doc, use_pos, false, &library(), PositionEncoding::Utf16);
        // Declaration + two uses = 3; without the declaration = 2.
        assert_eq!(with.len(), 3);
        assert_eq!(without.len(), 2);
        Ok(())
    }

    /// Renaming a user symbol rewrites every occurrence.
    #[test]
    #[expect(
        clippy::mutable_key_type,
        reason = "reading the `WorkspaceEdit::changes` map keyed by `lsp_types::Uri`, which \
                  clippy flags as an interior-mutable key though it is never mutated"
    )]
    fn rename_rewrites_all_occurrences() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() { counter = counter + 1; } }\n";
        let doc = open(source)?;
        let decl_pos = position_of(source, "counter", 0)?;
        let edit = rename(&doc, decl_pos, "total", &library(), PositionEncoding::Utf16)
            .map_err(|err| err.to_string())?;
        let changes = edit.changes.ok_or("no changes")?;
        let edits = changes.values().next().ok_or("no edits")?;
        // Declaration plus two uses.
        assert_eq!(edits.len(), 3);
        assert!(edits.iter().all(|e| e.new_text == "total"));
        Ok(())
    }

    /// Renaming a library symbol is refused.
    #[test]
    fn rename_library_refused() -> Result<(), String> {
        let source = "default { state_entry() { llSay(0, \"\"); } }\n";
        let doc = open(source)?;
        let say_pos = position_of(source, "llSay", 0)?;
        let outcome = rename(&doc, say_pos, "myFn", &library(), PositionEncoding::Utf16);
        assert_eq!(outcome.err(), Some(RenameError::LibrarySymbol));
        Ok(())
    }

    /// Renaming to an invalid identifier is refused.
    #[test]
    fn rename_invalid_name_refused() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() {} }\n";
        let doc = open(source)?;
        let decl_pos = position_of(source, "counter", 0)?;
        let outcome = rename(&doc, decl_pos, "1bad", &library(), PositionEncoding::Utf16);
        assert_eq!(
            outcome.err(),
            Some(RenameError::InvalidName {
                name: "1bad".to_owned()
            })
        );
        Ok(())
    }

    /// Document highlight marks every occurrence in the document.
    #[test]
    fn highlight_marks_occurrences() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() { counter = counter + 1; } }\n";
        let doc = open(source)?;
        let use_pos = position_of(source, "counter", 1)?;
        let highlights = document_highlights(&doc, use_pos, &library(), PositionEncoding::Utf16);
        assert_eq!(highlights.len(), 3);
        Ok(())
    }
}
