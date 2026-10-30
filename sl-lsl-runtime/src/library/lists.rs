//! Library tranche: lists (`server-lsl-lib-strings-lists`) — reading
//! elements, cutting and splicing, searching, sorting, statistics,
//! shuffling, and lists to and from text.
//!
//! Second Life's answers are PyOptimizer's (`lslbasefuncs.py`), held to
//! aditi's transcripts of `strings/functions`, `strings/conversions`,
//! `strings/newer`, `errors/bounds` and `errors/bounds-newer`; OpenSim's are
//! ported from `LSL_Api.cs` and `LSL_Types.list` and held to the local
//! grid's. A float an OpenSim list holds is a double there; here it is the
//! value model's `f32` on every grid, so the digits of a printed float are
//! Second Life's whatever the flavour.

use sl_types::lsl::{Rotation, Vector};

use crate::cast::element_to_string;
use crate::format;
use crate::library::dotnet::{is_uuid, parse_double};
use crate::library::range::{
    os_delete_list, os_get, os_insert, os_replace, py, signed, sl_delete, sl_get, sl_get_pieces,
    sl_insert, sl_replace,
};
use crate::library::text::{Text, until_nul};
use crate::library::{BuiltinId, CallError, Exception, Key, PureCtx};
use crate::num::{float_to_int, int_to_float, to_f32};
use crate::parse;
use crate::value::{Element, ZERO_ROTATION, ZERO_VECTOR};
use crate::vm::{EngineFlavour, ScriptCtx};

/// `llGetListLength`: the number of elements.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_list_length<C>(_ctx: &mut C, list: Vec<Element>) -> Result<i32, CallError> {
    Ok(length(&list))
}

/// A length as LSL's integer.
fn length<T>(items: &[T]) -> i32 {
    i32::try_from(items.len()).unwrap_or(i32::MAX)
}

/// The element at `index`, counted from the end when negative.
fn at(list: &[Element], index: i32) -> Option<&Element> {
    let index = if index < 0 {
        i64::from(index).saturating_add(signed(list.len()))
    } else {
        i64::from(index)
    };
    list.get(usize::try_from(index).ok()?)
}

/// `TYPE_INTEGER` … `TYPE_ROTATION`, and `TYPE_INVALID` (0).
const fn type_code(element: &Element) -> i32 {
    match element {
        Element::Integer(_) => 1,
        Element::Float(_) => 2,
        Element::String(_) => 3,
        Element::Key(_) => 4,
        Element::Vector(_) => 5,
        Element::Rotation(_) => 6,
    }
}

/// OpenSim's type of an element: its strings and keys are one type there,
/// told apart by whether the text is a UUID (`LSL_Api.llGetListEntryType`),
/// so a key literal `(key)"k"` is a string and the string `NULL_KEY` a key.
fn os_type_code(element: &Element) -> i32 {
    match element {
        Element::String(text) | Element::Key(text) => {
            if is_uuid(text) {
                4
            } else {
                3
            }
        }
        other => type_code(other),
    }
}

/// `llGetListEntryType`: the type of the element at `index`, or
/// `TYPE_INVALID` past either end. OpenSim tells strings from keys by their
/// text (see `os_type_code`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_list_entry_type<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<i32, CallError> {
    let opensim = ctx.flavour().is_opensim();
    Ok(at(&list, index).map_or(0, |element| {
        if opensim {
            os_type_code(element)
        } else {
            type_code(element)
        }
    }))
}

/// `llList2String`: the element as `(string)` of a list prints it (a vector
/// with six decimals), or `""` past either end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_string<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<String, CallError> {
    Ok(at(&list, index).map(element_to_string).unwrap_or_default())
}

/// `llList2Integer`: the element as an integer — a float truncated on
/// Second Life and rounded half to even on OpenSim (`Convert.ToInt32`,
/// which throws past the integer's range), a string or key read as
/// `(integer)` reads it — or `0` for a vector, a rotation and past either
/// end (`errors/bounds`).
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a float out of the integer's
/// range, or NaN.
pub fn ll_list2_integer<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<i32, CallError> {
    let opensim = ctx.flavour().is_opensim();
    Ok(match at(&list, index) {
        Some(Element::Integer(integer)) => *integer,
        Some(Element::Float(float)) if opensim => {
            let rounded = f64::from(*float).round_ties_even();
            if rounded.is_nan() || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded) {
                return Err(CallError::Exception {
                    id: BuiltinId::LlList2Integer,
                    exception: Exception::Overflow,
                });
            }
            float_to_int(to_f32(rounded))
        }
        Some(Element::Float(float)) => float_to_int(*float),
        Some(Element::String(text)) => parse::integer(&until_nul(text.clone())),
        Some(Element::Key(text)) if opensim => parse::integer(&until_nul(text.clone())),
        Some(Element::Key(_) | Element::Vector(_) | Element::Rotation(_)) | None => 0,
    })
}

/// `llList2Float`: the element as a float, or `0.0` for a vector, a rotation
/// and past either end.
///
/// Second Life reads a string as `(float)` does (`"1e3"` is 1000) and a key
/// as `0.0`. OpenSim reads a string's leading digits, commas and point and
/// stops at an exponent (`"1e3"` is 1), and a key that is a UUID as
/// `(integer)` would.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_float<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<f32, CallError> {
    let opensim = ctx.flavour().is_opensim();
    Ok(match at(&list, index) {
        Some(Element::Integer(integer)) => int_to_float(*integer),
        Some(Element::Float(float)) => *float,
        Some(Element::String(text)) if opensim => os_leading_number(text),
        Some(Element::Key(text)) if opensim => {
            if is_uuid(text) {
                int_to_float(parse::integer(text))
            } else {
                os_leading_number(text)
            }
        }
        Some(Element::String(text)) => parse::float(&until_nul(text.clone())),
        Some(Element::Key(_) | Element::Vector(_) | Element::Rotation(_)) | None => 0.0,
    })
}

/// OpenSim's `llList2Float` of a string: the match of
/// `^\s*(-?\+?[,0-9]+\.?[0-9]*)`, its commas dropped as .NET's thousands
/// separators, read as a double; `0.0` when nothing matches.
fn os_leading_number(text: &str) -> f32 {
    let trimmed = text.trim_start();
    let (sign, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", trimmed),
    };
    let rest = rest.strip_prefix('+').unwrap_or(rest);
    let integral: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == ',')
        .collect();
    if integral.is_empty() {
        return 0.0;
    }
    let after = rest.get(integral.len()..).unwrap_or_default();
    let fraction: String = after.strip_prefix('.').map_or_else(String::new, |digits| {
        let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
        format!(".{digits}")
    });
    let digits: String = integral
        .chars()
        .filter(|character| *character != ',')
        .collect();
    parse_double(&format!("{sign}{digits}{fraction}")).map_or(0.0, to_f32)
}

/// `llList2Key`: a key or string element as a key, or `""` for any other
/// element and past either end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_key<C>(_ctx: &mut C, list: Vec<Element>, index: i32) -> Result<Key, CallError> {
    Ok(Key(match at(&list, index) {
        Some(Element::Key(text) | Element::String(text)) => until_nul(text.clone()),
        _ => String::new(),
    }))
}

/// OpenSim's `LSL_Vector(string)` / `LSL_Rotation(string)`: the brackets
/// dropped, the text split at commas, and each of the first `N` read as a
/// double; any that is not a number makes the whole the zero value.
fn os_components<const N: usize>(text: &str) -> Option<[f32; N]> {
    let cleaned = text.replace(['<', '>'], " ");
    let parts: Vec<&str> = cleaned.split(',').collect();
    let mut components = [0.0_f32; N];
    for (slot, part) in components.iter_mut().zip(&parts) {
        *slot = to_f32(parse_double(part)?);
    }
    (parts.len() >= N).then_some(components)
}

