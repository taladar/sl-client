//! OpenSim's own library — the OSSL (`os*`), LightShare (`ls*`) and MOD
//! (`mod*`) families — as far as the fake grid runs it, ported from
//! `OSSL_Api.cs` and `MOD_Api.cs` (book chapter `simulator/lsl-engine.md`,
//! § OpenSim's own functions, for which are in and why).
//!
//! Only an OpenSim flavour compiles these, and every call has passed the
//! region's permission check by the time it gets here
//! (`vm::ossl::admit`). OpenSim's values are doubles; the value
//! model's are single precision, so each is worked in `f64` as OpenSim does
//! and stored as LSL's `f32`. OpenSim's strings are .NET's, so every index
//! and length counts UTF-16 units (`library::text::Text`).

use core::f64::consts::PI;
use core::fmt::Write as _;

use regex::{Captures, Regex};
use sha2::{Digest as _, Sha256};
use sl_types::lsl::{Rotation, Vector};

use crate::cast::element_to_string_for;
use crate::format;
use crate::library::collation::{self, Case, OutOfRange};
use crate::library::dotnet::is_uuid;
use crate::library::text::Text;
use crate::library::{CallError, Exception, Key, PureCtx, ThreatLevel, lists};
use crate::value::Element;
use crate::vm::comms::ListenFilter;
use crate::vm::ossl::warn;
use crate::vm::{EngineFlavour, ScriptCtx};

/// The flavour OSSL counts text in: OpenSim's, UTF-16 units.
const OPENSIM: EngineFlavour = EngineFlavour::YEngine;

/// A whole number of UTF-16 units as LSL's integer.
fn units(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// An index or a length as a count of units, negative as zero.
fn count_of(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// A double as LSL's float.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "OpenSim's double becomes the value model's single precision float"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

// ----------------------------------------------------------------------
// The engine and the simulator.
// ----------------------------------------------------------------------

/// `osGetScriptEngineName`: the engine's name, `YEngine` or `XEngine`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_script_engine_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(match ctx.flavour() {
        EngineFlavour::YEngine => "YEngine",
        EngineFlavour::XEngine => "XEngine",
        EngineFlavour::SecondLife => "",
    }
    .to_owned())
}

/// `osGetSimulatorVersion`: the simulator's version string.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_simulator_version(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(ctx.host().simulator().version)
}

/// `osGetPhysicsEngineType`: the physics engine's type — or, unlike every
/// other OSSL function, nothing rather than an error when the region does
/// not allow it (it asks the permission itself, at `High`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_physics_engine_type(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let standing = ctx.standing();
    if ctx
        .ossl()
        .refusal("osGetPhysicsEngineType", ThreatLevel::High, &standing)
        .is_some()
    {
        return Ok(String::new());
    }
    Ok(ctx.host().simulator().physics_engine_type)
}

/// `osGetPhysicsEngineName`: the physics engine's name and version.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_physics_engine_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(ctx.host().simulator().physics_engine_name)
}

/// `osCheckODE`: whether the physics engine is the old ODE plugin
/// (`OpenDynamicsEngine`); `ubODE` is not.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_check_ode(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(i32::from(
        ctx.host().simulator().physics_engine_type == "OpenDynamicsEngine",
    ))
}

/// `osGetGridName`: the grid's name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_grid_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(ctx.host().simulator().grid_name)
}

/// `osGetGridNick`: the grid's nickname.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_grid_nick(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(ctx.host().simulator().grid_nick)
}

// ----------------------------------------------------------------------
// The region.
// ----------------------------------------------------------------------

/// `osGetRegionSize`: the region's width and depth, and 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_region_size(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    let (x, y) = ctx.host().region_size(caller);
    Ok(Vector {
        x: single(f64::from(x)),
        y: single(f64::from(y)),
        z: 0.0,
    })
}

/// `osGetTerrainHeight`: the ground's height at a whole-metre point of the
/// region.
///
/// # Errors
///
/// `OSSL Runtime Error: osGetTerrainHeight: Coordinate out of bounds` for a
/// point outside the region; the event ends there.
pub fn os_get_terrain_height(ctx: &mut ScriptCtx<'_>, x: i32, y: i32) -> Result<f32, CallError> {
    terrain_height(ctx, x, y, crate::library::BuiltinId::OsGetTerrainHeight)
}

/// `osTerrainGetHeight`: the deprecated [`os_get_terrain_height`], whose
/// error names the new function too.
///
/// # Errors
///
/// As [`os_get_terrain_height`].
pub fn os_terrain_get_height(ctx: &mut ScriptCtx<'_>, x: i32, y: i32) -> Result<f32, CallError> {
    terrain_height(ctx, x, y, crate::library::BuiltinId::OsTerrainGetHeight)
}

/// The two terrain readers' shared body (`GetTerrainHeight`).
fn terrain_height(
    ctx: &mut ScriptCtx<'_>,
    x: i32,
    y: i32,
    id: crate::library::BuiltinId,
) -> Result<f32, CallError> {
    let caller = ctx.caller();
    let (width, depth) = ctx.host().region_size(caller);
    let inside = |coordinate: i32, size: u32| {
        u32::try_from(coordinate)
            .ok()
            .filter(|coordinate| *coordinate < size)
    };
    match (inside(x, width), inside(y, depth)) {
        (Some(x), Some(y)) => Ok(ctx.host().terrain_height(caller, x, y)),
        _ => Err(runtime_error(
            id,
            "osGetTerrainHeight: Coordinate out of bounds",
        )),
    }
}

/// `osGetSunParam`: a parameter of the sun — the day's length from the
/// environment over the prim; `year_length` 365, `day_night_offset` 0,
/// `update_interval` 0.1 and `day_time_sun_hour_scale` 1, which OpenSim no
/// longer models; 0 for any other name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_sun_param(ctx: &mut ScriptCtx<'_>, param: String) -> Result<f32, CallError> {
    Ok(match param.to_lowercase().as_str() {
        "day_length" => {
            let caller = ctx.caller();
            ctx.host().day_length(caller)
        }
        "year_length" => 365.0,
        "update_interval" => 0.1,
        "day_time_sun_hour_scale" => 1.0,
        _ => 0.0,
    })
}

