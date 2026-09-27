//! The **compile-time half of LSL's type rules**: which operator/operand
//! combinations are legal and what each legal one produces, which casts the
//! compiler accepts, and which implicit conversions exist.
//!
//! This is a pure table over [`TypeName`], read by the semantic pass (to type an
//! arithmetic expression instead of giving up on it) and by the lowering (which
//! cannot give up, and needs every legal combination's result type to emit the
//! right instruction). The *value* half — what `1 / 0` does, how a float prints,
//! what `(integer)"0x1A"` is — lives in the `sl-lsl-runtime` crate; the design
//! record is the book chapter `simulator/lsl-engine.md`. A test there runs every
//! combination through both halves and checks they agree.
//!
//! The source is **tailslide**'s `OPERATOR_RESULTS` and `LEGAL_CAST_TABLE`
//! (`libtailslide/types.cc`), which reproduce Linden's own compiler, cross-read
//! against LSL PyOptimizer's parser (`lslopt/lslparse.py`). Where the two
//! disagree it is said at the entry.
//!
//! Three things about LSL expressions that are *not* in these tables, because
//! they are not type rules, but that the lowering must honour:
//!
//! - **`&&` and `||` do not short-circuit.** Both operands are always
//!   evaluated; PyOptimizer folds `a && b` to the pure bitwise
//!   `!(!a | !b)` (`lslfoldconst.py`), which is only sound because nothing is
//!   skipped. A null guard on the left of `&&` guards nothing.
//! - **Operands are evaluated right to left.** `a - b + c` computes
//!   `(a - b) + c` but evaluates `c`, then `b`, then `a` (the
//!   `Parse_expression` docstring in `lslparse.py`), which is observable when
//!   an operand has a side effect.
//! - **`integer *= float` is legal although `integer = integer * float` is
//!   not**: it means `lhs = (integer)((float)lhs * rhs)` (see
//!   [`assign_result`]).

use crate::ast::{AssignOp, BinaryOp, PostfixOp, PrefixOp, TypeName};

/// Every one of LSL's seven types, in the reference's `LST_*` order — for
/// callers (and tests) that sweep the whole matrix.
pub const ALL_TYPES: [TypeName; 7] = [
    TypeName::Integer,
    TypeName::Float,
    TypeName::String,
    TypeName::Key,
    TypeName::Vector,
    TypeName::Rotation,
    TypeName::List,
];

/// Whether a numeric operand: `integer` or `float`.
const fn is_numeric(ty: TypeName) -> bool {
    matches!(ty, TypeName::Integer | TypeName::Float)
}

/// Whether a text operand: `string` or `key`.
const fn is_text(ty: TypeName) -> bool {
    matches!(ty, TypeName::String | TypeName::Key)
}

/// The result of `left op right`, or [`None`] when the compiler rejects the
/// combination (tailslide's type-mismatch error).
///
/// Mixed `integer`/`float` arithmetic and comparison promote to `float`.
/// `list + x` appends and `x + list` prepends, for any `x`. `vector * vector`
/// is the dot product (a `float`), `vector % vector` the cross product,
/// `vector * rotation` rotates and `vector / rotation` rotates by the inverse.
/// Every comparison yields `integer`; `<`, `<=`, `>` and `>=` exist only for
/// numbers, while `==` and `!=` also compare strings with keys, vectors,
/// rotations and lists (lists by **length** — the value half's concern).
#[must_use]
pub const fn binary_result(op: BinaryOp, left: TypeName, right: TypeName) -> Option<TypeName> {
    use TypeName::{Float, Integer, List, Rotation, String, Vector};
    match op {
        BinaryOp::Add => match (left, right) {
            (List, _) | (_, List) => Some(List),
            (String, String) => Some(String),
            // `key + key` and `key + string` are rejected: a key is not text
            // to the `+` operator (PyOptimizer accepts `key + string` only
            // under its non-default `allowkeyconcat` extension).
            _ => arithmetic(left, right),
        },
        BinaryOp::Sub => arithmetic(left, right),
        BinaryOp::Mul => match (left, right) {
            (Integer | Float, Vector) | (Vector, Integer | Float | Rotation) => Some(Vector),
            (Vector, Vector) => Some(Float),
            (Rotation, Rotation) => Some(Rotation),
            _ => numeric(left, right),
        },
        BinaryOp::Div => match (left, right) {
            (Vector, Integer | Float | Rotation) => Some(Vector),
            (Rotation, Rotation) => Some(Rotation),
            _ => numeric(left, right),
        },
        BinaryOp::Mod => match (left, right) {
            (Integer, Integer) => Some(Integer),
            (Vector, Vector) => Some(Vector),
            _ => None,
        },
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            if is_numeric(left) && is_numeric(right) {
                Some(Integer)
            } else {
                None
            }
        }
        BinaryOp::Eq | BinaryOp::Ne => {
            let comparable = (is_numeric(left) && is_numeric(right))
                || (is_text(left) && is_text(right))
                || matches!(
                    (left, right),
                    (Vector, Vector) | (Rotation, Rotation) | (List, List)
                );
            if comparable { Some(Integer) } else { None }
        }
        BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::And
        | BinaryOp::Or => match (left, right) {
            (Integer, Integer) => Some(Integer),
            _ => None,
        },
    }
}