/// `llList2Vector`: a vector element, or [`ZERO_VECTOR`]. Second Life does
/// not convert a string (`"<1,2,3>"` is `ZERO_VECTOR`); OpenSim does, by its
/// own parser, as it does a key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_vector<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<Vector, CallError> {
    let opensim = ctx.flavour().is_opensim();
    Ok(match at(&list, index) {
        Some(Element::Vector(vector)) => vector.clone(),
        Some(Element::String(text) | Element::Key(text)) if opensim => {
            os_components::<3>(text).map_or(ZERO_VECTOR, |[x, y, z]| Vector { x, y, z })
        }
        _ => ZERO_VECTOR,
    })
}

/// `llList2Rot`: a rotation element, or [`ZERO_ROTATION`]. Second Life does
/// not convert a string; OpenSim does, by its own parser.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_rot<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    index: i32,
) -> Result<Rotation, CallError> {
    let opensim = ctx.flavour().is_opensim();
    Ok(match at(&list, index) {
        Some(Element::Rotation(rotation)) => rotation.clone(),
        Some(Element::String(text) | Element::Key(text)) if opensim => {
            os_components::<4>(text).map_or(ZERO_ROTATION, |[x, y, z, s]| Rotation { x, y, z, s })
        }
        _ => ZERO_ROTATION,
    })
}

/// `llList2List`: the elements `start..=end`, each counted from the end when
/// negative, or everything outside the range when the start comes after the
/// end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_list<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    start: i32,
    end: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        os_get(&list, start, end)
    } else {
        sl_get(&list, start, end)
    })
}

/// `llDeleteSubList`: the list without `start..=end`, or with only what lies
/// between the two when the start comes after the end. Past both ends the
/// grids part: `llDeleteSubList(l, 1, -99)` keeps `[a]` on Second Life and
/// nothing on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_delete_sub_list<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    start: i32,
    end: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        os_delete_list(&list, start, end)
    } else {
        sl_delete(&list, start, end)
    })
}

/// `llListInsertList`: `insert` put before the element at `index`, counted
/// from the end when negative.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_insert_list<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    insert: Vec<Element>,
    index: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        os_insert(&list, &insert, index)
    } else {
        sl_insert(&list, &insert, index)
    })
}

/// `llListReplaceList`: `start..=end` replaced by `insert`, or, when the
/// start comes after the end, only what lies between the two with `insert`
/// after it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_replace_list<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    insert: Vec<Element>,
    start: i32,
    end: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        os_replace(&list, &insert, start, end)
    } else {
        sl_replace(&list, &insert, start, end)
    })
}

/// `llList2ListStrided`: every `stride`-th element of `start..=end`, counting
/// strides from the list's start.
///
/// Second Life takes the stride's magnitude (0 is 1), a reversed range as the
/// whole list, and the strides that start inside the range (PyOptimizer;
/// `errors/bounds`: strides of −1 and 99 give `[a, …, f]` and `[a]`).
/// OpenSim walks backwards for a negative stride and takes a reversed range
/// as its outside — and throws for a range that starts and ends at the same
/// index past the list.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a one-element range past the end.
pub fn ll_list2_list_strided<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    start: i32,
    end: i32,
    stride: i32,
) -> Result<Vec<Element>, CallError> {
    if ctx.flavour().is_opensim() {
        return os_strided(&list, start, end, stride);
    }
    let stride = i64::from(
        if stride == 0 {
            1
        } else {
            stride.unsigned_abs()
        }
        .max(1),
    );
    let length = signed(list.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length);
    }
    if end < 0 {
        end = end.saturating_add(length);
    }
    if start > end {
        start = 0;
        end = length.saturating_sub(1);
    }
    let start = start
        .saturating_add(stride)
        .saturating_sub(1)
        .div_euclid(stride)
        .saturating_mul(stride);
    let end = end.div_euclid(stride).saturating_mul(stride);
    let (from, to) = (py(start, list.len()), py(end.saturating_add(1), list.len()));
    let step = usize::try_from(stride).unwrap_or(1);
    Ok(list
        .get(from..to.max(from))
        .unwrap_or_default()
        .iter()
        .step_by(step)
        .cloned()
        .collect())
}

/// OpenSim's `llList2ListStrided`: see [`ll_list2_list_strided`].
fn os_strided(
    list: &[Element],
    start: i32,
    end: i32,
    stride: i32,
) -> Result<Vec<Element>, CallError> {
    let length = signed(list.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length);
    }
    if end < 0 {
        end = end.saturating_add(length);
    }
    start = start.min(length);
    end = end.min(length);
    let stride = i64::from(if stride == 0 { 1 } else { stride });
    if start == end {
        if start.checked_rem(stride) == Some(0) {
            let element = usize::try_from(start)
                .ok()
                .and_then(|at| list.get(at))
                .ok_or(CallError::Exception {
                    id: BuiltinId::LlList2ListStrided,
                    exception: Exception::IndexOutOfRange,
                })?;
            return Ok(vec![element.clone()]);
        }
        return Ok(Vec::new());
    }
    let ranges: [(i64, i64); 2] = if start <= end {
        [(start, end), (1, 0)]
    } else {
        [(0, end), (start, length)]
    };
    let inside = |index: i64| {
        ranges
            .iter()
            .filter(move |(from, to)| (*from..=*to).contains(&index))
    };
    let indices: Vec<i64> = if stride > 0 {
        (0..length)
            .step_by(usize::try_from(stride).unwrap_or(1))
            .collect()
    } else {
        (0..length)
            .rev()
            .step_by(usize::try_from(stride.unsigned_abs()).unwrap_or(1))
            .collect()
    };
    let mut out = Vec::new();
    for index in indices {
        for _range in inside(index) {
            if let Some(element) = usize::try_from(index).ok().and_then(|at| list.get(at)) {
                out.push(element.clone());
            }
        }
    }
    Ok(out)
}

/// `llList2ListSlice`: from `start..=end`, the element at `slice` of every
/// stride of `stride` — `[]` for a stride that is not positive or a slice
/// index outside it, a negative index counting from the stride's end. A
/// start after the end takes the outside of the range as `llList2List` does,
/// each of its two pieces strided from its own start (`strings/newer`:
/// `(5, 2)` of `[1, a, 2, b, 3, c, 4, d]` by 2 is `[1, 2, c, d]`, where
/// PyOptimizer strides the joined list). OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_list_slice<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    start: i32,
    end: i32,
    stride: i32,
    slice_index: i32,
) -> Result<Vec<Element>, CallError> {
    if stride <= 0
        || slice_index < stride.saturating_neg()
        || slice_index > stride.saturating_sub(1)
    {
        return Ok(Vec::new());
    }
    let first = usize::try_from(if slice_index < 0 {
        slice_index.saturating_add(stride)
    } else {
        slice_index
    })
    .unwrap_or(0);
    let step = usize::try_from(stride).unwrap_or(1);
    let pick = |piece: Vec<Element>| piece.into_iter().skip(first).step_by(step);
    Ok(match sl_get_pieces(&list, start, end) {
        Ok(inside) => pick(inside).collect(),
        Err((before, after)) => pick(before).chain(pick(after)).collect(),
    })
}

/// Whether two elements match for `llListFindList` on Second Life: the same
/// type and the same value — a float is equal to an equal float and NaN to
/// NaN, a vector or rotation component by component, so a NaN there never
/// matches (PyOptimizer's `InternalCompareElems`).
fn sl_matches(left: &Element, right: &Element) -> bool {
    match (left, right) {
        (Element::Float(a), Element::Float(b)) => {
            a.partial_cmp(b).is_some_and(core::cmp::Ordering::is_eq) || (a.is_nan() && b.is_nan())
        }
        (Element::Vector(a), Element::Vector(b)) => crate::value::vector_eq(a, b),
        (Element::Rotation(a), Element::Rotation(b)) => crate::value::rotation_eq(a, b),
        (Element::Integer(a), Element::Integer(b)) => a == b,
        (Element::String(a), Element::String(b)) | (Element::Key(a), Element::Key(b)) => a == b,
        _ => false,
    }
}