/// `osSunGetParam`: the deprecated [`os_get_sun_param`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_sun_get_param(ctx: &mut ScriptCtx<'_>, param: String) -> Result<f32, CallError> {
    os_get_sun_param(ctx, param)
}

/// `osGetAgentIP`: the address an avatar in the region connects from — for a
/// script whose owner is a god; otherwise, and for an avatar not in the
/// region, nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_agent_ip(ctx: &mut ScriptCtx<'_>, agent: String) -> Result<String, CallError> {
    if !ctx.standing().is(crate::vm::Allowed::God) {
        return Ok(String::new());
    }
    Ok(ctx.host().agent_ip(&agent))
}

// ----------------------------------------------------------------------
// Talking to other scripts and to region modules.
// ----------------------------------------------------------------------

/// `osMessageObject`: raise `dataserver(<this prim's key>, message)` in the
/// scripts of the prim `object`; a key that is no key, or no such prim, is
/// shouted about instead.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_message_object(
    ctx: &mut ScriptCtx<'_>,
    object: Key,
    message: String,
) -> Result<(), CallError> {
    let Some(target) = crate::library::dotnet::canonical_uuid(&object.0) else {
        warn(
            ctx,
            "osMessageObject() cannot send messages to objects with invalid UUIDs".to_owned(),
        );
        return Ok(());
    };
    let Some(scripts) = ctx.host().prim_scripts(&target) else {
        warn(
            ctx,
            format!(
                "osMessageObject() cannot send message to {target}, object was not found in scene."
            ),
        );
        return Ok(());
    };
    let caller = ctx.caller();
    let from = ctx.host().whereabouts(caller).prim;
    ctx.message_scripts(scripts, from, message);
    Ok(())
}

/// `osListenRegex`: `llListen` with the name, the message or both read as
/// regular expressions a line need only contain a match of. A pattern that
/// is no regular expression is shouted about, and nothing is opened (-1).
///
/// # Errors
///
/// None; OpenSim holds a script to no number of listens.
pub fn os_listen_regex(
    ctx: &mut ScriptCtx<'_>,
    channel: i32,
    name: String,
    id: Key,
    message: String,
    regex: i32,
) -> Result<i32, CallError> {
    use crate::vm::comms::{OS_LISTEN_REGEX_MESSAGE, OS_LISTEN_REGEX_NAME};
    if regex & OS_LISTEN_REGEX_NAME == OS_LISTEN_REGEX_NAME && Regex::new(&name).is_err() {
        warn(ctx, "Name regex is invalid.".to_owned());
        return Ok(-1);
    }
    if regex & OS_LISTEN_REGEX_MESSAGE == OS_LISTEN_REGEX_MESSAGE && Regex::new(&message).is_err() {
        warn(ctx, "Message regex is invalid.".to_owned());
        return Ok(-1);
    }
    let flavour = ctx.flavour();
    ctx.listens()
        .open(
            ListenFilter::regex(channel, name, &id.0, message, regex),
            flavour,
        )
        .map_err(|_too_many| CallError::TooManyListens)
}

/// `modSendCommand`: hand a command to the region modules that take script
/// commands, and answer a fresh request key. The fake grid runs no such
/// module, so nothing answers, as on a region without the one named.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn mod_send_command(
    ctx: &mut ScriptCtx<'_>,
    _module: String,
    _command: String,
    _k: String,
) -> Result<Key, CallError> {
    Ok(Key(ctx.fresh_key()))
}

// ----------------------------------------------------------------------
// Strings and regular expressions.
// ----------------------------------------------------------------------

/// An OSSL run-time error: `OSSL Runtime Error: <message>`, which ends the
/// event.
fn runtime_error(id: crate::library::BuiltinId, message: &str) -> CallError {
    CallError::Thrown {
        id,
        thrown: crate::vm::Thrown {
            text: format!("OSSL Runtime Error: {message}"),
            to_owner: false,
        },
    }
}

/// `pattern` compiled, or the `ArgumentException` .NET's `Regex` throws for
/// one it cannot parse.
fn compiled(id: crate::library::BuiltinId, pattern: &str) -> Result<Regex, CallError> {
    Regex::new(pattern).map_err(|_error| CallError::Exception {
        id,
        exception: Exception::InvalidPattern,
    })
}

/// A UTF-16 index of `text` as a byte offset, rounded down to a character
/// boundary.
fn byte_offset(text: &str, index: usize) -> usize {
    let mut seen = 0_usize;
    for (offset, character) in text.char_indices() {
        if seen >= index {
            return offset;
        }
        seen = seen.saturating_add(character.len_utf16());
    }
    text.len()
}

/// A byte offset of `text` as a UTF-16 index.
fn utf16_index(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(0, |before| before.encode_utf16().count())
}

/// A start counted from the end when negative (OpenSim's normalisation),
/// or [`None`] when it is outside `length`.
fn normalised_start(start: i32, length: usize) -> Option<usize> {
    let length = units(length);
    let start = if start < 0 {
        length.saturating_add(start)
    } else {
        start
    };
    (0..length).contains(&start).then(|| count_of(start))
}

