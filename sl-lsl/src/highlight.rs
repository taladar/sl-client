//! The **editor's view** of an LSL script: what each token is, which brackets
//! pair, what folds and what the script is made of — in the language-neutral
//! vocabulary of `sl-highlight`, so the editor that draws it knows nothing about
//! LSL.
//!
//! Everything here is read off the [`crate::lexer`]'s token stream and nothing
//! deeper, which is what keeps it cheap enough to run on every keystroke (a
//! re-lex of a 64 KB script is well under a millisecond) and tolerant of a
//! script that is half typed.
//!
//! # Words
//!
//! The lexer emits every word as an [`Token::Identifier`]; what it is comes from
//! the **grid's** table ([`LslSyntax`]) first, as the reference viewer colours
//! (`llkeywords.cpp`), so a function a grid adds colours with no change here.
//! The few words that are LSL's **grammar** rather than its library — the type
//! names and `default`, `state`, `if`, `jump`, … — are keywords whatever the
//! table says, because the parser treats them so; a grid that serves no table
//! at all still gets its control flow coloured.
//!
//! Beyond the table, the word's neighbours decide: the word after `state` is a
//! state's name, the word after `@` or `jump` a label, and a word followed by
//! `(` that the library does not know is a function of the script's own.
//!
//! # Folds
//!
//! Every `{ … }` and `[ … ]` that spans lines folds, and so does a block comment
//! that does.
//!
//! # Outline
//!
//! The global functions and variables and the states with their event handlers,
//! found by brace depth: a state is `default` or `state name` at depth 0, its
//! handlers the `name (` at depth 1 inside it, a function a `name (` at depth 0
//! outside any state, and a global variable a type, a name and `=` or `;` at
//! depth 0.

use core::ops::Range;

use sl_highlight::{
    Bracket, BracketKind, BracketPairs, Capture, CodeStructure, Fold, Highlight, OutlineItem,
    OutlineKind,
};

use crate::ast::TypeName;
use crate::lexer::{SpannedToken, tokens};
use crate::parser::is_reserved_word;
use crate::syntax::{LslSyntax, SymbolKind};
use crate::token::Token;

/// The editor's view of `source`, with words looked up in `syntax`.
#[must_use]
pub fn structure(source: &str, syntax: &LslSyntax) -> CodeStructure {
    let all: Vec<SpannedToken> = tokens(source).collect();
    // The tokens the neighbour rules look across: everything but comments.
    let code: Vec<&SpannedToken> = all.iter().filter(|token| !token.is_trivia()).collect();

    let mut highlights = Vec::with_capacity(all.len());
    let mut brackets = Vec::new();
    let mut folds = Vec::new();
    let mut place = 0_usize;
    for token in &all {
        if token.is_trivia() {
            highlights.push(Highlight::new(token.span.clone(), Capture::Comment));
            if token.token == Token::BlockComment
                && let Some(fold) = comment_fold(source, &token.span)
            {
                folds.push(fold);
            }
            continue;
        }
        let before = place.checked_sub(1).and_then(|at| code.get(at)).copied();
        let after = code.get(place.saturating_add(1)).copied();
        place = place.saturating_add(1);
        match token.token {
            Token::StringLiteral => string_highlights(source, &token.span, &mut highlights),
            Token::IntegerLiteral | Token::FloatLiteral => {
                highlights.push(Highlight::new(token.span.clone(), Capture::Number));
            }
            Token::Identifier => {
                if let Some(highlight) = word_highlight(source, token, before, after, syntax) {
                    highlights.push(highlight);
                }
            }
            Token::LParen
            | Token::RParen
            | Token::LBracket
            | Token::RBracket
            | Token::LBrace
            | Token::RBrace => {
                highlights.push(Highlight::new(
                    token.span.clone(),
                    Capture::PunctuationBracket,
                ));
                brackets.push(bracket(token));
            }
            Token::Comma | Token::Semicolon | Token::Dot => {
                highlights.push(Highlight::new(
                    token.span.clone(),
                    Capture::PunctuationDelimiter,
                ));
            }
            Token::At => {
                highlights.push(Highlight::new(
                    token.span.clone(),
                    Capture::PunctuationSpecial,
                ));
            }
            Token::Error => highlights.push(Highlight::new(token.span.clone(), Capture::Error)),
            _ => highlights.push(Highlight::new(token.span.clone(), Capture::Operator)),
        }
    }

    let brackets = BracketPairs::new(brackets);
    folds.extend(
        brackets
            .pairs()
            .iter()
            .filter(|pair| pair.kind != BracketKind::Round)
            .filter_map(|pair| {
                Fold::between(source, pair.open.start, pair.open.end, pair.close.start)
            }),
    );
    folds.sort_by_key(|fold| fold.header);
    CodeStructure {
        highlights,
        brackets,
        folds,
        outline: outline(source, &code, syntax),
    }
}

