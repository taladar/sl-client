//! LSL's operators on values.
//!
//! Integers **wrap** at 32 bits and floats are **`f32`**; a mixed
//! `integer op float` promotes the integer first. Vector and rotation
//! operators are free functions here, never `impl Mul` on the `sl-types`
//! structs, because rotation composition is in Linden's order — the reverse
//! of the textbook (and glam's) product — and that should be spelled out
//! where it happens.
//!
//! What these functions do not do is decide *order of evaluation*: both
//! operands arrive already evaluated, which is also how LSL behaves — `&&`
//! and `||` never short-circuit (see `sl_lsl::types`).

use sl_lsl::ast::{BinaryOp, PrefixOp};
use sl_types::lsl::{Rotation, Vector};

use crate::error::ValueError;
use crate::num::{int_to_float, to_f32};
use crate::value::{Value, rotation_eq, vector_eq};

/// `integer` or `float`, for the operators that promote.
#[derive(Clone, Copy)]
enum Number {
    /// An integer operand.
    Integer(i32),
    /// A float operand.
    Float(f32),
}

impl Number {
    /// The operand as a float — the promotion of a mixed operation.
    const fn to_float(self) -> f32 {
        match self {
            Self::Integer(integer) => int_to_float(integer),
            Self::Float(float) => float,
        }
    }
}

/// A value as a [`Number`], if it is one.
const fn number(value: &Value) -> Option<Number> {
    match value {
        Value::Integer(integer) => Some(Number::Integer(*integer)),
        Value::Float(float) => Some(Number::Float(*float)),
        Value::String(_)
        | Value::Key(_)
        | Value::Vector(_)
        | Value::Rotation(_)
        | Value::List(_) => None,
    }
}

/// A boolean as LSL's `TRUE` / `FALSE`.
const fn truth(value: bool) -> Value {
    Value::Integer(if value { 1 } else { 0 })
}

/// `left op right`.
///
/// The operand types accepted are exactly `sl_lsl::types::binary_result`'s,
/// and the result has the type that table states. The run-time behaviour
/// worth knowing:
///
/// - `integer / 0`, `integer % 0`, `float / 0.0`, `vector / 0.0` and a float
///   division that yields NaN (`inf / inf`) are a [`ValueError::MathError`] —
///   **float** division by zero included, in Second Life's Mono;
///   `-2147483648 / -1` is `-2147483648`.
/// - `%` truncates like C: the result takes the dividend's sign.
/// - `<<` and `>>` use the shift count modulo 32; `>>` is arithmetic.
/// - `==` of two lists compares their **lengths**, and `!=` of two lists is
///   `length(left) - length(right)` — an integer that may be negative.
///   `!=` of strings or keys is `1` or `0`.
/// - `a <= b` is `!(b < a)` and `a >= b` is `!(a < b)`, so with a NaN
///   operand `<` and `>` are false while `<=` and `>=` are **true**.
/// - `&&` and `||` are `integer` only and yield `0` or `1`.
///
/// # Errors
///
/// [`ValueError::MathError`] as above; [`ValueError::Binary`] for operand
/// types the compiler rejects.
pub fn binary(op: BinaryOp, left: Value, right: Value) -> Result<Value, ValueError> {
    let mismatch = ValueError::Binary {
        op,
        left: left.type_name(),
        right: right.type_name(),
    };
    let result = match op {
        BinaryOp::Add => add(left, right),
        BinaryOp::Sub => sub(&left, &right),
        BinaryOp::Mul => mul(&left, &right),
        BinaryOp::Div => return div(&left, &right).unwrap_or(Err(mismatch)),
        BinaryOp::Mod => return modulo(&left, &right).unwrap_or(Err(mismatch)),
        BinaryOp::Eq => equals(&left, &right).map(truth),
        BinaryOp::Ne => not_equals(&left, &right),
        BinaryOp::Lt => less(&left, &right).map(truth),
        BinaryOp::Gt => less(&right, &left).map(truth),
        BinaryOp::Le => less(&right, &left).map(|less| truth(!less)),
        BinaryOp::Ge => less(&left, &right).map(|less| truth(!less)),
        BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::And
        | BinaryOp::Or => match (&left, &right) {
            (Value::Integer(left), Value::Integer(right)) => {
                Some(Value::Integer(integer_op(op, *left, *right)))
            }
            _ => None,
        },
    };
    result.ok_or(mismatch)
}

/// The integer-only binary operators.
const fn integer_op(op: BinaryOp, left: i32, right: i32) -> i32 {
    // The shift count is taken modulo 32 (PyOptimizer `op2 & 31`), which is
    // exactly what `wrapping_shl`/`wrapping_shr` do with the count's bits.
    let count = right.cast_unsigned();
    match op {
        BinaryOp::Shl => left.wrapping_shl(count),
        BinaryOp::Shr => left.wrapping_shr(count),
        BinaryOp::BitAnd => left & right,
        BinaryOp::BitOr => left | right,
        BinaryOp::BitXor => left ^ right,
        BinaryOp::And => {
            if left != 0 && right != 0 {
                1
            } else {
                0
            }
        }
        BinaryOp::Or => {
            if left != 0 || right != 0 {
                1
            } else {
                0
            }
        }
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Mod
        | BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge => 0,
    }
}