/// `osFormatString`: .NET's `String.Format` of `format` with the list's
/// elements, each written as OpenSim writes the element (a float with six
/// decimals): `{0}` the first, `{1,5}` the second right-aligned in five
/// characters, `{{` and `}}` the braces themselves. A format specifier after
/// a colon is read and ignored, since no LSL type takes one.
///
/// # Errors
///
/// [`Exception::FormatIndex`] for an item past the list, and
/// [`Exception::FormatInvalid`] for a format that is not one.
pub fn os_format_string<C: PureCtx>(
    _ctx: &mut C,
    format: String,
    strings: Vec<Element>,
) -> Result<String, CallError> {
    let thrown = |exception| CallError::Exception {
        id: crate::library::BuiltinId::OsFormatString,
        exception,
    };
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                let _brace = chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                let _brace = chars.next();
                out.push('}');
            }
            '}' => return Err(thrown(Exception::FormatInvalid)),
            '{' => {
                let mut item = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(inner) => item.push(inner),
                        None => return Err(thrown(Exception::FormatInvalid)),
                    }
                }
                let item = item.split(':').next().unwrap_or_default();
                let (index, alignment) = match item.split_once(',') {
                    Some((index, alignment)) => (index, Some(alignment.trim())),
                    None => (item, None),
                };
                let index: usize = index
                    .trim()
                    .parse()
                    .map_err(|_error| thrown(Exception::FormatInvalid))?;
                let alignment: i32 = match alignment {
                    Some(alignment) => alignment
                        .parse()
                        .map_err(|_error| thrown(Exception::FormatInvalid))?,
                    None => 0,
                };
                let text = element_to_string_for(
                    strings
                        .get(index)
                        .ok_or_else(|| thrown(Exception::FormatIndex))?,
                    true,
                );
                let pad = count_of(alignment.saturating_abs())
                    .saturating_sub(text.encode_utf16().count());
                let padding = " ".repeat(pad);
                if alignment < 0 {
                    out.push_str(&text);
                    out.push_str(&padding);
                } else {
                    out.push_str(&padding);
                    out.push_str(&text);
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// `osMatchString`: every match of `pattern` in `text` from `start` (counted
/// from the end when negative), each as its groups — the whole match first
/// — that matched, every one a string and its UTF-16 index. Empty when the
/// start is outside the text.
///
/// # Errors
///
/// [`Exception::InvalidPattern`] for a pattern that is no regular
/// expression.
pub fn os_match_string<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    pattern: String,
    start: i32,
) -> Result<Vec<Element>, CallError> {
    let Some(start) = normalised_start(start, text.encode_utf16().count()) else {
        return Ok(Vec::new());
    };
    let regex = compiled(crate::library::BuiltinId::OsMatchString, &pattern)?;
    let mut out = Vec::new();
    for captures in matches_from(&regex, &text, byte_offset(&text, start)) {
        for group in captures.iter().flatten() {
            out.push(Element::String(group.as_str().to_owned()));
            out.push(Element::Integer(units(utf16_index(&text, group.start()))));
        }
    }
    Ok(out)
}

/// Every successive match of `regex` in `text` from the byte offset `from`,
/// as .NET's `Match(text, start)` then `NextMatch` finds them: an empty
/// match moves the search on a character.
fn matches_from<'text>(regex: &Regex, text: &'text str, from: usize) -> Vec<Captures<'text>> {
    let mut found = Vec::new();
    let mut at = from;
    while at <= text.len() {
        let Some(captures) = regex.captures_at(text, at) else {
            break;
        };
        let Some(whole) = captures.get(0) else {
            break;
        };
        at = if whole.end() > whole.start() {
            whole.end()
        } else {
            text.get(whole.end()..)
                .and_then(|rest| rest.chars().next())
                .map_or_else(
                    || text.len().saturating_add(1),
                    |next| whole.end().saturating_add(next.len_utf8()),
                )
        };
        found.push(captures);
    }
    found
}

/// `osReplaceString`: `text` with up to `count` matches of `pattern` from
/// `start` (counted from the end when negative; -1 for every match)
/// replaced by `replacement`, in which `$1`, `${name}`, `$&`, `` $` ``,
/// `$'`, `$+`, `$_` and `$$` mean what they mean to .NET. The text
/// unchanged when the start is outside it.
///
/// # Errors
///
/// [`Exception::InvalidPattern`] for a pattern that is no regular
/// expression.
pub fn os_replace_string<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    pattern: String,
    replacement: String,
    count: i32,
    start: i32,
) -> Result<String, CallError> {
    let Some(start) = normalised_start(start, text.encode_utf16().count()) else {
        return Ok(text);
    };
    let regex = compiled(crate::library::BuiltinId::OsReplaceString, &pattern)?;
    if count == 0 {
        return Ok(text);
    }
    let found = matches_from(&regex, &text, byte_offset(&text, start));
    let limit = usize::try_from(count).unwrap_or(usize::MAX);
    let mut out = String::new();
    let mut copied = 0_usize;
    for captures in found.iter().take(limit) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        out.push_str(text.get(copied..whole.start()).unwrap_or_default());
        substitute(&regex, captures, &text, &replacement, &mut out);
        copied = whole.end();
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    Ok(out)
}

/// .NET's substitution of one match into `out`.
fn substitute(
    regex: &Regex,
    captures: &Captures<'_>,
    text: &str,
    replacement: &str,
    out: &mut String,
) {
    let whole = captures.get(0);
    let group = |index: usize| captures.get(index).map_or("", |found| found.as_str());
    let mut rest = replacement;
    while let Some(at) = rest.find('$') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at.saturating_add(1)..).unwrap_or_default();
        let mut taken = 1_usize;
        match after.chars().next() {
            Some('$') => out.push('$'),
            Some('&') => out.push_str(group(0)),
            Some('`') => out.push_str(
                whole
                    .and_then(|whole| text.get(..whole.start()))
                    .unwrap_or_default(),
            ),
            Some('\'') => out.push_str(
                whole
                    .and_then(|whole| text.get(whole.end()..))
                    .unwrap_or_default(),
            ),
            Some('+') => out.push_str(group(captures.len().saturating_sub(1))),
            Some('_') => out.push_str(text),
            Some('{') => match after.find('}') {
                Some(end) => {
                    let name = after.get(1..end).unwrap_or_default();
                    let value = match name.parse::<usize>() {
                        Ok(index) if index < captures.len() => Some(group(index)),
                        Ok(_) => None,
                        Err(_) => regex
                            .capture_names()
                            .flatten()
                            .any(|known| known == name)
                            .then(|| captures.name(name).map_or("", |found| found.as_str())),
                    };
                    match value {
                        Some(value) => {
                            out.push_str(value);
                            taken = end.saturating_add(1);
                        }
                        None => {
                            out.push('$');
                            taken = 0;
                        }
                    }
                }
                None => {
                    out.push('$');
                    taken = 0;
                }
            },
            Some(digit) if digit.is_ascii_digit() => {
                // The longest run of digits that names a group.
                let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
                let named = (1..=digits.len()).rev().find_map(|length| {
                    digits
                        .get(..length)
                        .and_then(|prefix| prefix.parse::<usize>().ok())
                        .filter(|index| *index < captures.len())
                        .map(|index| (index, length))
                });
                match named {
                    Some((index, length)) => {
                        out.push_str(group(index));
                        taken = length;
                    }
                    None => {
                        out.push('$');
                        taken = 0;
                    }
                }
            }
            _ => {
                out.push('$');
                taken = 0;
            }
        }
        rest = after.get(taken..).unwrap_or_default();
    }
    out.push_str(rest);
}