/// Whether two elements match for `llListFindList` on OpenSim: the same type
/// as `os_type_code` tells them (so a key and a string of the same text
/// are one) and `Equals` (a NaN float equals a NaN).
fn os_matches(left: &Element, right: &Element) -> bool {
    if os_type_code(left) != os_type_code(right) {
        return false;
    }
    match (left, right) {
        (Element::String(a) | Element::Key(a), Element::String(b) | Element::Key(b)) => a == b,
        _ => sl_matches(left, right),
    }
}

/// Second Life's search, PyOptimizer's `InternalListFindList`: `pattern` in
/// `list` between `start` and `end`, trying every `stride`-th index from the
/// start, or from the end for a negative `instance`; the `instance`-th match
/// counting from 0 (from −1 backwards).
fn sl_find(
    list: &[Element],
    pattern: &[Element],
    (start, end): (i32, i32),
    stride: i32,
    mut instance: i32,
) -> i32 {
    let (outer, inner) = (signed(list.len()), signed(pattern.len()));
    if outer == 0 && inner == 0 {
        return 0;
    }
    if inner > outer {
        return -1;
    }
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(outer);
    }
    if end < 0 {
        end = end.saturating_add(outer);
    }
    end = end.min(outer.saturating_sub(1));
    if start < 0 || start >= outer || end < 0 || start > end {
        return -1;
    }
    if inner == 0 {
        return 0;
    }
    // A stride below one searches as one: aditi found `a` at 0 in
    // `[a, …, f]` by a stride of 0 (`errors/bounds-newer`), where
    // PyOptimizer answers -1.
    let stride = stride.max(1);
    let step = usize::try_from(stride).unwrap_or(1);
    let candidates: Vec<i64> = if instance >= 0 {
        (start..end.saturating_add(2).saturating_sub(inner))
            .step_by(step)
            .collect()
    } else {
        let mut down = Vec::new();
        let mut index = end.saturating_add(1).saturating_sub(inner);
        while index > start.saturating_sub(1) {
            down.push(index);
            index = index.saturating_sub(i64::from(stride));
        }
        down
    };
    for index in candidates {
        let Ok(from) = usize::try_from(index) else {
            continue;
        };
        let window = list.get(from..from.saturating_add(pattern.len()));
        let found = window.is_some_and(|window| {
            window
                .iter()
                .zip(pattern)
                .all(|(element, wanted)| sl_matches(element, wanted))
        });
        if found {
            if instance < 0 {
                instance = instance.saturating_add(1);
            }
            if instance == 0 {
                return i32::try_from(index).unwrap_or(-1);
            }
            if instance > 0 {
                instance = instance.saturating_sub(1);
            }
        }
    }
    -1
}

/// `llListFindList`: where `pattern` first starts in `list`, or `-1`. The
/// empty pattern is found at 0 on Second Life (in the empty list too) and
/// nowhere on OpenSim; Second Life wants the types to match, where OpenSim
/// takes a key and a string of the same text for one another
/// (`strings/functions`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_find_list<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    pattern: Vec<Element>,
) -> Result<i32, CallError> {
    if !ctx.flavour().is_opensim() {
        return Ok(sl_find(&list, &pattern, (0, -1), 1, 0));
    }
    if list.is_empty() || pattern.is_empty() {
        return Ok(-1);
    }
    let found = list
        .windows(pattern.len())
        .position(|window| window.iter().zip(&pattern).all(|(a, b)| os_matches(a, b)));
    Ok(found.map_or(-1, |at| i32::try_from(at).unwrap_or(-1)))
}

/// `llListFindListNext`: the `instance`-th place `pattern` starts in `list`
/// (counting from 0; −1 is the last), or `-1` (PyOptimizer). OpenSim does
/// not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_find_list_next<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    pattern: Vec<Element>,
    instance: i32,
) -> Result<i32, CallError> {
    Ok(sl_find(&list, &pattern, (0, -1), 1, instance))
}

/// `llListFindStrided`: where `pattern` first starts in `start..=end` of
/// `list`, trying only every `stride`-th index from `start` (`strings/newer`:
/// a start of 1 never matches at an even index; a stride below 1 is 1), or
/// `-1`. OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_find_strided<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    pattern: Vec<Element>,
    start: i32,
    end: i32,
    stride: i32,
) -> Result<i32, CallError> {
    Ok(sl_find(&list, &pattern, (start, end), stride, 0))
}

/// The squared length of a vector, in double.
fn magnitude_squared(vector: &Vector) -> f64 {
    let (x, y, z) = (
        f64::from(vector.x),
        f64::from(vector.y),
        f64::from(vector.z),
    );
    x.mul_add(x, y.mul_add(y, z * z))
}

/// Text compared as Mono compares it: by UTF-16 code unit.
fn utf16_cmp(left: &str, right: &str) -> core::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Whether `left` sorts after `right` for Second Life's `llListSort`: only
/// elements of one type are compared; a NaN, and a vector whose squared
/// length is NaN, sorts after anything; rotations never move
/// (PyOptimizer's `gt = not (a <= b)`).
fn sl_after(left: &Element, right: &Element) -> bool {
    match (left, right) {
        (Element::Integer(a), Element::Integer(b)) => a > b,
        (Element::Float(a), Element::Float(b)) => {
            !a.partial_cmp(b).is_some_and(core::cmp::Ordering::is_le)
        }
        (Element::String(a), Element::String(b)) | (Element::Key(a), Element::Key(b)) => {
            utf16_cmp(a, b).is_gt()
        }
        (Element::Vector(a), Element::Vector(b)) => !magnitude_squared(a)
            .partial_cmp(&magnitude_squared(b))
            .is_some_and(core::cmp::Ordering::is_le),
        _ => false,
    }
}

/// Swap the strides of `stride` elements at `a` and `b`.
fn swap_strides(items: &mut [Element], a: usize, b: usize, stride: usize) {
    for offset in 0..stride {
        items.swap(a.saturating_add(offset), b.saturating_add(offset));
    }
}

/// Second Life's `llListSort` and `llListSortStrided` (PyOptimizer's
/// `llListSort` with its `idx`): a selection sort over strides keyed by the
/// element at `key` of each, swapping whenever the later stride's key sorts
/// before — and, sorting descending, whenever it does not sort after, which
/// is what moves elements of different types past each other. The list is
/// returned as it is when its length is no multiple of the stride, and empty
/// when `key` is outside the stride.
fn sl_sort(list: &[Element], stride: i32, key: i32, ascending: i32) -> Vec<Element> {
    let stride = stride.max(1);
    let mut items = list.to_vec();
    let width = usize::try_from(stride).unwrap_or(1);
    if !items.len().is_multiple_of(width) {
        return items;
    }
    let key = if key < 0 {
        key.saturating_add(stride)
    } else {
        key
    };
    let Ok(key) = usize::try_from(key) else {
        return Vec::new();
    };
    if key >= width {
        return Vec::new();
    }
    let descending = ascending != 1;
    let mut i = 0_usize;
    while i.saturating_add(width) < items.len() {
        let mut j = i.saturating_add(width);
        while j < items.len() {
            let after = match (
                items.get(i.saturating_add(key)),
                items.get(j.saturating_add(key)),
            ) {
                (Some(a), Some(b)) if a.type_name() == b.type_name() => sl_after(a, b),
                _ => false,
            };
            if after != descending {
                swap_strides(&mut items, i, j, width);
            }
            j = j.saturating_add(width);
        }
        i = i.saturating_add(width);
    }
    items
}

/// OpenSim's type of an element for sorting: its keys are strings.
const fn os_sort_type(element: &Element) -> u8 {
    match element {
        Element::Integer(_) => 1,
        Element::Float(_) => 2,
        Element::String(_) | Element::Key(_) => 3,
        Element::Vector(_) => 5,
        Element::Rotation(_) => 6,
    }
}

/// The text of a string or key element.
fn text_of(element: &Element) -> Option<&str> {
    match element {
        Element::String(text) | Element::Key(text) => Some(text),
        _ => None,
    }
}

