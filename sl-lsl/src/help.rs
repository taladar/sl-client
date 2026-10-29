//! The **editor's help** while an LSL script is typed: completions, a call's
//! signature and a symbol's description, from the grid's table
//! ([`LslSyntax`]) and the script's own declarations, in `sl-highlight`'s
//! language-neutral shapes.
//!
//! Like [`crate::highlight`] it reads the [`crate::lexer`]'s token stream and
//! nothing deeper, so it answers a script that is half typed, on every
//! keystroke.
//!
//! # What is offered where
//!
//! The caret's place decides, by brace depth and the token before the word:
//!
//! - **between declarations** — the types, `default` and `state`; after an `=`
//!   the constants and the script's globals; inside a function's parentheses
//!   the types;
//! - **in a state, between handlers** — the events, completed to their whole
//!   header (`touch_start(integer num_detected)`);
//! - **in a body** — the library's functions and constants, the flow keywords
//!   and types, the script's functions and globals, and the parameters and
//!   locals declared above the caret in the same function or handler;
//! - after `state`, the script's states; after `jump`, its labels; after a
//!   `.`, a vector's or rotation's members.
//!
//! Nothing is offered inside a comment or a string, or where a new name is
//! being declared (after a type).
//!
//! # Signatures
//!
//! The call is the innermost unclosed `(` before the caret with a function's
//! name before it; the argument is the count of commas at that call's own
//! level, so a list or a vector inside the call does not move it.

use core::ops::Range;
use std::sync::Arc;

use sl_highlight::{
    Capture, CodeAssist, Completion, Completions, Hover, Note, OutlineKind, Parameter, Signature,
    rank,
};

use crate::ast::TypeName;
use crate::highlight::outline;
use crate::lexer::{SpannedToken, tokens};
use crate::syntax::{LslArgument, LslFunction, LslSyntax};
use crate::token::Token;

/// The most completions one answer carries.
const COMPLETION_LIMIT: usize = 200;

/// LSL's type keywords, `quaternion` (the legacy spelling) left out.
const TYPE_WORDS: [&str; 7] = [
    "integer", "float", "string", "key", "vector", "rotation", "list",
];

/// LSL's flow keywords, offered in a body whether or not the grid's table
/// lists them.
const CONTROL_WORDS: [&str; 8] = [
    "if", "else", "for", "while", "do", "jump", "return", "state",
];

/// A vector's and a rotation's members.
const MEMBERS: [&str; 4] = ["x", "y", "z", "s"];

/// LSL's help, from a grid's table.
#[derive(Debug, Clone, Default)]
pub struct LslAssist {
    /// The grid's library.
    syntax: Arc<LslSyntax>,
}

impl LslAssist {
    /// Help from `syntax`.
    #[must_use]
    pub const fn new(syntax: Arc<LslSyntax>) -> Self {
        Self { syntax }
    }
}

impl CodeAssist for LslAssist {
    fn completions(&self, source: &str, caret: usize) -> Option<Completions> {
        completions(source, caret, &self.syntax)
    }

    fn signature(&self, source: &str, caret: usize) -> Option<Signature> {
        signature(source, caret, &self.syntax)
    }

    fn hover(&self, source: &str, index: usize) -> Option<Hover> {
        hover(source, index, &self.syntax)
    }
}

// ---------------------------------------------------------------------------
// Reading the tokens.
// ---------------------------------------------------------------------------

/// A source and its tokens.
struct Lexed<'source> {
    /// The source.
    source: &'source str,
    /// Every token, comments included.
    all: Vec<SpannedToken>,
}

impl<'source> Lexed<'source> {
    /// Lex `source`.
    fn new(source: &'source str) -> Self {
        Self {
            source,
            all: tokens(source).collect(),
        }
    }

    /// The code tokens — everything but comments.
    fn code(&self) -> Vec<&SpannedToken> {
        self.all.iter().filter(|token| !token.is_trivia()).collect()
    }

