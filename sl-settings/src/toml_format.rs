//! The on-disk TOML representation of a settings scope.
//!
//! A scope is saved as a TOML document: each persisted override is a
//! `name = value` line, preceded by its declared comment, and grouped into a
//! nested `[section.subsection]` table by the declaration's section path. The
//! *type* of a value is never written — it is fixed by the setting's
//! declaration in code (the reference viewer's `settings.xml` works the same
//! way), so the file stays readable and hand-editable:
//!
//! ```toml
//! [spacenav.flycam]
//! # Flycam axis scaler
//! FlycamAxisScale0 = 1.5
//! ```
//!
//! On load the declared [`SettingKind`] drives coercion: a value that cannot be
//! read as its declared type (a type changed across versions) is dropped, and a
//! value for a setting this build does not declare is kept by inferring a
//! [`SettingValue`] from its TOML shape, so a newer version's setting survives a
//! round-trip through an older one.

use std::collections::BTreeMap;

use toml_edit::{Array, DocumentMut, Item, Table, TomlError, Value};

use crate::value::{SettingKind, SettingValue};

/// One override to persist: its name, value, the section path it is grouped
/// under, and the declared comment placed above it (both empty for an override
/// of a setting this build does not declare).
pub(crate) struct Entry<'a> {
    /// The setting's name (the leaf key in its section).
    pub name: &'a str,
    /// The value written to disk.
    pub value: &'a SettingValue,
    /// The section path the setting is grouped under (empty for the document
    /// root).
    pub section: &'a [String],
    /// The comment written on the line above the value (empty for none).
    pub comment: &'a str,
}

/// A node in the section tree assembled before rendering: the leaf settings
/// placed directly in this section, plus the nested child sections.
#[derive(Default)]
struct Node {
    /// The settings placed directly in this section, in the order inserted
    /// (name-sorted by the caller).
    leaves: Vec<Leaf>,
    /// The nested child sections, keyed (and thus rendered) by segment name.
    children: BTreeMap<String, Self>,
}

/// A single rendered setting within a [`Node`].
struct Leaf {
    /// The leaf key name.
    name: String,
    /// The already-encoded TOML value.
    value: Value,
    /// The comment for the line above (empty for none).
    comment: String,
}