/// OpenSim's `needSwapAscending` / `needSwapDescending` for two elements of
/// one type — with its quirk that an ascending sort of vectors compares one
/// squared length against the other's plain length.
fn os_need_swap(left: &Element, right: &Element, ascending: bool) -> bool {
    match (left, right) {
        (Element::Integer(a), Element::Integer(b)) => {
            if ascending {
                a > b
            } else {
                a < b
            }
        }
        (Element::Float(a), Element::Float(b)) => {
            if ascending {
                a > b
            } else {
                a < b
            }
        }
        (Element::Vector(a), Element::Vector(b)) => {
            if ascending {
                magnitude_squared(a) > magnitude_squared(b).sqrt()
            } else {
                magnitude_squared(a) < magnitude_squared(b)
            }
        }
        _ => match (text_of(left), text_of(right)) {
            (Some(a), Some(b)) => {
                let order = utf16_cmp(a, b);
                if ascending {
                    order.is_gt()
                } else {
                    order.is_lt()
                }
            }
            _ => false,
        },
    }
}

/// OpenSim's `HomogeneousComparer`: `Math.Sign` of a difference, the
/// integers' wrapping in C#'s unchecked arithmetic.
fn os_compare(left: &Element, right: &Element) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    match (left, right) {
        (Element::Integer(a), Element::Integer(b)) => a.wrapping_sub(*b).cmp(&0),
        (Element::Float(a), Element::Float(b)) => {
            let difference = f64::from(*a) - f64::from(*b);
            difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        }
        (Element::Vector(a), Element::Vector(b)) => {
            let difference = magnitude_squared(a) - magnitude_squared(b);
            difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        }
        _ => match (text_of(left), text_of(right)) {
            (Some(a), Some(b)) => utf16_cmp(a, b),
            _ => Ordering::Equal,
        },
    }
}

/// OpenSim's `list.Sort`: nothing for a list no longer than the stride or
/// no multiple of it; a list of one type sorted outright; otherwise its
/// pivot sort, which only ever exchanges elements of the pivot's type, so
/// the places keep their types.
fn os_sort(list: &[Element], stride: i32, ascending: bool) -> Vec<Element> {
    let mut items = list.to_vec();
    let width = usize::try_from(stride.max(1)).unwrap_or(1);
    if items.len() <= width || !items.len().is_multiple_of(width) {
        return items;
    }
    if width == 1 {
        let first = items.first().map(os_sort_type);
        if items.iter().all(|item| Some(os_sort_type(item)) == first) {
            items.sort_by(|a, b| {
                let order = os_compare(a, b);
                if ascending { order } else { order.reverse() }
            });
            return items;
        }
    }
    let mut i = 0_usize;
    while i.saturating_add(width) < items.len() {
        let Some(mut pivot) = items.get(i).cloned() else {
            break;
        };
        let mut j = i.saturating_add(width);
        while let Some(candidate) = items.get(j).cloned() {
            if os_sort_type(&candidate) == os_sort_type(&pivot)
                && os_need_swap(&pivot, &candidate, ascending)
            {
                if let Some(slot) = items.get_mut(j) {
                    *slot = pivot;
                }
                pivot = candidate;
                for offset in 1..width {
                    items.swap(i.saturating_add(offset), j.saturating_add(offset));
                }
            }
            j = j.saturating_add(width);
        }
        if let Some(slot) = items.get_mut(i) {
            *slot = pivot;
        }
        i = i.saturating_add(width);
    }
    items
}

/// `llListSort`: the list sorted in strides of `stride` by each stride's
/// first element, ascending when `ascending` is exactly `TRUE`.
///
/// On Second Life elements are compared only with elements of their own type
/// (strings by UTF-16 unit, vectors by length, rotations not at all), so an
/// ascending sort of mixed types sorts each type among the places it holds
/// and a descending one moves the types about (`strings/functions`). OpenSim
/// keeps the places' types both ways, and takes a key for a string.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_sort<C: PureCtx>(
    ctx: &mut C,
    list: Vec<Element>,
    stride: i32,
    ascending: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        os_sort(&list, stride, ascending == 1)
    } else {
        sl_sort(&list, stride, 0, ascending)
    })
}

/// `llListSortStrided`: as [`ll_list_sort`], keyed by the element at `key`
/// of each stride (counted from the stride's end when negative); `[]` for a
/// key outside the stride (`strings/newer`). OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_sort_strided<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    stride: i32,
    key: i32,
    ascending: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(sl_sort(&list, stride, key, ascending))
}

/// `llListRandomize`: the list shuffled in strides of `stride` (a stride
/// below 1 is 1), or as it is when its length is no multiple of the stride.
/// The draws come from the region's seeded source.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list_randomize(
    ctx: &mut ScriptCtx<'_>,
    list: Vec<Element>,
    stride: i32,
) -> Result<Vec<Element>, CallError> {
    let width = usize::try_from(stride.max(1)).unwrap_or(1);
    let mut items = list;
    if !items.len().is_multiple_of(width) {
        return Ok(items);
    }
    let records = items.len().checked_div(width).unwrap_or(0);
    for record in 0..records.saturating_sub(1) {
        let remaining = u32::try_from(records.saturating_sub(record)).unwrap_or(u32::MAX);
        let chosen =
            record.saturating_add(usize::try_from(ctx.random_below(remaining)).unwrap_or(0));
        if chosen != record {
            swap_strides(
                &mut items,
                record.saturating_mul(width),
                chosen.saturating_mul(width),
                width,
            );
        }
    }
    Ok(items)
}

/// The `LIST_STAT_*` operations.
mod stat {
    /// `LIST_STAT_RANGE`.
    pub(super) const RANGE: i32 = 0;
    /// `LIST_STAT_MIN`.
    pub(super) const MIN: i32 = 1;
    /// `LIST_STAT_MAX`.
    pub(super) const MAX: i32 = 2;
    /// `LIST_STAT_MEAN`.
    pub(super) const MEAN: i32 = 3;
    /// `LIST_STAT_MEDIAN`.
    pub(super) const MEDIAN: i32 = 4;
    /// `LIST_STAT_STD_DEV`.
    pub(super) const STD_DEV: i32 = 5;
    /// `LIST_STAT_SUM`.
    pub(super) const SUM: i32 = 6;
    /// `LIST_STAT_SUM_SQUARES`.
    pub(super) const SUM_SQUARES: i32 = 7;
    /// `LIST_STAT_NUM_COUNT`.
    pub(super) const NUM_COUNT: i32 = 8;
    /// `LIST_STAT_GEOMETRIC_MEAN`.
    pub(super) const GEOMETRIC_MEAN: i32 = 9;
    /// OpenSim's `LIST_STAT_HARMONIC_MEAN`, which Second Life has not.
    pub(super) const HARMONIC_MEAN: i32 = 100;
}

