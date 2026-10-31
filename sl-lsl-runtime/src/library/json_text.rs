//! `llJson*` and `llList2Json`: JSON as each grid's library reads and writes
//! it.
//!
//! This is **not** ordinary JSON handling, on either grid. A value comes back
//! as a *string*, with LSL's own sentinels for what is not one —
//! [`JSON_INVALID`], [`JSON_NULL`], [`JSON_TRUE`], [`JSON_FALSE`], each a
//! single noncharacter (`U+FDD0` … `U+FDD8`) — so a caller tells "absent"
//! from the text that looks like absent only by convention; a specifier list
//! addresses nested objects and arrays by alternating keys and indices, with
//! `JSON_APPEND` for one past the end; and what counts as a number, a string
//! or a document is each grid's own (book chapter `gridspec/lsl.md`, § llJson*
//! and the linkset data store, measured by the probe `data/json`).
//!
//! The two grids share nothing but the names:
//!
//! - **Second Life** keeps the text. A value read or left untouched comes
//!   back as written (`2.50`, `-1.0e2`, `[ 1 ]`), the parser is a lenient
//!   scanner that takes a scalar for a document, and a level written to is
//!   rewritten compact around values kept as they were — `second_life`, from
//!   LSL PyOptimizer's `lsljson.py`, adjusted to what aditi answered.
//! - **OpenSim** parses with LitJson into a tree and writes the tree anew
//!   (`2.5`, `-100.0`); a document must start with `[` or `{`, escapes are
//!   decoded, single quotes accepted, and `JSON_APPEND` is the *string*
//!   `"-1"` — `opensim`, from `LSL_Api.cs` and OpenSim's LitJson.

mod opensim;
mod second_life;

use crate::library::{CallError, PureCtx};
use crate::value::Element;

/// `JSON_INVALID`: what a function answers for what it cannot read or do.
pub const JSON_INVALID: &str = "\u{FDD0}";
/// `JSON_OBJECT`: the type of an object, and `llList2Json`'s object kind.
pub const JSON_OBJECT: &str = "\u{FDD1}";
/// `JSON_ARRAY`: the type of an array, and `llList2Json`'s array kind.
pub const JSON_ARRAY: &str = "\u{FDD2}";
/// `JSON_NUMBER`: the type of a number.
pub const JSON_NUMBER: &str = "\u{FDD3}";
/// `JSON_STRING`: the type of a string.
pub const JSON_STRING: &str = "\u{FDD4}";
/// `JSON_NULL`: `null`, as a value and as a type.
pub const JSON_NULL: &str = "\u{FDD5}";
/// `JSON_TRUE`: `true`, as a value and as a type.
pub const JSON_TRUE: &str = "\u{FDD6}";
/// `JSON_FALSE`: `false`, as a value and as a type.
pub const JSON_FALSE: &str = "\u{FDD7}";
/// `JSON_DELETE`: as `llJsonSetValue`'s value, remove what the path names.
pub const JSON_DELETE: &str = "\u{FDD8}";

/// `llList2Json`: `values` as a JSON array (`kind` [`JSON_ARRAY`]) or an
/// object of their pairs ([`JSON_OBJECT`]); [`JSON_INVALID`] for any other
/// kind, an odd count of pairs or a key that is not a string.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_json<C: PureCtx>(
    ctx: &mut C,
    kind: String,
    values: Vec<Element>,
) -> Result<String, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        opensim::list2_json(&kind, &values)
    } else {
        second_life::list2_json(&kind, &values)
    })
}

/// `llJson2List`: the elements of a JSON array, the keys and values of an
/// object in turn, or a scalar as one element; a nested array or object is
/// one element, the string of its text.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_json2_list<C: PureCtx>(ctx: &mut C, json: String) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        opensim::json2_list(&json)
    } else {
        second_life::json2_list(&json)
    })
}

/// `llJsonGetValue`: the value `specifiers` names in `json` — a string's
/// text, a number's or a container's JSON, or the sentinel of `true`,
/// `false` and `null` — or [`JSON_INVALID`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_json_get_value<C: PureCtx>(
    ctx: &mut C,
    json: String,
    specifiers: Vec<Element>,
) -> Result<String, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        opensim::get_value(&json, &specifiers)
    } else {
        second_life::get_value(&json, &specifiers)
    })
}

/// `llJsonValueType`: the type sentinel of the value `specifiers` names in
/// `json`, or [`JSON_INVALID`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_json_value_type<C: PureCtx>(
    ctx: &mut C,
    json: String,
    specifiers: Vec<Element>,
) -> Result<String, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        opensim::value_type(&json, &specifiers)
    } else {
        second_life::value_type(&json, &specifiers)
    })
}

/// `llJsonSetValue`: `json` with the value `specifiers` names set to `value`
/// — made, with the containers on its way, where it is not there yet — or
/// removed for [`JSON_DELETE`]; [`JSON_INVALID`] when the path cannot be
/// written.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_json_set_value<C: PureCtx>(
    ctx: &mut C,
    json: String,
    specifiers: Vec<Element>,
    value: String,
) -> Result<String, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        opensim::set_value(&json, &specifiers, &value)
    } else {
        second_life::set_value(&json, &specifiers, &value)
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::constant;
    use crate::value::Value;

    /// The sentinels are the table's `JSON_*` constants.
    #[test]
    fn the_sentinels_are_the_tables() {
        for (name, text) in [
            ("JSON_INVALID", JSON_INVALID),
            ("JSON_OBJECT", JSON_OBJECT),
            ("JSON_ARRAY", JSON_ARRAY),
            ("JSON_NUMBER", JSON_NUMBER),
            ("JSON_STRING", JSON_STRING),
            ("JSON_NULL", JSON_NULL),
            ("JSON_TRUE", JSON_TRUE),
            ("JSON_FALSE", JSON_FALSE),
            ("JSON_DELETE", JSON_DELETE),
        ] {
            assert_eq!(
                constant(name).map(|constant| constant.value.to_value()),
                Some(Value::String(text.to_owned())),
                "{name}"
            );
        }
    }
}