    /// The text of `token`.
    fn text(&self, token: &SpannedToken) -> &'source str {
        token.text(self.source).unwrap_or_default()
    }

    /// Whether `token` is the word `word`.
    fn is_word(&self, token: Option<&SpannedToken>, word: &str) -> bool {
        token.is_some_and(|token| token.token == Token::Identifier && self.text(token) == word)
    }

    /// The comment `caret` is inside, if any.
    fn in_comment(&self, caret: usize) -> bool {
        self.all.iter().any(|token| {
            token.is_trivia() && token.span.start < caret && {
                let open = match token.token {
                    Token::BlockComment => {
                        let text = self.text(token);
                        !(text.len() >= 4 && text.ends_with("*/"))
                    }
                    // A line comment runs to the end of its line, which the
                    // caret at its end is still on.
                    _ => true,
                };
                caret < token.span.end || (open && caret == token.span.end)
            }
        })
    }

    /// Whether `caret` is inside a string literal.
    fn in_string(&self, caret: usize) -> bool {
        self.all.iter().any(|token| {
            token.token == Token::StringLiteral && token.span.start < caret && {
                let text = self.text(token);
                let opener = if text.starts_with('L') { 2 } else { 1 };
                let closed = text.len() > opener && text.ends_with('"');
                caret < token.span.end || (!closed && caret == token.span.end)
            }
        })
    }
}

/// Whether `word` names a type, by the grammar or the grid's table.
fn is_type_word(word: &str, syntax: &LslSyntax) -> bool {
    TypeName::from_keyword(word).is_some() || syntax.is_type(word)
}

/// Whether `word` is a flow keyword, by the grammar or the grid's table.
fn is_control_word(word: &str, syntax: &LslSyntax) -> bool {
    CONTROL_WORDS.contains(&word) || syntax.is_control(word)
}

// ---------------------------------------------------------------------------
// Signatures.
// ---------------------------------------------------------------------------

/// A signature's label and its parameters' ranges in it, from a return type,
/// a name and `(type, name)` parameters.
fn signature_label(
    return_type: Option<&str>,
    name: &str,
    parameters: &[(Option<&str>, &str)],
) -> (String, Vec<Range<usize>>) {
    let mut label = String::new();
    if let Some(return_type) = return_type {
        label.push_str(return_type);
        label.push(' ');
    }
    label.push_str(name);
    label.push('(');
    let mut ranges = Vec::with_capacity(parameters.len());
    for (place, (kind, parameter)) in parameters.iter().enumerate() {
        if place > 0 {
            label.push_str(", ");
        }
        let start = label.len();
        if let Some(kind) = kind {
            label.push_str(kind);
            label.push(' ');
        }
        label.push_str(parameter);
        ranges.push(start..label.len());
    }
    label.push(')');
    (label, ranges)
}

/// The `(type, name)` pairs of a library entry's arguments.
fn library_parameters(arguments: &[LslArgument]) -> Vec<(Option<&'static str>, &str)> {
    arguments
        .iter()
        .map(|argument| {
            (
                argument.arg_type.map(TypeName::keyword),
                argument.name.as_str(),
            )
        })
        .collect()
}

/// A library function's signature, no argument active.
fn library_signature(name: &str, function: &LslFunction) -> Signature {
    let (label, ranges) = signature_label(
        function.return_type.map(TypeName::keyword),
        name,
        &library_parameters(&function.arguments),
    );
    Signature {
        label,
        parameters: ranges
            .into_iter()
            .zip(&function.arguments)
            .map(|(label, argument)| Parameter {
                label,
                documentation: argument.tooltip.clone(),
            })
            .collect(),
        active: None,
        documentation: function.tooltip.clone(),
        notes: function_notes(function),
    }
}

/// What is worth knowing about a library function beyond its signature.
fn function_notes(function: &LslFunction) -> Vec<Note> {
    let mut notes = Vec::new();
    if let Some(sleep) = function.sleep.filter(|sleep| *sleep > 0.0) {
        notes.push(Note::Sleep(sleep));
    }
    if function.deprecated {
        notes.push(Note::Deprecated);
    }
    if function.god_mode {
        notes.push(Note::GodMode);
    }
    notes
}

/// An event's signature, no argument active.
fn event_signature(name: &str, syntax: &LslSyntax) -> Option<Signature> {
    let event = syntax.event(name)?;
    let (label, ranges) = signature_label(None, name, &library_parameters(&event.arguments));
    let mut notes = Vec::new();
    if event.deprecated {
        notes.push(Note::Deprecated);
    }
    Some(Signature {
        label,
        parameters: ranges
            .into_iter()
            .zip(&event.arguments)
            .map(|(label, argument)| Parameter {
                label,
                documentation: argument.tooltip.clone(),
            })
            .collect(),
        active: None,
        documentation: event.tooltip.clone(),
        notes,
    })
}

// ---------------------------------------------------------------------------
// The script's own declarations.
// ---------------------------------------------------------------------------

/// Something the script declares.
#[derive(Debug, Clone)]
struct Declaration {
    /// Its name.
    name: String,
    /// What it is.
    kind: OutlineKind,
    /// Its signature (a function) or `type name` (a variable).
    detail: String,
    /// A function's parameters' ranges in `detail`.
    parameters: Vec<Range<usize>>,
}

