//! The scanning driver: turn LSL source into a stream of [`SpannedToken`]s.
//!
//! This wraps the [`logos`]-derived [`Token`] scanner and papers over its two
//! rough edges for a consumer that wants one uniform, error-tolerant stream:
//!
//! - [`logos`] yields `Result<Token, ()>`; the driver folds a lexing error
//!   into a [`Token::Error`] token so callers match on one enum, never a
//!   `Result`.
//! - each token is paired with its byte [`span`](SpannedToken::span) in the
//!   source, which is what an editor (to colour a range) and a parser (to point
//!   at a node) both need.
//!
//! Whitespace is dropped by the scanner and never appears; comments do appear
//! (an editor colours them) and can be filtered with [`SpannedToken::is_trivia`].

use core::ops::Range;

use logos::Logos as _;

use crate::token::Token;

/// A [`Token`] paired with its byte range in the source string.
///
/// The [`span`](SpannedToken::span) indexes the original `&str` the token was
/// lexed from (`source[token.span.clone()]` is the matched text). Spans are
/// half-open byte ranges and always fall on `char` boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpannedToken {
    /// The token kind.
    pub token: Token,
    /// The half-open byte range of the token within the source string.
    pub span: Range<usize>,
}

impl SpannedToken {
    /// The matched source text for this token, sliced out of the `source` it
    /// was lexed from.
    ///
    /// The caller must pass the same string that was lexed; passing a different
    /// or shorter string returns `None` rather than panicking.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> Option<&'source str> {
        source.get(self.span.clone())
    }

    /// Whether this token is trivia a parser should skip — i.e. a comment.
    /// (Whitespace never appears in the stream, so comments are the only
    /// trivia.)
    #[must_use]
    pub const fn is_trivia(&self) -> bool {
        self.token.is_comment()
    }
}

/// A lazy iterator over the [`SpannedToken`]s of an LSL source string.
///
/// Created by [`tokens`]. Yields every token — including comments and
/// [`Token::Error`] tokens — and never fails; iteration simply ends at
/// end-of-input.
#[derive(Debug)]
pub struct Tokens<'source> {
    /// The underlying [`logos`] lexer.
    inner: logos::Lexer<'source, Token>,
}

impl Iterator for Tokens<'_> {
    type Item = SpannedToken;

    /// Advance to the next token, folding a lexing error into [`Token::Error`].
    fn next(&mut self) -> Option<SpannedToken> {
        let result = self.inner.next()?;
        let span = self.inner.span();
        let token = result.unwrap_or(Token::Error);
        Some(SpannedToken { token, span })
    }
}

/// Lex an LSL source string into a lazy stream of spanned tokens.
///
/// Error-tolerant and total: it never fails, yielding [`Token::Error`] tokens
/// for input that begins no valid token and running unterminated block comments
/// and strings to end-of-input. Comments are included in the stream; whitespace
/// is not.
#[must_use]
pub fn tokens(source: &str) -> Tokens<'_> {
    Tokens {
        inner: Token::lexer(source),
    }
}

/// Lex an LSL source string into a vector of spanned tokens.
///
/// Convenience wrapper that collects [`tokens`]; see it for the error-tolerance
/// guarantees.
#[must_use]
pub fn lex(source: &str) -> Vec<SpannedToken> {
    tokens(source).collect()
}