/// `osRegexIsMatch`: whether `text` holds a match of `pattern`; a pattern
/// that is no regular expression is shouted about, and answers 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_regex_is_match(
    ctx: &mut ScriptCtx<'_>,
    text: String,
    pattern: String,
) -> Result<i32, CallError> {
    match Regex::new(&pattern) {
        Ok(regex) => Ok(i32::from(regex.is_match(&text))),
        Err(_error) => {
            warn(
                ctx,
                "Possible invalid regular expression detected.".to_owned(),
            );
            Ok(0)
        }
    }
}

/// `osStringSubString`: `length` units from `start`, as much as there is; the
/// whole text from a start at or before 0 with the text's own length.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_sub_string<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    start: i32,
    length: i32,
) -> Result<String, CallError> {
    let units_of = Text::new(OPENSIM, &text);
    let total = units_of.length();
    if text.is_empty() || length <= 0 || start >= total {
        return Ok(String::new());
    }
    let start = if start <= 0 {
        if length == total {
            return Ok(text);
        }
        0
    } else {
        start
    };
    let length = length.min(total.saturating_sub(start));
    let from = count_of(start);
    let to = from.saturating_add(count_of(length));
    Ok(units_of.string(units_of.units().get(from..to).unwrap_or_default()))
}

/// `osStringSubString(string, integer)`: the units from `start` to the end;
/// the whole text from a start at or before 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_sub_string_to_end<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    start: i32,
) -> Result<String, CallError> {
    let units_of = Text::new(OPENSIM, &text);
    if text.is_empty() || start >= units_of.length() {
        return Ok(String::new());
    }
    if start <= 0 {
        return Ok(text);
    }
    Ok(units_of.string(units_of.units().get(count_of(start)..).unwrap_or_default()))
}

/// `osStringRemove`: the text without `count` units from `start` (to the
/// end when `count` is not positive or reaches past it); empty from a start
/// past the text, or when the cut would take all of it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_remove<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    start: i32,
    count: i32,
) -> Result<String, CallError> {
    let units_of = Text::new(OPENSIM, &text);
    let total = units_of.length();
    if text.is_empty() || start >= total {
        return Ok(String::new());
    }
    let start = start.max(0);
    let left = total.saturating_sub(start);
    let count = if count <= 0 || count > left {
        left
    } else {
        count
    };
    if count >= total {
        return Ok(String::new());
    }
    let from = count_of(start);
    let to = from.saturating_add(count_of(count));
    let mut kept: Vec<u32> = units_of.units().get(..from).unwrap_or_default().to_vec();
    kept.extend_from_slice(units_of.units().get(to..).unwrap_or_default());
    Ok(units_of.string(&kept))
}

/// `osStringReplace`: every occurrence of `old` replaced by `new`, compared
/// unit by unit; empty when the text or `old` is.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_replace<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    old: String,
    new: String,
) -> Result<String, CallError> {
    if text.is_empty() || old.is_empty() {
        return Ok(String::new());
    }
    Ok(text.replace(&old, &new))
}

/// .NET's UTF-16 code units of `text`, which the culture-sensitive search
/// works on.
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The case mode an OSSL search's `ignorecase` argument picks: any value
/// but 0 ignores case.
const fn case_of(ignore_case: i32) -> Case {
    if ignore_case == 0 {
        Case::Sensitive
    } else {
        Case::Ignored
    }
}

/// OpenSim's start and count for a ranged search: nothing from a start at
/// or past the end; a negative start is 0; a count that is not positive, or
/// reaches past the end, is the rest of the string.
fn clamped_range(total: i32, start: i32, count: i32) -> Option<(i32, i32)> {
    if start >= total {
        return None;
    }
    let start = start.max(0);
    let left = total.saturating_sub(start);
    let count = if count <= 0 || count > left {
        left
    } else {
        count
    };
    Some((start, count))
}

/// A search's answer, or the exception .NET threw for its range.
fn searched(
    id: crate::library::BuiltinId,
    found: Result<i32, OutOfRange>,
) -> Result<i32, CallError> {
    found.map_err(|OutOfRange| CallError::Exception {
        id,
        exception: Exception::CountOutOfRange,
    })
}

/// `osStringStartsWith`: whether `text` starts with `value` as the en-US
/// culture compares (`library::collation`), folding case unless
/// `ignore_case` is 0; 0 when either is empty.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_starts_with<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(0);
    }
    Ok(i32::from(collation::starts_with(
        &utf16(&text),
        &utf16(&value),
        case_of(ignore_case),
    )))
}

/// `osStringEndsWith`: [`os_string_starts_with`] at the end.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_ends_with<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(0);
    }
    Ok(i32::from(collation::ends_with(
        &utf16(&text),
        &utf16(&value),
        case_of(ignore_case),
    )))
}

/// `osStringIndexOf`: where `value` first starts in `text` as the en-US
/// culture compares, in UTF-16 units; -1 when either is empty.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_index_of<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(-1);
    }
    let text = utf16(&text);
    let length = i32::try_from(text.len()).unwrap_or(i32::MAX);
    searched(
        crate::library::BuiltinId::OsStringIndexOf2,
        collation::index_of(&text, &utf16(&value), 0, length, case_of(ignore_case)),
    )
}

/// `osStringIndexOf(src, value, start, count, ignorecase)`: the search
/// within `count` units from `start`, both clamped to the string
/// (`clamped_range`); -1 from a start at or past its end.
///
/// # Errors
///
/// None that OpenSim's clamping lets through; the signature is the
/// library's.
pub fn os_string_index_of_range<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    start: i32,
    count: i32,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(-1);
    }
    let text = utf16(&text);
    let total = i32::try_from(text.len()).unwrap_or(i32::MAX);
    let Some((start, count)) = clamped_range(total, start, count) else {
        return Ok(-1);
    };
    searched(
        crate::library::BuiltinId::OsStringIndexOf,
        collation::index_of(&text, &utf16(&value), start, count, case_of(ignore_case)),
    )
}