/// `+`: list append/prepend, string concatenation, then the component-wise
/// sums.
fn add(left: Value, right: Value) -> Option<Value> {
    match (left, right) {
        (Value::List(mut elements), right) => {
            elements.extend(right.into_elements());
            Some(Value::List(elements))
        }
        (left, Value::List(elements)) => {
            let mut joined = left.into_elements();
            joined.extend(elements);
            Some(Value::List(joined))
        }
        (Value::String(mut left), Value::String(right)) => {
            left.push_str(&right);
            Some(Value::String(left))
        }
        (left, right) => componentwise(&left, &right, i32::wrapping_add, |a, b| a + b),
    }
}

/// `-`: the component-wise differences.
fn sub(left: &Value, right: &Value) -> Option<Value> {
    componentwise(left, right, i32::wrapping_sub, |a, b| a - b)
}

/// A `+`/`-`-shaped operator over two numbers (promoting a mixed pair), two
/// vectors or two rotations, component by component.
fn componentwise(
    left: &Value,
    right: &Value,
    integer: fn(i32, i32) -> i32,
    float: fn(f32, f32) -> f32,
) -> Option<Value> {
    match (left, right) {
        (Value::Vector(a), Value::Vector(b)) => Some(Value::Vector(Vector {
            x: float(a.x, b.x),
            y: float(a.y, b.y),
            z: float(a.z, b.z),
        })),
        (Value::Rotation(a), Value::Rotation(b)) => Some(Value::Rotation(Rotation {
            x: float(a.x, b.x),
            y: float(a.y, b.y),
            z: float(a.z, b.z),
            s: float(a.s, b.s),
        })),
        _ => match (number(left)?, number(right)?) {
            (Number::Integer(a), Number::Integer(b)) => Some(Value::Integer(integer(a, b))),
            (a, b) => Some(Value::Float(float(a.to_float(), b.to_float()))),
        },
    }
}

/// `float * float`. Two NaN operands give a NaN that is negative only when
/// both were (PyOptimizer's `mul`), rather than whichever sign the hardware
/// happens to propagate.
fn float_mul(left: f32, right: f32) -> f32 {
    if left.is_nan() && right.is_nan() {
        if left.is_sign_negative() && right.is_sign_negative() {
            -f32::NAN
        } else {
            f32::NAN
        }
    } else {
        left * right
    }
}

/// A vector scaled component-wise by `factor` through `op`.
fn scale(vector: &Vector, factor: f32, op: fn(f32, f32) -> f32) -> Vector {
    Vector {
        x: op(vector.x, factor),
        y: op(vector.y, factor),
        z: op(vector.z, factor),
    }
}

/// `*`: numbers, vector scaling, the dot product, rotating a vector and
/// composing rotations.
fn mul(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Vector(vector), scalar) | (scalar, Value::Vector(vector))
            if number(scalar).is_some() =>
        {
            let factor = number(scalar)?.to_float();
            Some(Value::Vector(scale(vector, factor, float_mul)))
        }
        (Value::Vector(a), Value::Vector(b)) => Some(Value::Float(dot(a, b))),
        (Value::Vector(vector), Value::Rotation(rotation)) => {
            Some(Value::Vector(rotate(vector, rotation)))
        }
        (Value::Rotation(a), Value::Rotation(b)) => Some(Value::Rotation(compose(a, b))),
        _ => match (number(left)?, number(right)?) {
            (Number::Integer(a), Number::Integer(b)) => Some(Value::Integer(a.wrapping_mul(b))),
            (a, b) => Some(Value::Float(float_mul(a.to_float(), b.to_float()))),
        },
    }
}

/// `/`. The outer `Option` is "defined for these types", the inner `Result`
/// the run-time `Math Error`.
fn div(left: &Value, right: &Value) -> Option<Result<Value, ValueError>> {
    match (left, right) {
        (Value::Vector(vector), Value::Rotation(rotation)) => {
            Some(Ok(Value::Vector(rotate(vector, &conjugate(rotation)))))
        }
        (Value::Rotation(a), Value::Rotation(b)) => {
            Some(Ok(Value::Rotation(compose(a, &conjugate(b)))))
        }
        (Value::Vector(vector), divisor) => {
            let divisor = number(divisor)?.to_float();
            if divisor == 0.0 {
                return Some(Err(ValueError::MathError));
            }
            Some(Ok(Value::Vector(scale(vector, divisor, component_div))))
        }
        _ => {
            let result = match (number(left)?, number(right)?) {
                (_, Number::Integer(0)) => Err(ValueError::MathError),
                (Number::Integer(a), Number::Integer(b)) => {
                    Ok(Value::Integer(a.checked_div(b).unwrap_or(a)))
                }
                (a, b) => {
                    let (a, b) = (a.to_float(), b.to_float());
                    let quotient = a / b;
                    if b == 0.0 || quotient.is_nan() {
                        Err(ValueError::MathError)
                    } else {
                        Ok(Value::Float(quotient))
                    }
                }
            };
            Some(result)
        }
    }
}