/// Second Life's `llListStatistics` (PyOptimizer): over the integers and
/// floats only, in double, rounded to a float at the end; `0.0` for a list
/// with no number, or an unknown operation.
fn sl_statistics(operation: i32, list: &[Element]) -> f32 {
    // Reversed, as PyOptimizer collects them: the median's sort starts there.
    let numbers: Vec<f64> = list
        .iter()
        .rev()
        .filter_map(|element| match element {
            Element::Integer(integer) => Some(f64::from(*integer)),
            Element::Float(float) => Some(f64::from(*float)),
            _ => None,
        })
        .collect();
    if numbers.is_empty() {
        return 0.0;
    }
    let count = f64::from(u32::try_from(numbers.len()).unwrap_or(u32::MAX));
    match operation {
        stat::NUM_COUNT => to_f32(count),
        stat::RANGE | stat::MIN | stat::MAX => {
            let (low, high) = numbers.iter().skip(1).fold(
                (
                    numbers.first().copied().unwrap_or(0.0),
                    numbers.first().copied().unwrap_or(0.0),
                ),
                |(low, high), number| {
                    (
                        if *number < low { *number } else { low },
                        if *number > high { *number } else { high },
                    )
                },
            );
            to_f32(match operation {
                stat::RANGE => high - low,
                stat::MIN => low,
                _ => high,
            })
        }
        stat::MEDIAN => {
            let sorted = sl_sort(
                &numbers
                    .iter()
                    .map(|number| Element::Float(to_f32(*number)))
                    .collect::<Vec<_>>(),
                1,
                0,
                1,
            );
            let value = |at: usize| match sorted.get(at) {
                Some(Element::Float(float)) => f64::from(*float),
                _ => 0.0,
            };
            let middle = sorted.len() / 2;
            if !sorted.len().is_multiple_of(2) {
                to_f32(value(middle))
            } else {
                to_f32(f64::midpoint(
                    value(middle.saturating_sub(1)),
                    value(middle),
                ))
            }
        }
        stat::MEAN | stat::STD_DEV | stat::SUM | stat::SUM_SQUARES => {
            let mut sum = 0.0_f64;
            let mut squares = 0.0_f64;
            let mut mean = 0.0_f64;
            let mut seen = 0.0_f64;
            let mut m2 = 0.0_f64;
            for number in &numbers {
                seen += 1.0;
                sum += number;
                squares += number * number;
                let delta = number - mean;
                mean += delta / seen;
                m2 += delta * (number - mean);
            }
            match operation {
                stat::STD_DEV if (seen - 1.0).abs() < f64::EPSILON => 0.0,
                stat::STD_DEV => to_f32((m2 / (seen - 1.0)).sqrt()),
                stat::SUM => to_f32(sum),
                stat::SUM_SQUARES => to_f32(squares),
                _ => to_f32(mean),
            }
        }
        stat::GEOMETRIC_MEAN => {
            let mut seen = 0.0_f64;
            let mut log_mean = 0.0_f64;
            for number in &numbers {
                if *number <= 0.0 {
                    return 0.0;
                }
                seen += 1.0;
                log_mean += (number.ln() - log_mean) / seen;
            }
            to_f32(log_mean.exp())
        }
        _ => 0.0,
    }
}

/// OpenSim's number of an element: its text (`ToString`) read by
/// `Double.TryParse`, so a string or key that reads as a number counts.
fn os_number(element: &Element) -> Option<f64> {
    match element {
        Element::Integer(integer) => Some(f64::from(*integer)),
        Element::Float(float) => Some(f64::from(*float)),
        Element::String(text) | Element::Key(text) => parse_double(text),
        Element::Vector(_) | Element::Rotation(_) => None,
    }
}

/// OpenSim's `llListStatistics`, from `LSL_Types.list`: every element whose
/// text reads as a number counts, the range is the maximum *divided by* the
/// minimum, the mean of nothing is NaN, the geometric mean the root of the
/// product (NaN for a negative one), and the median of nothing throws.
fn os_statistics(operation: i32, list: &[Element]) -> Result<f32, CallError> {
    let numbers: Vec<f64> = list.iter().filter_map(os_number).collect();
    let count = f64::from(u32::try_from(numbers.len()).unwrap_or(u32::MAX));
    let sum: f64 = numbers.iter().sum();
    let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
    let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let value = match operation {
        stat::RANGE => max / min,
        stat::MIN => min,
        stat::MAX => max,
        stat::MEAN => sum / count,
        stat::MEDIAN => {
            let mut sorted = numbers;
            sorted.sort_by(f64::total_cmp);
            let half = sorted.len() / 2;
            let thrown = CallError::Exception {
                id: BuiltinId::LlListStatistics,
                exception: Exception::IndexOutOfRange,
            };
            if sorted.len().is_multiple_of(2) {
                let low = sorted
                    .get(half.wrapping_sub(1))
                    .ok_or_else(|| thrown.clone())?;
                let high = sorted.get(half).ok_or(thrown)?;
                (low + high) / 2.0
            } else {
                *sorted.get(half).ok_or(thrown)?
            }
        }
        stat::NUM_COUNT => count,
        stat::STD_DEV => {
            let squares: f64 = numbers.iter().map(|number| number.powi(2)).sum();
            let mean = sum / count;
            ((squares - count * mean.powi(2)) / (count - 1.0)).sqrt()
        }
        stat::SUM => sum,
        stat::SUM_SQUARES => numbers.iter().map(|number| number.powi(2)).sum(),
        stat::GEOMETRIC_MEAN => (numbers.iter().product::<f64>().ln() / count).exp(),
        stat::HARMONIC_MEAN => count / numbers.iter().map(|number| 1.0 / number).sum::<f64>(),
        _ => 0.0,
    };
    Ok(to_f32(value))
}

/// `llListStatistics`: the statistic `operation` names of the list's
/// numbers.
///
/// Second Life counts only integers and floats, answers `0.0` for a list
/// with none and for an unknown operation, and gives a geometric mean of
/// `0.0` when a number is not positive. OpenSim parses strings and keys too
/// (`[1, 2.5, "3", (key)"4", <1,2,3>, 4]` sums to 14.5 there, 7.5 here), and
/// see `os_statistics` for the rest of its answers.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for the median of a list with no
/// number in it.
pub fn ll_list_statistics<C: PureCtx>(
    ctx: &mut C,
    operation: i32,
    list: Vec<Element>,
) -> Result<f32, CallError> {
    if ctx.flavour().is_opensim() {
        os_statistics(operation, &list)
    } else {
        Ok(sl_statistics(operation, &list))
    }
}

/// An element as `llDumpList2String` writes it on Second Life: as in
/// `(string)list`, except that negative zero is written as zero.
fn dump_element(element: &Element) -> String {
    let unsigned = |float: f32| if float == 0.0 { 0.0 } else { float };
    match element {
        Element::Float(float) => format::float(unsigned(*float), format::FLOAT_DECIMALS),
        Element::Vector(vector) => format::vector(
            &Vector {
                x: unsigned(vector.x),
                y: unsigned(vector.y),
                z: unsigned(vector.z),
            },
            format::FLOAT_DECIMALS,
        ),
        Element::Rotation(rotation) => format::rotation(
            &Rotation {
                x: unsigned(rotation.x),
                y: unsigned(rotation.y),
                z: unsigned(rotation.z),
                s: unsigned(rotation.s),
            },
            format::FLOAT_DECIMALS,
        ),
        other => element_to_string(other),
    }
}

/// `llDumpList2String`: the elements as text, joined by `separator`; a float
/// written as `(string)` writes it, but negative zero as `0.000000`
/// (`strings/conversions`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_dump_list2_string<C>(
    _ctx: &mut C,
    list: Vec<Element>,
    separator: String,
) -> Result<String, CallError> {
    Ok(list
        .iter()
        .map(dump_element)
        .collect::<Vec<_>>()
        .join(&separator))
}

/// A float as `llList2CSV` writes it on Second Life: its exact value with six
/// decimals — no seven-digit rounding — `inf`, `-inf`, `nan` and `-nan`.
fn csv_float(float: f32) -> String {
    if float.is_nan() {
        return if float.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned();
    }
    if float.is_infinite() {
        return if float > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    format!("{:.6}", f64::from(float))
}

/// `llList2CSV`: the elements as text joined by `", "`, a float with all its
/// digits (`123456789.0` is `123456792.000000`, where `(string)` writes
/// `123456800.000000`) and a vector's or rotation's components likewise.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_list2_csv<C>(_ctx: &mut C, list: Vec<Element>) -> Result<String, CallError> {
    Ok(list
        .iter()
        .map(|element| match element {
            Element::Float(float) => csv_float(*float),
            Element::Vector(vector) => format!(
                "<{}, {}, {}>",
                csv_float(vector.x),
                csv_float(vector.y),
                csv_float(vector.z)
            ),
            Element::Rotation(rotation) => format!(
                "<{}, {}, {}, {}>",
                csv_float(rotation.x),
                csv_float(rotation.y),
                csv_float(rotation.z),
                csv_float(rotation.s)
            ),
            other => element_to_string(other),
        })
        .collect::<Vec<_>>()
        .join(", "))
}