/// `osStringLastIndexOf`: where `value` last starts in `text` as the en-US
/// culture compares; -1 when either is empty.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_string_last_index_of<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(-1);
    }
    let text = utf16(&text);
    let length = i32::try_from(text.len()).unwrap_or(i32::MAX);
    searched(
        crate::library::BuiltinId::OsStringLastIndexOf2,
        collation::last_index_of(
            &text,
            &utf16(&value),
            length.saturating_sub(1),
            length,
            case_of(ignore_case),
        ),
    )
}

/// `osStringLastIndexOf(src, value, start, count, ignorecase)`: OpenSim
/// clamps `start` and `count` as for a forward search (`clamped_range`)
/// and hands them to `String.LastIndexOf`, which reads the start as where
/// its search **ends** and counts back from there — so `count` units back
/// from `start` must fit, which they do only from a start at or past the
/// middle of the string (`ossl/strings`).
///
/// # Errors
///
/// [`Exception::CountOutOfRange`] where they do not: from a start of 0 in
/// any string longer than one unit.
pub fn os_string_last_index_of_range<C: PureCtx>(
    _ctx: &mut C,
    text: String,
    value: String,
    start: i32,
    count: i32,
    ignore_case: i32,
) -> Result<i32, CallError> {
    if text.is_empty() || value.is_empty() {
        return Ok(-1);
    }
    let text = utf16(&text);
    let total = i32::try_from(text.len()).unwrap_or(i32::MAX);
    let Some((start, count)) = clamped_range(total, start, count) else {
        return Ok(-1);
    };
    searched(
        crate::library::BuiltinId::OsStringLastIndexOf,
        collation::last_index_of(&text, &utf16(&value), start, count, case_of(ignore_case)),
    )
}

/// `osListSortInPlace`: `llListSort`'s OpenSim sort of the list, ascending
/// only for an `ascending` of exactly 1, written into the list object it was
/// handed (`ScriptCtx::write_back`), so every variable holding that object
/// reads it sorted.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_list_sort_in_place(
    ctx: &mut ScriptCtx<'_>,
    list: Vec<Element>,
    stride: i32,
    ascending: i32,
) -> Result<(), CallError> {
    ctx.write_back(0, lists::os_sort(&list, stride, ascending == 1));
    Ok(())
}

/// `osSHA256`: the SHA-256 of the text's UTF-8 bytes, in lower-case hex.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_sha256<C: PureCtx>(_ctx: &mut C, text: String) -> Result<String, CallError> {
    let mut out = String::new();
    for byte in Sha256::digest(text.as_bytes()) {
        let _infallible = write!(out, "{byte:02x}");
    }
    Ok(out)
}

/// `osIsUUID`: whether the text is a key in any form `UUID.TryParse` takes.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_is_uuid<C: PureCtx>(_ctx: &mut C, text: String) -> Result<i32, CallError> {
    Ok(i32::from(is_uuid(&text)))
}

/// `osUnixTimeToTimestamp`: a Unix time as `yyyy-MM-ddTHH:mm:ss.fffffffZ`,
/// its fraction always seven zeros.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_unix_time_to_timestamp<C: PureCtx>(_ctx: &mut C, time: i32) -> Result<String, CallError> {
    // Every `i32` second is a date `OffsetDateTime` holds.
    let date = time::OffsetDateTime::from_unix_timestamp(i64::from(time))
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.0000000Z",
        date.year(),
        u8::from(date.month()),
        date.day(),
        date.hour(),
        date.minute(),
        date.second()
    ))
}

// ----------------------------------------------------------------------
// Numbers and vectors.
// ----------------------------------------------------------------------

/// `osIsNotValidNumber`: 1 for NaN, 2 for negative infinity, 3 for positive
/// infinity, 0 for a number.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_is_not_valid_number<C: PureCtx>(_ctx: &mut C, value: f32) -> Result<i32, CallError> {
    Ok(if value.is_nan() {
        1
    } else if value == f32::NEG_INFINITY {
        2
    } else if value == f32::INFINITY {
        3
    } else {
        0
    })
}

/// .NET's `Math.Min` and `Math.Max`: NaN when either is.
const fn min_max(a: f32, b: f32, max: bool) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if max { a.max(b) } else { a.min(b) }
}

/// `osMin`: the lesser of two floats.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_min<C: PureCtx>(_ctx: &mut C, a: f32, b: f32) -> Result<f32, CallError> {
    Ok(min_max(a, b, false))
}

/// `osMax`: the greater of two floats.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_max<C: PureCtx>(_ctx: &mut C, a: f32, b: f32) -> Result<f32, CallError> {
    Ok(min_max(a, b, true))
}

/// `osRound`: the value rounded half away from zero to `digits` decimals
/// (none when not positive, at most 15).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_round<C: PureCtx>(_ctx: &mut C, value: f32, digits: i32) -> Result<f32, CallError> {
    let value = f64::from(value);
    if digits <= 0 {
        return Ok(single(value.round()));
    }
    let scale = 10_f64.powi(digits.min(15));
    let scaled = value * scale;
    if !scaled.is_finite() {
        return Ok(single(value));
    }
    Ok(single(scaled.round() / scale))
}

/// A vector in OpenSim's doubles.
fn doubles(vector: &Vector) -> [f64; 3] {
    [
        f64::from(vector.x),
        f64::from(vector.y),
        f64::from(vector.z),
    ]
}

/// `osVecMagSquare`: the vector's squared length.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_vec_mag_square<C: PureCtx>(_ctx: &mut C, vector: Vector) -> Result<f32, CallError> {
    let [x, y, z] = doubles(&vector);
    Ok(single(x * x + y * y + z * z))
}

/// `osVecDistSquare`: the squared distance between two points.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_vec_dist_square<C: PureCtx>(
    _ctx: &mut C,
    a: Vector,
    b: Vector,
) -> Result<f32, CallError> {
    let [ax, ay, az] = doubles(&a);
    let [bx, by, bz] = doubles(&b);
    let (x, y, z) = (ax - bx, ay - by, az - bz);
    Ok(single(x * x + y * y + z * z))
}

