//! The **language-neutral** structure of a source file, as a script editor
//! needs it: which ranges are what kind of token, which brackets pair, which
//! ranges fold, and what the file's named parts are.
//!
//! A language's highlighter (`sl-lsl` for LSL) produces a [`CodeStructure`];
//! the viewer's code editor draws one. Neither knows about the other, so a
//! second language — Second Life is adding Lua — needs a second producer and
//! nothing else.
//!
//! - [`capture`] — the [`Capture`] vocabulary: **tree-sitter's capture names**
//!   (`keyword`, `function.builtin`, `string.escape`, …), the nearest thing
//!   editors share to a standard, and hierarchical, so a scheme that colours
//!   only `function` still colours `function.builtin`.
//! - [`brackets`] — [`BracketPairs`]: every bracket with its partner, and the
//!   pair at a caret.
//! - [`folds`] — [`Fold`]: a range an editor can collapse, and the rule for
//!   cutting one out of a bracketed block so its closing line stays visible.
//! - [`OutlineItem`] — the named parts of a file, nested.
//! - [`help`] — what an editor offers while the file is typed: completions,
//!   a call's [`Signature`], a symbol's [`Hover`], through a language's
//!   [`CodeAssist`].
//!
//! Every offset is a **byte offset** into the source the structure was made
//! from, on a character boundary — what an editor's buffer is indexed by.

pub mod brackets;
pub mod capture;
pub mod folds;
pub mod help;

use core::ops::Range;

pub use brackets::{Bracket, BracketKind, BracketPairs};
pub use capture::Capture;
pub use folds::Fold;
pub use help::{
    CodeAssist, Completion, Completions, Hover, MatchTier, Note, Parameter, Signature, match_tier,
    rank,
};

/// One highlighted range of the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Highlight {
    /// The bytes it covers.
    pub range: Range<usize>,
    /// What kind of token they are.
    pub capture: Capture,
    /// Whether the symbol is deprecated — drawn apart from its capture, the
    /// way the reference draws a deprecated library call.
    pub deprecated: bool,
}

impl Highlight {
    /// A range of `capture`, not deprecated.
    #[must_use]
    pub const fn new(range: Range<usize>, capture: Capture) -> Self {
        Self {
            range,
            capture,
            deprecated: false,
        }
    }
}

/// Everything a highlighter knows about one source file that an editor draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeStructure {
    /// The highlighted ranges, in source order and not overlapping. A byte in
    /// none of them is plain text.
    pub highlights: Vec<Highlight>,
    /// The brackets and their partners.
    pub brackets: BracketPairs,
    /// The ranges that can fold, in source order of their headers.
    pub folds: Vec<Fold>,
    /// The file's named parts.
    pub outline: Vec<OutlineItem>,
}

// ---------------------------------------------------------------------------
// The outline: the named parts of a file, for a jump list.
// ---------------------------------------------------------------------------

/// What kind of part an outline entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutlineKind {
    /// A function of the file's own.
    Function,
    /// A global variable.
    Variable,
    /// A named scope of handlers — an LSL state.
    State,
    /// An event handler.
    Event,
}

/// One named part of a file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineItem {
    /// What it is.
    pub kind: OutlineKind,
    /// Its name, as the source spells it.
    pub name: String,
    /// The bytes of the name where it is declared — where a jump lands.
    pub name_range: Range<usize>,
    /// The parts nested in it: a state's handlers.
    pub children: Vec<Self>,
}

impl OutlineItem {
    /// A part with nothing nested in it.
    #[must_use]
    pub const fn leaf(kind: OutlineKind, name: String, name_range: Range<usize>) -> Self {
        Self {
            kind,
            name,
            name_range,
            children: Vec::new(),
        }
    }
}
