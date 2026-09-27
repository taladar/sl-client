//! The **compiler**: LSL source to a [`Program`], or the compile errors a
//! grid would answer an upload with.
//!
//! [`compile`] runs three stages and stops at the first that fails:
//!
//! 1. **Parse** (`sl_lsl::parse`). The grid's parser stops at its first
//!    syntax error, so only the first is reported.
//! 2. **The semantic pass** (`sl_lsl::analyze`), against the library table
//!    this crate runs ([`crate::library::lsl_syntax`]). Its errors are
//!    reliable — it is held to a no-false-positive bar against tailslide —
//!    but it is conservative by design and lets through code a grid rejects.
//! 3. **The lowering** (`lower`), which cannot be conservative: to emit an
//!    instruction it has to know every expression's type and every name's
//!    meaning, so it enforces the rest of the grid's rules itself — operator
//!    and assignment typing, void values, lists in lists, members, casts,
//!    declarations that need a scope, constant global initialisers, the
//!    namespace rules, `state` in a function, and "not all code paths return
//!    a value". A construct the pass allowed and the lowering cannot
//!    represent is a compile error here, never a panic.
//!
//! The rules follow tailslide (`libtailslide/passes/`), which reproduces
//! Linden's compiler, and the messages are Linden's own, as LSL PyOptimizer
//! quotes them (`lslopt/lslparse.py`'s `EParse*` classes).

mod literal;
mod lower;
#[cfg(test)]
mod tests;

use core::fmt;
use core::ops::Range;

use sl_lsl::{DiagnosticKind, Severity};

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
    /// Linden's message for this error.
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

    /// The grid's message for one of the semantic pass's findings, or
    /// [`None`] for the ones the lowering reports better or the grid does
    /// not reject as the pass states them: an unreachable state and a label
    /// repeated in one function compile; the pass's "may reach its end
    /// without returning" is a cautious reading of a rule the lowering
    /// applies exactly; and a missing `default` state, which the pass pins
    /// to the start of the script, the lowering reports at the first state
    /// header, where the grid's grammar fails.
    #[must_use]
    pub const fn from_diagnostic(kind: &DiagnosticKind) -> Option<Self> {
        Some(match kind {
            DiagnosticKind::UndefinedFunction { .. }
            | DiagnosticKind::UndefinedVariable { .. }
            | DiagnosticKind::UndefinedState { .. }
            | DiagnosticKind::UndefinedLabel { .. } => Self::Undefined,
            DiagnosticKind::UnknownEvent { .. }
            | DiagnosticKind::WrongEventArgCount { .. }
            | DiagnosticKind::EventArgTypeMismatch { .. }
            | DiagnosticKind::AssignToConstant { .. } => Self::Syntax,
            DiagnosticKind::WrongArgCount { .. } | DiagnosticKind::ArgTypeMismatch { .. } => {
                Self::FunctionMismatch
            }
            DiagnosticKind::ReturnValueInVoid => Self::ReturnValueInVoid,
            DiagnosticKind::MissingReturnValue { .. } => Self::ReturnWithoutValue,
            DiagnosticKind::ReturnTypeMismatch { .. } => Self::TypeMismatch,
            DiagnosticKind::DuplicateFunction { .. }
            | DiagnosticKind::DuplicateGlobal { .. }
            | DiagnosticKind::DuplicateState { .. }
            | DiagnosticKind::DuplicateParam { .. }
            | DiagnosticKind::DuplicateEvent { .. }
            | DiagnosticKind::DuplicateLocal { .. } => Self::AlreadyDefined,
            DiagnosticKind::MissingDefaultState
            | DiagnosticKind::UnreachableState { .. }
            | DiagnosticKind::DuplicateLabel { .. }
            | DiagnosticKind::MissingReturn { .. } => return None,
        })
    }
}

/// One compile error: what, and where.
///
/// `Display` renders it the way Second Life sends it in an upload's error
/// list — `(line, column): ERROR : message` — with the line and column
/// counted **from zero** (measured on aditi, 2026-09-27: a function whose
/// closing brace is on line 5, column 1 is `(4, 0): ERROR : Not all code
/// paths return a value`; the reference viewer passes the numbers straight to
/// a zero-based `setCursor`, and OpenSim subtracts one from its own for the
/// same reason). [`Self::position`] is the one-based position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// Which error.
    pub kind: CompileErrorKind,
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
            self.kind.message()
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
/// of the stage that found any.
pub fn compile(source: &str) -> Result<Program, Vec<CompileError>> {
    let text = Source::new(source);
    let error = |kind, span: Range<usize>| CompileError {
        kind,
        position: text.position(span.start),
        span,
    };

    let parsed = sl_lsl::parse(source);
    if let Some(first) = parsed.errors.first() {
        return Err(vec![error(CompileErrorKind::Syntax, first.span.clone())]);
    }

    let syntax = crate::library::lsl_syntax();
    let rejected: Vec<CompileError> = sl_lsl::analyze(&parsed.script, &syntax)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .filter_map(|diagnostic| {
            CompileErrorKind::from_diagnostic(&diagnostic.kind)
                .map(|kind| error(kind, diagnostic.span))
        })
        .collect();
    if !rejected.is_empty() {
        return Err(rejected);
    }

    lower::lower(&parsed.script, text)
}