/// Where `span` is in `code`, by its start.
fn code_index(code: &[&SpannedToken], span: &Range<usize>) -> usize {
    code.partition_point(|token| token.span.start < span.start)
}

/// The `(type, name)` parameters of the parameter list opening at code token
/// `open`, which must be a `(`.
fn declared_parameters<'source>(
    lexed: &Lexed<'source>,
    code: &[&SpannedToken],
    open: usize,
) -> Vec<(Option<&'source str>, &'source str)> {
    let mut parameters = Vec::new();
    let mut kind: Option<&str> = None;
    for token in code.iter().skip(open.saturating_add(1)) {
        match token.token {
            Token::RParen | Token::LBrace | Token::Semicolon => break,
            Token::Identifier => {
                let word = lexed.text(token);
                if kind.is_none() && TypeName::from_keyword(word).is_some() {
                    kind = Some(word);
                } else {
                    parameters.push((kind.take(), word));
                }
            }
            _ => kind = None,
        }
    }
    parameters
}

/// The script's globals, functions and states.
fn declarations(lexed: &Lexed<'_>, code: &[&SpannedToken], syntax: &LslSyntax) -> Vec<Declaration> {
    let mut found = Vec::new();
    for item in outline(lexed.source, code, syntax) {
        let at = code_index(code, &item.name_range);
        let before = at.checked_sub(1).and_then(|place| code.get(place)).copied();
        let kind_word = before
            .map(|token| lexed.text(token))
            .filter(|word| is_type_word(word, syntax));
        match item.kind {
            OutlineKind::Function => {
                let parameters = declared_parameters(lexed, code, at.saturating_add(1));
                let (detail, ranges) = signature_label(kind_word, &item.name, &parameters);
                found.push(Declaration {
                    name: item.name,
                    kind: OutlineKind::Function,
                    detail,
                    parameters: ranges,
                });
            }
            OutlineKind::Variable => found.push(Declaration {
                detail: kind_word
                    .map_or_else(|| item.name.clone(), |kind| format!("{kind} {}", item.name)),
                name: item.name,
                kind: OutlineKind::Variable,
                parameters: Vec::new(),
            }),
            OutlineKind::State => found.push(Declaration {
                detail: if item.name == "default" {
                    item.name.clone()
                } else {
                    format!("state {}", item.name)
                },
                name: item.name,
                kind: OutlineKind::State,
                parameters: Vec::new(),
            }),
            OutlineKind::Event => {}
        }
    }
    found
}

/// The labels the script declares (`@name;`).
fn labels<'source>(lexed: &Lexed<'source>, code: &[&SpannedToken]) -> Vec<&'source str> {
    code.windows(2)
        .filter_map(|pair| match pair {
            [at, name] if at.token == Token::At && name.token == Token::Identifier => {
                Some(lexed.text(name))
            }
            _ => None,
        })
        .collect()
}

/// A completion for a declaration.
fn declaration_completion(declaration: &Declaration) -> Completion {
    let capture = match declaration.kind {
        OutlineKind::Function => Some(Capture::Function),
        OutlineKind::State => Some(Capture::Namespace),
        OutlineKind::Variable | OutlineKind::Event => None,
    };
    Completion {
        detail: Some(declaration.detail.clone()),
        ..Completion::word(declaration.name.clone(), capture)
    }
}

// ---------------------------------------------------------------------------
// Where the caret is.
// ---------------------------------------------------------------------------

/// What kind of place a word is being typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Between global declarations.
    Global,
    /// A global's initializer, after its `=`.
    Initializer,
    /// A parameter list being declared.
    Parameters,
    /// A state's block, between handlers.
    Handlers,
    /// A function's or a handler's body.
    Body,
}