/// One vector component divided by a scalar. Two NaNs of opposite sign give
/// a positive NaN (PyOptimizer's `div`); a NaN result is not an error here,
/// unlike a scalar division.
fn component_div(component: f32, divisor: f32) -> f32 {
    if component.is_nan()
        && divisor.is_nan()
        && component.is_sign_negative() != divisor.is_sign_negative()
    {
        f32::NAN
    } else {
        component / divisor
    }
}

/// `%`: the integer remainder (truncating, so it takes the dividend's sign)
/// or the vector cross product.
fn modulo(left: &Value, right: &Value) -> Option<Result<Value, ValueError>> {
    match (left, right) {
        (Value::Integer(_), Value::Integer(0)) => Some(Err(ValueError::MathError)),
        (Value::Integer(a), Value::Integer(b)) => {
            Some(Ok(Value::Integer(a.checked_rem(*b).unwrap_or(0))))
        }
        (Value::Vector(a), Value::Vector(b)) => Some(Ok(Value::Vector(cross(a, b)))),
        _ => None,
    }
}

/// `==`, as a Rust boolean.
#[expect(
    clippy::float_cmp,
    reason = "LSL float equality is exact IEEE equality"
)]
fn equals(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::String(a) | Value::Key(a), Value::String(b) | Value::Key(b)) => Some(a == b),
        (Value::Vector(a), Value::Vector(b)) => Some(vector_eq(a, b)),
        (Value::Rotation(a), Value::Rotation(b)) => Some(rotation_eq(a, b)),
        (Value::List(a), Value::List(b)) => Some(a.len() == b.len()),
        _ => match (number(left)?, number(right)?) {
            (Number::Integer(a), Number::Integer(b)) => Some(a == b),
            (a, b) => Some(a.to_float() == b.to_float()),
        },
    }
}

/// `!=`. For two lists it is the length difference, which is why this is
/// not simply the negation of [`equals`].
fn not_equals(left: &Value, right: &Value) -> Option<Value> {
    if let (Value::List(a), Value::List(b)) = (left, right) {
        let length = |list: &Vec<_>| i32::try_from(list.len()).unwrap_or(i32::MAX);
        return Some(Value::Integer(length(a).wrapping_sub(length(b))));
    }
    equals(left, right).map(|equal| truth(!equal))
}

/// `<` on two numbers, promoting a mixed pair.
fn less(left: &Value, right: &Value) -> Option<bool> {
    match (number(left)?, number(right)?) {
        (Number::Integer(a), Number::Integer(b)) => Some(a < b),
        (a, b) => Some(a.to_float() < b.to_float()),
    }
}

/// The sum of `terms` rounded once, as Python's `math.fsum` gives it: a
/// compensated (Neumaier) sum in `f64`, which is exact for the three exact
/// products of a dot product in all but pathological cancellations.
fn compensated_sum(terms: &[f64]) -> f64 {
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for term in terms {
        let next = sum + term;
        compensation += if sum.abs() >= term.abs() {
            (sum - next) + term
        } else {
            (term - next) + sum
        };
        sum = next;
    }
    sum + compensation
}

/// `vector * vector`: the dot product. The component products are exact in
/// `f64`, summed with one rounding and then rounded to `f32` (PyOptimizer's
/// `fsum`).
fn dot(a: &Vector, b: &Vector) -> f32 {
    to_f32(compensated_sum(&[
        f64::from(a.x) * f64::from(b.x),
        f64::from(a.y) * f64::from(b.y),
        f64::from(a.z) * f64::from(b.z),
    ]))
}

/// `vector % vector`: the cross product, each component computed in `f64`
/// and rounded to `f32` once.
fn cross(a: &Vector, b: &Vector) -> Vector {
    let (ax, ay, az) = (f64::from(a.x), f64::from(a.y), f64::from(a.z));
    let (bx, by, bz) = (f64::from(b.x), f64::from(b.y), f64::from(b.z));
    Vector {
        x: to_f32(ay * bz - az * by),
        y: to_f32(az * bx - ax * bz),
        z: to_f32(ax * by - ay * bx),
    }
}

/// The conjugate `<-x, -y, -z, s>` — the inverse of a unit rotation, and
/// what `/ rotation` multiplies by. Not normalised, as in the reference.
const fn conjugate(rotation: &Rotation) -> Rotation {
    Rotation {
        x: -rotation.x,
        y: -rotation.y,
        z: -rotation.z,
        s: rotation.s,
    }
}

