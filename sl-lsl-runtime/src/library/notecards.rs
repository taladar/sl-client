//! Library tranche: reading notecards (`server-lsl-lib-task-inventory`) —
//! `llGetNotecardLine` and `llGetNumberOfNotecardLines`, answered over the
//! `dataserver` event with the key the call returned, Second Life's
//! `llGetNotecardLineSync`, and OpenSim's synchronous `osGetNotecard*`.
//!
//! The host hands over a notecard's text; how it splits into lines, where a
//! line is cut and what a line past the end reads are each flavour's
//! (`inventory/notecards`, `book/src/gridspec/lsl.md` § Notecards).

use crate::library::inventory::named;
use crate::library::{CallError, Key};
use crate::value::NULL_KEY;
use crate::vm::ScriptCtx;
use crate::vm::contents::{INVENTORY_NOTECARD, notecard_lines};

/// `EOF`: what a line past the end of a notecard reads.
pub(crate) const EOF: &str = "\n\n\n";

/// `NAK`: what `llGetNotecardLineSync` reads of a notecard the region has
/// not read.
pub(crate) const NAK: &str = "\n\u{15}\n";

/// The most bytes of a line Second Life answers: a longer one is cut at the
/// last character that fits (1100 ASCII characters read as 1024, 600
/// two-byte ones as 512).
const SECOND_LIFE_LINE_BYTES: usize = 1024;

/// The most UTF-16 units of a line OpenSim answers
/// (`m_notecardLineReadCharsMax`).
const OPENSIM_LINE_UNITS: usize = 255;

/// Where the notecard a script names is: the asset of the notecard item of
/// that name, or of that asset key, in the caller's prim — and on OpenSim any
/// key at all, which it reads as an asset key whatever the prim holds.
fn notecard_asset(ctx: &mut ScriptCtx<'_>, name: &str) -> Option<String> {
    if let Some(item) = named(ctx, name).filter(|item| item.kind == INVENTORY_NOTECARD) {
        return Some(item.asset);
    }
    let canonical = crate::library::dotnet::canonical_uuid(name)?;
    let caller = ctx.caller();
    let held =
        ctx.host().contents(caller).into_iter().any(|item| {
            item.kind == INVENTORY_NOTECARD && item.asset.eq_ignore_ascii_case(&canonical)
        });
    (held || ctx.flavour().is_opensim()).then_some(canonical)
}

/// The line `line` of `text` as the flavour answers it.
fn line_of(text: &str, line: i32, opensim: bool) -> String {
    let lines = notecard_lines(text, opensim);
    let Ok(at) = usize::try_from(line) else {
        // OpenSim's `GetLine` answers a negative line with nothing.
        return if opensim {
            String::new()
        } else {
            EOF.to_owned()
        };
    };
    let Some(found) = lines.get(at) else {
        return EOF.to_owned();
    };
    if opensim {
        cut_units(found, OPENSIM_LINE_UNITS)
    } else {
        cut_bytes(found, SECOND_LIFE_LINE_BYTES)
    }
}

/// `text` cut to at most `limit` bytes, at a character boundary.
pub(crate) fn cut_bytes(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// `text` cut to at most `limit` UTF-16 units, as .NET's `Substring` cuts
/// it — but never through a character, which a Rust string cannot hold half
/// of.
fn cut_units(text: &str, limit: usize) -> String {
    let mut units = 0_usize;
    text.chars()
        .take_while(|c| {
            units = units.saturating_add(c.len_utf16());
            units <= limit
        })
        .collect()
}

/// Refuses a notecard the caller's prim does not hold, as `call` is refused
/// in the flavour's words: Second Life's request then never answers, and
/// OpenSim's call answers `NULL_KEY` after its second's sleep.
fn no_notecard(ctx: &mut ScriptCtx<'_>, call: &str, name: &str) -> Key {
    if ctx.flavour().is_opensim() {
        ctx.refuse(format!("{call}: Can't find notecard '{name}'"));
        // It returns before its own `ScriptSleep`.
        ctx.delay(0.0);
        return Key(NULL_KEY.to_owned());
    }
    ctx.refuse(format!("Could not find notecard '{name}'."));
    Key(ctx.fresh_key())
}

/// A request answered over `dataserver` once the slice is over: a fresh key,
/// and `answer` of the notecard's text when the host has the notecard, or
/// `""` when it has not (OpenSim's answer to a key that is no notecard).
fn answered(
    ctx: &mut ScriptCtx<'_>,
    asset: &str,
    answer: impl FnOnce(&str, bool) -> String,
) -> Key {
    let opensim = ctx.flavour().is_opensim();
    let data = ctx
        .host()
        .notecard_text(asset)
        .map_or_else(String::new, |text| answer(&text, opensim));
    let id = ctx.fresh_key();
    ctx.raise_dataserver(id.clone(), data);
    Key(id)
}

/// `llGetNotecardLine`: ask for line `line` of the notecard `name`; the
/// answer comes over `dataserver` under the key returned. Second Life cuts a
/// line at 1024 bytes and reads line −1 as `EOF`; OpenSim cuts at 255
/// characters and reads −1 as `""`. Ten asked at once answer in order.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_notecard_line(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    line: i32,
) -> Result<Key, CallError> {
    let Some(asset) = notecard_asset(ctx, &name) else {
        return Ok(no_notecard(ctx, "llGetNotecardLine", &name));
    };
    Ok(answered(ctx, &asset, |text, opensim| {
        line_of(text, line, opensim)
    }))
}

