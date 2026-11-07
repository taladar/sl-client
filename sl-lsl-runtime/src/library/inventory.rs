//! Library tranche: what a prim carries (`server-lsl-lib-task-inventory`) —
//! reading its contents, removing from them, and the scripts among them.
//! Giving and rezzing are [`super::rezzing`], notecards [`super::notecards`].
//!
//! The host lends the contents as plain [`Item`]s in any order; the order a
//! listing is read in, the words of a refusal and the `changed` a removal
//! raises are each flavour's, measured by `inventory/listing`
//! (`book/src/gridspec/lsl.md` § Reading the inventory).

use core::cmp::Ordering;

use crate::library::collation;
use crate::library::{CallError, Key};
use crate::value::NULL_KEY;
use crate::vm::ScriptCtx;
use crate::vm::contents::{Control, INVENTORY_ALL, INVENTORY_NONE, Item};

/// `CHANGED_INVENTORY`.
pub(crate) const CHANGED_INVENTORY: i32 = 1;

/// The items of the caller's prim of `kind` (every item for
/// [`INVENTORY_ALL`]), in the order the flavour lists them: on Second Life by
/// name with case folded and runs of digits compared as numbers (`9` before
/// `10`); on OpenSim by .NET's culture order (`ArrayList.Sort`), which puts
/// `_under` first and `10` before `9`.
pub(crate) fn listing(ctx: &mut ScriptCtx<'_>, kind: i32) -> Vec<Item> {
    let caller = ctx.caller();
    let mut items: Vec<Item> = ctx
        .host()
        .contents(caller)
        .into_iter()
        .filter(|item| kind == INVENTORY_ALL || item.kind == kind)
        .collect();
    if ctx.flavour().is_opensim() {
        items.sort_by(|a, b| culture_order(&a.name, &b.name));
    } else {
        items.sort_by(|a, b| second_life_order(&a.name, &b.name));
    }
    items
}

/// The item of the caller's prim named exactly `name` — the lookup is exact
/// on both grids: `ALPHA` is not `alpha`.
pub(crate) fn named(ctx: &mut ScriptCtx<'_>, name: &str) -> Option<Item> {
    let caller = ctx.caller();
    ctx.host()
        .contents(caller)
        .into_iter()
        .find(|item| item.name == name)
}

/// Second Life's listing order: case folded, digits as numbers, and the
/// names as they are where that leaves two equal.
fn second_life_order(a: &str, b: &str) -> Ordering {
    let lower = |text: &str| -> Vec<char> { text.chars().flat_map(char::to_lowercase).collect() };
    let (left, right) = (lower(a), lower(b));
    let (mut i, mut j) = (0_usize, 0_usize);
    while let (Some(x), Some(y)) = (left.get(i), right.get(j)) {
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let run = |text: &[char], from: usize| -> usize {
                text.iter()
                    .skip(from)
                    .take_while(|c| c.is_ascii_digit())
                    .count()
            };
            let (n, m) = (run(&left, i), run(&right, j));
            let digits = |text: &[char], from: usize, count: usize| -> String {
                let all: String = text.iter().skip(from).take(count).collect();
                let trimmed = all.trim_start_matches('0');
                trimmed.to_owned()
            };
            let (p, q) = (digits(&left, i, n), digits(&right, j, m));
            let by_number = p.len().cmp(&q.len()).then_with(|| p.cmp(&q));
            if by_number != Ordering::Equal {
                return by_number;
            }
            i = i.saturating_add(n);
            j = j.saturating_add(m);
            continue;
        }
        match x.cmp(y) {
            Ordering::Equal => {
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
            other => return other,
        }
    }
    left.len()
        .saturating_sub(i)
        .cmp(&right.len().saturating_sub(j))
        .then_with(|| a.cmp(b))
}

/// OpenSim's listing order: `String.Compare` under en-US, as Mono's collator
/// answers it.
fn culture_order(a: &str, b: &str) -> Ordering {
    let left: Vec<u16> = a.encode_utf16().collect();
    let right: Vec<u16> = b.encode_utf16().collect();
    collation::compare(&left, &right)
}

/// Says `No item named '…'.`, Second Life's refusal of a missing item by
/// every reader but `llGetInventoryType` and `llGetInventoryKey`.
fn no_item(ctx: &mut ScriptCtx<'_>, name: &str) {
    ctx.refuse(format!("No item named '{name}'."));
}