/// `+` / `-` over numbers, vectors and rotations: the component-wise ones.
const fn arithmetic(left: TypeName, right: TypeName) -> Option<TypeName> {
    match (left, right) {
        (TypeName::Vector, TypeName::Vector) => Some(TypeName::Vector),
        (TypeName::Rotation, TypeName::Rotation) => Some(TypeName::Rotation),
        _ => numeric(left, right),
    }
}

/// Two numbers: `integer` when both are, `float` when either is a float.
const fn numeric(left: TypeName, right: TypeName) -> Option<TypeName> {
    match (left, right) {
        (TypeName::Integer, TypeName::Integer) => Some(TypeName::Integer),
        (TypeName::Integer | TypeName::Float, TypeName::Integer | TypeName::Float) => {
            Some(TypeName::Float)
        }
        _ => None,
    }
}

/// The result of a prefix operator on an operand, or [`None`] when rejected.
///
/// `-` negates numbers, vectors and rotations; `!` and `~` take only an
/// `integer` (a condition accepts every type, the `!` operator does not);
/// `++` and `--` take a numeric variable.
#[must_use]
pub const fn prefix_result(op: PrefixOp, operand: TypeName) -> Option<TypeName> {
    match op {
        PrefixOp::Neg => match operand {
            TypeName::Integer | TypeName::Float | TypeName::Vector | TypeName::Rotation => {
                Some(operand)
            }
            TypeName::String | TypeName::Key | TypeName::List => None,
        },
        PrefixOp::Not | PrefixOp::BitNot => match operand {
            TypeName::Integer => Some(TypeName::Integer),
            _ => None,
        },
        PrefixOp::PreInc | PrefixOp::PreDec => increment(operand),
    }
}

/// The result of a postfix `++` / `--`, or [`None`] when rejected.
#[must_use]
pub const fn postfix_result(op: PostfixOp, operand: TypeName) -> Option<TypeName> {
    match op {
        PostfixOp::PostInc | PostfixOp::PostDec => increment(operand),
    }
}

/// `++` / `--` exist for `integer` and `float` only.
const fn increment(operand: TypeName) -> Option<TypeName> {
    if is_numeric(operand) {
        Some(operand)
    } else {
        None
    }
}

/// The result of `target op= value`, or [`None`] when rejected.
///
/// A compound assignment is legal when the plain operator is and its result
/// has the target's type — so `float += integer` is fine and
/// `integer += float` is not — with the single exception the reference
/// compiler grew by accident: **`integer *= float`** is accepted, and behaves
/// as `target = (integer)((float)target * value)` (tailslide's
/// `getResultType` comment; PyOptimizer's `*=` special case). tailslide types
/// the expression's own value as `float` (what LSO leaves behind) and warns;
/// this table does the same, since the assignment itself is legal.
///
/// A plain `=` accepts the implicit conversions of [`implicitly_converts`].
#[must_use]
pub const fn assign_result(op: AssignOp, target: TypeName, value: TypeName) -> Option<TypeName> {
    let operator = match op {
        AssignOp::Assign => {
            return if implicitly_converts(value, target) {
                Some(target)
            } else {
                None
            };
        }
        AssignOp::AddAssign => BinaryOp::Add,
        AssignOp::SubAssign => BinaryOp::Sub,
        AssignOp::MulAssign => BinaryOp::Mul,
        AssignOp::DivAssign => BinaryOp::Div,
        AssignOp::ModAssign => BinaryOp::Mod,
    };
    if matches!(
        (op, target, value),
        (AssignOp::MulAssign, TypeName::Integer, TypeName::Float)
    ) {
        return Some(TypeName::Float);
    }
    match binary_result(operator, target, value) {
        Some(result) if type_eq(result, target) => Some(target),
        Some(_) | None => None,
    }
}

/// `TypeName` equality usable in a `const fn` (the derived `PartialEq` is not
/// `const`).
const fn type_eq(left: TypeName, right: TypeName) -> bool {
    matches!(
        (left, right),
        (TypeName::Integer, TypeName::Integer)
            | (TypeName::Float, TypeName::Float)
            | (TypeName::String, TypeName::String)
            | (TypeName::Key, TypeName::Key)
            | (TypeName::Vector, TypeName::Vector)
            | (TypeName::Rotation, TypeName::Rotation)
            | (TypeName::List, TypeName::List)
    )
}

