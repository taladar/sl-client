//! The cost function held to `memory/sizes` as each grid answered it.

use pretty_assertions::assert_eq;
use sl_lsl::ast::TypeName;
use sl_types::lsl::{Rotation, Vector};

use super::{Holder, Literals, MEMORY_LIMIT, element_cost, global_cost, value_cost};
use crate::probe::Line;
use crate::value::{Element, Value};
use crate::vm::EngineFlavour;

/// How far a row may sit from its median when no two of its three reads
/// agree: `llGetUsedMemory` on aditi wanders by up to a few hundred bytes
/// between two reads of nothing (`memory/budget`), and a row of large counts
/// rarely repeats. A row two of whose reads agree must match exactly.
const WANDER: u64 = 200;

/// A UUID-shaped key, as `llGenerateKey` makes one: 36 characters.
const UUID: &str = "5748decc-f629-461c-9a36-a35a221fe21f";

/// The probe's lines in one grid's transcript.
fn lines(transcript: &str) -> Vec<Line> {
    transcript
        .lines()
        .filter_map(|line| Line::parse(line).ok())
        .collect()
}

/// The literal the probe puts in every element of its shared-string list.
const SHARED: &str = "abcdefgh";

/// The probe's string literals, as far as the rows need them: the one its
/// shared-string list holds.
fn literals() -> Literals {
    Literals::of(&[Value::String(SHARED.to_owned())])
}

/// The field `key` of `line`.
fn field<'line>(line: &'line Line, key: &str) -> Result<&'line str, String> {
    line.field(key)
        .ok_or_else(|| format!("no {key} in {line:?}"))
}

/// The field `key` of `line`, as a number.
fn number(line: &Line, key: &str) -> Result<u32, String> {
    field(line, key)?
        .parse()
        .map_err(|error| format!("{key} of {line:?}: {error}"))
}

/// The comma-separated numbers of the field `key`.
fn numbers(line: &Line, key: &str) -> Result<Vec<i64>, String> {
    field(line, key)?
        .split(',')
        .map(|text| text.parse().map_err(|error| format!("{line:?}: {error}")))
        .collect()
}

/// Whether `model` is what a row of three reads says: exactly the value two
/// of them agree on, or within [`WANDER`] of their median when none do.
fn agrees(model: u32, reads: &[i64]) -> bool {
    let model = i64::from(model);
    let agreed = reads
        .iter()
        .find(|read| reads.iter().filter(|other| other == read).count() >= 2);
    if let Some(agreed) = agreed {
        return *agreed == model;
    }
    let mut sorted = reads.to_vec();
    sorted.sort_unstable();
    sorted
        .get(sorted.len().div_ceil(2).saturating_sub(1))
        .is_some_and(|median| median.abs_diff(model) <= WANDER)
}

/// `length` characters of the class the probe names.
fn text(class: &str, length: u32) -> String {
    let character = match class {
        "two-byte" => 'é',
        "three-byte" => '€',
        _ => 'a',
    };
    core::iter::repeat_n(character, usize::try_from(length).unwrap_or(0)).collect()
}

/// The element the probe puts in its list as the `index`th of `kind`.
fn element(line: &Line, kind: &str, index: u32) -> Result<Element, String> {
    let n = f32::from(u16::try_from(index).map_err(|error| error.to_string())?);
    Ok(match kind {
        "integer" => Element::Integer(i32::try_from(index).map_err(|error| error.to_string())?),
        "float" => Element::Float(n),
        "vector" => Element::Vector(Vector {
            x: n,
            y: 0.0,
            z: 0.0,
        }),
        "rotation" => Element::Rotation(Rotation {
            x: n,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }),
        "key" => Element::Key(UUID.to_owned()),
        "shared-string" => Element::String(SHARED.to_owned()),
        "string" => Element::String(text(field(line, "class")?, number(line, "length")?)),
        other => return Err(format!("an element of kind {other}")),
    })
}

/// A value of `kind` as the probe's local and argument functions hold four.
fn sample(kind: &str) -> Option<Value> {
    Some(match kind {
        "integer" => Value::Integer(1),
        "float" => Value::Float(1.0),
        "vector" => Value::Vector(Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        }),
        "rotation" => Value::Rotation(Rotation {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }),
        "string" => Value::String(text("ascii", 16)),
        "key" => Value::Key(UUID.to_owned()),
        "list" => Value::List(vec![Element::Integer(1)]),
        _ => return None,
    })
}

/// The type a Globals script declares, by its kind.
fn type_named(kind: &str) -> Result<TypeName, String> {
    Ok(match kind {
        "integer" => TypeName::Integer,
        "float" => TypeName::Float,
        "string" => TypeName::String,
        "key" => TypeName::Key,
        "vector" => TypeName::Vector,
        "rotation" => TypeName::Rotation,
        "list" => TypeName::List,
        other => return Err(format!("a global of kind {other}")),
    })
}