/// `llGetInventoryNumber`: how many items of `kind` the prim holds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_number(ctx: &mut ScriptCtx<'_>, kind: i32) -> Result<i32, CallError> {
    Ok(i32::try_from(listing(ctx, kind).len()).unwrap_or(i32::MAX))
}

/// `llGetInventoryName`: the name of the item at `index` among those of
/// `kind`, in the flavour's order; `""` past either end, and for a kind none
/// is of. OpenSim's `ArrayList` throws at −1 (`ArgumentOutOfRangeException`),
/// which YEngine holds the script for.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim at a negative index with items of the
/// kind.
pub fn ll_get_inventory_name(
    ctx: &mut ScriptCtx<'_>,
    kind: i32,
    index: i32,
) -> Result<String, CallError> {
    let items = listing(ctx, kind);
    if items.is_empty() {
        return Ok(String::new());
    }
    let Ok(at) = usize::try_from(index) else {
        if ctx.flavour().is_opensim() {
            return Err(CallError::Exception {
                id: crate::library::BuiltinId::LlGetInventoryName,
                exception: crate::library::Exception::ItemIndexOutOfRange,
            });
        }
        return Ok(String::new());
    };
    Ok(items
        .get(at)
        .map(|item| item.name.clone())
        .unwrap_or_default())
}

/// `llGetInventoryType`: the item's `INVENTORY_*` type, `INVENTORY_NONE`
/// for a name the prim does not hold, silently on both grids.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_type(ctx: &mut ScriptCtx<'_>, name: String) -> Result<i32, CallError> {
    Ok(named(ctx, &name).map_or(INVENTORY_NONE, |item| item.kind))
}

/// `llGetInventoryKey`: the item's asset key when its owner may modify,
/// copy and transfer it, else `NULL_KEY`; `NULL_KEY`, silently, for a name
/// the prim does not hold.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_key(ctx: &mut ScriptCtx<'_>, name: String) -> Result<Key, CallError> {
    Ok(Key(named(ctx, &name)
        .filter(Item::full_permission)
        .map_or_else(|| NULL_KEY.to_owned(), |item| item.asset)))
}

/// `llGetInventoryCreator`: the item's creator. A missing name is
/// `NULL_KEY` and `No item named '…'.` on Second Life; on OpenSim `""` and
/// `llGetInventoryCreator: Can't find item ''` — the name left out of the
/// words, as OpenSim's own message leaves it — with its second's sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_creator(ctx: &mut ScriptCtx<'_>, name: String) -> Result<Key, CallError> {
    if let Some(item) = named(ctx, &name) {
        return Ok(Key(item.creator));
    }
    if ctx.flavour().is_opensim() {
        ctx.refuse("llGetInventoryCreator: Can't find item ''".to_owned());
        return Ok(Key(String::new()));
    }
    no_item(ctx, &name);
    Ok(Key(NULL_KEY.to_owned()))
}

/// `llGetInventoryPermMask`: the item's mask `which` (`MASK_BASE` …
/// `MASK_NEXT`). A missing name is 0 and `No item named '…'.` on Second
/// Life, −1 and silence on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_perm_mask(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    which: i32,
) -> Result<i32, CallError> {
    if let Some(item) = named(ctx, &name) {
        return Ok(item.masks.get(which).unwrap_or(0));
    }
    if ctx.flavour().is_opensim() {
        return Ok(-1);
    }
    no_item(ctx, &name);
    Ok(0)
}

/// `llSetInventoryPermMask`: a god's call. For a resident's script it does
/// nothing and says nothing on either grid (`inventory/listing`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_inventory_perm_mask(
    _ctx: &mut ScriptCtx<'_>,
    _name: String,
    _which: i32,
    _mask: i32,
) -> Result<(), CallError> {
    Ok(())
}

/// `llGetInventoryDesc`: the item's description (`(No Description)` for a
/// taken object, `""` for a script); a missing name `""` and
/// `No item named '…'.`. Second Life's, which OpenSim does not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_desc(ctx: &mut ScriptCtx<'_>, name: String) -> Result<String, CallError> {
    if let Some(item) = named(ctx, &name) {
        return Ok(item.description);
    }
    no_item(ctx, &name);
    Ok(String::new())
}

