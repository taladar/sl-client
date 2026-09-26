//! `(type)value` on actual values: the cast matrix.

use sl_lsl::ast::TypeName;

use crate::error::ValueError;
use crate::format::{self, FLOAT_DECIMALS, VECTOR_DECIMALS};
use crate::num::{float_to_int, int_to_float};
use crate::parse;
use crate::value::{Element, Value};

/// Text up to its first NUL: a cast out of a string or key stops there
/// (PyOptimizer's `zstr`), since the reference's strings are NUL-terminated.
fn until_nul(text: String) -> String {
    match text.find('\0') {
        Some(end) => text.get(..end).unwrap_or_default().to_owned(),
        None => text,
    }
}

/// `(to)value`.
///
/// The legal pairs are exactly `sl_lsl::types::cast_legal`'s; any other is a
/// [`ValueError::Cast`], which the lowering never emits because the compiler
/// has already rejected it.
///
/// Every cast to `list` wraps the value as a one-element list (a list stays
/// itself). `(string)` prints: an integer in decimal, a float with six
/// decimals in Mono's seven significant digits
/// ([`format::float`]), a vector or rotation with **five** decimals, and a list
/// as the concatenation of its elements, each printed as in a list — floats
/// and vector components with six decimals, no separators. A string casts to
/// anything through the lenient parsers of [`crate::parse`], and `integer` ↔
/// `float` convert numerically (`num`'s rules: truncation, and
/// `-2147483648` for a float out of range).
///
/// # Errors
///
/// [`ValueError::Cast`] for a pair the compiler rejects.
pub fn cast(value: Value, to: TypeName) -> Result<Value, ValueError> {
    let from = value.type_name();
    let mismatch = ValueError::Cast { from, to };
    Ok(match (value, to) {
        (value, TypeName::List) => Value::List(value.into_elements()),
        (Value::Integer(integer), TypeName::Integer) => Value::Integer(integer),
        (Value::Integer(integer), TypeName::Float) => Value::Float(int_to_float(integer)),
        (Value::Float(float), TypeName::Integer) => Value::Integer(float_to_int(float)),
        (Value::Float(float), TypeName::Float) => Value::Float(float),
        (Value::String(text) | Value::Key(text), TypeName::String) => {
            Value::String(until_nul(text))
        }
        (Value::String(text) | Value::Key(text), TypeName::Key) => Value::Key(until_nul(text)),
        (Value::String(text), TypeName::Integer) => {
            Value::Integer(parse::integer(&until_nul(text)))
        }
        (Value::String(text), TypeName::Float) => Value::Float(parse::float(&until_nul(text))),
        (Value::String(text), TypeName::Vector) => Value::Vector(parse::vector(&until_nul(text))),
        (Value::String(text), TypeName::Rotation) => {
            Value::Rotation(parse::rotation(&until_nul(text)))
        }
        (Value::Vector(vector), TypeName::Vector) => Value::Vector(vector),
        (Value::Rotation(rotation), TypeName::Rotation) => Value::Rotation(rotation),
        (Value::List(elements), TypeName::String) => {
            Value::String(elements.iter().map(element_to_string).collect())
        }
        (Value::Integer(integer), TypeName::String) => Value::String(integer.to_string()),
        (Value::Float(float), TypeName::String) => {
            Value::String(format::float(float, FLOAT_DECIMALS))
        }
        (Value::Vector(vector), TypeName::String) => {
            Value::String(format::vector(&vector, VECTOR_DECIMALS))
        }
        (Value::Rotation(rotation), TypeName::String) => {
            Value::String(format::rotation(&rotation, VECTOR_DECIMALS))
        }
        (
            Value::Integer(_)
            | Value::Float(_)
            | Value::Key(_)
            | Value::Vector(_)
            | Value::Rotation(_)
            | Value::List(_),
            _,
        ) => return Err(mismatch),
    })
}