/// `osApproxEquals`: whether two floats are within a millionth of each
/// other.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals<C: PureCtx>(_ctx: &mut C, a: f32, b: f32) -> Result<i32, CallError> {
    Ok(within(&[f64::from(a)], &[f64::from(b)], 1.0e-6))
}

/// Whether every pair of components is within `margin` of each other.
fn within(a: &[f64], b: &[f64], margin: f64) -> i32 {
    let margin = margin.abs();
    i32::from(
        a.iter()
            .zip(b)
            .all(|(a, b)| !(*a > *b + margin || *a < *b - margin)),
    )
}

/// A rotation in OpenSim's doubles.
fn rotation_doubles(rotation: &Rotation) -> [f64; 4] {
    [
        f64::from(rotation.x),
        f64::from(rotation.y),
        f64::from(rotation.z),
        f64::from(rotation.s),
    ]
}

/// `osApproxEquals(float, float, float)`: whether two floats are within
/// `margin` of each other.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals_margin<C: PureCtx>(
    _ctx: &mut C,
    a: f32,
    b: f32,
    margin: f32,
) -> Result<i32, CallError> {
    Ok(within(&[f64::from(a)], &[f64::from(b)], f64::from(margin)))
}

/// `osApproxEquals(vector, vector, float)`: whether two vectors are within
/// `margin` of each other in every component.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals_vector_margin<C: PureCtx>(
    _ctx: &mut C,
    a: Vector,
    b: Vector,
    margin: f32,
) -> Result<i32, CallError> {
    Ok(within(&doubles(&a), &doubles(&b), f64::from(margin)))
}

/// `osApproxEquals(rotation, rotation)`: whether two rotations are within a
/// millionth of each other in every component.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals_rotation<C: PureCtx>(
    _ctx: &mut C,
    a: Rotation,
    b: Rotation,
) -> Result<i32, CallError> {
    Ok(within(&rotation_doubles(&a), &rotation_doubles(&b), 1.0e-6))
}

/// `osApproxEquals(vector, vector)`: whether two vectors are within a
/// millionth of each other in every component.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals_vector<C: PureCtx>(
    _ctx: &mut C,
    a: Vector,
    b: Vector,
) -> Result<i32, CallError> {
    Ok(within(&doubles(&a), &doubles(&b), 1.0e-6))
}

/// `osApproxEquals(rotation, rotation, float)`: whether two rotations are
/// within `margin` of each other in every component.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_approx_equals_rotation_margin<C: PureCtx>(
    _ctx: &mut C,
    a: Rotation,
    b: Rotation,
    margin: f32,
) -> Result<i32, CallError> {
    Ok(within(
        &rotation_doubles(&a),
        &rotation_doubles(&b),
        f64::from(margin),
    ))
}

/// `osAngleBetween`: the angle between two vectors, in radians, from
/// 0 to π.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_angle_between<C: PureCtx>(_ctx: &mut C, a: Vector, b: Vector) -> Result<f32, CallError> {
    let [ax, ay, az] = doubles(&a);
    let [bx, by, bz] = doubles(&b);
    let dot = ax * bx + ay * by + az * bz;
    let (cx, cy, cz) = (ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx);
    let cross = (cx * cx + cy * cy + cz * cz).sqrt();
    Ok(single(cross.atan2(dot)))
}

/// `osSlerp`: the spherical interpolation from `a` to `b` by `amount`
/// (clamped to 0 … 1), as `LSL_Vector.Slerp` does it — which takes the
/// vectors for unit ones without normalising them.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_slerp<C: PureCtx>(
    _ctx: &mut C,
    a: Vector,
    b: Vector,
    amount: f32,
) -> Result<Vector, CallError> {
    let amount = f64::from(amount.clamp(0.0, 1.0));
    let [ax, ay, az] = doubles(&a);
    let [bx, by, bz] = doubles(&b);
    let cosine = ax * bx + ay * by + az * bz;
    let (scale, other) = if cosine < f64::from(0.999_f32) {
        let angle = cosine.acos();
        let inverse = 1.0 / angle.sin();
        (
            ((1.0 - amount) * angle).sin() * inverse,
            inverse * (amount * angle).sin(),
        )
    } else {
        (1.0 - amount, amount)
    };
    Ok(Vector {
        x: single(ax * scale + bx * other),
        y: single(ay * scale + by * other),
        z: single(az * scale + bz * other),
    })
}

/// A rotation of unit length, as OpenSim's `Quaternion.Normalize` makes
/// one: the identity for one of (nearly) no length.
fn normalised(rotation: [f64; 4]) -> [f64; 4] {
    let [x, y, z, s] = rotation;
    let length_squared = x * x + y * y + z * z + s * s;
    if length_squared < f64::from(f32::from_bits(1)) {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inverse = 1.0 / length_squared.sqrt();
    [x * inverse, y * inverse, z * inverse, s * inverse]
}

/// `osSlerp(rotation, rotation, float)`: the spherical interpolation from
/// `a` to `b`, both normalised first, by `amount` (clamped to 0 … 1), the
/// shorter way round (`LSL_Rotation.Slerp`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_slerp_rotation<C: PureCtx>(
    _ctx: &mut C,
    a: Rotation,
    b: Rotation,
    amount: f32,
) -> Result<Rotation, CallError> {
    let amount = f64::from(amount.clamp(0.0, 1.0));
    let mut a = normalised(rotation_doubles(&a));
    let mut b = normalised(rotation_doubles(&b));
    let mut cosine: f64 = a.iter().zip(&b).map(|(a, b)| a * b).sum();
    if cosine < 0.0 {
        a = a.map(|component| -component);
        cosine = -cosine;
    }
    let (scale, other) = if cosine + 1.0 > 0.0005 {
        if 1.0 - cosine >= 0.0005 {
            let theta = cosine.acos();
            let inverse = 1.0 / theta.sin();
            (
                (theta * (1.0 - amount)).sin() * inverse,
                (theta * amount).sin() * inverse,
            )
        } else {
            (1.0 - amount, amount)
        }
    } else {
        let [x, y, z, s] = a;
        b = [-y, x, -s, z];
        ((PI * (0.5 - amount)).sin(), (PI * amount).sin())
    };
    let [ax, ay, az, as_] = a;
    let [bx, by, bz, bs] = b;
    Ok(Rotation {
        x: single(ax * scale + bx * other),
        y: single(ay * scale + by * other),
        z: single(az * scale + bz * other),
        s: single(as_ * scale + bs * other),
    })
}