/// The [`Bracket`] a bracket token is.
fn bracket(token: &SpannedToken) -> Bracket {
    let (kind, open) = match token.token {
        Token::LParen => (BracketKind::Round, true),
        Token::RParen => (BracketKind::Round, false),
        Token::LBracket => (BracketKind::Square, true),
        Token::RBracket => (BracketKind::Square, false),
        Token::LBrace => (BracketKind::Curly, true),
        _ => (BracketKind::Curly, false),
    };
    Bracket {
        range: token.span.clone(),
        kind,
        open,
    }
}

/// The text of `token` in `source`, or `""` when it does not fit.
fn text_of<'source>(source: &'source str, token: &SpannedToken) -> &'source str {
    token.text(source).unwrap_or_default()
}

/// Whether `token` is the word `word`.
fn is_word(source: &str, token: Option<&SpannedToken>, word: &str) -> bool {
    token.is_some_and(|token| token.token == Token::Identifier && text_of(source, token) == word)
}

/// A string literal's highlights: the string, cut around each escape sequence
/// (`\` and the character after it) so the escape is a capture of its own.
fn string_highlights(source: &str, span: &Range<usize>, highlights: &mut Vec<Highlight>) {
    let Some(literal) = source.get(span.clone()) else {
        return;
    };
    let mut plain_from = span.start;
    let mut characters = literal.char_indices();
    while let Some((offset, character)) = characters.next() {
        if character != '\\' {
            continue;
        }
        let start = span.start.saturating_add(offset);
        let end = characters.next().map_or(span.end, |(next, escaped)| {
            span.start
                .saturating_add(next)
                .saturating_add(escaped.len_utf8())
        });
        if plain_from < start {
            highlights.push(Highlight::new(plain_from..start, Capture::String));
        }
        highlights.push(Highlight::new(start..end, Capture::StringEscape));
        plain_from = end;
    }
    if plain_from < span.end {
        highlights.push(Highlight::new(plain_from..span.end, Capture::String));
    }
}

/// What the word `token` is, given the code tokens either side of it.
fn word_highlight(
    source: &str,
    token: &SpannedToken,
    before: Option<&SpannedToken>,
    after: Option<&SpannedToken>,
    syntax: &LslSyntax,
) -> Option<Highlight> {
    let word = text_of(source, token);
    let span = token.span.clone();
    if is_word(source, before, "state") && word != "default" {
        return Some(Highlight::new(span, Capture::Namespace));
    }
    if before.is_some_and(|before| before.token == Token::At) || is_word(source, before, "jump") {
        return Some(Highlight::new(span, Capture::Label));
    }
    match syntax.classify(word) {
        Some(SymbolKind::Function) => {
            return Some(Highlight {
                range: span,
                capture: Capture::FunctionBuiltin,
                deprecated: syntax.function(word).is_some_and(|f| f.deprecated),
            });
        }
        Some(SymbolKind::Constant) => {
            return Some(Highlight {
                range: span,
                capture: Capture::ConstantBuiltin,
                deprecated: syntax.constant(word).is_some_and(|c| c.deprecated),
            });
        }
        Some(SymbolKind::Event) => {
            return Some(Highlight {
                range: span,
                capture: Capture::FunctionEvent,
                deprecated: syntax.event(word).is_some_and(|e| e.deprecated),
            });
        }
        Some(SymbolKind::Control) => return Some(Highlight::new(span, Capture::Keyword)),
        Some(SymbolKind::Type) => return Some(Highlight::new(span, Capture::Type)),
        None => {}
    }
    if TypeName::from_keyword(word).is_some() {
        return Some(Highlight::new(span, Capture::Type));
    }
    if is_reserved_word(word) {
        return Some(Highlight::new(span, Capture::Keyword));
    }
    after
        .is_some_and(|after| after.token == Token::LParen)
        .then(|| Highlight::new(span, Capture::Function))
}