/// Where the code tokens before `end` (an index into `code`) leave the next
/// word: its [`Place`], and the index the enclosing function or handler starts
/// at, for its parameters and locals.
fn place_of(lexed: &Lexed<'_>, code: &[&SpannedToken], end: usize) -> (Place, usize) {
    let mut depth = 0_usize;
    let mut parens = 0_usize;
    let mut in_state = false;
    let mut item_start = 0_usize;
    let mut assigned = false;
    for (index, token) in code.iter().take(end).enumerate() {
        let next = index.saturating_add(1);
        match token.token {
            Token::LBrace => {
                if depth == 0 {
                    let before = index.checked_sub(1).and_then(|at| code.get(at)).copied();
                    let two_before = index.checked_sub(2).and_then(|at| code.get(at)).copied();
                    in_state =
                        lexed.is_word(before, "default") || lexed.is_word(two_before, "state");
                    if in_state {
                        item_start = next;
                    }
                }
                depth = depth.saturating_add(1);
                parens = 0;
            }
            Token::RBrace => {
                depth = depth.saturating_sub(1);
                parens = 0;
                if depth == 0 || (depth == 1 && in_state) {
                    item_start = next;
                    assigned = false;
                }
                if depth == 0 {
                    in_state = false;
                }
            }
            Token::Semicolon if depth == 0 => {
                item_start = next;
                assigned = false;
                parens = 0;
            }
            Token::Assign if depth == 0 => assigned = true,
            Token::LParen => parens = parens.saturating_add(1),
            Token::RParen => parens = parens.saturating_sub(1),
            _ => {}
        }
    }
    // A parameter list is being declared inside parentheses between globals
    // or between a state's handlers; anywhere deeper they are a call's.
    let declaring = depth == 0 || (depth == 1 && in_state);
    let place = match (depth, in_state) {
        _ if declaring && parens > 0 => Place::Parameters,
        (0, _) if assigned => Place::Initializer,
        (0, _) => Place::Global,
        (1, true) => Place::Handlers,
        _ => Place::Body,
    };
    (place, item_start)
}

