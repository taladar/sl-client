//! What the VM's operand stack holds: a [`Value`], or a float that has not
//! been rounded to a float yet.
//!
//! Second Life's Mono evaluates an expression of floats in **double** and
//! rounds to single precision only where the value leaves the expression.
//! `math/float-rounding` on aditi (book `gridspec/lsl.md`, § Precision) found
//! where that is, one point at a time:
//!
//! - **kept in double**: `+`, `-`, `*` and `/` of floats, unary `-`, the dot
//!   product (`vector * vector`), an `integer` converted to `float` (exact:
//!   `(float)16777217` is 16777217 until it is stored), a `(float)` cast of a
//!   float, the value of an assignment, a compound assignment, `++` and `--`,
//!   and so the operands of a comparison and the value of a condition;
//! - **rounded**: a store into a variable or a component, an argument to a
//!   user or library function, a function's return value, a list element, a
//!   component of a vector or rotation being built, the factor of `vector *
//!   float`, and the operand of an `(integer)` or `(string)` cast.
//!
//! So [`Operand::Double`] is produced only by those first operations, and
//! every consumer that is not one of them takes the operand through
//! [`Operand::round`]. `Value::Float` stays an `f32`: whatever is stored is a
//! float.
//!
//! OpenSim keeps every float a double and so agrees with the double
//! evaluation; only its stored values differ, which this crate's value model
//! does not follow.

use sl_lsl::ast::{BinaryOp, PrefixOp, TypeName};

use crate::error::{Division, ValueError};
use crate::num::to_f32;
use crate::ops::dot_unrounded;
use crate::value::Value;

/// One entry of the operand stack.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Operand {
    /// A value of any type, floats already rounded.
    Value(Value),
    /// A `float` still in double precision.
    Double(f64),
}

impl From<Value> for Operand {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

impl Operand {
    /// The operand as it is stored, passed or put in a list: a double rounded
    /// to the nearest float.
    pub(crate) fn round(self) -> Value {
        match self {
            Self::Value(value) => value,
            Self::Double(double) => Value::Float(to_f32(double)),
        }
    }

    /// A float operand, rounded or not, as a double.
    fn double(&self) -> Option<f64> {
        match self {
            Self::Value(Value::Float(float)) => Some(f64::from(*float)),
            Self::Double(double) => Some(*double),
            Self::Value(_) => None,
        }
    }

    /// Whether a condition takes the operand as true. A double is tested
    /// before it is rounded, so `1e-30 * 1e-20` is true (`math/float-rounding`'s
    /// `condition`) though it rounds to `0.0`.
    pub(crate) fn is_true(&self) -> bool {
        match self {
            Self::Value(value) => value.is_true(),
            Self::Double(double) => *double != 0.0,
        }
    }
}

/// `left op right` on the operand stack: two floats in double, the dot
/// product left unrounded, and everything else on rounded values by
/// [`crate::binary`].
///
/// # Errors
///
/// As [`crate::binary`]: a float division by zero, or one that yields NaN,
/// is a [`ValueError::MathError`].
pub(crate) fn binary(op: BinaryOp, left: Operand, right: Operand) -> Result<Operand, ValueError> {
    if let (Some(left), Some(right)) = (left.double(), right.double()) {
        return double_binary(op, left, right);
    }
    if let (BinaryOp::Mul, Operand::Value(Value::Vector(a)), Operand::Value(Value::Vector(b))) =
        (op, &left, &right)
    {
        return Ok(Operand::Double(dot_unrounded(a, b)));
    }
    crate::binary(op, left.round(), right.round()).map(Operand::Value)
}

/// A boolean as LSL's `TRUE` / `FALSE`.
const fn truth(value: bool) -> Operand {
    Operand::Value(Value::Integer(if value { 1 } else { 0 }))
}

/// `left op right` for two floats, in double: the rules of
/// [`crate::binary`] for floats, without its rounding.
#[expect(
    clippy::float_cmp,
    reason = "LSL float equality is exact IEEE equality"
)]
fn double_binary(op: BinaryOp, left: f64, right: f64) -> Result<Operand, ValueError> {
    Ok(match op {
        BinaryOp::Add => Operand::Double(left + right),
        BinaryOp::Sub => Operand::Double(left - right),
        BinaryOp::Mul => Operand::Double(double_mul(left, right)),
        BinaryOp::Div => {
            let quotient = left / right;
            if right == 0.0 || quotient.is_nan() {
                return Err(ValueError::MathError(Division::Float));
            }
            Operand::Double(quotient)
        }
        BinaryOp::Eq => truth(left == right),
        BinaryOp::Ne => truth(left != right),
        BinaryOp::Lt => truth(left < right),
        BinaryOp::Gt => truth(right < left),
        // `a <= b` is `!(b < a)`, so true with a NaN operand, as
        // `crate::binary` has it.
        BinaryOp::Le => {
            let greater = right < left;
            truth(!greater)
        }
        BinaryOp::Ge => {
            let less = left < right;
            truth(!less)
        }
        BinaryOp::Mod
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::And
        | BinaryOp::Or => {
            return Err(ValueError::Binary {
                op,
                left: TypeName::Float,
                right: TypeName::Float,
            });
        }
    })
}

/// `float * float` in double, with [`crate::binary`]'s NaN sign: negative
/// only when both NaN operands were.
fn double_mul(left: f64, right: f64) -> f64 {
    if left.is_nan() && right.is_nan() {
        if left.is_sign_negative() && right.is_sign_negative() {
            -f64::NAN
        } else {
            f64::NAN
        }
    } else {
        left * right
    }
}

/// `op operand` on the operand stack: `-` of a double stays a double, and a
/// float's `++` or `--` yields its unrounded sum (`math/float-rounding`'s
/// `increment-value`: `++` of 16777216.0 is 16777217 until it is stored).
///
/// # Errors
///
/// As [`crate::prefix`].
pub(crate) fn prefix(op: PrefixOp, operand: Operand) -> Result<Operand, ValueError> {
    match (op, operand) {
        (PrefixOp::Neg, Operand::Double(double)) => Ok(Operand::Double(-double)),
        (PrefixOp::PreInc, Operand::Value(Value::Float(float))) => {
            Ok(Operand::Double(f64::from(float) + 1.0))
        }
        (PrefixOp::PreDec, Operand::Value(Value::Float(float))) => {
            Ok(Operand::Double(f64::from(float) - 1.0))
        }
        (PrefixOp::PreInc, Operand::Double(double)) => Ok(Operand::Double(double + 1.0)),
        (PrefixOp::PreDec, Operand::Double(double)) => Ok(Operand::Double(double - 1.0)),
        (op, operand) => crate::prefix(op, operand.round()).map(Operand::Value),
    }
}

/// `(to)operand` on the operand stack: a cast to `float` keeps a double and
/// makes an integer one exactly; every other cast takes the rounded value
/// (`math/float-rounding`'s `cast-float`, `integer-cast`, `cast-integer` and
/// `cast-string`).
///
/// # Errors
///
/// As [`crate::cast()`].
pub(crate) fn cast(operand: Operand, to: TypeName) -> Result<Operand, ValueError> {
    match (operand, to) {
        (Operand::Double(double), TypeName::Float) => Ok(Operand::Double(double)),
        (Operand::Value(Value::Integer(integer)), TypeName::Float) => {
            Ok(Operand::Double(f64::from(integer)))
        }
        (operand, to) => crate::cast(operand.round(), to).map(Operand::Value),
    }
}