/// `vector * rotation`: the vector rotated by the rotation, `q v q*`,
/// expanded and evaluated in `f64` from the `f32` components, then rounded
/// to `f32` (PyOptimizer's `mul`). The rotation is not normalised first.
fn rotate(vector: &Vector, rotation: &Rotation) -> Vector {
    let (a0, a1, a2) = (
        f64::from(vector.x),
        f64::from(vector.y),
        f64::from(vector.z),
    );
    let (b0, b1, b2, b3) = (
        f64::from(rotation.x),
        f64::from(rotation.y),
        f64::from(rotation.z),
        f64::from(rotation.s),
    );
    let (b0b0, b0b1, b0b2, b0b3) = (b0 * b0, b0 * b1, b0 * b2, b0 * b3);
    let (b1b1, b1b2, b1b3) = (b1 * b1, b1 * b2, b1 * b3);
    let (b2b2, b2b3, b3b3) = (b2 * b2, b2 * b3, b3 * b3);
    Vector {
        x: to_f32(
            a0 * (b0b0 - b1b1 - b2b2 + b3b3) + a1 * (b0b1 - b2b3) * 2.0 + a2 * (b0b2 + b1b3) * 2.0,
        ),
        y: to_f32(
            a0 * (b0b1 + b2b3) * 2.0 + a1 * (b1b1 - b0b0 - b2b2 + b3b3) + a2 * (b1b2 - b0b3) * 2.0,
        ),
        z: to_f32(
            a0 * (b0b2 - b1b3) * 2.0 + a1 * (b1b2 + b0b3) * 2.0 + a2 * (b2b2 - b0b0 - b1b1 + b3b3),
        ),
    }
}

/// `left * right` on rotations, in **Linden's order**: the result applies
/// `left` first and then `right`, so `v * (a * b) == (v * a) * b`. That is
/// the Hamilton product `right ⊗ left` — the reverse of what glam's
/// `left * right` means.
///
/// Each partial product is rounded to `f32`, the four summed in `f64` and
/// rounded once more (PyOptimizer's `mul`). Second Life itself is known to
/// round at least one case differently: `<3,5,7,17> * <.22,.26,.38,.86>` has
/// a `y` of exactly `8.32` there and `8.320001` here, a difference neither
/// oracle reproduces.
fn compose(left: &Rotation, right: &Rotation) -> Rotation {
    let (a, b) = (left, right);
    let term = |x: f32, y: f32| f64::from(x * y);
    Rotation {
        x: to_f32(term(a.x, b.s) + term(a.s, b.x) + term(a.z, b.y) - term(a.y, b.z)),
        y: to_f32(term(a.y, b.s) - term(a.z, b.x) + term(a.s, b.y) + term(a.x, b.z)),
        z: to_f32(term(a.z, b.s) + term(a.y, b.x) - term(a.x, b.y) + term(a.s, b.z)),
        s: to_f32(term(a.s, b.s) - term(a.x, b.x) - term(a.y, b.y) - term(a.z, b.z)),
    }
}