/// `llGetNumberOfNotecardLines`: ask how many lines the notecard `name` has;
/// the answer, a number as a string, comes over `dataserver`. A final newline
/// starts one more, empty, line on Second Life and not on OpenSim; an empty
/// notecard has one line there and none here.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_number_of_notecard_lines(
    ctx: &mut ScriptCtx<'_>,
    name: String,
) -> Result<Key, CallError> {
    let Some(asset) = notecard_asset(ctx, &name) else {
        return Ok(no_notecard(ctx, "llGetNumberOfNotecardLines", &name));
    };
    Ok(answered(ctx, &asset, |text, opensim| {
        notecard_lines(text, opensim).len().to_string()
    }))
}

/// `llGetNotecardLineSync`: line `line` of the notecard `name` at once, if
/// the region has read the notecard (`NAK` before); −1 and past the end
/// `EOF`; a missing notecard `NAK` and `Could not find notecard '…'.`.
/// Second Life's, which OpenSim does not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_notecard_line_sync(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    line: i32,
) -> Result<String, CallError> {
    let Some(asset) = notecard_asset(ctx, &name) else {
        ctx.refuse(format!("Could not find notecard '{name}'."));
        return Ok(NAK.to_owned());
    };
    Ok(ctx
        .host()
        .notecard_read(&asset)
        .map_or_else(|| NAK.to_owned(), |text| line_of(&text, line, false)))
}

/// Shouts OpenSim's complaint of a notecard it cannot find, as the
/// `osGetNotecard*` functions do (`OSSLShoutError`).
fn not_found(ctx: &mut ScriptCtx<'_>, name: &str) {
    ctx.shout_error(format!("Notecard '{name}' could not be found."));
    ctx.error_pause();
}

/// `osGetNotecardLine`: line `line` of the notecard `name` at once, read
/// through the region's cache; `ERROR!` and a shout for a notecard it cannot
/// find.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_notecard_line(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    line: i32,
) -> Result<String, CallError> {
    let text = notecard_asset(ctx, &name).and_then(|asset| ctx.host().notecard_text(&asset));
    let Some(text) = text else {
        not_found(ctx, &name);
        return Ok("ERROR!".to_owned());
    };
    // `NotecardCache.GetLine` without a length: the whole line.
    let lines = notecard_lines(&text, true);
    Ok(match usize::try_from(line) {
        Err(_) => String::new(),
        Ok(at) => lines.get(at).cloned().unwrap_or_else(|| EOF.to_owned()),
    })
}

/// `osGetNotecard`: the whole of the notecard `name`, its lines each ended
/// by a newline (`LoadNotecard`); `ERROR!` and a shout for one it cannot find.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_notecard(ctx: &mut ScriptCtx<'_>, name: String) -> Result<String, CallError> {
    let text = notecard_asset(ctx, &name).and_then(|asset| ctx.host().notecard_text(&asset));
    let Some(text) = text else {
        not_found(ctx, &name);
        return Ok("ERROR!".to_owned());
    };
    Ok(notecard_lines(&text, true)
        .iter()
        .fold(String::new(), |mut all, line| {
            all.push_str(line);
            all.push('\n');
            all
        }))
}

/// `osGetNumberOfNotecardLines`: how many lines the notecard `name` has, at
/// once; −1 and a shout for one it cannot find.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_number_of_notecard_lines(
    ctx: &mut ScriptCtx<'_>,
    name: String,
) -> Result<i32, CallError> {
    let text = notecard_asset(ctx, &name).and_then(|asset| ctx.host().notecard_text(&asset));
    let Some(text) = text else {
        not_found(ctx, &name);
        return Ok(-1);
    };
    Ok(i32::try_from(notecard_lines(&text, true).len()).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{EOF, line_of};

    /// Ten lines and a final newline: eleven on Second Life, ten on OpenSim;
    /// line −1 `EOF` there and `""` here.
    #[test]
    fn a_final_newline_is_a_line_on_second_life_only() {
        let text = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        assert_eq!(line_of(text, 10, false), "");
        assert_eq!(line_of(text, 10, true), EOF);
        assert_eq!(line_of(text, -1, false), EOF);
        assert_eq!(line_of(text, -1, true), "");
        assert_eq!(line_of("", 0, false), "");
        assert_eq!(line_of("", 0, true), EOF);
    }

    /// A line is cut at 1024 bytes on Second Life, between characters, and at
    /// 255 characters on OpenSim.
    #[test]
    fn a_long_line_is_cut_as_each_grid_cuts_it() {
        let wide = "é".repeat(600);
        assert_eq!(line_of(&wide, 0, false).chars().count(), 512);
        assert_eq!(line_of(&wide, 0, true).chars().count(), 255);
        let narrow = "b".repeat(1100);
        assert_eq!(line_of(&narrow, 0, false).len(), 1024);
    }
}