/// `llCSV2List`: the text split at the commas outside `<…>`, every element a
/// string. Second Life drops one space after each comma (and before the
/// first element); OpenSim trims each element at both ends. Quotes are not
/// special on either.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_csv2_list<C: PureCtx>(ctx: &mut C, text: String) -> Result<Vec<Element>, CallError> {
    let opensim = ctx.flavour().is_opensim();
    let mut out = Vec::new();
    let mut depth = 0_u32;
    let mut current = String::new();
    let mut after_comma = true;
    for character in text.chars() {
        if depth > 0 {
            match character {
                '<' => depth = depth.saturating_add(1),
                '>' => depth = depth.saturating_sub(1),
                _ => {}
            }
            current.push(character);
        } else if after_comma && character == ' ' && !opensim {
            after_comma = false;
        } else {
            after_comma = false;
            match character {
                ',' => {
                    out.push(core::mem::take(&mut current));
                    after_comma = true;
                }
                '<' => {
                    depth = depth.saturating_add(1);
                    current.push(character);
                }
                _ => current.push(character),
            }
        }
    }
    out.push(current);
    Ok(out
        .into_iter()
        .map(|piece| {
            Element::String(if opensim {
                piece.trim().to_owned()
            } else {
                piece
            })
        })
        .collect())
}

/// How many separators and how many spacers Second Life's parser uses; the
/// rest are ignored (`errors/bounds`: nine separators split at eight).
const SL_PARSE_LIMIT: usize = 8;

/// `llParseString2List` and `llParseStringKeepNulls`: the text split at each
/// separator (dropped) and spacer (kept as an element); at one place the
/// first separator wins over the spacers and an earlier one in either list
/// over a later one (`["b", "bc"]` on `abcabc` gives `[a, ca, c]`); empty
/// separators and spacers are ignored, and empty pieces kept only by
/// `keep_nulls`. Second Life uses only the first eight of each list.
fn parse_string(
    flavour: EngineFlavour,
    text: &str,
    separators: &[Element],
    spacers: &[Element],
    keep_nulls: bool,
) -> Vec<Element> {
    if text.is_empty() && keep_nulls {
        return vec![Element::String(String::new())];
    }
    let limit = if flavour.is_opensim() {
        usize::MAX
    } else {
        SL_PARSE_LIMIT
    };
    let source = Text::new(flavour, text);
    let delimiters: Vec<(Text, bool)> = separators
        .iter()
        .take(limit)
        .map(|element| (element, false))
        .chain(spacers.iter().take(limit).map(|element| (element, true)))
        .map(|(element, kept)| (Text::new(flavour, &element_to_string(element)), kept))
        .filter(|(delimiter, _)| delimiter.len() > 0)
        .collect();
    let mut out = Vec::new();
    let mut from = 0_usize;
    loop {
        let earliest = delimiters
            .iter()
            .filter_map(|(delimiter, kept)| {
                source
                    .find(delimiter.units(), from)
                    .map(|at| (at, delimiter, *kept))
            })
            .min_by_key(|(at, ..)| *at);
        let end = earliest.map_or(source.len(), |(at, ..)| at);
        let piece = source.units().get(from..end).unwrap_or_default();
        if keep_nulls || !piece.is_empty() {
            out.push(Element::String(source.string(piece)));
        }
        let Some((at, delimiter, kept)) = earliest else {
            break;
        };
        if kept {
            out.push(Element::String(source.string(delimiter.units())));
        }
        from = at.saturating_add(delimiter.len());
    }
    out
}

/// `llParseString2List`: see `parse_string`; empty pieces dropped.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_parse_string2_list<C: PureCtx>(
    ctx: &mut C,
    text: String,
    separators: Vec<Element>,
    spacers: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    Ok(parse_string(
        ctx.flavour(),
        &text,
        &separators,
        &spacers,
        false,
    ))
}

