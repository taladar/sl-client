//! The LSL lexical token kinds and the [`logos`] scanner that produces them.
//!
//! The [`Token`] enum classifies exactly what a hand-written editor scanner
//! classifies (comments, strings, numbers, operators — see the reference
//! `llkeywords.cpp`) and emits every *word* as a single [`Token::Identifier`].
//! It deliberately does **not** know the LSL library: distinguishing a
//! keyword, a built-in function, a constant or a user symbol is a lookup one
//! layer up against the grid-served syntax table, not a property of the token
//! stream.
//!
//! The scanner is **error-tolerant** — the whole point of an editor lexer is to
//! re-lex broken, half-typed code on every keystroke. Unterminated block
//! comments and strings run to end-of-input (matching the reference viewer's
//! two-sided-delimiter behaviour) rather than aborting, and any byte that
//! starts no token becomes a [`Token::Error`] the caller can highlight.

use logos::Logos;

/// A single lexical token kind in an LSL source file.
///
/// Produced by the [`crate::lexer`] driver, paired with its byte span. The
/// variants fall into a few groups:
///
/// - **Trivia** the editor colours but the parser skips:
///   [`Token::LineComment`] and [`Token::BlockComment`] (see
///   [`Token::is_comment`]).
/// - **Literals**: [`Token::IntegerLiteral`] (decimal or `0x` hex),
///   [`Token::FloatLiteral`] and [`Token::StringLiteral`].
/// - **Words**: every identifier-shaped run is a single [`Token::Identifier`],
///   with no attempt to recognise LSL keywords, types or library symbols.
/// - **Punctuation and operators**: the fixed LSL operator set.
/// - [`Token::Error`]: a byte that begins no valid token. This variant is
///   *not* produced by [`logos`] itself; the driver synthesises it from the
///   lexer's error result so callers see one uniform token stream.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r\n\x0c]+")]
pub enum Token {
    /// A `//` single-line comment, running to just before the line break.
    #[regex(r"//[^\r\n]*")]
    LineComment,
    /// A `/* … */` block comment. Error-tolerant: an unterminated `/*` runs to
    /// end-of-input, mirroring the reference viewer's two-sided delimiter.
    #[token("/*", block_comment)]
    BlockComment,
    /// A `"…"` double-quoted string literal, honouring `\"` (and `\\`) escapes.
    /// Error-tolerant: an unterminated string runs to end-of-input.
    #[token("\"", string_literal)]
    StringLiteral,
    /// An integer literal: a decimal `[0-9]+` run or a `0x` hexadecimal run.
    #[regex(r"[0-9]+")]
    #[regex(r"0[xX][0-9a-fA-F]+")]
    IntegerLiteral,
    /// A floating-point literal: a run with a decimal point and/or an exponent,
    /// and an optional `f`/`F` suffix — matching the LSL compiler's grammar
    /// (`1.0`, `.5`, `1.`, `1e10`, `1.5e-3f`). A bare `10f` is *not* a float
    /// (it lexes as [`Token::IntegerLiteral`] `10` then an identifier `f`).
    #[regex(r"[0-9]+\.[0-9]*([eE][+-]?[0-9]+)?[fF]?")]
    #[regex(r"\.[0-9]+([eE][+-]?[0-9]+)?[fF]?")]
    #[regex(r"[0-9]+[eE][+-]?[0-9]+[fF]?")]
    FloatLiteral,
    /// An identifier-shaped word: `[A-Za-z_][A-Za-z0-9_]*`. Whether it is a
    /// keyword, a library function/constant or a user symbol is resolved a
    /// layer up, not here.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Identifier,