/// `op operand` for a prefix operator.
///
/// `-` negates (`-(-2147483648)` stays `-2147483648`), `!` is `1` for zero
/// and `0` otherwise, `~` complements the bits. `++` and `--` yield the
/// operand plus or minus one — the value a pre-increment stores and yields;
/// a post-increment stores it and yields the operand.
///
/// # Errors
///
/// [`ValueError::Prefix`] for an operand type the compiler rejects.
pub fn prefix(op: PrefixOp, operand: Value) -> Result<Value, ValueError> {
    let mismatch = ValueError::Prefix {
        op,
        operand: operand.type_name(),
    };
    match (op, operand) {
        (PrefixOp::Neg, Value::Integer(integer)) => Ok(Value::Integer(integer.wrapping_neg())),
        (PrefixOp::Neg, Value::Float(float)) => Ok(Value::Float(-float)),
        (PrefixOp::Neg, Value::Vector(vector)) => Ok(Value::Vector(Vector {
            x: -vector.x,
            y: -vector.y,
            z: -vector.z,
        })),
        (PrefixOp::Neg, Value::Rotation(rotation)) => Ok(Value::Rotation(Rotation {
            x: -rotation.x,
            y: -rotation.y,
            z: -rotation.z,
            s: -rotation.s,
        })),
        (PrefixOp::Not, Value::Integer(integer)) => Ok(truth(integer == 0)),
        (PrefixOp::BitNot, Value::Integer(integer)) => Ok(Value::Integer(!integer)),
        (PrefixOp::PreInc, Value::Integer(integer)) => Ok(Value::Integer(integer.wrapping_add(1))),
        (PrefixOp::PreDec, Value::Integer(integer)) => Ok(Value::Integer(integer.wrapping_sub(1))),
        (PrefixOp::PreInc, Value::Float(float)) => Ok(Value::Float(float + 1.0)),
        (PrefixOp::PreDec, Value::Float(float)) => Ok(Value::Float(float - 1.0)),
        _ => Err(mismatch),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_lsl::types::{ALL_TYPES, binary_result, prefix_result};

    use super::*;
    use crate::value::Element;

    /// A vector from three components.
    const fn vector(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// A rotation from four components.
    const fn rotation(x: f32, y: f32, z: f32, s: f32) -> Rotation {
        Rotation { x, y, z, s }
    }

    /// One non-zero sample of each type, so no sample trips a `Math Error`.
    fn samples() -> Vec<Value> {
        vec![
            Value::Integer(3),
            Value::Float(1.5),
            Value::String("a".to_owned()),
            Value::Key("5748decc-f629-461c-9a36-a35a221fe21f".to_owned()),
            Value::Vector(vector(1.0, 2.0, 3.0)),
            Value::Rotation(rotation(0.0, 0.0, 0.0, 1.0)),
            Value::List(vec![Element::Integer(1)]),
        ]
    }

    /// Every binary operator.
    const BINARY_OPS: [BinaryOp; 18] = [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
        BinaryOp::Eq,
        BinaryOp::Ne,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::Shl,
        BinaryOp::Shr,
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::BitXor,
        BinaryOp::And,
        BinaryOp::Or,
    ];

    #[test]
    fn the_value_operators_agree_with_the_compile_time_table() {
        for op in BINARY_OPS {
            for left in samples() {
                for right in samples() {
                    let types = (left.type_name(), right.type_name());
                    let expected = binary_result(op, types.0, types.1);
                    match binary(op, left.clone(), right.clone()) {
                        Ok(result) => {
                            assert_eq!(Some(result.type_name()), expected, "{op:?} {types:?}");
                        }
                        Err(error) => {
                            assert_eq!(expected, None, "{op:?} {types:?}: {error}");
                        }
                    }
                }
            }
        }
        let prefix_ops = [
            PrefixOp::Neg,
            PrefixOp::Not,
            PrefixOp::BitNot,
            PrefixOp::PreInc,
            PrefixOp::PreDec,
        ];
        for op in prefix_ops {
            for operand in samples() {
                let ty = operand.type_name();
                let result = prefix(op, operand).ok().map(|value| value.type_name());
                assert_eq!(result, prefix_result(op, ty), "{op:?} {ty:?}");
            }
        }
        // Sanity: the sweep covered every type.
        assert_eq!(samples().len(), ALL_TYPES.len());
    }

    /// `op` on two integers.
    fn int_op(op: BinaryOp, left: i32, right: i32) -> Result<Value, ValueError> {
        binary(op, Value::Integer(left), Value::Integer(right))
    }

    #[test]
    fn integer_arithmetic_wraps_and_truncates() {
        // PyOptimizer unit_tests/expr.suite/operators.lsl → operators.out.
        let cases: [(BinaryOp, i32, i32, i32); 36] = [
            (BinaryOp::Mul, 1, 0, 0),
            (BinaryOp::Div, 0, 9, 0),
            (BinaryOp::Div, -1, -9, 0),
            (BinaryOp::Div, -9, -9, 1),
            (BinaryOp::Div, -8, 9, 0),
            (BinaryOp::Div, -9, 9, -1),
            (BinaryOp::Div, 9, -9, -1),
            (BinaryOp::Div, 8, 9, 0),
            (BinaryOp::Div, i32::MIN, -1, i32::MIN),
            (BinaryOp::Add, i32::MIN, -1, i32::MAX),
            (BinaryOp::Sub, -0x7FFF_FFFF, 2, i32::MAX),
            (BinaryOp::Sub, 1, 2, -1),
            (BinaryOp::Mod, 6, 5, 1),
            (BinaryOp::Mod, -1, 5, -1),
            (BinaryOp::Mod, -6, 5, -1),
            (BinaryOp::Mod, 6, -5, 1),
            (BinaryOp::Mod, -4, -5, -4),
            (BinaryOp::Mod, -5, -5, 0),
            (BinaryOp::Mod, i32::MIN, 5, -3),
            (BinaryOp::Mod, i32::MIN, -5, -3),
            (BinaryOp::Mod, 5, i32::MIN, 5),
            (BinaryOp::Mod, 5, i32::MAX, 5),
            (BinaryOp::Shl, 1, -33, i32::MIN),
            (BinaryOp::Shl, 1, -1, i32::MIN),
            (BinaryOp::Shl, 1, 31, i32::MIN),
            (BinaryOp::Shl, 1, 32, 1),
            (BinaryOp::Shl, 1, 33, 2),
            (BinaryOp::Shl, 1, 66, 4),
            (BinaryOp::Shr, -0x4000_0000, -33, -1),
            (BinaryOp::Shr, -0x4000_0000, 0, -0x4000_0000),
            (BinaryOp::Shr, -0x4000_0000, 1, -0x2000_0000),
            (BinaryOp::Shr, -0x4000_0000, 30, -1),
            (BinaryOp::Shr, -0x4000_0000, 32, -0x4000_0000),
            (BinaryOp::Shr, -0x4000_0000, 66, -0x1000_0000),
            (BinaryOp::And, 2, -1, 1),
            (BinaryOp::Or, 0, 0, 0),
        ];
        for (op, left, right, expected) in cases {
            assert_eq!(
                int_op(op, left, right),
                Ok(Value::Integer(expected)),
                "{left} {op:?} {right}"
            );
        }
        // operators.lsl: -(-2147483648) is -2147483648.
        assert_eq!(
            prefix(PrefixOp::Neg, Value::Integer(i32::MIN)),
            Ok(Value::Integer(i32::MIN))
        );
    }

    #[test]
    fn division_by_zero_is_a_math_error_for_floats_too() {
        // PyOptimizer unit_tests/expr.suite/math-error.lsl: none of these
        // fold, because each is a run-time Math Error in Second Life. This
        // is where SL and the roadmap's first draft disagreed ("float
        // division by zero is not" an error) — SL wins.
        let math_error = Err(ValueError::MathError);
        assert_eq!(int_op(BinaryOp::Div, 1, 0), math_error);
        assert_eq!(int_op(BinaryOp::Mod, 5, 0), math_error);
        assert_eq!(
            binary(BinaryOp::Div, Value::Float(1.0), Value::Integer(0)),
            math_error
        );
        assert_eq!(
            binary(BinaryOp::Div, Value::Float(1.0), Value::Float(-0.0)),
            math_error
        );
        assert_eq!(
            binary(
                BinaryOp::Div,
                Value::Float(f32::INFINITY),
                Value::Float(f32::INFINITY)
            ),
            math_error
        );
        assert_eq!(
            binary(BinaryOp::Div, Value::Float(f32::NAN), Value::Integer(1)),
            math_error
        );
        assert_eq!(
            binary(BinaryOp::Div, Value::Float(1.0), Value::Float(f32::NAN)),
            math_error
        );
        assert_eq!(
            binary(
                BinaryOp::Div,
                Value::Vector(vector(1.0, 2.0, 3.0)),
                Value::Float(0.0)
            ),
            math_error
        );
        // …but a vector divided by NaN is not (nan-fcast-vcast-minus0.lsl).
        assert!(matches!(
            binary(
                BinaryOp::Div,
                Value::Vector(vector(3.0, 0.0, -0.0)),
                Value::Float(f32::NAN)
            ),
            Ok(Value::Vector(_))
        ));
    }

    /// A float result's bits, or a sentinel for anything else.
    fn float_bits(result: Result<Value, ValueError>) -> Option<u32> {
        match result {
            Ok(Value::Float(float)) => Some(float.to_bits()),
            _ => None,
        }
    }

    #[test]
    fn mixed_arithmetic_promotes_to_single_precision() {
        // operators.lsl → operators.out.
        let cases: [(BinaryOp, Value, Value, f32); 8] = [
            (BinaryOp::Mul, Value::Float(-1.0), Value::Float(0.0), -0.0),
            (BinaryOp::Mul, Value::Float(-0.0), Value::Float(-1.0), 0.0),
            (
                BinaryOp::Add,
                Value::Float(-2_147_483_648.0),
                Value::Integer(-1),
                -2_147_483_648.0,
            ),
            (
                BinaryOp::Add,
                Value::Integer(i32::MIN),
                Value::Float(-1.0),
                -2_147_483_648.0,
            ),
            (BinaryOp::Sub, Value::Float(1.0), Value::Integer(2), -1.0),
            (BinaryOp::Mul, Value::Integer(1), Value::Float(2.0), 2.0),
            // casts.lsl: `3 * 1.0 / 2` is 1.5, `2147483647 * 1.0 * 2` 4294967296.
            (BinaryOp::Div, Value::Float(3.0), Value::Integer(2), 1.5),
            (
                BinaryOp::Mul,
                Value::Float(2_147_483_648.0),
                Value::Integer(2),
                4_294_967_296.0,
            ),
        ];
        for (op, left, right, expected) in cases {
            assert_eq!(
                float_bits(binary(op, left.clone(), right.clone())),
                Some(expected.to_bits()),
                "{left:?} {op:?} {right:?}"
            );
        }
        // f32 drift: 0.1 added ten times is not 1.0 in single precision.
        let mut sum = Value::Float(0.0);
        for _ in 0..10 {
            sum = binary(BinaryOp::Add, sum, Value::Float(0.1)).unwrap_or(Value::Integer(0));
        }
        assert_eq!(float_bits(Ok(sum)), Some(1.000_000_1_f32.to_bits()));
    }

    #[test]
    fn nan_times_nan_is_negative_only_when_both_are() {
        // nan-fcast-vcast-minus0.lsl: (1e40*0)*(1e40*0) etc.
        let product =
            |a: f32, b: f32| float_bits(binary(BinaryOp::Mul, Value::Float(a), Value::Float(b)));
        let negative = (-f32::NAN).to_bits();
        let positive = f32::NAN.to_bits();
        assert_eq!(product(-f32::NAN, -f32::NAN), Some(negative));
        assert_eq!(product(f32::NAN, -f32::NAN), Some(positive));
        assert_eq!(product(-f32::NAN, f32::NAN), Some(positive));
        assert_eq!(product(f32::NAN, f32::NAN), Some(positive));
    }

    #[test]
    fn vector_and_rotation_arithmetic() {
        // operators.lsl → operators.out; the expected components are the
        // shortest decimal spellings of the f32 results, so compared exactly.
        let v345 = || Value::Vector(vector(3.0, 4.0, 5.0));
        let q = || Value::Rotation(rotation(0.22, 0.26, 0.38, 0.86));
        let cases: [(BinaryOp, Value, Value, Value); 12] = [
            (
                BinaryOp::Div,
                Value::Vector(vector(3.0, 6.0, 9.0)),
                Value::Integer(3),
                Value::Vector(vector(1.0, 2.0, 3.0)),
            ),
            (
                BinaryOp::Add,
                Value::Vector(vector(1.0, 2.0, 3.0)),
                Value::Vector(vector(2.0, 4.0, 6.0)),
                Value::Vector(vector(3.0, 6.0, 9.0)),
            ),
            (
                BinaryOp::Sub,
                Value::Rotation(rotation(1.0, 2.0, 3.0, 4.0)),
                Value::Rotation(rotation(2.0, 4.0, 6.0, 8.0)),
                Value::Rotation(rotation(-1.0, -2.0, -3.0, -4.0)),
            ),
            (
                BinaryOp::Mul,
                Value::Float(2.0),
                Value::Vector(vector(1.0, 2.0, 3.0)),
                Value::Vector(vector(2.0, 4.0, 6.0)),
            ),
            (BinaryOp::Mul, v345(), v345(), Value::Float(50.0)),
            (BinaryOp::Mul, v345(), Value::Float(1.0), v345()),
            (
                BinaryOp::Mul,
                v345(),
                Value::Rotation(rotation(0.0, 0.0, 0.0, 1.0)),
                v345(),
            ),
            (
                BinaryOp::Mul,
                v345(),
                q(),
                Value::Vector(vector(2.643_2, 3.857_6, 5.304)),
            ),
            (
                BinaryOp::Div,
                v345(),
                q(),
                Value::Vector(vector(3.4, 3.72, 4.96)),
            ),
            (
                BinaryOp::Mul,
                Value::Rotation(rotation(1.6, 3.2, 6.4, 6.8)),
                Value::Rotation(rotation(1.0, 0.0, 0.0, 0.0)),
                Value::Rotation(rotation(6.8, -6.4, 3.2, -1.6)),
            ),
            // Second Life reports y = 8.32 exactly here; both oracles'
            // formula gives 8.320001 (the FIXME in operators.lsl). Recorded
            // as a known divergence: we match PyOptimizer.
            (
                BinaryOp::Mul,
                Value::Rotation(rotation(3.0, 5.0, 7.0, 17.0)),
                q(),
                Value::Rotation(rotation(6.24, 8.320_001, 12.8, 10.0)),
            ),
            (
                BinaryOp::Div,
                Value::Rotation(rotation(3.0, 5.0, 7.0, 17.0)),
                q(),
                Value::Rotation(rotation(-1.08, 0.280_000_1, -0.760_000_1, 19.24)),
            ),
        ];
        for (op, left, right, expected) in cases {
            assert_eq!(
                binary(op, left.clone(), right.clone()),
                Ok(expected),
                "{left:?} {op:?} {right:?}"
            );
        }
        assert_eq!(
            binary(
                BinaryOp::Mod,
                Value::Vector(vector(1.0, 0.0, 0.0)),
                Value::Vector(vector(0.0, 1.0, 0.0))
            ),
            Ok(Value::Vector(vector(0.0, 0.0, 1.0)))
        );
        assert_eq!(
            prefix(
                PrefixOp::Neg,
                Value::Rotation(rotation(-0.5, 0.5, 0.4, -0.4))
            ),
            Ok(Value::Rotation(rotation(0.5, -0.5, -0.4, 0.4)))
        );
    }

    /// Whether two vectors agree to within `1e-5` per component.
    fn close(a: &Vector, b: &Vector) -> bool {
        (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
    }

    #[test]
    fn rotations_compose_in_lindens_order() {
        // LSL's `a * b` applies `a` first: `v * (a * b) == (v * a) * b`. A
        // 90° turn about Z then one about X takes <1,0,0> to <0,1,0> and on
        // to <0,0,1>; the reverse (textbook) product would turn about X first
        // and land on <0,1,0>.
        let half = core::f32::consts::FRAC_1_SQRT_2;
        let about_z = rotation(0.0, 0.0, half, half);
        let about_x = rotation(half, 0.0, 0.0, half);
        let unit_x = vector(1.0, 0.0, 0.0);
        let composed = compose(&about_z, &about_x);
        let rotated = rotate(&unit_x, &composed);
        let stepwise = rotate(&rotate(&unit_x, &about_z), &about_x);
        assert!(close(&rotated, &stepwise), "{rotated:?} vs {stepwise:?}");
        assert!(close(&rotated, &vector(0.0, 0.0, 1.0)), "{rotated:?}");
        // Dividing by a rotation undoes it.
        let back = rotate(&rotated, &conjugate(&composed));
        assert!(close(&back, &unit_x), "{back:?}");
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "3.14 is the oracle's own test literal, not an approximation of pi"
    )]
    fn comparisons() {
        // PyOptimizer unit_tests/expr.suite/operators-compare.lsl → .out.
        let text = |s: &str| Value::String(s.to_owned());
        let key = |s: &str| Value::Key(s.to_owned());
        let list = |n: usize| Value::List(vec![Element::Integer(0); n]);
        let nan = || Value::Float(f32::NAN);
        let cases: [(BinaryOp, Value, Value, i32); 26] = [
            (BinaryOp::Eq, nan(), nan(), 0),
            (BinaryOp::Ne, nan(), nan(), 1),
            (
                BinaryOp::Eq,
                Value::Float(3.14),
                Value::Float(3.139_999_9),
                0,
            ),
            (BinaryOp::Eq, Value::Integer(1), Value::Float(1.0), 1),
            (BinaryOp::Ne, text("a"), text("b"), 1),
            (BinaryOp::Ne, text("a"), text("a"), 0),
            (BinaryOp::Eq, text("a"), text("a"), 1),
            (BinaryOp::Ne, list(2), list(2), 0),
            (BinaryOp::Eq, list(2), list(2), 1),
            (BinaryOp::Ne, list(1), list(2), -1),
            (BinaryOp::Ne, list(3), list(1), 2),
            (BinaryOp::Eq, list(0), list(1), 0),
            (
                BinaryOp::Eq,
                key("00000000-0000-0000-0000-000000000000"),
                key("5748decc-f629-461c-9a36-a35a221fe21f"),
                0,
            ),
            (
                BinaryOp::Eq,
                key("00000000-0000-0000-0000-000000000000"),
                text("00000000-0000-0000-0000-000000000000"),
                1,
            ),
            (
                BinaryOp::Eq,
                key("ABCDEFAB-ABCD-ABCD-ABCD-ABCDEFABCDEF"),
                key("abcdefab-abcd-abcd-abcd-abcdefabcdef"),
                0,
            ),
            (
                BinaryOp::Eq,
                Value::Rotation(rotation(1.0, 2.0, 3.0, 4.0)),
                Value::Rotation(rotation(1.0, 2.0, 3.0, 4.2)),
                0,
            ),
            (
                BinaryOp::Eq,
                Value::Vector(vector(1.0, 2.0, 3.0)),
                Value::Vector(vector(1.0, 2.0, 3.0)),
                1,
            ),
            (BinaryOp::Lt, Value::Integer(1), Value::Integer(2), 1),
            (BinaryOp::Gt, Value::Integer(2), Value::Integer(2), 0),
            (BinaryOp::Lt, Value::Float(-0.0), Value::Float(0.0), 0),
            (BinaryOp::Lt, nan(), Value::Integer(2), 0),
            (BinaryOp::Gt, nan(), Value::Integer(2), 0),
            (BinaryOp::Lt, Value::Integer(2), nan(), 0),
            // lslfoldconst: `<=` is `1 - less(b, a)`, so NaN compares true.
            (BinaryOp::Le, nan(), Value::Integer(2), 1),
            (BinaryOp::Ge, nan(), Value::Integer(2), 1),
            (
                BinaryOp::Lt,
                Value::Float(-f32::INFINITY),
                Value::Float(f32::INFINITY),
                1,
            ),
        ];
        for (op, left, right, expected) in cases {
            assert_eq!(
                binary(op, left.clone(), right.clone()),
                Ok(Value::Integer(expected)),
                "{left:?} {op:?} {right:?}"
            );
        }
    }

    #[test]
    fn plus_appends_prepends_and_concatenates() {
        // operators.lsl: ["1"] + ["2"], "1" + ["2"], ["1"] + "2", "1" + "2".
        let one = || Element::String("1".to_owned());
        let two = || Element::String("2".to_owned());
        let both = Ok(Value::List(vec![one(), two()]));
        assert_eq!(
            binary(
                BinaryOp::Add,
                Value::List(vec![one()]),
                Value::List(vec![two()])
            ),
            both
        );
        assert_eq!(
            binary(
                BinaryOp::Add,
                Value::String("1".to_owned()),
                Value::List(vec![two()])
            ),
            both
        );
        assert_eq!(
            binary(
                BinaryOp::Add,
                Value::List(vec![one()]),
                Value::String("2".to_owned())
            ),
            both
        );
        assert_eq!(
            binary(
                BinaryOp::Add,
                Value::String("1".to_owned()),
                Value::String("2".to_owned())
            ),
            Ok(Value::String("12".to_owned()))
        );
        // `key + string` is a compile error, so the value half refuses it.
        assert!(matches!(
            binary(
                BinaryOp::Add,
                Value::Key("k".to_owned()),
                Value::String("s".to_owned())
            ),
            Err(ValueError::Binary { .. })
        ));
    }

    #[test]
    fn logical_operators_take_both_operands_and_yield_zero_or_one() {
        // `&&` / `||` see both values: there is no short-circuit to model,
        // only the truth table (lslfoldconst folds `a && b` to `!(!a | !b)`).
        assert_eq!(int_op(BinaryOp::And, 5, 7), Ok(Value::Integer(1)));
        assert_eq!(int_op(BinaryOp::And, 5, 0), Ok(Value::Integer(0)));
        assert_eq!(int_op(BinaryOp::Or, 0, -3), Ok(Value::Integer(1)));
        assert_eq!(
            prefix(PrefixOp::Not, Value::Integer(0)),
            Ok(Value::Integer(1))
        );
        assert_eq!(
            prefix(PrefixOp::Not, Value::Integer(9)),
            Ok(Value::Integer(0))
        );
        assert_eq!(
            prefix(PrefixOp::BitNot, Value::Integer(0)),
            Ok(Value::Integer(-1))
        );
        assert_eq!(
            prefix(PrefixOp::PreInc, Value::Integer(i32::MAX)),
            Ok(Value::Integer(i32::MIN))
        );
    }
}
