//! Pure **Linden Scripting Language** (LSL) tooling: the first piece is a
//! `logos` **lexer** that turns LSL source into a token stream.
//!
//! See the crate `README.md` for an overview. Like its siblings `sl-prim`
//! (prim tessellation), `sl-anim` (keyframe motion) and `sl-avatar` (skeleton),
//! the crate is deliberately **Bevy-free and I/O-free**: it turns a borrowed
//! `&str` into owned tokens and never opens a file or fetches from the grid. A
//! lexer has no business knowing about circuits or capabilities, which keeps it
//! testable, fuzzable and reusable (a linter, a CI check, an external tool)
//! without a grid.
//!
//! Crucially the lexer does **not** bake in the LSL library. It emits every
//! identifier-shaped word as a single [`Token::Identifier`]; classifying a word
//! as a keyword, a built-in function/constant or a user symbol is a lookup one
//! layer up against the keyword table the grid serves at runtime (the
//! `LSLSyntax` capability), rather than a set of grammar literals baked in at
//! build time. That layering is why a hand-written scanner over this one token
//! stream — shared by both the highlighter and the parser — was chosen over a
//! tree-sitter grammar (every existing LSL grammar enumerates the ~500 library
//! functions as literals, exactly backwards for a grid-served symbol table).
//!
//! The pieces are:
//!
//! - [`token`] — the [`Token`] kinds and the `logos`-derived scanner that
//!   classifies comments, strings, numbers and operators.
//! - [`lexer`] — the driver ([`lex`] / [`tokens`]) that pairs each token with
//!   its byte span and folds lexing errors into [`Token::Error`], so callers
//!   see one uniform, error-tolerant stream.
//! - [`ast`] — the owned, fully-spanned syntax tree the parser builds.
//! - [`parser`] — the error-tolerant recursive-descent [`parse`] that turns the
//!   token stream into an [`ast::Script`], recording (never aborting on) syntax
//!   errors so a half-typed statement does not discard the rest of the file.
//! - [`syntax`] — the owned **library symbol table** ([`LslSyntax`]): the
//!   grid-served functions, constants, events and keywords the lexer refuses to
//!   bake in, decoded (by `sl-wire`) from the `LSLSyntax` capability and read
//!   here for highlighting, tooltips and the semantic pass.
//! - [`semantics`] — the **semantic pass** ([`analyze`]): the checks LSL's type
//!   rules make decidable locally (undefined symbols, call arity and types,
//!   `return` correctness, duplicate definitions, state reachability), held to a
//!   deliberate no-false-positive bar so a save happens when the script is
//!   right, not as a way of finding out.
//! - [`render`] — **reader-facing diagnostic rendering** ([`render_diagnostic`],
//!   [`render_grid_error`]): the semantic findings (and the grid's own compiler
//!   errors) turned into `rustc`-grade output — a caret under the source line,
//!   an edit-distance "did you mean…?" against the real library, and the grid's
//!   own signature quoted back on a type error.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod semantics;
pub mod syntax;
pub mod token;

pub use lexer::{SpannedToken, Tokens, lex, tokens};
pub use parser::{Parse, ParseError, parse};
pub use render::{
    RenderStyle, closest, render_diagnostic, render_diagnostic_styled, render_diagnostics,
    render_grid_error, render_parse_error,
};
pub use semantics::{Diagnostic, DiagnosticKind, Severity, analyze};
pub use syntax::{
    LSL_SYNTAX_VERSION, LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax,
    SymbolKind,
};
pub use token::Token;