/// `llGetInventoryAcquireTime`: when the item came into the owner's hands,
/// `YYYY-MM-DDThh:mm:ssZ`; a missing name `""` and `No item named '…'.`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_inventory_acquire_time(
    ctx: &mut ScriptCtx<'_>,
    name: String,
) -> Result<String, CallError> {
    if let Some(item) = named(ctx, &name) {
        return Ok(timestamp(item.acquired));
    }
    no_item(ctx, &name);
    Ok(String::new())
}

/// `seconds` since the epoch as an ISO 8601 UTC time to the second.
fn timestamp(seconds: i64) -> String {
    crate::library::calendar::timestamp(seconds.saturating_mul(1_000_000), 0)
}

/// `llRemoveInventory`: take the item out of the prim. Second Life raises no
/// `changed` in the prim, OpenSim `CHANGED_INVENTORY`; a missing name is
/// silent on both. The script removing itself stops there.
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(
    clippy::module_name_repetitions,
    reason = "the library function is named after its LSL call"
)]
pub fn ll_remove_inventory(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    let caller = ctx.caller();
    let own = ctx.script_named(&name) == Some(caller);
    let changed: &[i32] = if ctx.flavour().is_opensim() {
        &[CHANGED_INVENTORY]
    } else {
        &[]
    };
    if ctx.host().remove_item(caller, &name, changed) && own {
        ctx.die();
    }
    Ok(())
}

/// `llGetScriptName`: the calling script's inventory name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_script_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().whereabouts(caller).script)
}

/// Refuses a script name the prim does not hold, as `call` is refused in
/// the flavour's words, with the second it costs on both grids (OpenSim's
/// is every refusal's).
fn no_script(ctx: &mut ScriptCtx<'_>, call: &str, name: &str) {
    if ctx.flavour().is_opensim() {
        ctx.refuse(format!("{call}: Can't find script '{name}'"));
    } else {
        ctx.refuse(format!("Could not find script '{name}'."));
        ctx.error_pause();
    }
}

/// `llSetScriptState`: set the run flag of the script `name` of the prim,
/// which reads back at once. Stopping is not resetting, and a link message
/// to a stopped script is lost.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_script_state(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    running: i32,
) -> Result<(), CallError> {
    let Some(target) = ctx.script_named(&name) else {
        no_script(ctx, "llSetScriptState", &name);
        return Ok(());
    };
    ctx.control_script(target, Control::Run(running != 0));
    Ok(())
}

/// `llResetOtherScript`: reset the script `name` of the prim as the flavour
/// resets another script (the engine's rule: Second Life leaves a stopped
/// one where it was, OpenSim resets it; YEngine lets a running one run what
/// it has queued first). Naming the caller resets the caller, as OpenSim's
/// `llResetScript` does.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_reset_other_script(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    let Some(target) = ctx.script_named(&name) else {
        no_script(ctx, "llResetOtherScript", &name);
        return Ok(());
    };
    if target == ctx.caller() {
        ctx.reset();
    } else {
        ctx.control_script(target, Control::Reset);
    }
    Ok(())
}

/// `llSetRemoteScriptAccessPin`: the pin another prim's script must give to
/// load a script here (0: none may).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_remote_script_access_pin(ctx: &mut ScriptCtx<'_>, pin: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().set_remote_pin(caller, pin);
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{second_life_order, timestamp};

    /// Second Life's order of `inventory/listing`'s names: digits as numbers,
    /// case folded, the underscore before the letters.
    #[test]
    fn second_life_lists_digits_as_numbers_with_case_folded() {
        let mut names = vec![
            "Zed",
            "Probe",
            "lan",
            "Box",
            "Beta",
            "alpha",
            "aaa Helper",
            "_under",
            "10",
            "9",
        ];
        names.sort_by(|a, b| second_life_order(a, b));
        assert_eq!(
            names,
            vec![
                "9",
                "10",
                "_under",
                "aaa Helper",
                "alpha",
                "Beta",
                "Box",
                "lan",
                "Probe",
                "Zed"
            ]
        );
    }

    /// An acquire time is a UTC timestamp to the second.
    #[test]
    fn an_acquire_time_reads_as_iso_8601() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(1_793_059_200), "2026-10-27T00:00:00Z");
    }
}