/// What the model says one row of the probe should read, or [`None`] for a
/// record it does not cost.
fn modelled(line: &Line, flavour: EngineFlavour) -> Result<Option<u32>, String> {
    let four = |value: Option<Value>, holder| {
        value.map_or(0, |value| {
            value_cost(&value, flavour, holder, &literals()).saturating_mul(4)
        })
    };
    Ok(Some(match line.rec.as_str() {
        "list" => {
            let kind = field(line, "kind")?;
            let count = number(line, "count")?;
            let elements = (0..count)
                .map(|index| element(line, kind, index))
                .collect::<Result<Vec<_>, _>>()?;
            // One literal shared by every element is the string once and a
            // reference for each of the rest, which the literals tell.
            value_cost(&Value::List(elements), flavour, Holder::Global, &literals())
        }
        "text" => value_cost(
            &Value::String(text(field(line, "class")?, number(line, "length")?)),
            flavour,
            Holder::Global,
            &literals(),
        ),
        "global-key" => value_cost(
            &Value::Key(UUID.to_owned()),
            flavour,
            Holder::Global,
            &literals(),
        ),
        "local" => four(sample(field(line, "kind")?), Holder::Local),
        "argument" => four(sample(field(line, "kind")?), Holder::Argument),
        // A call frame costs nothing anywhere.
        "frames" => 0,
        _ => return Ok(None),
    }))
}

/// Holds every row of one grid's transcript to the model, and returns how
/// many rows it checked.
fn check(transcript: &str, flavour: EngineFlavour) -> Result<u32, String> {
    let lines = lines(transcript);
    let mut checked = 0_u32;
    for line in &lines {
        if let Some(model) = modelled(line, flavour)? {
            let reads = numbers(line, "deltas")?;
            assert!(
                agrees(model, &reads),
                "{flavour:?}: {line:?} read {reads:?}, the model says {model}"
            );
            checked = checked.saturating_add(1);
        }
    }
    let limit = lines
        .iter()
        .find(|line| line.rec == "limit")
        .ok_or("no limit")?;
    assert_eq!(number(limit, "value")?, MEMORY_LIMIT, "{flavour:?}");

    // Each Globals script against Globals none: 32 globals, and the two
    // bytes a character its own name costs as the value of its KIND global.
    let used = |kind: &str| -> Result<i64, String> {
        let line = lines
            .iter()
            .find(|line| line.rec == "globals" && line.field("kind") == Some(kind))
            .ok_or_else(|| format!("no globals of {kind}"))?;
        numbers(line, "used")?
            .first()
            .copied()
            .ok_or_else(|| format!("{line:?}"))
    };
    let none = used("none")?;
    for kind in [
        "integer", "float", "string", "key", "vector", "rotation", "list",
    ] {
        let seen = used(kind)?;
        if flavour == EngineFlavour::XEngine {
            assert_eq!(seen, i64::from(MEMORY_LIMIT), "XEngine counts nothing");
            continue;
        }
        let name = i64::try_from(kind.len()).map_err(|error| error.to_string())?;
        let model = i64::from(global_cost(type_named(kind)?, flavour))
            .saturating_mul(32)
            .saturating_add(name.saturating_sub(4).saturating_mul(2));
        assert_eq!(
            seen.saturating_sub(none),
            model,
            "{flavour:?}: 32 {kind} globals"
        );
        checked = checked.saturating_add(1);
    }
    Ok(checked)
}

#[test]
fn every_value_costs_what_memory_sizes_read_on_each_grid() -> Result<(), String> {
    // 78 list rows, 35 string lengths, 3 keys, 8 locals, 8 arguments, 7
    // depths, and the 7 kinds of global on the grids that count them.
    let rows = 78 + 35 + 3 + 8 + 8 + 7;
    let grids = [
        (
            include_str!("../../../probes/results/aditi/memory/sizes.txt"),
            EngineFlavour::SecondLife,
            rows + 7,
        ),
        (
            include_str!("../../../probes/results/opensim-yengine/memory/sizes.txt"),
            EngineFlavour::YEngine,
            rows + 7,
        ),
        (
            include_str!("../../../probes/results/opensim-xengine/memory/sizes.txt"),
            EngineFlavour::XEngine,
            rows,
        ),
    ];
    for (transcript, flavour, expected) in grids {
        assert_eq!(check(transcript, flavour)?, expected, "{flavour:?}");
    }
    Ok(())
}

#[test]
fn a_mono_key_costs_a_uuid_in_a_list_and_nothing_more_in_a_global() {
    let key = Element::Key(UUID.to_owned());
    assert_eq!(element_cost(&key, EngineFlavour::SecondLife), 102);
    assert_eq!(element_cost(&key, EngineFlavour::YEngine), 36);
    assert_eq!(
        value_cost(
            &Value::Key(UUID.to_owned()),
            EngineFlavour::SecondLife,
            Holder::Global,
            &Literals::default()
        ),
        0
    );
    assert_eq!(
        value_cost(
            &Value::Key(UUID.to_owned()),
            EngineFlavour::YEngine,
            Holder::Global,
            &Literals::default()
        ),
        72
    );
}