/// `llParseStringKeepNulls`: see `parse_string`; empty pieces kept, and the
/// empty string is `[""]`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_parse_string_keep_nulls<C: PureCtx>(
    ctx: &mut C,
    text: String,
    separators: Vec<Element>,
    spacers: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    Ok(parse_string(
        ctx.flavour(),
        &text,
        &separators,
        &spacers,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::{Bare, os, sl};

    /// An integer element.
    const fn i(value: i32) -> Element {
        Element::Integer(value)
    }

    /// A float element.
    const fn f(value: f32) -> Element {
        Element::Float(value)
    }

    /// A string element.
    fn s(text: &str) -> Element {
        Element::String(text.to_owned())
    }

    /// A key element.
    fn k(text: &str) -> Element {
        Element::Key(text.to_owned())
    }

    /// A vector element.
    const fn v(x: f32, y: f32, z: f32) -> Element {
        Element::Vector(Vector { x, y, z })
    }

    /// `[a, b, c, d, e, f]`, the bounds probes' list.
    fn letters() -> Vec<Element> {
        ["a", "b", "c", "d", "e", "f"].into_iter().map(s).collect()
    }

    /// A list written as `llList2CSV` writes it, in brackets.
    fn csv(list: &[Element]) -> String {
        format!(
            "[{}]",
            ll_list2_csv(&mut (), list.to_vec()).unwrap_or_default()
        )
    }

    #[test]
    fn reading_elements_converts_as_each_grid_does() {
        // errors/bounds, `list2-conversions`.
        let list = vec![
            f(1.9),
            s("1e3"),
            s("0x10"),
            s(" 7 apples"),
            v(1.0, 2.0, 3.0),
            s("<1,2,3>"),
        ];
        let integer = |ctx: &mut Bare, at: i32| ll_list2_integer(ctx, list.clone(), at);
        assert_eq!(integer(&mut sl(), 0), Ok(1));
        assert_eq!(integer(&mut os(), 0), Ok(2));
        for ctx in [&mut sl(), &mut os()] {
            assert_eq!(integer(ctx, 2), Ok(16));
            assert_eq!(integer(ctx, 3), Ok(7));
            assert_eq!(integer(ctx, 4), Ok(0));
            assert_eq!(integer(ctx, 99), Ok(0));
        }
        assert_eq!(ll_list2_float(&mut sl(), list.clone(), 1), Ok(1000.0));
        assert_eq!(ll_list2_float(&mut os(), list.clone(), 1), Ok(1.0));
        assert_eq!(ll_list2_vector(&mut sl(), list.clone(), 5), Ok(ZERO_VECTOR));
        assert_eq!(
            ll_list2_vector(&mut os(), list.clone(), 5),
            Ok(Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0
            })
        );
        assert_eq!(
            ll_list2_rot(&mut os(), vec![s("<1,2,3,4>")], 0),
            Ok(Rotation {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                s: 4.0
            })
        );
        assert_eq!(
            ll_list2_rot(&mut sl(), vec![s("<1,2,3,4>")], 0),
            Ok(ZERO_ROTATION)
        );
        assert_eq!(
            ll_list2_rot(&mut sl(), list.clone(), -99),
            Ok(ZERO_ROTATION)
        );
        assert_eq!(ll_list2_key(&mut (), vec![i(5)], 0), Ok(Key(String::new())));
        assert_eq!(
            ll_list2_key(&mut (), vec![s("k")], 0),
            Ok(Key("k".to_owned()))
        );
        assert_eq!(
            ll_list2_integer(&mut os(), vec![f(3e9)], 0),
            Err(CallError::Exception {
                id: BuiltinId::LlList2Integer,
                exception: Exception::Overflow,
            })
        );
        assert_eq!(ll_list2_integer(&mut sl(), vec![f(3e9)], 0), Ok(i32::MIN));
        assert_eq!(
            ll_list2_string(&mut (), vec![f(-0.0)], 0),
            Ok("-0.000000".to_owned())
        );
    }

    #[test]
    fn opensim_tells_keys_from_strings_by_their_text() {
        // strings/conversions, `entry-type`.
        let list = vec![
            i(1),
            f(1.0),
            s("s"),
            k("key"),
            v(0.0, 0.0, 0.0),
            s(crate::value::NULL_KEY),
            k(crate::value::NULL_KEY),
        ];
        let types = |ctx: &mut Bare| {
            (0..7)
                .map(|at| ll_get_list_entry_type(ctx, list.clone(), at).unwrap_or(-1))
                .collect::<Vec<_>>()
        };
        assert_eq!(types(&mut sl()), vec![1, 2, 3, 4, 5, 3, 4]);
        assert_eq!(types(&mut os()), vec![1, 2, 3, 3, 5, 4, 4]);
        assert_eq!(ll_get_list_entry_type(&mut sl(), list.clone(), 99), Ok(0));
    }

    #[test]
    fn cutting_and_splicing_per_grid() {
        // errors/bounds: the grids part only past both ends.
        assert_eq!(
            csv(&ll_delete_sub_list(&mut sl(), letters(), 1, -99).unwrap_or_default()),
            "[a]"
        );
        assert_eq!(
            csv(&ll_delete_sub_list(&mut os(), letters(), 1, -99).unwrap_or_default()),
            "[]"
        );
        for ctx in [&mut sl(), &mut os()] {
            assert_eq!(
                csv(&ll_list2_list(ctx, letters(), 4, 1).unwrap_or_default()),
                "[a, b, e, f]"
            );
            assert_eq!(
                csv(&ll_list_replace_list(ctx, letters(), vec![s("x")], 1, 2).unwrap_or_default()),
                "[a, x, d, e, f]"
            );
            assert_eq!(
                csv(&ll_list_insert_list(ctx, letters(), vec![s("x")], -1).unwrap_or_default()),
                "[a, b, c, d, e, x, f]"
            );
        }
    }

    #[test]
    fn strided_reading_per_grid() {
        let strided = |ctx: &mut Bare, start: i32, end: i32, stride: i32| {
            ll_list2_list_strided(ctx, letters(), start, end, stride).map(|list| csv(&list))
        };
        // errors/bounds `strided-odd` on aditi.
        assert_eq!(
            strided(&mut sl(), 0, -1, -1),
            Ok("[a, b, c, d, e, f]".to_owned())
        );
        assert_eq!(strided(&mut sl(), 0, -1, 99), Ok("[a]".to_owned()));
        assert_eq!(strided(&mut sl(), 4, 1, 2), Ok("[a, c, e]".to_owned()));
        assert_eq!(strided(&mut sl(), 99, 100, 2), Ok("[]".to_owned()));
        assert_eq!(
            strided(&mut sl(), 0, -1, 0),
            Ok("[a, b, c, d, e, f]".to_owned())
        );
        // OpenSim: backwards for a negative stride, the outside of a reversed
        // range, and a throw for a one-element range past the end.
        assert_eq!(
            strided(&mut os(), 0, -1, -1),
            Ok("[f, e, d, c, b, a]".to_owned())
        );
        assert_eq!(strided(&mut os(), 0, -1, 99), Ok("[a]".to_owned()));
        assert_eq!(strided(&mut os(), 4, 1, 2), Ok("[a, e]".to_owned()));
        assert_eq!(
            strided(&mut os(), 99, 100, 2),
            Err(CallError::Exception {
                id: BuiltinId::LlList2ListStrided,
                exception: Exception::IndexOutOfRange,
            })
        );
    }

    #[test]
    fn slices_of_strides() {
        let list = vec![i(1), s("a"), i(2), s("b"), i(3), s("c"), i(4), s("d")];
        let slice = |start: i32, end: i32, index: i32| {
            ll_list2_list_slice(&mut (), list.clone(), start, end, 2, index).map(|list| csv(&list))
        };
        // strings/newer.
        assert_eq!(slice(0, -1, 1), Ok("[a, b, c, d]".to_owned()));
        assert_eq!(slice(0, -1, -1), Ok("[a, b, c, d]".to_owned()));
        assert_eq!(slice(0, -1, 2), Ok("[]".to_owned()));
        assert_eq!(slice(5, 2, 0), Ok("[1, 2, c, d]".to_owned()));
    }

    #[test]
    fn finding_per_grid() {
        let find = |ctx: &mut Bare, list: Vec<Element>, pattern: Vec<Element>| {
            ll_list_find_list(ctx, list, pattern)
        };
        // strings/functions and errors/bounds.
        for ctx in [&mut sl(), &mut os()] {
            assert_eq!(find(ctx, vec![f(1.0), i(1)], vec![i(1)]), Ok(1));
            assert_eq!(find(ctx, vec![i(1), f(1.0)], vec![f(1.0)]), Ok(1));
            assert_eq!(find(ctx, vec![i(1), s("1")], vec![s("1")]), Ok(1));
            assert_eq!(
                find(ctx, vec![i(1), i(2), i(3), i(2), i(3)], vec![i(2), i(3)]),
                Ok(1)
            );
            assert_eq!(find(ctx, vec![f(0.0)], vec![f(-0.0)]), Ok(0));
        }
        let null = crate::value::NULL_KEY;
        assert_eq!(
            find(&mut sl(), vec![k(null), s(null)], vec![s(null)]),
            Ok(1)
        );
        assert_eq!(
            find(&mut os(), vec![k(null), s(null)], vec![s(null)]),
            Ok(0)
        );
        assert_eq!(find(&mut sl(), letters(), vec![]), Ok(0));
        assert_eq!(find(&mut sl(), vec![], vec![]), Ok(0));
        assert_eq!(find(&mut os(), letters(), vec![]), Ok(-1));
        let list = vec![i(1), i(2), i(1), i(2), i(1)];
        assert_eq!(
            ll_list_find_list_next(&mut (), list.clone(), vec![i(1)], 1),
            Ok(2)
        );
        assert_eq!(
            ll_list_find_list_next(&mut (), list.clone(), vec![i(1)], -1),
            Ok(4)
        );
        assert_eq!(
            ll_list_find_list_next(&mut (), list.clone(), vec![i(1)], 3),
            Ok(-1)
        );
        // A start of 1 never matches at an even index.
        assert_eq!(
            ll_list_find_strided(&mut (), list.clone(), vec![i(1)], 1, -1, 2),
            Ok(-1)
        );
        assert_eq!(
            ll_list_find_strided(&mut (), list, vec![i(2)], 1, -1, 2),
            Ok(1)
        );
    }

    #[test]
    fn sorting_mixed_types_per_grid() {
        let mixed = || {
            vec![
                i(3),
                s("b"),
                f(1.5),
                s("a"),
                i(2),
                v(1.0, 0.0, 0.0),
                s("B"),
                i(-1),
                v(0.0, 0.0, 2.0),
            ]
        };
        let sorted = |ctx: &mut Bare, ascending: i32| {
            ll_list_sort(ctx, mixed(), 1, ascending).map(|list| csv(&list))
        };
        // strings/functions `sort mixed-*`.
        let ascending = "[-1, B, 1.500000, a, 2, <1.000000, 0.000000, 0.000000>, b, 3, <0.000000, 0.000000, 2.000000>]";
        assert_eq!(sorted(&mut sl(), 1), Ok(ascending.to_owned()));
        assert_eq!(sorted(&mut os(), 1), Ok(ascending.to_owned()));
        assert_eq!(
            sorted(&mut sl(), 0),
            Ok("[<0.000000, 0.000000, 2.000000>, -1, B, <1.000000, 0.000000, 0.000000>, 2, a, 1.500000, b, 3]".to_owned())
        );
        assert_eq!(
            sorted(&mut os(), 0),
            Ok("[3, b, 1.500000, a, 2, <0.000000, 0.000000, 2.000000>, B, -1, <1.000000, 0.000000, 0.000000>]".to_owned())
        );
        let strings = || {
            ["b", "B", "a", "A", "é", "10", "9", ""]
                .into_iter()
                .map(s)
                .collect::<Vec<_>>()
        };
        for ctx in [&mut sl(), &mut os()] {
            assert_eq!(
                ll_list_sort(ctx, strings(), 1, 1).map(|list| csv(&list)),
                Ok("[, 10, 9, A, B, a, b, é]".to_owned())
            );
            assert_eq!(
                ll_list_sort(
                    ctx,
                    vec![i(2), s("x"), i(1), s("y"), i(2), s("a"), i(1), s("b")],
                    2,
                    1
                )
                .map(|list| csv(&list)),
                Ok("[1, y, 1, b, 2, a, 2, x]".to_owned())
            );
        }
        // Keys and strings: one type on OpenSim only.
        let keyed = || vec![k("b"), s("a"), k("A"), s("c")];
        assert_eq!(
            ll_list_sort(&mut sl(), keyed(), 1, 1),
            Ok(vec![k("A"), s("a"), k("b"), s("c")])
        );
        assert_eq!(
            ll_list_sort(&mut os(), keyed(), 1, 1),
            Ok(vec![k("A"), s("a"), k("b"), s("c")])
        );
        // A stride index past the stride sorts to nothing.
        assert_eq!(
            ll_list_sort_strided(&mut (), letters(), 2, 2, 1),
            Ok(vec![])
        );
        assert_eq!(
            ll_list_sort_strided(&mut (), letters(), 0, 0, 1).map(|list| csv(&list)),
            Ok("[a, b, c, d, e, f]".to_owned())
        );
    }

    #[test]
    fn statistics_per_grid() {
        let mixed = || vec![i(1), f(2.5), s("3"), k("4"), v(1.0, 2.0, 3.0), i(4)];
        let stat = |ctx: &mut Bare, operation: i32| ll_list_statistics(ctx, operation, mixed());
        // strings/functions `statistics mixed op N`.
        let sl_answers = [3.0, 1.0, 4.0, 2.5, 1.5, 7.5, 23.25, 3.0, 2.154_435];
        let os_answers = [4.0, 1.0, 4.0, 2.9, 1.244_99, 14.5, 48.25, 5.0, 2.605_171];
        for (operation, (sl_answer, os_answer)) in [0, 1, 2, 3, 5, 6, 7, 8, 9]
            .into_iter()
            .zip(sl_answers.into_iter().zip(os_answers))
        {
            let ours = stat(&mut sl(), operation).unwrap_or(f32::NAN);
            assert!((ours - sl_answer).abs() < 1e-5, "op {operation}: {ours}");
            let theirs = stat(&mut os(), operation).unwrap_or(f32::NAN);
            assert!(
                (theirs - os_answer).abs() < 1e-5,
                "op {operation}: {theirs}"
            );
        }
        assert_eq!(ll_list_statistics(&mut sl(), 9, vec![i(-1), i(4)]), Ok(0.0));
        assert!(ll_list_statistics(&mut os(), 9, vec![i(-1), i(4)]).is_ok_and(f32::is_nan));
        assert_eq!(
            ll_list_statistics(&mut sl(), 4, vec![i(4), i(1), i(3), i(2)]),
            Ok(2.5)
        );
        assert_eq!(
            ll_list_statistics(&mut os(), 4, vec![i(4), i(1), i(3), i(2)]),
            Ok(2.5)
        );
        // errors/bounds `statistics-odd`.
        for operation in [3, 99, 0] {
            assert_eq!(ll_list_statistics(&mut sl(), operation, vec![]), Ok(0.0));
        }
        assert_eq!(
            ll_list_statistics(&mut sl(), 4, vec![s("a"), s("b")]),
            Ok(0.0)
        );
        assert_eq!(
            ll_list_statistics(&mut os(), 4, vec![s("a"), s("b")]),
            Err(CallError::Exception {
                id: BuiltinId::LlListStatistics,
                exception: Exception::IndexOutOfRange,
            })
        );
    }

    #[test]
    fn lists_as_text() {
        // strings/conversions: `llList2CSV` writes a float's every digit,
        // `llDumpList2String` a float as `(string)` does but negative zero.
        assert_eq!(
            ll_list2_csv(
                &mut (),
                vec![f(123_456_789.0), f(-0.0), f(f32::INFINITY), f(f32::NAN)]
            ),
            Ok("123456792.000000, -0.000000, inf, nan".to_owned())
        );
        assert_eq!(
            ll_list2_csv(&mut (), vec![v(1.0, -0.000_002, 123_456.7)]),
            Ok("<1.000000, -0.000002, 123456.703125>".to_owned())
        );
        assert_eq!(
            ll_dump_list2_string(
                &mut (),
                vec![f(123_456_789.0), f(-0.0), i(1), s("s"), v(1.0, 2.0, 3.0)],
                "|".to_owned()
            ),
            Ok("123456800.000000|0.000000|1|s|<1.000000, 2.000000, 3.000000>".to_owned())
        );
    }

    #[test]
    fn csv_to_list_per_grid() {
        let text = || "1,1.5,<1,2,3>,<1,2,3,4>,a b, c ,\"q,r\",,x".to_owned();
        let pieces = |ctx: &mut Bare| {
            ll_csv2_list(ctx, text()).map(|list| {
                list.iter()
                    .map(element_to_string)
                    .collect::<Vec<_>>()
                    .join("|")
            })
        };
        // strings/conversions `csv2list`.
        assert_eq!(
            pieces(&mut sl()),
            Ok("1|1.5|<1,2,3>|<1,2,3,4>|a b|c |\"q|r\"||x".to_owned())
        );
        assert_eq!(
            pieces(&mut os()),
            Ok("1|1.5|<1,2,3>|<1,2,3,4>|a b|c|\"q|r\"||x".to_owned())
        );
        assert_eq!(
            ll_csv2_list(&mut sl(), "<1,2>,<a,<b>,c>,d".to_owned()),
            Ok(vec![s("<1,2>"), s("<a,<b>,c>"), s("d")])
        );
    }

    #[test]
    fn parsing_separators_and_spacers() {
        let parse = |text: &str, separators: &[&str], spacers: &[&str], keep: bool| {
            let separators = separators.iter().copied().map(s).collect();
            let spacers = spacers.iter().copied().map(s).collect();
            let parsed = if keep {
                ll_parse_string_keep_nulls(&mut sl(), text.to_owned(), separators, spacers)
            } else {
                ll_parse_string2_list(&mut sl(), text.to_owned(), separators, spacers)
            };
            parsed.map(|list| csv(&list))
        };
        // strings/functions `parse *`.
        assert_eq!(
            parse("a,b;;c", &[","], &[";"], false),
            Ok("[a, b, ;, ;, c]".to_owned())
        );
        assert_eq!(
            parse("a--b-c", &["-"], &["--"], false),
            Ok("[a, b, c]".to_owned())
        );
        assert_eq!(
            parse("a--b-c", &["--"], &["-"], false),
            Ok("[a, b, -, c]".to_owned())
        );
        assert_eq!(
            parse("abcabc", &["b", "bc"], &[], false),
            Ok("[a, ca, c]".to_owned())
        );
        assert_eq!(
            parse(",a,,b,", &[","], &[], true),
            Ok("[, a, , b, ]".to_owned())
        );
        assert_eq!(parse("", &[","], &[], true), Ok("[]".to_owned()));
        assert_eq!(
            ll_parse_string_keep_nulls(&mut sl(), String::new(), vec![s(",")], vec![]),
            Ok(vec![s("")])
        );
        assert_eq!(parse(",a,,b,", &[","], &[], false), Ok("[a, b]".to_owned()));
        assert_eq!(parse("a", &[""], &[""], false), Ok("[a]".to_owned()));
        // errors/bounds: nine separators, and Second Life uses eight.
        let nine = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
        assert_eq!(
            parse("a1b2c3d4e5f6g7h8i9j", &nine, &[], false),
            Ok("[a, b, c, d, e, f, g, h, i9j]".to_owned())
        );
        let separators: Vec<Element> = nine.into_iter().map(s).collect();
        assert_eq!(
            ll_parse_string2_list(
                &mut os(),
                "a1b2c3d4e5f6g7h8i9j".to_owned(),
                separators,
                vec![]
            )
            .map(|list| csv(&list)),
            Ok("[a, b, c, d, e, f, g, h, i, j]".to_owned())
        );
    }
}