/// Whether `(to)value` compiles for a value of type `from` — tailslide's
/// `LEGAL_CAST_TABLE`.
///
/// Every type casts to itself, to `string` and to `list`. A `string` casts to
/// anything. Beyond that only `integer` ↔ `float` and `key` → … `key`: a key
/// cannot be cast to a number (`(integer)some_key` is tailslide's `E10035`,
/// "illegal cast") and a number cannot be cast to a key. A `list` casts only
/// to `string` and `list`.
#[must_use]
pub const fn cast_legal(from: TypeName, to: TypeName) -> bool {
    if type_eq(from, to) || matches!(to, TypeName::String | TypeName::List) {
        return true;
    }
    match from {
        TypeName::String => true,
        TypeName::Integer | TypeName::Float => is_numeric(to),
        TypeName::Key | TypeName::Vector | TypeName::Rotation | TypeName::List => false,
    }
}

/// Whether a value of type `from` may stand where `to` is expected with no
/// cast — a parameter, a return value, the right of `=`. LSL has exactly two
/// implicit conversions: `integer` → `float` (widening) and `string` ↔ `key`
/// (freely interchangeable). tailslide's `COERCION_TABLE`.
#[must_use]
pub const fn implicitly_converts(from: TypeName, to: TypeName) -> bool {
    type_eq(from, to)
        || matches!(
            (from, to),
            (TypeName::Integer, TypeName::Float)
                | (TypeName::String, TypeName::Key)
                | (TypeName::Key, TypeName::String)
        )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::ast::TypeName::{Float, Integer, Key, List, Rotation, String, Vector};

    /// Every `(op, left, right, result)` row of tailslide's `OPERATOR_RESULTS`
    /// (`libtailslide/types.cc`) for the binary operators, transcribed in its
    /// order. `LST_ANY` rows are expanded over all seven types. Any pair not
    /// listed here must be rejected.
    fn tailslide_binary_rows() -> Vec<(BinaryOp, TypeName, TypeName, TypeName)> {
        let mut rows = vec![
            (BinaryOp::Sub, Integer, Integer, Integer),
            (BinaryOp::Sub, Integer, Float, Float),
            (BinaryOp::Sub, Float, Integer, Float),
            (BinaryOp::Sub, Float, Float, Float),
            (BinaryOp::Sub, Vector, Vector, Vector),
            (BinaryOp::Sub, Rotation, Rotation, Rotation),
            (BinaryOp::Add, Integer, Integer, Integer),
            (BinaryOp::Add, Integer, Float, Float),
            (BinaryOp::Add, Float, Integer, Float),
            (BinaryOp::Add, Float, Float, Float),
            (BinaryOp::Add, String, String, String),
            (BinaryOp::Add, Vector, Vector, Vector),
            (BinaryOp::Add, Rotation, Rotation, Rotation),
            (BinaryOp::Mul, Integer, Integer, Integer),
            (BinaryOp::Mul, Integer, Float, Float),
            (BinaryOp::Mul, Integer, Vector, Vector),
            (BinaryOp::Mul, Float, Integer, Float),
            (BinaryOp::Mul, Float, Float, Float),
            (BinaryOp::Mul, Float, Vector, Vector),
            (BinaryOp::Mul, Vector, Integer, Vector),
            (BinaryOp::Mul, Vector, Float, Vector),
            (BinaryOp::Mul, Vector, Vector, Float),
            (BinaryOp::Mul, Vector, Rotation, Vector),
            (BinaryOp::Mul, Rotation, Rotation, Rotation),
            (BinaryOp::Div, Integer, Integer, Integer),
            (BinaryOp::Div, Integer, Float, Float),
            (BinaryOp::Div, Float, Integer, Float),
            (BinaryOp::Div, Float, Float, Float),
            (BinaryOp::Div, Vector, Integer, Vector),
            (BinaryOp::Div, Vector, Float, Vector),
            (BinaryOp::Div, Vector, Rotation, Vector),
            (BinaryOp::Div, Rotation, Rotation, Rotation),
            (BinaryOp::Mod, Integer, Integer, Integer),
            (BinaryOp::Mod, Vector, Vector, Vector),
            (BinaryOp::BitAnd, Integer, Integer, Integer),
            (BinaryOp::BitOr, Integer, Integer, Integer),
            (BinaryOp::BitXor, Integer, Integer, Integer),
            (BinaryOp::And, Integer, Integer, Integer),
            (BinaryOp::Or, Integer, Integer, Integer),
            (BinaryOp::Shl, Integer, Integer, Integer),
            (BinaryOp::Shr, Integer, Integer, Integer),
        ];
        for any in ALL_TYPES {
            rows.push((BinaryOp::Add, List, any, List));
            if any != List {
                rows.push((BinaryOp::Add, any, List, List));
            }
        }
        for op in [BinaryOp::Lt, BinaryOp::Le, BinaryOp::Gt, BinaryOp::Ge] {
            for (left, right) in [
                (Integer, Integer),
                (Integer, Float),
                (Float, Integer),
                (Float, Float),
            ] {
                rows.push((op, left, right, Integer));
            }
        }
        for op in [BinaryOp::Eq, BinaryOp::Ne] {
            for (left, right) in [
                (Integer, Integer),
                (Integer, Float),
                (Float, Integer),
                (Float, Float),
                (Vector, Vector),
                (Rotation, Rotation),
                (String, String),
                (String, Key),
                (Key, String),
                (Key, Key),
                (List, List),
            ] {
                rows.push((op, left, right, Integer));
            }
        }
        rows
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
    fn binary_table_is_tailslides_exactly() {
        let rows = tailslide_binary_rows();
        for op in BINARY_OPS {
            for left in ALL_TYPES {
                for right in ALL_TYPES {
                    let expected = rows
                        .iter()
                        .find(|(o, l, r, _)| *o == op && *l == left && *r == right)
                        .map(|row| row.3);
                    assert_eq!(
                        binary_result(op, left, right),
                        expected,
                        "{op:?} {left:?} {right:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn prefix_and_postfix_follow_tailslide() {
        // tailslide: unary `-` on integer/float/vector/rotation, `!`/`~` on
        // integer only, `++`/`--` on integer/float only.
        for ty in ALL_TYPES {
            let neg = matches!(ty, Integer | Float | Vector | Rotation).then_some(ty);
            assert_eq!(prefix_result(PrefixOp::Neg, ty), neg, "-{ty:?}");
            let int_only = (ty == Integer).then_some(Integer);
            assert_eq!(prefix_result(PrefixOp::Not, ty), int_only, "!{ty:?}");
            assert_eq!(prefix_result(PrefixOp::BitNot, ty), int_only, "~{ty:?}");
            let step = matches!(ty, Integer | Float).then_some(ty);
            assert_eq!(prefix_result(PrefixOp::PreInc, ty), step, "++{ty:?}");
            assert_eq!(postfix_result(PostfixOp::PostDec, ty), step, "{ty:?}--");
        }
    }

    #[test]
    fn cast_table_is_tailslides_legal_cast_table() {
        // `LEGAL_CAST_TABLE` rows in `LST_*` order (integer, float, string,
        // key, vector, rotation, list); 1 = legal.
        let table: [[u8; 7]; 7] = [
            [1, 1, 1, 0, 0, 0, 1],
            [1, 1, 1, 0, 0, 0, 1],
            [1, 1, 1, 1, 1, 1, 1],
            [0, 0, 1, 1, 0, 0, 1],
            [0, 0, 1, 0, 1, 0, 1],
            [0, 0, 1, 0, 0, 1, 1],
            [0, 0, 1, 0, 0, 0, 1],
        ];
        for (from, row) in ALL_TYPES.iter().zip(table) {
            for (to, legal) in ALL_TYPES.iter().zip(row) {
                assert_eq!(cast_legal(*from, *to), legal == 1, "({to:?}){from:?}");
            }
        }
    }

    #[test]
    fn compound_assignment_needs_the_targets_type_bar_one_quirk() {
        // tailslide: "`int_val += 1.0` and `vec *= <1,1,1>` are forbidden,
        // but something like `float_val += 1` is fine" — and `int *= float`.
        assert_eq!(assign_result(AssignOp::AddAssign, Integer, Float), None);
        assert_eq!(assign_result(AssignOp::MulAssign, Vector, Vector), None);
        assert_eq!(
            assign_result(AssignOp::AddAssign, Float, Integer),
            Some(Float)
        );
        assert_eq!(
            assign_result(AssignOp::MulAssign, Integer, Float),
            Some(Float)
        );
        assert_eq!(
            assign_result(AssignOp::MulAssign, Vector, Rotation),
            Some(Vector)
        );
        assert_eq!(
            assign_result(AssignOp::AddAssign, List, Integer),
            Some(List)
        );
        assert_eq!(assign_result(AssignOp::AddAssign, Key, Key), None);
        assert_eq!(assign_result(AssignOp::Assign, Key, String), Some(Key));
        assert_eq!(assign_result(AssignOp::Assign, Integer, Float), None);
    }

    #[test]
    fn implicit_conversions_are_tailslides_coercion_table() {
        for from in ALL_TYPES {
            for to in ALL_TYPES {
                let expected = from == to
                    || matches!((from, to), (Integer, Float) | (String, Key) | (Key, String));
                assert_eq!(implicitly_converts(from, to), expected, "{from:?}→{to:?}");
            }
        }
    }
}
