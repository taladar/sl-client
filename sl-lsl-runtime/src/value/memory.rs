//! What a value costs against a script's memory, as each grid's
//! `llGetUsedMemory` counts it — so a fake grid's script runs out where the
//! grid it imitates would, and a script that reads its own memory
//! (`llGetFreeMemory() < 2000`) branches where it would there.
//!
//! Every number is measured: the probe `memory/sizes` (book chapter
//! `gridspec/lsl.md`, § Memory and limits) read `llGetUsedMemory` around each
//! kind of value in each place a script holds one, three times over, on
//! aditi (Mono) and on both OpenSim engines; the transcripts are in
//! `probes/results/`. YEngine's numbers are also its source's
//! (`XMRHeapTracker.Size`, `LSL_List.Size`).
//!
//! What Second Life's Mono counts is the heap reachable from the script's
//! globals: a local, a function argument or a call frame shows nothing, even
//! a fresh string held only by a local. A value held in a list is a
//! reference into the list plus the value as an object of its own; a list of
//! one shared string literal holds the literal once and a reference per
//! element ([`MONO_REFERENCE`]). The value model has no object identity, so
//! [`Literals`] stands in for it: an element whose text is one of the
//! script's string literals is taken to *be* that literal, paid for once in
//! its list. The whole of `llGetUsedMemory` also counts the script's code,
//! which is not a value's and is not here (`vm`'s memory accounting models
//! it): a script that only reads its memory and says it once used 4 456
//! bytes on aditi.
//!
//! A string's length is counted in UTF-16 code units, as .NET counts one:
//! U+00E9 and U+20AC, two and three bytes in UTF-8, each cost what an ASCII
//! character costs on every grid. No character outside the Basic
//! Multilingual Plane was measured.
//!
//! XEngine counts nothing: every memory function answers 65 536 there, and
//! every cost here is 0.

use std::collections::BTreeSet;

use sl_lsl::ast::TypeName;

use crate::value::{Element, Value};
use crate::vm::EngineFlavour;

/// The memory limit a script starts with, `llGetMemoryLimit`'s answer: 65 536
/// bytes on Second Life's Mono and on both OpenSim engines.
pub const MEMORY_LIMIT: u32 = 0x0001_0000;

/// YEngine: the heap a script's values may hold, `[YEngine] ScriptHeapSize`'s
/// default of 1 024 KiB. `llGetFreeMemory` is this less what is used; a store
/// past it is an `OutOfHeapException`.
pub const YENGINE_HEAP: u32 = 0x0010_0000;

/// Mono: what a reference to a value costs — a list's slot for one element.
/// A list of one shared string pays the string once and this per element.
pub const MONO_REFERENCE: u32 = 4;

/// Mono: what a list costs once it holds anything, over the empty list a
/// global list starts as.
const MONO_LIST: u32 = 64;

/// Mono: a string object less its characters (`""` costs this; each UTF-16
/// unit 2 more).
const MONO_STRING: u32 = 14;

/// Mono: an integer or a float boxed into a list.
const MONO_BOXED_NUMBER: u32 = 12;

/// Mono: a vector boxed into a list.
const MONO_BOXED_VECTOR: u32 = 20;

/// Mono: a rotation boxed into a list.
const MONO_BOXED_ROTATION: u32 = 24;

/// Mono: a key boxed into a list, less the string it wraps.
const MONO_BOXED_KEY: u32 = 12;

/// Where a script holds a value, for what its grid charges the value there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Holder {
    /// A global variable.
    Global,
    /// A local variable of the function or event handler running.
    Local,
    /// A function's parameter, holding what its caller passed.
    Argument,
}

/// The string literals of a script, standing in for the object identity the
/// value model does not have: on Mono a list element whose text is one of
/// them is taken to be that literal, which every element holding it shares.
///
/// A string the script computed that happens to equal a literal is taken for
/// it too, and so costs a reference where Mono would pay a copy; content
/// rarely builds a literal's text by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Literals(BTreeSet<String>);