/// The `(type, name)` locals and parameters declared in `code[start..end]`.
fn locals<'source>(
    lexed: &Lexed<'source>,
    code: &[&SpannedToken],
    range: Range<usize>,
    syntax: &LslSyntax,
) -> Vec<(&'source str, &'source str)> {
    code.get(range)
        .unwrap_or_default()
        .windows(2)
        .filter_map(|pair| match pair {
            [kind, name]
                if kind.token == Token::Identifier
                    && name.token == Token::Identifier
                    && is_type_word(lexed.text(kind), syntax) =>
            {
                Some((lexed.text(kind), lexed.text(name)))
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The three answers.
// ---------------------------------------------------------------------------

/// What to offer at `caret` in `source`.
#[must_use]
pub fn completions(source: &str, caret: usize, syntax: &LslSyntax) -> Option<Completions> {
    let lexed = Lexed::new(source);
    if lexed.in_comment(caret) || lexed.in_string(caret) {
        return None;
    }
    let code = lexed.code();
    // The word the caret is in or at the end of, if any.
    let current = code
        .iter()
        .find(|token| token.span.start < caret && caret <= token.span.end);
    let replace = match current {
        Some(token) if token.token == Token::Identifier => token.span.clone(),
        // Inside an operator, or at the end of a number, completes nothing.
        Some(token)
            if caret < token.span.end
                || matches!(
                    token.token,
                    Token::IntegerLiteral | Token::FloatLiteral | Token::Error
                ) =>
        {
            return None;
        }
        _ => caret..caret,
    };
    let typed = source.get(replace.start..caret)?;
    let word_index = code_index(&code, &replace);
    let previous = word_index
        .checked_sub(1)
        .and_then(|at| code.get(at))
        .copied();

    let mut candidates: Vec<Completion> = Vec::new();
    let declared = declarations(&lexed, &code, syntax);
    if let Some(previous) = previous {
        match previous.token {
            Token::Dot => {
                candidates.extend(MEMBERS.iter().map(|member| Completion::word(*member, None)));
                return finish(typed, replace, candidates);
            }
            Token::Identifier => {
                let word = lexed.text(previous);
                if word == "state" {
                    candidates.extend(
                        declared
                            .iter()
                            .filter(|declaration| declaration.kind == OutlineKind::State)
                            .map(declaration_completion),
                    );
                    if !declared
                        .iter()
                        .any(|declaration| declaration.name == "default")
                    {
                        candidates.push(Completion::word("default", Some(Capture::Keyword)));
                    }
                    return finish(typed, replace, candidates);
                }
                if word == "jump" {
                    candidates.extend(
                        labels(&lexed, &code)
                            .into_iter()
                            .map(|label| Completion::word(label, Some(Capture::Label))),
                    );
                    return finish(typed, replace, candidates);
                }
                // A word after a word is a new name being declared (after a
                // type) or a mistake — either way nothing to offer — unless
                // the first is a keyword an expression follows.
                if !matches!(word, "return" | "else" | "do") {
                    return None;
                }
            }
            // After `@` a label is being declared; after a literal nothing
            // follows without an operator.
            Token::At | Token::IntegerLiteral | Token::FloatLiteral | Token::StringLiteral => {
                return None;
            }
            _ => {}
        }
    }

    let (place, item_start) = place_of(&lexed, &code, word_index);
    let types = || {
        TYPE_WORDS
            .iter()
            .map(|word| Completion::word(*word, Some(Capture::Type)))
    };
    match place {
        Place::Global => {
            candidates.extend(types());
            candidates.push(Completion::word("default", Some(Capture::Keyword)));
            candidates.push(Completion::word("state", Some(Capture::Keyword)));
        }
        Place::Parameters => candidates.extend(types()),
        Place::Initializer => {
            constant_candidates(typed, syntax, &mut candidates);
            candidates.extend(
                declared
                    .iter()
                    .filter(|declaration| declaration.kind == OutlineKind::Variable)
                    .map(declaration_completion),
            );
        }
        Place::Handlers => event_candidates(typed, syntax, &mut candidates),
        Place::Body => {
            function_candidates(typed, syntax, &mut candidates);
            constant_candidates(typed, syntax, &mut candidates);
            candidates.extend(types());
            candidates.extend(
                CONTROL_WORDS
                    .iter()
                    .copied()
                    .chain(syntax.controls.keys().map(String::as_str))
                    .map(|word| Completion::word(word, Some(Capture::Keyword))),
            );
            candidates.extend(
                declared
                    .iter()
                    .filter(|declaration| declaration.kind != OutlineKind::State)
                    .map(declaration_completion),
            );
            candidates.extend(
                locals(&lexed, &code, item_start..word_index, syntax)
                    .into_iter()
                    .map(|(kind, name)| Completion {
                        detail: Some(format!("{kind} {name}")),
                        ..Completion::word(name, None)
                    }),
            );
        }
    }
    finish(typed, replace, candidates)
}

/// The ranked answer, or `None` when nothing matches.
fn finish(typed: &str, replace: Range<usize>, candidates: Vec<Completion>) -> Option<Completions> {
    let items = rank(typed, candidates, COMPLETION_LIMIT);
    (!items.is_empty()).then_some(Completions { replace, items })
}

/// The library functions matching `typed`.
fn function_candidates(typed: &str, syntax: &LslSyntax, candidates: &mut Vec<Completion>) {
    for (name, function) in &syntax.functions {
        if sl_highlight::match_tier(typed, name).is_none() {
            continue;
        }
        candidates.push(Completion {
            label: name.clone(),
            insert: name.clone(),
            capture: Some(Capture::FunctionBuiltin),
            detail: Some(library_signature(name, function).label),
            documentation: function.tooltip.clone(),
            deprecated: function.deprecated,
        });
    }
}

/// The library constants matching `typed`.
fn constant_candidates(typed: &str, syntax: &LslSyntax, candidates: &mut Vec<Completion>) {
    for (name, constant) in &syntax.constants {
        if sl_highlight::match_tier(typed, name).is_none() {
            continue;
        }
        candidates.push(Completion {
            label: name.clone(),
            insert: name.clone(),
            capture: Some(Capture::ConstantBuiltin),
            detail: Some(constant_title(name, constant)),
            documentation: constant.tooltip.clone(),
            deprecated: constant.deprecated,
        });
    }
}

/// The events matching `typed`, each completed to its whole header.
fn event_candidates(typed: &str, syntax: &LslSyntax, candidates: &mut Vec<Completion>) {
    for (name, event) in &syntax.events {
        if sl_highlight::match_tier(typed, name).is_none() {
            continue;
        }
        let Some(signature) = event_signature(name, syntax) else {
            continue;
        };
        candidates.push(Completion {
            label: name.clone(),
            insert: signature.label.clone(),
            capture: Some(Capture::FunctionEvent),
            detail: Some(signature.label),
            documentation: event.tooltip.clone(),
            deprecated: event.deprecated,
        });
    }
}

/// A constant's one-line description: `integer TRUE = 1`.
fn constant_title(name: &str, constant: &crate::syntax::LslConstant) -> String {
    let mut title = String::new();
    if let Some(kind) = constant.constant_type {
        title.push_str(kind.keyword());
        title.push(' ');
    }
    title.push_str(name);
    if let Some(value) = &constant.value {
        title.push_str(" = ");
        title.push_str(value);
    }
    title
}

/// Whether the `<` at code index `index` opens a vector or rotation literal
/// rather than comparing: it is where an operand belongs.
fn opens_vector(lexed: &Lexed<'_>, code: &[&SpannedToken], index: usize) -> bool {
    let Some(before) = index.checked_sub(1).and_then(|at| code.get(at)) else {
        return true;
    };
    match before.token {
        Token::Identifier => lexed.text(before) == "return",
        Token::IntegerLiteral
        | Token::FloatLiteral
        | Token::StringLiteral
        | Token::RParen
        | Token::RBracket
        | Token::PlusPlus
        | Token::MinusMinus => false,
        _ => true,
    }
}

/// The argument the caret is in, counting the commas at the call's own level
/// from its `(` at code index `open` to code index `end`.
fn argument_index(lexed: &Lexed<'_>, code: &[&SpannedToken], open: usize, end: usize) -> usize {
    /// What a nesting is.
    #[derive(PartialEq, Eq)]
    enum Nest {
        /// `(` or `[`.
        Bracket,
        /// A vector's `<`.
        Vector,
    }
    let mut nesting: Vec<Nest> = Vec::new();
    let mut commas = 0_usize;
    for index in open.saturating_add(1)..end {
        let Some(token) = code.get(index) else {
            break;
        };
        match token.token {
            Token::LParen | Token::LBracket => nesting.push(Nest::Bracket),
            Token::RParen | Token::RBracket => {
                // A `>` that never came closes with its bracket.
                while nesting.pop() == Some(Nest::Vector) {}
            }
            Token::Less if opens_vector(lexed, code, index) => nesting.push(Nest::Vector),
            Token::Greater if nesting.last() == Some(&Nest::Vector) => {
                nesting.pop();
            }
            Token::Comma if nesting.is_empty() => commas = commas.saturating_add(1),
            _ => {}
        }
    }
    commas
}

/// The signature of the call whose parentheses `caret` is in.
#[must_use]
pub fn signature(source: &str, caret: usize, syntax: &LslSyntax) -> Option<Signature> {
    let lexed = Lexed::new(source);
    if lexed.in_comment(caret) {
        return None;
    }
    let code = lexed.code();
    // Every token that starts before the caret: a string the caret is inside
    // is an argument like any other.
    let end = code.partition_point(|token| token.span.start < caret);
    let mut nest = 0_usize;
    let mut open = None;
    for index in (0..end).rev() {
        let token = code.get(index)?;
        match token.token {
            Token::RParen | Token::RBracket => nest = nest.saturating_add(1),
            Token::LBracket => nest = nest.saturating_sub(1),
            Token::LParen if nest > 0 => nest = nest.saturating_sub(1),
            Token::LParen => {
                let name = index.checked_sub(1).and_then(|at| code.get(at)).copied();
                match name {
                    Some(name) if name.token == Token::Identifier => {
                        let word = lexed.text(name);
                        if is_control_word(word, syntax) {
                            return None;
                        }
                        open = Some((index, word));
                        break;
                    }
                    // A grouping or a cast: the call, if any, is further out.
                    _ => {}
                }
            }
            Token::Semicolon | Token::LBrace | Token::RBrace if nest == 0 => return None,
            _ => {}
        }
    }
    let (open, name) = open?;
    // The caret must be past the `(`, not on it.
    if code.get(open).is_some_and(|token| caret < token.span.end) {
        return None;
    }
    let active = argument_index(&lexed, &code, open, end);
    let mut found = if let Some(function) = syntax.function(name) {
        library_signature(name, function)
    } else if let Some(declared) = declarations(&lexed, &code, syntax)
        .into_iter()
        .find(|declaration| declaration.kind == OutlineKind::Function && declaration.name == name)
    {
        Signature {
            label: declared.detail,
            parameters: declared
                .parameters
                .into_iter()
                .map(|label| Parameter {
                    label,
                    documentation: None,
                })
                .collect(),
            active: None,
            documentation: None,
            notes: Vec::new(),
        }
    } else {
        event_signature(name, syntax)?
    };
    found.active = (active < found.parameters.len()).then_some(active);
    Some(found)
}

/// What the word at byte `index` of `source` is.
#[must_use]
pub fn hover(source: &str, index: usize, syntax: &LslSyntax) -> Option<Hover> {
    let lexed = Lexed::new(source);
    let code = lexed.code();
    let token = code
        .iter()
        .find(|token| token.span.start <= index && index < token.span.end)
        .filter(|token| token.token == Token::Identifier)?;
    let word = lexed.text(token);
    let range = token.span.clone();
    if let Some(function) = syntax.function(word) {
        let signature = library_signature(word, function);
        return Some(Hover {
            range,
            title: signature.label,
            documentation: signature.documentation,
            notes: signature.notes,
        });
    }
    if let Some(constant) = syntax.constant(word) {
        let mut notes = Vec::new();
        if constant.deprecated {
            notes.push(Note::Deprecated);
        }
        return Some(Hover {
            range,
            title: constant_title(word, constant),
            documentation: constant.tooltip.clone(),
            notes,
        });
    }
    if let Some(signature) = event_signature(word, syntax) {
        return Some(Hover {
            range,
            title: signature.label,
            documentation: signature.documentation,
            notes: signature.notes,
        });
    }
    if let Some(keyword) = syntax.controls.get(word).or_else(|| syntax.types.get(word))
        && keyword.tooltip.is_some()
    {
        return Some(Hover {
            range,
            title: String::from(word),
            documentation: keyword.tooltip.clone(),
            notes: Vec::new(),
        });
    }
    declarations(&lexed, &code, syntax)
        .into_iter()
        .find(|declaration| declaration.name == word && declaration.kind != OutlineKind::State)
        .map(|declaration| Hover {
            range,
            title: declaration.detail,
            documentation: None,
            notes: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::{completions, hover, signature};
    use crate::ast::TypeName;
    use crate::syntax::{LslArgument, LslConstant, LslEvent, LslFunction, LslSyntax};
    use pretty_assertions::assert_eq;
    use sl_highlight::Note;

    /// A boxed error so a test can use `?` instead of `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// An argument of `kind` named `name`.
    fn argument(kind: TypeName, name: &str) -> LslArgument {
        LslArgument {
            name: String::from(name),
            arg_type: Some(kind),
            tooltip: Some(format!("the {name}")),
        }
    }

    /// A small library: `llSay`, `llSetText` (which sleeps), `llRegionSay`,
    /// two constants and two events.
    fn syntax() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        syntax.functions.insert(
            String::from("llSay"),
            LslFunction {
                arguments: vec![
                    argument(TypeName::Integer, "channel"),
                    argument(TypeName::String, "msg"),
                ],
                tooltip: Some(String::from("Says msg on channel.")),
                ..LslFunction::default()
            },
        );
        syntax.functions.insert(
            String::from("llRegionSay"),
            LslFunction {
                arguments: vec![
                    argument(TypeName::Integer, "channel"),
                    argument(TypeName::String, "msg"),
                ],
                ..LslFunction::default()
            },
        );
        syntax.functions.insert(
            String::from("llSetText"),
            LslFunction {
                arguments: vec![
                    argument(TypeName::String, "text"),
                    argument(TypeName::Vector, "color"),
                    argument(TypeName::Float, "alpha"),
                ],
                sleep: Some(0.2),
                ..LslFunction::default()
            },
        );
        syntax.constants.insert(
            String::from("TRUE"),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some(String::from("1")),
                ..LslConstant::default()
            },
        );
        syntax.constants.insert(
            String::from("PUBLIC_CHANNEL"),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some(String::from("0")),
                ..LslConstant::default()
            },
        );
        syntax.events.insert(
            String::from("touch_start"),
            LslEvent {
                arguments: vec![argument(TypeName::Integer, "num_detected")],
                ..LslEvent::default()
            },
        );
        syntax
            .events
            .insert(String::from("state_entry"), LslEvent::default());
        syntax
    }

    /// The labels offered at the `|` in `marked`, and the replaced text.
    fn offered(marked: &str) -> Result<(Vec<String>, String), TestError> {
        let caret = marked.find('|').ok_or("no caret")?;
        let source = marked.replacen('|', "", 1);
        let Some(found) = completions(&source, caret, &syntax()) else {
            return Ok((Vec::new(), String::new()));
        };
        let replaced = source.get(found.replace).unwrap_or_default().to_owned();
        Ok((
            found.items.into_iter().map(|item| item.label).collect(),
            replaced,
        ))
    }

    /// In a body: library words, the script's own and the locals above.
    #[test]
    fn a_body_offers_the_library_and_the_script() -> Result<(), TestError> {
        let script = "integer count;\nsay(string m) { }\ndefault {\n    touch_start(integer n) {\n        string text = \"\";\n        llS|\n    }\n}\n";
        let (labels, replaced) = offered(script)?;
        assert_eq!(labels, vec!["llSay", "llSetText"]);
        assert_eq!(replaced, "llS");
        let (labels, _replaced) = offered(&script.replace("llS|", "s|"))?;
        assert_eq!(labels, vec!["say", "state", "string"]);
        // One letter matches a prefix; two match inside a word too.
        let (labels, _replaced) = offered(&script.replace("llS|", "te|"))?;
        assert_eq!(labels, vec!["text", "integer", "llSetText", "state"]);
        let (labels, _replaced) = offered(&script.replace("llS|", "n|"))?;
        assert_eq!(labels, vec!["n"]);
        let (labels, _replaced) = offered(&script.replace("llS|", "co|"))?;
        assert_eq!(labels, vec!["count"]);
        Ok(())
    }

    /// Between handlers the events, completed to their header; between
    /// globals the types and states; after `state` the states.
    #[test]
    fn the_place_decides() -> Result<(), TestError> {
        let (labels, _replaced) = offered("default {\n    touc|\n}")?;
        assert_eq!(labels, vec!["touch_start"]);
        let found = completions("default {\n    touc\n}", 18, &syntax()).ok_or("no events")?;
        assert_eq!(
            found.items.first().map(|item| item.insert.as_str()),
            Some("touch_start(integer num_detected)")
        );
        let (labels, _replaced) = offered("de|")?;
        assert_eq!(labels, vec!["default"]);
        let (labels, _replaced) = offered("integer x = TR|")?;
        assert_eq!(labels, vec!["TRUE"]);
        let (labels, _replaced) =
            offered("default { state_entry() { state o|; } }\nstate other { }")?;
        assert_eq!(labels, vec!["other"]);
        let (labels, _replaced) = offered("default { state_entry() { v.|; } }")?;
        assert_eq!(labels, vec!["s", "x", "y", "z"]);
        Ok(())
    }

    /// Nothing in a comment, a string, after a number, or where a name is
    /// being declared.
    #[test]
    fn nothing_where_nothing_belongs() -> Result<(), TestError> {
        for marked in [
            "default { state_entry() { // llS| } }",
            "default { state_entry() { llSay(0, \"llS|\"); } }",
            "default { state_entry() { integer ll|; } }",
            "default { state_entry() { x = 12|; } }",
            "/* llS|",
        ] {
            assert_eq!(offered(marked)?.0, Vec::<String>::new(), "{marked}");
        }
        Ok(())
    }

    /// The call's signature, its argument counted past lists and vectors.
    #[test]
    fn signatures_follow_the_argument() -> Result<(), TestError> {
        let at = |marked: &str| -> Result<Option<(String, Option<usize>)>, TestError> {
            let caret = marked.find('|').ok_or("no caret")?;
            let source = marked.replacen('|', "", 1);
            Ok(signature(&source, caret, &syntax()).map(|found| (found.label, found.active)))
        };
        assert_eq!(
            at("x() { llSay(|")?,
            Some((String::from("llSay(integer channel, string msg)"), Some(0)))
        );
        assert_eq!(
            at("x() { llSay(0, \"a, b|\"")?,
            Some((String::from("llSay(integer channel, string msg)"), Some(1)))
        );
        assert_eq!(
            at("x() { llSetText(\"t\", <1, 0, 0>, |")?.and_then(|(_label, active)| active),
            Some(2)
        );
        assert_eq!(
            at("x() { llSay(llList2Integer([1, 2], 0), |")?.and_then(|(_label, active)| active),
            Some(1)
        );
        assert_eq!(at("x() { llSay(0, \"a\"); |")?, None);
        assert_eq!(at("x() { if (|")?, None);
        // The script's own function.
        assert_eq!(
            at("say(string m, integer n) { }\nx() { say(\"a\", |")?,
            Some((String::from("say(string m, integer n)"), Some(1)))
        );
        let found = signature("x() { llSetText(", 16, &syntax()).ok_or("no signature")?;
        assert_eq!(found.notes, vec![Note::Sleep(0.2)]);
        assert_eq!(
            found
                .active_range()
                .and_then(|range| found.label.get(range)),
            Some("string text")
        );
        Ok(())
    }

    /// The pointer over a library word, or a declaration of the script's own.
    #[test]
    fn hovering() {
        let source =
            "integer count;\ndefault { state_entry() { llSay(PUBLIC_CHANNEL, (string)count); } }";
        let title = |word: &str| {
            source
                .find(word)
                .and_then(|at| hover(source, at.saturating_add(1), &syntax()))
                .map(|found| found.title)
        };
        assert_eq!(
            title("llSay"),
            Some(String::from("llSay(integer channel, string msg)"))
        );
        assert_eq!(
            title("PUBLIC_CHANNEL"),
            Some(String::from("integer PUBLIC_CHANNEL = 0"))
        );
        assert_eq!(title("count"), Some(String::from("integer count")));
        assert_eq!(title("state_entry"), Some(String::from("state_entry()")));
        assert_eq!(title("default"), None);
    }
}
