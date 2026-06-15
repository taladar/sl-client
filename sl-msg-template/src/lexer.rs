//! A tiny tokenizer for the message template format.
//!
//! The format is line oriented and brace delimited: comments run from `//` to
//! the end of a line, and the only significant tokens are the braces `{` and
//! `}` plus whitespace-separated words. This module turns a source string into
//! a flat list of [`Token`]s annotated with their source line.

/// The kind of a lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// An opening brace `{`.
    OpenBrace,
    /// A closing brace `}`.
    CloseBrace,
    /// A whitespace-delimited word (identifier, keyword, or number literal).
    Word(String),
}

/// A token together with the 1-based source line it was found on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    /// The kind of token.
    pub(crate) kind: TokenKind,
    /// The 1-based source line the token appeared on.
    pub(crate) line: usize,
}

/// Tokenizes `input` into a flat list of [`Token`]s, stripping `//` comments.
#[must_use]
pub(crate) fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (index, raw_line) in input.lines().enumerate() {
        let line = index.saturating_add(1);
        // Strip any trailing line comment.
        let content = match raw_line.split_once("//") {
            Some((before, _)) => before,
            None => raw_line,
        };
        lex_line(content, line, &mut tokens);
    }
    tokens
}

/// Tokenizes a single comment-stripped line, appending tokens to `out`.
fn lex_line(content: &str, line: usize, out: &mut Vec<Token>) {
    /// Pushes the accumulated word (if any) as a token and clears the buffer.
    fn flush(word: &mut String, line: usize, out: &mut Vec<Token>) {
        if !word.is_empty() {
            out.push(Token {
                kind: TokenKind::Word(core::mem::take(word)),
                line,
            });
        }
    }

    let mut word = String::new();
    for ch in content.chars() {
        if ch == '{' {
            flush(&mut word, line, out);
            out.push(Token {
                kind: TokenKind::OpenBrace,
                line,
            });
        } else if ch == '}' {
            flush(&mut word, line, out);
            out.push(Token {
                kind: TokenKind::CloseBrace,
                line,
            });
        } else if ch.is_whitespace() {
            flush(&mut word, line, out);
        } else {
            word.push(ch);
        }
    }
    flush(&mut word, line, out);
}
