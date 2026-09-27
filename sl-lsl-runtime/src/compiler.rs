//! The **compiler**: LSL source to a [`Program`], or the compile errors a
//! grid would answer an upload with.
//!
//! [`compile`] runs two stages and stops at the first that fails:
//!
//! 1. **Parse** (`sl_lsl::parse`). The grid's parser stops at its first
//!    syntax error, so only the first is reported.
//! 2. **The lowering** (`lower`). To emit an instruction it has to know every
//!    expression's type and every name's meaning, so it enforces the grid's
//!    rules itself — names and scopes, call arity and argument types,
//!    `return`s, operator and assignment typing, void values, lists in lists,
//!    members, casts, declarations that need a scope, constant global
//!    initialisers, event signatures, the state layout, `state` in a
//!    function, and "not all code paths return a value". A construct it
//!    cannot represent is a compile error, never a panic.
//!
//! `sl_lsl::analyze`, the editor's semantic pass, is not a stage: it is
//! deliberately conservative (it must never flag code the grid accepts), and
//! everything it reports the lowering reports too, with the scopes at hand
//! for better suggestions.
//!
//! The rules follow tailslide (`libtailslide/passes/`), which reproduces
//! Linden's compiler: a script is rejected exactly when, and where, the grid
//! rejects it. Each error has the grid's [`CompileErrorKind`] — Linden's own
//! message, as LSL PyOptimizer quotes it (`lslopt/lslparse.py`'s `EParse*`
//! classes) — but says more than that terse text: the name that is not
//! defined and the near names it may be a typo of, the types expected and
//! found, the operator and its operands, the signature of the function
//! called.

mod literal;
mod lower;
#[cfg(test)]
mod tests;

use core::fmt;
use core::ops::Range;

use crate::bytecode::{Position, Program, Source};

/// What a compile error says, as one of the messages Linden's compiler uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompileErrorKind {
    /// The source does not parse — including every mistake Linden's grammar
    /// catches rather than its checker: an unknown event or a wrong event
    /// signature, a missing or misplaced `default` state, a state with no
    /// handlers, a constant or event name used as a variable, an assignment
    /// to something that is not a variable, a global initialiser that is not
    /// a constant.
    Syntax,
    /// A name already declared in the same scope, or a global named like a
    /// library function.
    AlreadyDefined,
    /// A name that resolves to nothing of the kind it is used as.
    Undefined,
    /// An operator, assignment, condition, cast or constructor on types it
    /// does not accept, or a void value where a value is needed.
    TypeMismatch,
    /// `return value;` where no value is returned.
    ReturnValueInVoid,
    /// `return;` in a function that returns a value.
    ReturnWithoutValue,
    /// `.x` / `.y` / `.z` / `.s` on something that is not a vector or
    /// rotation variable, or a component the type does not have.
    InvalidMember,
    /// A call with the wrong number or types of arguments.
    FunctionMismatch,
    /// A declaration as the unbraced body of an `if`, `else` or loop.
    DeclarationNeedsScope,
    /// `state name;` in a function, outside an `if`.
    StateChangeInFunction,
    /// A function with a return type whose body can end without a `return`.
    MissingReturn,
    /// A list element that is itself a list.
    ListInList,
    /// The program does not fit the bytecode's indices.
    TooLarge,
}

impl CompileErrorKind {
    /// Linden's own message for this error — what Second Life says in the
    /// same situation.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Syntax => "Syntax error",
            Self::AlreadyDefined => "Name previously declared within scope",
            Self::Undefined => "Name not defined within scope",
            Self::TypeMismatch => "Type mismatch",
            Self::ReturnValueInVoid => "Return statement type doesn't match function return type",
            Self::ReturnWithoutValue => "Function returns a value but return statement doesn't",
            Self::InvalidMember => "Use of vector or quaternion method on incorrect type",
            Self::FunctionMismatch => "Function call mismatches type or number of arguments",
            Self::DeclarationNeedsScope => "Declaration requires a new scope -- use { and }",
            Self::StateChangeInFunction => "Global functions can't change state",
            Self::MissingReturn => "Not all code paths return a value",
            Self::ListInList => "Lists can't be included in lists",
            Self::TooLarge => "Byte code assembly failed -- out of memory",
        }
    }
}

/// One compile error: what, and where.
///
/// `Display` renders it in the shape Second Life sends in an upload's error
/// list — `(line, column): ERROR : message`, which the viewer parses — with
/// [`Self::message`] in place of Linden's terser text, and the line and
/// column counted **from zero** (measured on aditi, 2026-09-27: a function whose
/// closing brace is on line 5, column 1 is `(4, 0): ERROR : Not all code
/// paths return a value`; the reference viewer passes the numbers straight to
/// a zero-based `setCursor`, and OpenSim subtracts one from its own for the
/// same reason). [`Self::position`] is the one-based position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// Which error, as the grid classifies it.
    pub kind: CompileErrorKind,
    /// What exactly is wrong, naming the symbols and types involved.
    pub message: String,
    /// The byte span of the source it points at.
    pub span: Range<usize>,
    /// Where that span starts, one-based.
    pub position: Position,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}): ERROR : {}",
            self.position.line.saturating_sub(1),
            self.position.column.saturating_sub(1),
            self.message
        )
    }
}

impl core::error::Error for CompileError {}

/// Compile LSL `source`.
///
/// # Errors
///
/// The compile errors, in source order, when the script does not compile:
/// only the first syntax error, as the grid reports; otherwise every error
/// the lowering found.
pub fn compile(source: &str) -> Result<Program, Vec<CompileError>> {
    let text = Source::new(source);
    let error = |kind, span: Range<usize>, message: String| CompileError {
        kind,
        message,
        position: text.position(span.start),
        span,
    };

    let parsed = sl_lsl::parse(source);
    if let Some(first) = parsed.errors.first() {
        return Err(vec![error(
            CompileErrorKind::Syntax,
            first.span.clone(),
            format!("syntax error: {}", first.message),
        )]);
    }

    lower::lower(&parsed.script, text)
}