/// Render a scope's persistable overrides to a TOML document string.
///
/// Entries are expected name-sorted; sections render in sorted order, and all
/// document-root settings render before any `[section]` header (as TOML
/// requires).
pub(crate) fn to_toml(entries: &[Entry<'_>]) -> String {
    let mut root = Node::default();
    for entry in entries {
        let node = entry.section.iter().fold(&mut root, |node, segment| {
            node.children.entry(segment.clone()).or_default()
        });
        node.leaves.push(Leaf {
            name: entry.name.to_owned(),
            value: setting_to_value(entry.value),
            comment: entry.comment.to_owned(),
        });
    }

    let mut doc = DocumentMut::new();
    render(doc.as_table_mut(), &root, true);
    doc.to_string()
}

/// Render a [`Node`] into a TOML table: its leaves first (each preceded by its
/// comment), then its child sections. A `[section]` header gets a blank line
/// before it, and a pure-parent section (children but no leaves of its own) is
/// implicit so only the deepest dotted header prints.
fn render(table: &mut Table, node: &Node, is_root: bool) {
    for leaf in &node.leaves {
        let _prev = table.insert(&leaf.name, Item::Value(leaf.value.clone()));
        if let Some(mut key) = table.key_mut(&leaf.name) {
            key.leaf_decor_mut()
                .set_prefix(comment_prefix(&leaf.comment));
        }
    }
    for (segment, child) in &node.children {
        let mut child_table = Table::new();
        render(&mut child_table, child, false);
        if child.leaves.is_empty() {
            child_table.set_implicit(true);
        } else if !is_root {
            // A blank line separates this section header from the content above
            // it; the document root has nothing above its first header.
            child_table.decor_mut().set_prefix("\n");
        }
        let _prev = table.insert(segment, Item::Table(child_table));
    }
}

/// The leading decoration for a setting's key: its comment on the line above,
/// or nothing. A multi-line comment has each line prefixed with `# `.
fn comment_prefix(comment: &str) -> String {
    if comment.is_empty() {
        return String::new();
    }
    let mut prefix = String::new();
    for line in comment.lines() {
        prefix.push_str("# ");
        prefix.push_str(line);
        prefix.push('\n');
    }
    prefix
}

/// Encode a [`SettingValue`] as the TOML [`Value`] written to disk (no type
/// tag — the type is recovered from the declaration on load).
fn setting_to_value(value: &SettingValue) -> Value {
    match value {
        SettingValue::Bool(flag) => Value::from(*flag),
        SettingValue::I32(number) => Value::from(i64::from(*number)),
        SettingValue::U32(number) => Value::from(i64::from(*number)),
        SettingValue::F32(number) => f32_value(*number),
        SettingValue::String(text) => Value::from(text.clone()),
        SettingValue::Color3([r, g, b]) => f32_array_value(&[*r, *g, *b]),
        SettingValue::Color4([r, g, b, a]) => f32_array_value(&[*r, *g, *b, *a]),
        SettingValue::Vec3([x, y, z]) => f32_array_value(&[*x, *y, *z]),
        SettingValue::Vec3d([x, y, z]) => f64_array_value(&[*x, *y, *z]),
        SettingValue::Rect([left, top, right, bottom]) => {
            i32_array_value(&[*left, *top, *right, *bottom])
        }
    }
}

/// A TOML float value built from an `f32`, keeping the shortest representation
/// that round-trips the `f32` (rather than widening to `f64` first, which would
/// print spurious trailing digits).
fn f32_value(number: f32) -> Value {
    f32_repr(number)
        .parse::<Value>()
        .unwrap_or_else(|_error| Value::from(f64::from(number)))
}

/// The shortest TOML float literal that round-trips `number`, always carrying a
/// fractional part so it is unambiguously a float.
fn f32_repr(number: f32) -> String {
    if number.is_nan() {
        return "nan".to_owned();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_owned();
    }
    let text = format!("{number}");
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// The shortest TOML float literal that round-trips an `f64` value.
fn f64_repr(number: f64) -> String {
    if number.is_nan() {
        return "nan".to_owned();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_owned();
    }
    let text = format!("{number}");
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// A TOML array of floats from an `f32` slice.
fn f32_array_value(numbers: &[f32]) -> Value {
    let mut array = Array::new();
    for &number in numbers {
        array.push(f32_value(number));
    }
    Value::from(array)
}

/// A TOML array of floats from an `f64` slice.
fn f64_array_value(numbers: &[f64]) -> Value {
    let mut array = Array::new();
    for &number in numbers {
        let value = f64_repr(number)
            .parse::<Value>()
            .unwrap_or_else(|_error| Value::from(number));
        array.push(value);
    }
    Value::from(array)
}

/// A TOML array of integers from an `i32` slice.
fn i32_array_value(numbers: &[i32]) -> Value {
    let mut array = Array::new();
    for &number in numbers {
        array.push(i64::from(number));
    }
    Value::from(array)
}

/// Parse a TOML document into a scope's overrides, keyed by setting name.
///
/// `declared_kind` gives the declared [`SettingKind`] of a name, if this build
/// declares it. A declared setting is coerced to its declared type (dropped if
/// the file's value no longer fits it); an undeclared setting is kept by
/// inferring a [`SettingValue`] from its TOML shape.
///
/// # Errors
///
/// [`TomlError`] if `text` is not valid TOML.
pub(crate) fn from_toml(
    text: &str,
    declared_kind: &impl Fn(&str) -> Option<SettingKind>,
) -> Result<BTreeMap<String, SettingValue>, TomlError> {
    let doc = text.parse::<DocumentMut>()?;
    let mut out = BTreeMap::new();
    collect(doc.as_table(), declared_kind, &mut out);
    Ok(out)
}

/// Walk a table (recursing into nested sections), reading each `name = value`
/// leaf into `out`.
fn collect(
    table: &Table,
    declared_kind: &impl Fn(&str) -> Option<SettingKind>,
    out: &mut BTreeMap<String, SettingValue>,
) {
    for (key, item) in table {
        match item {
            Item::Value(value) => {
                if let Some(setting) = read_setting(key, value, declared_kind) {
                    let _prev = out.insert(key.to_owned(), setting);
                }
            }
            Item::Table(sub) => collect(sub, declared_kind, out),
            Item::ArrayOfTables(_) | Item::None => {}
        }
    }
}

/// Turn one leaf `name = value` into a [`SettingValue`]: coerce to the declared
/// type if `name` is declared, otherwise infer from the TOML shape. `None`
/// drops the entry.
fn read_setting(
    name: &str,
    value: &Value,
    declared_kind: &impl Fn(&str) -> Option<SettingKind>,
) -> Option<SettingValue> {
    match declared_kind(name) {
        Some(kind) => coerce(value, kind),
        None => infer(value),
    }
}

/// Coerce a TOML value into a specific declared kind, or `None` if it does not
/// fit (the setting's type changed since the file was written).
fn coerce(value: &Value, kind: SettingKind) -> Option<SettingValue> {
    match kind {
        SettingKind::Bool => value.as_bool().map(SettingValue::Bool),
        SettingKind::I32 => value_as_i32(value).map(SettingValue::I32),
        SettingKind::U32 => value_as_u32(value).map(SettingValue::U32),
        SettingKind::F32 => value_as_f32(value).map(SettingValue::F32),
        SettingKind::String => value
            .as_str()
            .map(|text| SettingValue::String(text.to_owned())),
        SettingKind::Color3 => array_f32::<3>(value).map(SettingValue::Color3),
        SettingKind::Color4 => array_f32::<4>(value).map(SettingValue::Color4),
        SettingKind::Vec3 => array_f32::<3>(value).map(SettingValue::Vec3),
        SettingKind::Vec3d => array_f64::<3>(value).map(SettingValue::Vec3d),
        SettingKind::Rect => array_i32::<4>(value).map(SettingValue::Rect),
    }
}

/// Infer a [`SettingValue`] from the TOML shape of an undeclared setting, so it
/// round-trips through this build. The choice among same-shape types (an RGB
/// colour vs an `f32` vector) is arbitrary but preserves the on-disk value.
fn infer(value: &Value) -> Option<SettingValue> {
    if let Some(flag) = value.as_bool() {
        return Some(SettingValue::Bool(flag));
    }
    if let Some(number) = value.as_integer() {
        return i32::try_from(number).ok().map(SettingValue::I32);
    }
    if let Some(number) = value.as_float() {
        return f64_to_f32(number).map(SettingValue::F32);
    }
    if let Some(text) = value.as_str() {
        return Some(SettingValue::String(text.to_owned()));
    }
    let array = value.as_array()?;
    if array.iter().all(Value::is_integer) {
        return match array.len() {
            4 => array_i32::<4>(value).map(SettingValue::Rect),
            _ => array_f32::<3>(value).map(SettingValue::Vec3),
        };
    }
    match array.len() {
        4 => array_f32::<4>(value).map(SettingValue::Color4),
        _ => array_f32::<3>(value).map(SettingValue::Vec3),
    }
}

/// Read a TOML value as an `i32` (an integer within range).
fn value_as_i32(value: &Value) -> Option<i32> {
    value
        .as_integer()
        .and_then(|number| i32::try_from(number).ok())
}

/// Read a TOML value as a `u32` (a non-negative integer within range).
fn value_as_u32(value: &Value) -> Option<u32> {
    value
        .as_integer()
        .and_then(|number| u32::try_from(number).ok())
}

/// Read a TOML value as an `f32`, accepting an integer literal too, parsing the
/// source representation so an `f32` written by [`f32_value`] round-trips
/// exactly.
fn value_as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Float(number) => number.display_repr().parse::<f32>().ok(),
        Value::Integer(number) => number.display_repr().parse::<f32>().ok(),
        _ => None,
    }
}

/// Read a TOML value as an `f64`, accepting an integer literal too.
fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Float(number) => Some(*number.value()),
        Value::Integer(number) => number.display_repr().parse::<f64>().ok(),
        _ => None,
    }
}