/// The fold of a block comment that spans lines: its first line stays, the rest
/// up to the line holding `*/` folds.
fn comment_fold(source: &str, span: &Range<usize>) -> Option<Fold> {
    let comment = source.get(span.clone())?;
    let first_break = span.start.saturating_add(comment.find('\n')?);
    let close = if comment.ends_with("*/") && comment.len() >= 4 {
        span.end.saturating_sub(2)
    } else {
        span.end
    };
    Fold::between(source, span.start, first_break, close)
}

/// The outline of the script whose code tokens (comments left out) are `code`.
pub(crate) fn outline(
    source: &str,
    code: &[&SpannedToken],
    syntax: &LslSyntax,
) -> Vec<OutlineItem> {
    let mut items: Vec<OutlineItem> = Vec::new();
    let mut depth = 0_usize;
    // The state being read, while inside one.
    let mut state: Option<OutlineItem> = None;
    for (place, token) in code.iter().enumerate() {
        let next = code.get(place.saturating_add(1)).copied();
        let previous = place.checked_sub(1).and_then(|at| code.get(at)).copied();
        match token.token {
            Token::LBrace => depth = depth.saturating_add(1),
            Token::RBrace => {
                depth = depth.saturating_sub(1);
                if depth == 0
                    && let Some(finished) = state.take()
                {
                    items.push(finished);
                }
            }
            Token::Identifier if depth == 0 => {
                let word = text_of(source, token);
                if word == "default" || is_word(source, previous, "state") {
                    finish_state(&mut state, &mut items);
                    state = Some(OutlineItem::leaf(
                        OutlineKind::State,
                        String::from(word),
                        token.span.clone(),
                    ));
                } else if word == "state" || is_type_word(word, syntax) {
                    // A declaration's leading word; its name comes next.
                } else if next.is_some_and(|next| next.token == Token::LParen) {
                    items.push(OutlineItem::leaf(
                        OutlineKind::Function,
                        String::from(word),
                        token.span.clone(),
                    ));
                } else if previous.is_some_and(|previous| {
                    previous.token == Token::Identifier
                        && is_type_word(text_of(source, previous), syntax)
                }) && next
                    .is_some_and(|next| matches!(next.token, Token::Assign | Token::Semicolon))
                {
                    items.push(OutlineItem::leaf(
                        OutlineKind::Variable,
                        String::from(word),
                        token.span.clone(),
                    ));
                }
            }
            Token::Identifier if depth == 1 => {
                if let Some(state) = state.as_mut()
                    && next.is_some_and(|next| next.token == Token::LParen)
                {
                    state.children.push(OutlineItem::leaf(
                        OutlineKind::Event,
                        String::from(text_of(source, token)),
                        token.span.clone(),
                    ));
                }
            }
            _ => {}
        }
    }
    finish_state(&mut state, &mut items);
    items
}

/// File a state that was being read (one whose block never closed, or never
/// opened before the next began).
fn finish_state(state: &mut Option<OutlineItem>, items: &mut Vec<OutlineItem>) {
    if let Some(finished) = state.take() {
        items.push(finished);
    }
}

/// Whether `word` names a type, by the grammar or by the grid's table.
fn is_type_word(word: &str, syntax: &LslSyntax) -> bool {
    TypeName::from_keyword(word).is_some() || syntax.is_type(word)
}

#[cfg(test)]
mod tests {
    use super::structure;
    use crate::syntax::{LslConstant, LslFunction, LslSyntax};
    use pretty_assertions::assert_eq;
    use sl_highlight::{Capture, OutlineKind};