/// One list element as `(string)list` prints it: like `(string)` of the
/// element, except that a vector or rotation keeps six decimals.
#[must_use]
pub fn element_to_string(element: &Element) -> String {
    match element {
        Element::Integer(integer) => integer.to_string(),
        Element::Float(float) => format::float(*float, FLOAT_DECIMALS),
        Element::String(text) | Element::Key(text) => until_nul(text.clone()),
        Element::Vector(vector) => format::vector(vector, FLOAT_DECIMALS),
        Element::Rotation(rotation) => format::rotation(rotation, FLOAT_DECIMALS),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_lsl::types::{ALL_TYPES, cast_legal};
    use sl_types::lsl::{Rotation, Vector};

    use super::*;

    /// One sample value of each type.
    fn samples() -> Vec<Value> {
        vec![
            Value::Integer(7),
            Value::Float(2.5),
            Value::String("12".to_owned()),
            Value::Key("5748decc-f629-461c-9a36-a35a221fe21f".to_owned()),
            Value::Vector(Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
            Value::Rotation(Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            }),
            Value::List(vec![Element::Integer(1)]),
        ]
    }

    #[test]
    fn the_value_casts_are_exactly_the_compiler_legal_ones() {
        for value in samples() {
            for to in ALL_TYPES {
                let from = value.type_name();
                match cast(value.clone(), to) {
                    Ok(result) => {
                        assert!(cast_legal(from, to), "({to:?}){from:?} succeeded");
                        assert_eq!(result.type_name(), to, "({to:?}){from:?}");
                    }
                    Err(error) => {
                        assert!(!cast_legal(from, to), "({to:?}){from:?}: {error}");
                    }
                }
            }
        }
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "3.14 is the oracle's own test literal, not an approximation of pi"
    )]
    fn string_casts() -> Result<(), ValueError> {
        // PyOptimizer unit_tests/expr.suite/casts.lsl → casts.out.
        assert_eq!(
            cast(
                Value::List(vec![
                    Element::Integer(1),
                    Element::Float(3.14),
                    Element::Key("blah".to_owned()),
                    Element::Rotation(Rotation {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                        s: 0.0,
                    }),
                ]),
                TypeName::String
            )?,
            Value::String("13.140000blah<1.000000, 0.000000, 0.000000, 0.000000>".to_owned())
        );
        assert_eq!(
            cast(
                Value::List(vec![Element::Vector(Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                })]),
                TypeName::String
            )?,
            Value::String("<0.000000, 0.000000, 0.000000>".to_owned())
        );
        assert_eq!(
            cast(
                Value::Vector(Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                TypeName::String
            )?,
            Value::String("<0.00000, 0.00000, 0.00000>".to_owned())
        );
        assert_eq!(
            cast(Value::Integer(-5), TypeName::String)?,
            Value::String("-5".to_owned())
        );
        assert_eq!(
            cast(Value::Float(1.0), TypeName::String)?,
            Value::String("1.000000".to_owned())
        );
        Ok(())
    }

    #[test]
    fn keys_and_strings_are_the_same_text() -> Result<(), ValueError> {
        // casts.lsl: (key)"xyz" is the key "xyz" — no validation on the cast.
        assert_eq!(
            cast(Value::String("xyz".to_owned()), TypeName::Key)?,
            Value::Key("xyz".to_owned())
        );
        assert_eq!(
            cast(Value::String("ab\0cd".to_owned()), TypeName::String)?,
            Value::String("ab".to_owned())
        );
        // casts.lsl: `(integer)"3.14e+0a"` through the parser.
        assert_eq!(
            cast(Value::String("3.14e+0a".to_owned()), TypeName::Integer)?,
            Value::Integer(3)
        );
        Ok(())
    }

    #[test]
    fn numeric_casts() -> Result<(), ValueError> {
        // casts.lsl: `(integer)3333333333333.` is -2147483648.
        assert_eq!(
            cast(Value::Float(3_333_333_333_333.0), TypeName::Integer)?,
            Value::Integer(i32::MIN)
        );
        assert_eq!(
            cast(Value::Float(-2.9), TypeName::Integer)?,
            Value::Integer(-2)
        );
        assert_eq!(cast(Value::Integer(3), TypeName::Float)?, Value::Float(3.0));
        Ok(())
    }

    #[test]
    fn a_list_casts_to_itself_and_anything_else_wraps() -> Result<(), ValueError> {
        assert_eq!(
            cast(Value::Integer(1), TypeName::List)?,
            Value::List(vec![Element::Integer(1)])
        );
        assert_eq!(
            cast(Value::List(vec![Element::Integer(1)]), TypeName::List)?,
            Value::List(vec![Element::Integer(1)])
        );
        Ok(())
    }
}