/// Narrow an inferred `f64` to `f32` for an undeclared float setting, keeping
/// the source representation so the value round-trips.
fn f64_to_f32(number: f64) -> Option<f32> {
    format!("{number}").parse::<f32>().ok()
}

/// Read a TOML array of exactly `N` floats.
fn array_f32<const N: usize>(value: &Value) -> Option<[f32; N]> {
    let array = value.as_array()?;
    if array.len() != N {
        return None;
    }
    let mut out = Vec::with_capacity(N);
    for element in array {
        out.push(value_as_f32(element)?);
    }
    <[f32; N]>::try_from(out).ok()
}

/// Read a TOML array of exactly `N` `f64`s.
fn array_f64<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let array = value.as_array()?;
    if array.len() != N {
        return None;
    }
    let mut out = Vec::with_capacity(N);
    for element in array {
        out.push(value_as_f64(element)?);
    }
    <[f64; N]>::try_from(out).ok()
}

/// Read a TOML array of exactly `N` `i32`s.
fn array_i32<const N: usize>(value: &Value) -> Option<[i32; N]> {
    let array = value.as_array()?;
    if array.len() != N {
        return None;
    }
    let mut out = Vec::with_capacity(N);
    for element in array {
        out.push(value_as_i32(element)?);
    }
    <[i32; N]>::try_from(out).ok()
}
