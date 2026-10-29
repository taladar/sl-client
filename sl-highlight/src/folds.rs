//! The ranges an editor can **fold** away.
//!
//! A fold hides the lines between a header and its closing line and keeps both
//! of those in view, the way every code editor folds a block:
//!
//! ```text
//! touch_start(integer n) {⋯        ← the header line, and a marker
//! }                                ← the closing line, still there
//! ```
//!
//! What it hides runs from just after the opener to the **start** of the
//! closing line — so the line breaks inside go too and the lines collapse, but
//! the closer keeps its indentation. A closer that shares its line with code
//! (`x; }`) hides up to the closer itself instead.

use core::ops::Range;

/// One foldable range.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fold {
    /// Where the opener starts — the fold's identity, and the line its marker
    /// sits on.
    pub header: usize,
    /// The bytes hidden while it is folded.
    pub hidden: Range<usize>,
}

impl Fold {
    /// The fold of a block whose opener starts at `header` and ends at
    /// `open_end` and whose closer starts at `close_start`, in `text` — or
    /// `None` when the block sits on one line, which has nothing to fold.
    #[must_use]
    pub fn between(text: &str, header: usize, open_end: usize, close_start: usize) -> Option<Self> {
        let inside = text.get(open_end..close_start)?;
        let last_break = inside.rfind('\n')?;
        let closing_line = open_end.saturating_add(last_break).saturating_add(1);
        // A closer alone on its line (after its indentation) keeps the line;
        // one after code on the same line hides that code with the rest.
        let alone = text
            .get(closing_line..close_start)
            .is_some_and(|indent| indent.chars().all(|c| c == ' ' || c == '\t'));
        let end = if alone { closing_line } else { close_start };
        Some(Self {
            header,
            hidden: open_end..end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Fold;
    use pretty_assertions::assert_eq;

    /// The text `fold` hides, if there is one.
    fn hidden(text: &str, fold: Option<Fold>) -> Option<&str> {
        fold.and_then(|fold| text.get(fold.hidden))
    }

    /// A block folds from after its opener to the start of its closing line.
    #[test]
    fn a_block_keeps_its_closing_line() {
        let text = "a {\n  b;\n  c;\n}\n";
        assert_eq!(
            Fold::between(text, 2, 3, 14),
            Some(Fold {
                header: 2,
                hidden: 3..14
            })
        );
        assert_eq!(
            hidden(text, Fold::between(text, 2, 3, 14)),
            Some("\n  b;\n  c;\n")
        );
        let indented = "  x {\n    y;\n  }";
        assert_eq!(
            hidden(indented, Fold::between(indented, 4, 5, 15)),
            Some("\n    y;\n")
        );
    }

    /// A closer after code hides the code too; a one-line block has no fold.
    #[test]
    fn a_closer_after_code_and_a_one_line_block() {
        let text = "a {\n  b; }";
        assert_eq!(hidden(text, Fold::between(text, 2, 3, 9)), Some("\n  b; "));
        assert_eq!(Fold::between("a { b; }", 2, 3, 7), None);
    }
}
