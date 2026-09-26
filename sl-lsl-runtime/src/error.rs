//! What an operation on values can fail with.

use sl_lsl::ast::{BinaryOp, PrefixOp, TypeName};

/// A failed operation on values.
///
/// Only [`MathError`](Self::MathError) can happen to a script that compiled:
/// it is LSL's run-time `Math Error`, which stops the script. The other
/// variants are operand combinations the compiler rejects
/// (`sl_lsl::types`), so reaching one means the lowering emitted an
/// instruction it should not have.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `ValueError` is the name that reads"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    /// Division or modulo by zero, or a float division whose result is not a
    /// number — the reference's `Math Error`.
    #[error("Math Error")]
    MathError,
    /// A cast the compiler rejects, such as `(integer)` of a key.
    #[error("cannot cast {from:?} to {to:?}")]
    Cast {
        /// The value's type.
        from: TypeName,
        /// The type cast to.
        to: TypeName,
    },
    /// A binary operator applied to operand types it is not defined for.
    #[error("no operator {op:?} for {left:?} and {right:?}")]
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand's type.
        left: TypeName,
        /// The right operand's type.
        right: TypeName,
    },
    /// A prefix operator applied to an operand type it is not defined for.
    #[error("no operator {op:?} for {operand:?}")]
    Prefix {
        /// The operator.
        op: PrefixOp,
        /// The operand's type.
        operand: TypeName,
    },
}
