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
    /// Whose library the function is: Second Life's, or one of the families
    /// only OpenSim's engines compile.
    pub family: Family,
    /// What OpenSim checks before it runs the function.
    pub gate: Gate,
    /// What a deprecated OSSL function's warning names in its place
    /// (`Use of function <name> is deprecated. Use <replacement> instead.`).
    pub replacement: Option<&'static str>,
}

/// Whose library a function is.
///
/// Second Life compiles only [`Self::Linden`]; OpenSim's engines compile
/// every family (book chapter `gridspec/lsl.md`, § OSSL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    /// Linden Lab's `ll*` library, as Second Life serves it.
    Linden,
    /// An `ll*` function only OpenSim has (`llPointAt`, `llRemoteLoadScript`,
    /// `llStopPointAt`).
    OpenSim,
    /// OpenSim's own `os*` functions (OSSL).
    Ossl,
    /// OpenSim's `ls*` functions (LightShare).
    LightShare,
    /// OpenSim's `mod*` functions, a script's line to region modules.
    Mod,
}

impl Family {
    /// Whether only OpenSim's engines compile the family.
    #[must_use]
    pub const fn is_opensim(self) -> bool {
        !matches!(self, Self::Linden)
    }
}

/// What OpenSim checks before it runs a function: the permission model of
/// its `[OSSL]` configuration (`OSSL_Api.CheckThreatLevel`), or a family's
/// switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Nothing: the function runs wherever it compiles.
    Unchecked,
    /// Only that OSSL is enabled at all (`AllowOSFunctions`).
    Enabled,
    /// The region's per-function permission: an `Allow_<name>` line when the
    /// configuration has one, otherwise the threat level against
    /// `OSFunctionThreatLevel`.
    Threat {
        /// How dangerous OpenSim rates the function.
        level: ThreatLevel,
        /// The name the check looks the `Allow_` line up under and its
        /// denial names: the function's own, but for the few that share
        /// another's (`osSetDynamicTextureDataFace` is checked as
        /// `osSetDynamicTextureData`).
        name: &'static str,
    },
    /// Only that LightShare is enabled (`AllowLightShareFunctions`).
    LightShare,
    /// Only that MOD is enabled (`AllowMODFunctions`).
    Mod,
}

/// OSSL's threat levels, lowest first (`ThreatLevel` in `IOSSL_Api.cs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThreatLevel {
    /// Allowed nowhere by level.
    NoAccess,
    /// No threat at all.
    None,
    /// Can cause a nuisance, such as log spew.
    Nuisance,
    /// Very low.
    VeryLow,
    /// Low.
    Low,
    /// Moderate.
    Moderate,
    /// High.
    High,
    /// Very high.
    VeryHigh,
    /// Severe.
    Severe,
}

impl ThreatLevel {
    /// The level a configuration names, as `OSFunctionThreatLevel` spells
    /// it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "NoAccess" => Self::NoAccess,
            "None" => Self::None,
            "Nuisance" => Self::Nuisance,
            "VeryLow" => Self::VeryLow,
            "Low" => Self::Low,
            "Moderate" => Self::Moderate,
            "High" => Self::High,
            "VeryHigh" => Self::VeryHigh,
            "Severe" => Self::Severe,
            _ => return None,
        })
    }

    /// The level's name, as a denial words it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoAccess => "NoAccess",
            Self::None => "None",
            Self::Nuisance => "Nuisance",
            Self::VeryLow => "VeryLow",
            Self::Low => "Low",
            Self::Moderate => "Moderate",
            Self::High => "High",
            Self::VeryHigh => "VeryHigh",
            Self::Severe => "Severe",
        }
    }
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
    /// Whether only OpenSim's engines know it (`OS_LISTEN_REGEX_MESSAGE`,
    /// `WL_WATER_COLOR`, …).
    pub opensim_only: bool,
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

/// The library function called `name`, if there is one: its first
/// overload, where OpenSim has several ([`overloads`]).
#[must_use]
pub fn builtin(name: &str) -> Option<&'static Builtin> {
    overloads(name).first()
}

/// Every overload of the library function called `name`, in the order its
/// document lists them; empty when there is no such function. Second Life's
/// functions have one; some of OpenSim's several, which its engines tell
/// apart by their arguments (`osApproxEquals` of two floats, of two vectors,
/// with a margin, …).
#[must_use]
pub fn overloads(name: &str) -> &'static [Builtin] {
    let start = BUILTINS.partition_point(|builtin| builtin.name < name);
    let end = BUILTINS.partition_point(|builtin| builtin.name <= name);
    BUILTINS.get(start..end).unwrap_or_default()
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