impl Literals {
    /// The string literals among `constants`, a program's literal pool.
    #[must_use]
    pub fn of(constants: &[Value]) -> Self {
        Self(
            constants
                .iter()
                .filter_map(|constant| match constant {
                    Value::String(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
        )
    }

    /// Whether `text` is one of the literals.
    #[must_use]
    pub fn contains(&self, text: &str) -> bool {
        self.0.contains(text)
    }
}

/// What `text` costs at `per_unit` bytes a UTF-16 code unit.
fn per_unit(text: &str, per_unit: u32) -> u32 {
    u32::try_from(text.encode_utf16().count())
        .unwrap_or(u32::MAX)
        .saturating_mul(per_unit)
}

/// Mono: a string as an object of its own, characters included.
fn mono_string(text: &str) -> u32 {
    MONO_STRING.saturating_add(per_unit(text, 2))
}

/// What declaring a global of type `ty` costs in `flavour`, the global
/// holding its default: on Mono 20 bytes for an integer, a float or a key,
/// 34 for a string, 56 for a list, 60 for a vector and 64 for a rotation (32
/// globals of each beside none, `memory/sizes`); on YEngine the default's
/// heap size — 4 for an integer, 8 for a float (a double), 24 for a vector
/// and 32 for a rotation, nothing for an empty string, key or list.
#[must_use]
pub const fn global_cost(ty: TypeName, flavour: EngineFlavour) -> u32 {
    match flavour {
        EngineFlavour::SecondLife => match ty {
            TypeName::Integer | TypeName::Float | TypeName::Key => 20,
            TypeName::String => 34,
            TypeName::List => 56,
            TypeName::Vector => 60,
            TypeName::Rotation => 64,
        },
        EngineFlavour::YEngine => match ty {
            TypeName::Integer => 4,
            TypeName::Float => 8,
            TypeName::Vector => 24,
            TypeName::Rotation => 32,
            TypeName::String | TypeName::Key | TypeName::List => 0,
        },
        EngineFlavour::XEngine => 0,
    }
}

/// What holding `value` in `holder` costs in `flavour` over holding
/// its type's default there; a global's whole cost is this plus
/// [`global_cost`]. `literals` are the script's string literals, which a
/// Mono list shares ([`Literals`]).
///
/// - **Mono**: a global string 2 bytes a UTF-16 unit; a global list 64
///   once it holds anything, plus each element's [`element_cost`] — but an
///   element holding one of the `literals` pays the literal only the first
///   time the list holds it, and a reference after; a global key nothing,
///   whatever its text (an empty key and a 36-character one read alike); a
///   number, vector or rotation nothing over the declaration. A local or an
///   argument costs nothing at all.
/// - **YEngine**: a global string or key 2 bytes a UTF-16 unit, a global
///   list its elements'; a local string 2 a unit and a local list its
///   elements', where a local number, vector, rotation or key costs
///   nothing; an argument nothing beyond what its caller holds. A string
///   that is empty or all white space costs nothing (`HeapTrackerString.Size`
///   uses `string.IsNullOrWhiteSpace`).
/// - **XEngine**: nothing.
#[must_use]
pub fn value_cost(
    value: &Value,
    flavour: EngineFlavour,
    holder: Holder,
    literals: &Literals,
) -> u32 {
    let elements = |list: &[Element]| {
        list.iter().fold(0_u32, |sum, element| {
            sum.saturating_add(element_cost(element, flavour))
        })
    };
    match (flavour, holder, value) {
        (EngineFlavour::SecondLife, Holder::Global, Value::List(list)) if !list.is_empty() => {
            MONO_LIST.saturating_add(mono_elements(list, literals))
        }
        (EngineFlavour::SecondLife, Holder::Global, Value::String(text)) => per_unit(text, 2),
        (EngineFlavour::YEngine, Holder::Global, Value::String(text) | Value::Key(text))
        | (EngineFlavour::YEngine, Holder::Local, Value::String(text)) => {
            if text.chars().all(char::is_whitespace) {
                0
            } else {
                per_unit(text, 2)
            }
        }
        (EngineFlavour::YEngine, Holder::Global | Holder::Local, Value::List(list)) => {
            elements(list)
        }
        _ => 0,
    }
}

/// Mono: the elements of one list, each literal it holds paid once and a
/// reference every later time.
fn mono_elements(list: &[Element], literals: &Literals) -> u32 {
    let mut shared: BTreeSet<&str> = BTreeSet::new();
    list.iter().fold(0_u32, |sum, element| {
        let cost = match element {
            Element::String(text) if literals.contains(text) && !shared.insert(text) => {
                MONO_REFERENCE
            }
            other => element_cost(other, EngineFlavour::SecondLife),
        };
        sum.saturating_add(cost)
    })
}

/// What `element` costs as one element of a list in `flavour`.
///
/// - **Mono**: a reference ([`MONO_REFERENCE`]) and the value boxed —
///   16 bytes for an integer or a float, 24 for a vector, 28 for a
///   rotation, 18 and 2 a UTF-16 unit for a string, 30 and 2 a unit for
///   a key (102 for a UUID).
/// - **YEngine** (`LSL_List.Size`): 4 for an integer, 8 for a float, 32
///   for a vector, 64 for a rotation, and **1** a UTF-16 unit for a
///   string or a key — half what the same string costs held outside a
///   list.
/// - **XEngine**: nothing.
#[must_use]
pub fn element_cost(element: &Element, flavour: EngineFlavour) -> u32 {
    match flavour {
        EngineFlavour::SecondLife => MONO_REFERENCE.saturating_add(match element {
            Element::Integer(_) | Element::Float(_) => MONO_BOXED_NUMBER,
            Element::Vector(_) => MONO_BOXED_VECTOR,
            Element::Rotation(_) => MONO_BOXED_ROTATION,
            Element::String(text) => mono_string(text),
            Element::Key(text) => MONO_BOXED_KEY.saturating_add(mono_string(text)),
        }),
        EngineFlavour::YEngine => match element {
            Element::Integer(_) => 4,
            Element::Float(_) => 8,
            Element::Vector(_) => 32,
            Element::Rotation(_) => 64,
            Element::String(text) | Element::Key(text) => per_unit(text, 1),
        },
        EngineFlavour::XEngine => 0,
    }
}

#[cfg(test)]
mod tests;
