//! Run-time errors: what stops a script.

use crate::bytecode::Position;
use crate::error::{Division, ValueError};
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
    /// — the reference's `Math Error`, with the kind of division, which
    /// OpenSim's engines name.
    #[error("Math Error")]
    MathError(Division),
    /// The body is one Mono refuses to load (the value of `integer *=
    /// float` used), so it fails before its first line, as on the grid.
    #[error("System.InvalidProgramException: Invalid IL code")]
    InvalidProgram,
    /// Mono's script ran out of memory: its heap and its stack together past
    /// its limit, or a call stack past what any script could hold.
    #[error("Stack-Heap Collision")]
    StackHeapCollision,
    /// YEngine's script stored more than its heap holds
    /// (`XMRInstAbstract`'s `OutOfHeapException`), with the engine's own
    /// three numbers: what it held before, after, and the heap.
    #[error("OutOfHeapException: oldtotal={old_total}, newtotal={new_total}, limit={limit}")]
    OutOfHeap {
        /// The engine's `oldtotal`: `0` for a global, the locals' total before
        /// the store for a local.
        old_total: u32,
        /// The engine's `newtotal`: a global's growth, or the locals' total
        /// after the store.
        new_total: u32,
        /// The heap, `ScriptHeapSize`.
        limit: u32,
    },
    /// The script held more than this host allows a script where its grid
    /// sets no limit (XEngine; a value YEngine has not yet stored). This
    /// runtime's own backstop, not a grid's error.
    #[error("script memory past this host's limit of 64 MiB")]
    HostMemory,
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
            ValueError::MathError(division) => Self::MathError(division),
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
    /// map has one. No grid tells a resident the line, so the error lines
    /// leave it out; it is here for the host's own log.
    pub position: Option<Position>,
    /// The event whose handler was running — `link_message`, `state_entry`
    /// — or [`None`] for the global initialisers. OpenSim's error line
    /// names it.
    pub event: Option<&'static str>,
}
