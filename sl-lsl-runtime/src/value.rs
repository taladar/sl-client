//! LSL's seven value types as one enum, and the flat list element.

use sl_lsl::ast::TypeName;
use sl_types::lsl::{Rotation, Vector};

/// The all-zero key, `NULL_KEY`. It is false in a condition, like a key that
/// is not a UUID at all.
pub const NULL_KEY: &str = "00000000-0000-0000-0000-000000000000";

/// `ZERO_VECTOR`, `<0, 0, 0>` — the one vector a condition treats as false.
pub const ZERO_VECTOR: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// `ZERO_ROTATION`, `<0, 0, 0, 1>` — the identity, and the one rotation a
/// condition treats as false.
pub const ZERO_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// One LSL value.
///
/// `integer` is a 32-bit signed integer that **wraps**, and `float` is an
/// `f32` — not an `f64` — because scripts observe both: a counter that
/// overflows goes negative, and adding `0.1` a hundred times drifts the way
/// single precision does. A `key` is text that is not a `string`: it compares
/// equal to a string with the same text, but only a key's validity decides
/// its truth in a condition.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `integer`.
    Integer(i32),
    /// `float`.
    Float(f32),
    /// `string`.
    String(String),
    /// `key` — any text, not necessarily a UUID.
    Key(String),
    /// `vector`.
    Vector(Vector),
    /// `rotation`.
    Rotation(Rotation),
    /// `list` — ordered, heterogeneous and **flat**.
    List(Vec<Element>),
}

/// One element of a `list`: any value but another list. LSL lists are flat —
/// `[1] + [2]` concatenates, and nothing produces a nested list — so the type
/// has no way to hold one.
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// `integer`.
    Integer(i32),
    /// `float`.
    Float(f32),
    /// `string`.
    String(String),
    /// `key`.
    Key(String),
    /// `vector`.
    Vector(Vector),
    /// `rotation`.
    Rotation(Rotation),
}

impl Value {
    /// LSL's default for a type: `0`, `0.0`, `""`, an empty key,
    /// [`ZERO_VECTOR`], [`ZERO_ROTATION`] or the empty list — what a variable
    /// declared without an initialiser holds, and what a stubbed library
    /// function returns.
    #[must_use]
    pub const fn default_of(ty: TypeName) -> Self {
        match ty {
            TypeName::Integer => Self::Integer(0),
            TypeName::Float => Self::Float(0.0),
            TypeName::String => Self::String(String::new()),
            TypeName::Key => Self::Key(String::new()),
            TypeName::Vector => Self::Vector(ZERO_VECTOR),
            TypeName::Rotation => Self::Rotation(ZERO_ROTATION),
            TypeName::List => Self::List(Vec::new()),
        }
    }

    /// The LSL type of this value.
    #[must_use]
    pub const fn type_name(&self) -> TypeName {
        match self {
            Self::Integer(_) => TypeName::Integer,
            Self::Float(_) => TypeName::Float,
            Self::String(_) => TypeName::String,
            Self::Key(_) => TypeName::Key,
            Self::Vector(_) => TypeName::Vector,
            Self::Rotation(_) => TypeName::Rotation,
            Self::List(_) => TypeName::List,
        }
    }

