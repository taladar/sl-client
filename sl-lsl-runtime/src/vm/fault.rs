//! Run-time errors: what stops a script.

use crate::bytecode::Position;
use crate::error::ValueError;
use crate::library::{BuiltinId, CallError};

/// Why a script stopped with an error.
///
/// A run-time error stops the one script it happens in and nothing else: the
/// VM never panics, whatever the program does. The first two are the
/// reference's own errors; the others are this runtime's, and name what is
/// missing or wrong rather than pretend to be one of Second Life's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    /// Division or modulo by zero, or a float quotient that is not a number
    /// — the reference's `Math Error`.
    #[error("Math Error")]
    MathError,
    /// The body is one Mono refuses to load (the value of `integer *=
    /// float` used), so it fails before its first line, as on the grid.
    #[error("System.InvalidProgramException: Invalid IL code")]
    InvalidProgram,
    /// The call stack grew past what any script's memory could hold — a
    /// runaway recursion. The reference calls this a stack-heap collision.
    #[error("Stack-Heap Collision")]
    StackHeapCollision,
    /// A library function this runtime has neither implemented nor stubbed.
    #[error("{} is not implemented by this grid", .0.descriptor().name)]
    Unimplemented(BuiltinId),
    /// The program broke a promise the compiler makes to the VM — a missing
    /// operand, an out-of-range slot, a value of the wrong type. It means a
    /// lowering bug, and the message says which promise.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<ValueError> for RuntimeError {
    fn from(error: ValueError) -> Self {
        match error {
            ValueError::MathError => Self::MathError,
            other @ (ValueError::Cast { .. }
            | ValueError::Binary { .. }
            | ValueError::Prefix { .. }) => Self::Internal(other.to_string()),
        }
    }
}

impl From<CallError> for RuntimeError {
    fn from(error: CallError) -> Self {
        match error {
            CallError::Missing(id) => Self::Unimplemented(id),
            CallError::Value(value) => value.into(),
            other @ (CallError::Arity { .. } | CallError::Argument { .. }) => {
                Self::Internal(other.to_string())
            }
        }
    }
}

/// A run-time error with where it happened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct Fault {
    /// What went wrong.
    pub error: RuntimeError,
    /// The source position of the instruction that failed, when the source
    /// map has one.
    pub position: Option<Position>,
}