    /// `+`
    #[token("+")]
    Plus,
    /// `-`
    #[token("-")]
    Minus,
    /// `*`
    #[token("*")]
    Star,
    /// `/`
    #[token("/")]
    Slash,
    /// `%`
    #[token("%")]
    Percent,
    /// `++`
    #[token("++")]
    PlusPlus,
    /// `--`
    #[token("--")]
    MinusMinus,
    /// `=`
    #[token("=")]
    Assign,
    /// `+=`
    #[token("+=")]
    PlusAssign,
    /// `-=`
    #[token("-=")]
    MinusAssign,
    /// `*=`
    #[token("*=")]
    StarAssign,
    /// `/=`
    #[token("/=")]
    SlashAssign,
    /// `%=`
    #[token("%=")]
    PercentAssign,
    /// `==`
    #[token("==")]
    EqEq,
    /// `!=`
    #[token("!=")]
    NotEq,
    /// `<`
    #[token("<")]
    Less,
    /// `<=`
    #[token("<=")]
    LessEq,
    /// `>`
    #[token(">")]
    Greater,
    /// `>=`
    #[token(">=")]
    GreaterEq,
    /// `&&`
    #[token("&&")]
    AndAnd,
    /// `||`
    #[token("||")]
    OrOr,
    /// `!`
    #[token("!")]
    Bang,
    /// `&`
    #[token("&")]
    Amp,
    /// `|`
    #[token("|")]
    Pipe,
    /// `^`
    #[token("^")]
    Caret,
    /// `~`
    #[token("~")]
    Tilde,
    /// `<<`
    #[token("<<")]
    ShiftLeft,
    /// `>>`
    #[token(">>")]
    ShiftRight,
    /// `.` — member access on the components of a `vector` / `rotation`.
    #[token(".")]
    Dot,
    /// `,`
    #[token(",")]
    Comma,
    /// `;`
    #[token(";")]
    Semicolon,
    /// `(`
    #[token("(")]
    LParen,
    /// `)`
    #[token(")")]
    RParen,
    /// `{`
    #[token("{")]
    LBrace,
    /// `}`
    #[token("}")]
    RBrace,
    /// `[`
    #[token("[")]
    LBracket,
    /// `]`
    #[token("]")]
    RBracket,
    /// `@` — the jump-label definition prefix (`@label;`).
    #[token("@")]
    At,

    /// A byte (or run of bytes) that begins no valid token. Synthesised by the
    /// [`crate::lexer`] driver from [`logos`]' error result; never produced by
    /// the derived scanner directly.
    Error,
}

impl Token {
    /// Whether this token is a comment — the trivia an editor colours but a
    /// parser skips. (Whitespace is dropped by the scanner and never appears as
    /// a token, so comments are the only trivia in the stream.)
    #[must_use]
    pub const fn is_comment(self) -> bool {
        matches!(self, Self::LineComment | Self::BlockComment)
    }

    /// Whether this token is the [`Token::Error`] catch-all for input that
    /// began no valid token.
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// [`logos`] callback for a `/* … */` block comment.
///
/// Called after the opening `/*` has matched; consumes the remainder up to and
/// including the closing `*/`, or to end-of-input if there is none (an
/// unterminated block comment, which an editor still colours as a comment).
fn block_comment(lex: &mut logos::Lexer<'_, Token>) {
    let rest = lex.remainder();
    match rest.find("*/") {
        // `+ 2` includes the closing `*/`; both bytes are ASCII, so the sum
        // lands on a char boundary. Saturating keeps us panic-free even though
        // `find` guarantees `i + 2 <= rest.len()`.
        Some(i) => lex.bump(i.saturating_add(2)),
        None => lex.bump(rest.len()),
    }
}

/// [`logos`] callback for a `"…"` string literal.
///
/// Called after the opening `"` has matched; consumes the remainder up to and
/// including the closing `"`, treating `\` as an escape so an escaped quote
/// (`\"`) does not end the string. Runs to end-of-input if there is no closing
/// quote (an unterminated string, which an editor still colours as a string).
fn string_literal(lex: &mut logos::Lexer<'_, Token>) {
    let rest = lex.remainder();
    let mut escaped = false;
    for (i, byte) in rest.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            // `"` is ASCII, so `i` is a char boundary and `i + 1` (past the
            // closing quote) is too.
            b'"' => {
                lex.bump(i.saturating_add(1));
                return;
            }
            _ => {}
        }
    }
    lex.bump(rest.len());
}