    /// Whether a condition (`if`, `while`, `for`, `do … while`) takes this
    /// value as true. Every type may stand in a condition.
    ///
    /// False are `0`, `0.0` (and `-0.0`), `""`, the empty list,
    /// [`ZERO_VECTOR`], [`ZERO_ROTATION`], and a key that is [`NULL_KEY`] or
    /// is not a UUID at all — `(key)"xyz"` is false, where the string `"xyz"`
    /// is true. A NaN float is true, as is a vector or rotation with a NaN
    /// component (it is not *equal* to zero). PyOptimizer's `cond`.
    #[must_use]
    pub fn is_true(&self) -> bool {
        match self {
            Self::Integer(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
            Self::String(text) => !text.is_empty(),
            Self::Key(text) => text != NULL_KEY && is_uuid(text),
            Self::Vector(vector) => !vector_eq(vector, &ZERO_VECTOR),
            Self::Rotation(rotation) => !rotation_eq(rotation, &ZERO_ROTATION),
            Self::List(elements) => !elements.is_empty(),
        }
    }

    /// This value as the elements it contributes to a list: a list its own
    /// elements, anything else itself as the one element. It is what
    /// `(list)value`, `list + value` and `value + list` put in the list.
    #[must_use]
    pub fn into_elements(self) -> Vec<Element> {
        match self {
            Self::Integer(value) => vec![Element::Integer(value)],
            Self::Float(value) => vec![Element::Float(value)],
            Self::String(text) => vec![Element::String(text)],
            Self::Key(text) => vec![Element::Key(text)],
            Self::Vector(vector) => vec![Element::Vector(vector)],
            Self::Rotation(rotation) => vec![Element::Rotation(rotation)],
            Self::List(elements) => elements,
        }
    }
}

impl Element {
    /// The LSL type of this element (never `list`).
    #[must_use]
    pub const fn type_name(&self) -> TypeName {
        match self {
            Self::Integer(_) => TypeName::Integer,
            Self::Float(_) => TypeName::Float,
            Self::String(_) => TypeName::String,
            Self::Key(_) => TypeName::Key,
            Self::Vector(_) => TypeName::Vector,
            Self::Rotation(_) => TypeName::Rotation,
        }
    }
}

impl From<Element> for Value {
    fn from(element: Element) -> Self {
        match element {
            Element::Integer(value) => Self::Integer(value),
            Element::Float(value) => Self::Float(value),
            Element::String(text) => Self::String(text),
            Element::Key(text) => Self::Key(text),
            Element::Vector(vector) => Self::Vector(vector),
            Element::Rotation(rotation) => Self::Rotation(rotation),
        }
    }
}

/// Whether `text` has the shape of a UUID: 36 characters, hex digits in
/// either case, dashes after the 8th, 12th, 16th and 20th. The validity test
/// of a key in a condition (PyOptimizer's `key_re`).
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

/// LSL vector equality: component-wise `==`, so a NaN component is unequal
/// even to itself.
#[must_use]
pub fn vector_eq(left: &Vector, right: &Vector) -> bool {
    left.x == right.x && left.y == right.y && left.z == right.z
}

/// LSL rotation equality: component-wise `==` on the raw components, with no
/// normalisation — `<0, 0, 0, 1>` and `<0, 0, 0, -1>` are the same rotation
/// and still unequal.
#[must_use]
pub fn rotation_eq(left: &Rotation, right: &Rotation) -> bool {
    left.x == right.x && left.y == right.y && left.z == right.z && left.s == right.s
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A valid, non-null key (`TEXTURE_BLANK`).
    const BLANK: &str = "5748decc-f629-461c-9a36-a35a221fe21f";

    #[test]
    fn condition_truth_per_type() {
        // PyOptimizer `cond`: int/float/string/list by Python truthiness, a key
        // by `NULL_KEY`/length/`key_re`, a vector/rotation by `!= ZERO_*`.
        let cases = [
            (Value::Integer(0), false),
            (Value::Integer(-1), true),
            (Value::Float(0.0), false),
            (Value::Float(-0.0), false),
            (Value::Float(f32::NAN), true),
            (Value::Float(1e-45), true),
            (Value::String(String::new()), false),
            (Value::String("0".to_owned()), true),
            (Value::Key(String::new()), false),
            (Value::Key(NULL_KEY.to_owned()), false),
            (Value::Key("xyz".to_owned()), false),
            (Value::Key(BLANK.to_owned()), true),
            (Value::Key(BLANK.to_uppercase()), true),
            // a dash where a hex digit belongs, and one missing
            (
                Value::Key("5748decc-f629-461c-9a36-a35a221fe21-".to_owned()),
                false,
            ),
            (
                Value::Key("5748decc0f629-461c-9a36-a35a221fe21f".to_owned()),
                false,
            ),
            (Value::Vector(ZERO_VECTOR), false),
            (
                Value::Vector(Vector {
                    x: 0.0,
                    y: -0.0,
                    z: 0.0,
                }),
                false,
            ),
            (
                Value::Vector(Vector {
                    x: 0.0,
                    y: 0.0,
                    z: f32::NAN,
                }),
                true,
            ),
            (Value::Rotation(ZERO_ROTATION), false),
            (
                Value::Rotation(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 0.0,
                }),
                true,
            ),
            (Value::List(vec![]), false),
            (Value::List(vec![Element::Integer(0)]), true),
        ];
        for (value, expected) in cases {
            assert_eq!(value.is_true(), expected, "{value:?}");
        }
    }

    #[test]
    fn a_list_contributes_its_elements_and_anything_else_itself() {
        assert_eq!(
            Value::List(vec![Element::Integer(1), Element::Float(2.0)]).into_elements(),
            vec![Element::Integer(1), Element::Float(2.0)]
        );
        assert_eq!(
            Value::Key("k".to_owned()).into_elements(),
            vec![Element::Key("k".to_owned())]
        );
    }
}