// ----------------------------------------------------------------------
// Drawing lists: the commands a dynamic texture is rendered from, which
// these only write down.
// ----------------------------------------------------------------------

/// A float in a drawing list, as OpenSim's `LSL_Float` writes itself.
fn draw_float(value: f32) -> String {
    format::opensim_float(value)
}

/// `osMovePen`: `MoveTo x,y;`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_move_pen<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: i32,
    y: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}MoveTo {x},{y};"))
}

/// `osDrawLine`: `MoveTo x,y; LineTo x,y; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_line<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) -> Result<String, CallError> {
    Ok(format!(
        "{list}MoveTo {start_x},{start_y}; LineTo {end_x},{end_y}; "
    ))
}

/// `osDrawLine(string, integer, integer)`: `LineTo x,y; ` from where the pen
/// is.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_line_to<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    end_x: i32,
    end_y: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}LineTo {end_x},{end_y}; "))
}

/// `osDrawText`: `Text text; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_text<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    text: String,
) -> Result<String, CallError> {
    Ok(format!("{list}Text {text}; "))
}

/// `osDrawEllipse`: `Ellipse w,h; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_ellipse<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    width: i32,
    height: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}Ellipse {width},{height}; "))
}

/// `osDrawFilledEllipse`: `FillEllipse w,h; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_filled_ellipse<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    width: i32,
    height: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}FillEllipse {width},{height}; "))
}

/// `osDrawRectangle`: `Rectangle w,h; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_rectangle<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    width: i32,
    height: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}Rectangle {width},{height}; "))
}

/// `osDrawFilledRectangle`: `FillRectangle w,h; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_filled_rectangle<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    width: i32,
    height: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}FillRectangle {width},{height}; "))
}

/// A polygon command: the points in pairs, or nothing at all — the whole
/// drawing list dropped — for fewer than three points or lists of unequal
/// length.
fn polygon(list: String, command: &str, x: &[Element], y: &[Element]) -> String {
    if x.len() != y.len() || x.len() < 3 {
        return String::new();
    }
    let points: Vec<String> = x
        .iter()
        .zip(y)
        .map(|(x, y)| {
            format!(
                "{},{}",
                element_to_string_for(x, true),
                element_to_string_for(y, true)
            )
        })
        .collect();
    format!("{list}{command} {}; ", points.join(","))
}

/// `osDrawPolygon`: `Polygon x,y,x,y,…; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_polygon<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: Vec<Element>,
    y: Vec<Element>,
) -> Result<String, CallError> {
    Ok(polygon(list, "Polygon", &x, &y))
}

/// `osDrawFilledPolygon`: `FillPolygon x,y,x,y,…; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_filled_polygon<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: Vec<Element>,
    y: Vec<Element>,
) -> Result<String, CallError> {
    Ok(polygon(list, "FillPolygon", &x, &y))
}

/// `osDrawImage`: `Image w,h,url; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_image<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    width: i32,
    height: i32,
    url: String,
) -> Result<String, CallError> {
    Ok(format!("{list}Image {width},{height},{url}; "))
}

/// `osDrawResetTransform`: `ResetTransf;`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_reset_transform<C: PureCtx>(
    _ctx: &mut C,
    list: String,
) -> Result<String, CallError> {
    Ok(format!("{list}ResetTransf;"))
}

/// `osDrawRotationTransform`: `RotTransf x;`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_rotation_transform<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: f32,
) -> Result<String, CallError> {
    Ok(format!("{list}RotTransf {};", draw_float(x)))
}

/// `osDrawScaleTransform`: `ScaleTransf x,y;`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_scale_transform<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: f32,
    y: f32,
) -> Result<String, CallError> {
    Ok(format!(
        "{list}ScaleTransf {},{};",
        draw_float(x),
        draw_float(y)
    ))
}

/// `osDrawTranslationTransform`: `TransTransf x,y;`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_draw_translation_transform<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    x: f32,
    y: f32,
) -> Result<String, CallError> {
    Ok(format!(
        "{list}TransTransf {},{};",
        draw_float(x),
        draw_float(y)
    ))
}

/// `osSetFontName`: `FontName name; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_font_name<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    name: String,
) -> Result<String, CallError> {
    Ok(format!("{list}FontName {name}; "))
}

/// `osSetFontSize`: `FontSize n; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_font_size<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    size: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}FontSize {size}; "))
}

/// `osSetPenSize`: `PenSize n; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_pen_size<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    size: i32,
) -> Result<String, CallError> {
    Ok(format!("{list}PenSize {size}; "))
}

/// `osSetPenColor`: `PenColor colour; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_pen_color<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    color: String,
) -> Result<String, CallError> {
    Ok(format!("{list}PenColor {color}; "))
}

/// `osSetPenColour`: the deprecated spelling, which writes `PenColour`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_pen_colour<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    colour: String,
) -> Result<String, CallError> {
    Ok(format!("{list}PenColour {colour}; "))
}