    /// A grid table with one function, one deprecated function and a constant.
    fn syntax() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        syntax
            .functions
            .insert(String::from("llSay"), LslFunction::default());
        syntax.functions.insert(
            String::from("llSound"),
            LslFunction {
                deprecated: true,
                ..LslFunction::default()
            },
        );
        syntax
            .constants
            .insert(String::from("TRUE"), LslConstant::default());
        syntax
    }

    /// Each highlighted range's text and capture name, deprecated ones marked.
    fn captures(source: &str, syntax: &LslSyntax) -> Vec<(String, String)> {
        structure(source, syntax)
            .highlights
            .iter()
            .map(|highlight| {
                let mut name = String::from(highlight.capture.name());
                if highlight.deprecated {
                    name.push('!');
                }
                (
                    String::from(source.get(highlight.range.clone()).unwrap_or_default()),
                    name,
                )
            })
            .collect()
    }

    /// The table colours library words; the grammar colours keywords and types
    /// with no table at all; neighbours colour states, labels and the script's
    /// own functions.
    #[test]
    fn words_are_coloured_by_table_grammar_and_neighbours() {
        let source = "integer f(){ if (TRUE) llSay(0, \"a\\nb\"); llSound(); \
                      jump done; @done; state two; }";
        let found = captures(source, &syntax());
        let want: Vec<(String, String)> = [
            ("integer", "type"),
            ("f", "function"),
            ("(", "punctuation.bracket"),
            (")", "punctuation.bracket"),
            ("{", "punctuation.bracket"),
            ("if", "keyword"),
            ("(", "punctuation.bracket"),
            ("TRUE", "constant.builtin"),
            (")", "punctuation.bracket"),
            ("llSay", "function.builtin"),
            ("(", "punctuation.bracket"),
            ("0", "number"),
            (",", "punctuation.delimiter"),
            ("\"a", "string"),
            ("\\n", "string.escape"),
            ("b\"", "string"),
            (")", "punctuation.bracket"),
            (";", "punctuation.delimiter"),
            ("llSound", "function.builtin!"),
            ("(", "punctuation.bracket"),
            (")", "punctuation.bracket"),
            (";", "punctuation.delimiter"),
            ("jump", "keyword"),
            ("done", "label"),
            (";", "punctuation.delimiter"),
            ("@", "punctuation.special"),
            ("done", "label"),
            (";", "punctuation.delimiter"),
            ("state", "keyword"),
            ("two", "namespace"),
            (";", "punctuation.delimiter"),
            ("}", "punctuation.bracket"),
        ]
        .iter()
        .map(|(text, name)| (String::from(*text), String::from(*name)))
        .collect();
        assert_eq!(found, want);
    }

    /// A word the table does not know and nothing marks stays plain, and a
    /// comment is one range whatever is in it.
    #[test]
    fn plain_words_and_comments() {
        let found = captures("x = y; // llSay(", &LslSyntax::default());
        let names: Vec<&str> = found.iter().map(|(_text, name)| name.as_str()).collect();
        assert_eq!(names, vec!["operator", "punctuation.delimiter", "comment"]);
        assert_eq!(
            Capture::from_name(names.first().copied().unwrap_or_default()),
            Some(Capture::Operator)
        );
    }

    /// Blocks and lists that span lines fold, and so does a long comment; a
    /// one-line block does not.
    #[test]
    fn multi_line_blocks_lists_and_comments_fold() {
        let source = "/* one\n two\n*/\nlist l = [\n 1\n];\ndefault {\n    x() { }\n}\n";
        let structure = structure(source, &LslSyntax::default());
        let hidden: Vec<&str> = structure
            .folds
            .iter()
            .map(|fold| source.get(fold.hidden.clone()).unwrap_or_default())
            .collect();
        assert_eq!(hidden, vec!["\n two\n", "\n 1\n", "\n    x() { }\n"]);
    }

    /// The outline is the globals, then each state with its handlers, in source
    /// order.
    #[test]
    fn the_outline_names_globals_states_and_handlers() {
        let source = "integer count = 0;\nstring name;\nsay(string m) { llSay(0, m); }\n\
                      default {\n    state_entry() { if (1) { } }\n    touch_start(integer n) { }\n}\n\
                      state other {\n    timer() { state default; }\n}\n";
        let outline = structure(source, &syntax()).outline;
        let flat: Vec<(OutlineKind, &str, Vec<&str>)> = outline
            .iter()
            .map(|item| {
                (
                    item.kind,
                    item.name.as_str(),
                    item.children
                        .iter()
                        .map(|child| child.name.as_str())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            flat,
            vec![
                (OutlineKind::Variable, "count", vec![]),
                (OutlineKind::Variable, "name", vec![]),
                (OutlineKind::Function, "say", vec![]),
                (
                    OutlineKind::State,
                    "default",
                    vec!["state_entry", "touch_start"]
                ),
                (OutlineKind::State, "other", vec!["timer"]),
            ]
        );
        let first = outline.first().map(|item| item.name_range.clone());
        assert_eq!(first.and_then(|range| source.get(range)), Some("count"));
    }
}
