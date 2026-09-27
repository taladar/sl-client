//! The descriptor types the generated table is made of, and lookups by name.

use sl_lsl::ast::TypeName;
use sl_types::lsl::{Rotation, Vector};

use crate::library::generated::{BUILTINS, BuiltinId, CONSTANTS, EVENTS};
use crate::value::Value;

/// One parameter of a library function or an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argument {
    /// The name the reference documents it under.
    pub name: &'static str,
    /// Its type.
    pub ty: TypeName,
}

/// One library function, as the reference defines it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Builtin {
    /// Its id.
    pub id: BuiltinId,
    /// The `ll*` name.
    pub name: &'static str,
    /// The parameters, in order.
    pub args: &'static [Argument],
    /// The return type; [`None`] for a function that returns nothing.
    pub ret: Option<TypeName>,
    /// The reference's forced delay after the call, in seconds.
    pub sleep: f32,
    /// The energy cost. Nothing enforces energy; it is kept so the served
    /// `LSLSyntax` document can state it.
    pub energy: f32,
    /// Whether the reference marks the function deprecated.
    pub deprecated: bool,
    /// Whether only a god may call it.
    pub god_mode: bool,
    /// The reference's description.
    pub tooltip: &'static str,
}

/// A library constant's value, typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstantValue {
    /// An `integer` constant.
    Integer(i32),
    /// A `float` constant.
    Float(f32),
    /// A `string` constant. `NULL_KEY` and the `TEXTURE_*` ids are strings
    /// in LSL, not keys.
    String(&'static str),
    /// A `key` constant (the reference defines none today).
    Key(&'static str),
    /// A `vector` constant.
    Vector([f32; 3]),
    /// A `rotation` constant.
    Rotation([f32; 4]),
}

impl ConstantValue {
    /// The constant's type.
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

    /// The constant as a runtime value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Integer(value) => Value::Integer(*value),
            Self::Float(value) => Value::Float(*value),
            Self::String(text) => Value::String((*text).to_owned()),
            Self::Key(text) => Value::Key((*text).to_owned()),
            Self::Vector([x, y, z]) => Value::Vector(Vector {
                x: *x,
                y: *y,
                z: *z,
            }),
            Self::Rotation([x, y, z, s]) => Value::Rotation(Rotation {
                x: *x,
                y: *y,
                z: *z,
                s: *s,
            }),
        }
    }
}

/// One library constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constant {
    /// Its name, e.g. `PI` or `CHANGED_INVENTORY`.
    pub name: &'static str,
    /// Its value.
    pub value: ConstantValue,
    /// The reference's description.
    pub tooltip: &'static str,
}

/// One event a script may handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Its name, e.g. `touch_start`.
    pub name: &'static str,
    /// The handler's parameters, in order.
    pub args: &'static [Argument],
    /// The reference's description.
    pub tooltip: &'static str,
}

/// The library function called `name`, if there is one.
#[must_use]
pub fn builtin(name: &str) -> Option<&'static Builtin> {
    let index = BUILTINS
        .binary_search_by(|builtin| builtin.name.cmp(name))
        .ok()?;
    BUILTINS.get(index)
}

/// The library constant called `name`, if there is one.
#[must_use]
pub fn constant(name: &str) -> Option<&'static Constant> {
    let index = CONSTANTS
        .binary_search_by(|constant| constant.name.cmp(name))
        .ok()?;
    CONSTANTS.get(index)
}

/// The event called `name`, if there is one.
#[must_use]
pub fn event(name: &str) -> Option<&'static Event> {
    let index = EVENTS.binary_search_by(|event| event.name.cmp(name)).ok()?;
    EVENTS.get(index)
}