/// `osSetPenCap`: `PenCap direction,type; `.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_pen_cap<C: PureCtx>(
    _ctx: &mut C,
    list: String,
    direction: String,
    kind: String,
) -> Result<String, CallError> {
    Ok(format!("{list}PenCap {direction},{kind}; "))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::os;

    #[test]
    fn format_string_as_dotnet_formats() -> Result<(), CallError> {
        let list = vec![
            Element::String("a".to_owned()),
            Element::Integer(2),
            Element::Float(1.5),
        ];
        let format = |text: &str| os_format_string(&mut os(), text.to_owned(), list.clone());
        assert_eq!(format("{0}-{1}-{0}")?, "a-2-a");
        assert_eq!(format("[{1,3}|{0,-3}]")?, "[  2|a  ]");
        assert_eq!(format("{{{2:F1}}}")?, "{1.500000}");
        assert!(matches!(
            format("{3}"),
            Err(CallError::Exception {
                exception: Exception::FormatIndex,
                ..
            })
        ));
        assert!(matches!(
            format("{0"),
            Err(CallError::Exception {
                exception: Exception::FormatInvalid,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn regular_expressions_match_and_replace_as_dotnet() -> Result<(), CallError> {
        let found = os_match_string(&mut os(), "abcabc".to_owned(), "b".to_owned(), 0)?;
        assert_eq!(
            found,
            vec![
                Element::String("b".to_owned()),
                Element::Integer(1),
                Element::String("b".to_owned()),
                Element::Integer(4),
            ]
        );
        let grouped = os_match_string(
            &mut os(),
            "x12y".to_owned(),
            "([0-9])([0-9])".to_owned(),
            -3,
        )?;
        assert_eq!(grouped.len(), 6);
        assert_eq!(
            os_match_string(&mut os(), "abc".to_owned(), "b".to_owned(), 3)?,
            Vec::new()
        );
        let replace = |pattern: &str, with: &str, count, start| {
            os_replace_string(
                &mut os(),
                "abcabc".to_owned(),
                pattern.to_owned(),
                with.to_owned(),
                count,
                start,
            )
        };
        assert_eq!(replace("b", "x", -1, 0)?, "axcaxc");
        assert_eq!(replace("b", "x", 1, 0)?, "axcabc");
        assert_eq!(replace("b", "x", -1, 2)?, "abcaxc");
        assert_eq!(replace("(b)(c)", "$2$1$$", -1, 0)?, "acb$acb$");
        assert_eq!(replace("b", "[$`|$']", 1, 0)?, "a[a|cabc]cabc");
        assert!(matches!(
            replace("(", "x", -1, 0),
            Err(CallError::Exception {
                exception: Exception::InvalidPattern,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn strings_cut_in_utf16_units() -> Result<(), CallError> {
        let cut = |text: &str, start, length| {
            os_string_sub_string(&mut os(), text.to_owned(), start, length)
        };
        assert_eq!(cut("abcdef", 2, 3)?, "cde");
        assert_eq!(cut("abcdef", -1, 6)?, "abcdef");
        assert_eq!(cut("abcdef", -1, 2)?, "ab");
        assert_eq!(cut("abcdef", 4, 9)?, "ef");
        assert_eq!(cut("a\u{1D11E}b", 1, 2)?, "\u{1D11E}");
        let remove =
            |text: &str, start, count| os_string_remove(&mut os(), text.to_owned(), start, count);
        assert_eq!(remove("abcdef", 1, 2)?, "adef");
        assert_eq!(remove("abcdef", 2, 0)?, "ab");
        assert_eq!(remove("abcdef", 0, 0)?, "");
        assert_eq!(
            os_string_replace(
                &mut os(),
                "abcabc".to_owned(),
                "b".to_owned(),
                String::new()
            )?,
            "acac"
        );
        Ok(())
    }

    #[test]
    fn numbers_and_hashes_as_the_probe_read_them() -> Result<(), CallError> {
        assert_eq!(
            os_sha256(&mut os(), "abc".to_owned())?,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            os_unix_time_to_timestamp(&mut os(), 0)?,
            "1970-01-01T00:00:00.0000000Z"
        );
        assert_eq!(
            os_unix_time_to_timestamp(&mut os(), 951_782_400)?,
            "2000-02-29T00:00:00.0000000Z"
        );
        assert_eq!(
            os_unix_time_to_timestamp(&mut os(), i32::MIN)?,
            "1901-12-13T20:45:52.0000000Z"
        );
        assert_eq!(
            os_round(&mut os(), 1.234_56, 2)?.to_bits(),
            1.23_f32.to_bits()
        );
        assert_eq!(os_round(&mut os(), 2.5, 0)?.to_bits(), 3.0_f32.to_bits());
        assert_eq!(
            os_round(&mut os(), -2.5, 0)?.to_bits(),
            (-3.0_f32).to_bits()
        );
        assert_eq!(
            os_min(&mut os(), 1.5, -2.0)?.to_bits(),
            (-2.0_f32).to_bits()
        );
        assert_eq!(os_max(&mut os(), 1.5, -2.0)?.to_bits(), 1.5_f32.to_bits());
        assert!(os_max(&mut os(), f32::NAN, 1.0)?.is_nan());
        let v = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        assert_eq!(
            os_vec_mag_square(&mut os(), v.clone())?.to_bits(),
            14.0_f32.to_bits()
        );
        assert_eq!(os_approx_equals(&mut os(), 1.0, 1.000_000_1)?, 1);
        assert_eq!(os_approx_equals(&mut os(), 1.0, 1.001)?, 0);
        assert_eq!(os_is_not_valid_number(&mut os(), f32::NEG_INFINITY)?, 2);
        assert_eq!(os_is_uuid(&mut os(), "nope".to_owned())?, 0);
        let x = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let y = Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let right = os_angle_between(&mut os(), x.clone(), y.clone())?;
        assert!((right - core::f32::consts::FRAC_PI_2).abs() < 1.0e-6);
        let half = os_slerp(&mut os(), x, y, 0.5)?;
        assert!((half.x - core::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-6);
        Ok(())
    }

    #[test]
    fn drawing_lists_as_opensim_writes_them() -> Result<(), CallError> {
        assert_eq!(
            os_draw_text(&mut os(), String::new(), "hi".to_owned())?,
            "Text hi; "
        );
        assert_eq!(
            os_draw_line(&mut os(), "PenSize 2; ".to_owned(), 1, 2, 3, 4)?,
            "PenSize 2; MoveTo 1,2; LineTo 3,4; "
        );
        assert_eq!(
            os_draw_scale_transform(&mut os(), String::new(), 1.5, 2.0)?,
            "ScaleTransf 1.500000,2.000000;"
        );
        let points = |values: [i32; 3]| values.map(Element::Integer).to_vec();
        assert_eq!(
            os_draw_polygon(
                &mut os(),
                String::new(),
                points([0, 5, 10]),
                points([0, 10, 0])
            )?,
            "Polygon 0,0,5,10,10,0; "
        );
        assert_eq!(
            os_draw_filled_polygon(&mut os(), "kept? ".to_owned(), points([0, 5, 10]), vec![])?,
            ""
        );
        Ok(())
    }
}
