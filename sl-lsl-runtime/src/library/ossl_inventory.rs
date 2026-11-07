//! OpenSim's own functions that shadow the task-inventory tranche
//! (`server-lsl-lib-task-inventory`, scope from `server-lsl-lib-ossl`): the
//! inventory readers by name or item key, the notecard writer, the rezzing
//! object, `osDie`, `osResetAllScripts` and the region's loaded-OAR data,
//! each ported from `OSSL_Api.cs`. They reach these functions only on the
//! OpenSim flavours, past the `[OSSL]` gate (`vm::ossl::admit`); the
//! synchronous notecard readers are [`super::notecards`].

use crate::library::control::ll_die;
use crate::library::inventory::CHANGED_INVENTORY;
use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::contents::{Control, Item};

/// The item of the caller's prim whose item key is `name`, when `name` is a
/// key, else whose name is `name` — OpenSim's `GetInventoryItem` by id or by
/// name.
fn by_name_or_id(ctx: &mut ScriptCtx<'_>, name: &str) -> Option<Item> {
    let caller = ctx.caller();
    let contents = ctx.host().contents(caller);
    match crate::library::dotnet::canonical_uuid(name) {
        Some(id) => contents
            .into_iter()
            .find(|item| item.item.eq_ignore_ascii_case(&id)),
        None => contents.into_iter().find(|item| item.name == name),
    }
}

/// `osGetInventoryDesc`: the description of the item named, or keyed, so;
/// `""` for none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_inventory_desc(ctx: &mut ScriptCtx<'_>, name: String) -> Result<String, CallError> {
    Ok(by_name_or_id(ctx, &name)
        .map(|item| item.description)
        .unwrap_or_default())
}

/// `osGetInventoryItemKey`: the item key of the item named so, when its
/// owner may modify, copy and transfer it; `NULL_KEY` otherwise and for
/// none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_inventory_item_key(ctx: &mut ScriptCtx<'_>, name: String) -> Result<Key, CallError> {
    Ok(Key(crate::library::inventory::named(ctx, &name)
        .filter(Item::full_permission)
        .map_or_else(|| NULL_KEY.to_owned(), |item| item.item)))
}

/// `osGetInventoryLastOwner`: who owned the item named, or keyed, so before
/// — its owner when nobody did; `NULL_KEY` for none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_inventory_last_owner(
    ctx: &mut ScriptCtx<'_>,
    name: String,
) -> Result<Key, CallError> {
    let Some(item) = by_name_or_id(ctx, &name) else {
        return Ok(Key(NULL_KEY.to_owned()));
    };
    if item.last_owner != NULL_KEY && !item.last_owner.is_empty() {
        return Ok(Key(item.last_owner));
    }
    let caller = ctx.caller();
    Ok(Key(ctx.host().owner(caller)))
}

/// `osGetInventoryName`: the name of the item keyed `item`; `""` for none
/// and for a string that is no key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_inventory_name(ctx: &mut ScriptCtx<'_>, item: Key) -> Result<String, CallError> {
    if crate::library::dotnet::canonical_uuid(&item.0).is_none() {
        return Ok(String::new());
    }
    Ok(by_name_or_id(ctx, &item.0)
        .map(|found| found.name)
        .unwrap_or_default())
}

/// `osGetLastChangedEventKey`: the key of the detected block's first entry
/// in the event being handled, `""` when it has none — the fake grid fills
/// no detected block for a `changed`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_last_changed_event_key(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(Key(ctx
        .detected(0)
        .map(|detected| detected.key.clone())
        .unwrap_or_default()))
}

/// `osGetRezzingObject`: the object that rezzed the caller's, `NULL_KEY`
/// for one an avatar rezzed.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_rezzing_object(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let object = ctx.host().whereabouts(caller).object;
    let rezzer = ctx
        .host()
        .object_details(caller, &object)
        .map(|details| details.rezzer)
        .unwrap_or_default();
    let by_avatar = rezzer.is_empty()
        || rezzer == NULL_KEY
        || ctx
            .host()
            .object_details(caller, &rezzer)
            .is_some_and(|details| details.avatar);
    Ok(Key(if by_avatar {
        NULL_KEY.to_owned()
    } else {
        rezzer
    }))
}

/// `osDie`: `llDie` of the caller's own object for `NULL_KEY` or its own
/// key; otherwise take out an object of the same owner the caller's object
/// rezzed, and nothing for any other (an attachment, another owner's). A key
/// that is none is shouted about.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_die(ctx: &mut ScriptCtx<'_>, object: Key) -> Result<(), CallError> {
    let Some(target) = crate::library::dotnet::canonical_uuid(&object.0) else {
        ctx.shout_error("osDie() cannot delete objects with invalid UUIDs".to_owned());
        ctx.sleep(1.0);
        return Ok(());
    };
    let caller = ctx.caller();
    let own = ctx.host().whereabouts(caller).object;
    let root = ctx.host().linkset_of(&target).into_iter().next();
    if target == NULL_KEY
        || root
            .as_deref()
            .is_some_and(|root| root.eq_ignore_ascii_case(&own))
    {
        return ll_die(ctx);
    }
    let _gone = ctx.host().derez(caller, &target);
    Ok(())
}

/// `osResetAllScripts`: reset every other script of the caller's prim — of
/// its whole linkset when `linkset` is not 0 — as the engine resets another
/// script, then the caller.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_reset_all_scripts(ctx: &mut ScriptCtx<'_>, linkset: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let scripts: Vec<_> = links
        .prims
        .iter()
        .filter(|prim| linkset != 0 || prim.scripts.contains(&caller))
        .flat_map(|prim| prim.scripts.iter().copied())
        .filter(|script| *script != caller)
        .collect();
    for script in scripts {
        ctx.control_script(script, Control::Reset);
    }
    ctx.reset();
    Ok(())
}

/// `osMakeNotecard`: write a notecard of the list's elements, each ended by
/// a newline, into the caller's prim — under `name`, or the first `name N`
/// the prim does not hold — described `Script generated notecard`; a
/// `changed(CHANGED_INVENTORY)` follows.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_make_notecard(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    contents: Vec<Element>,
) -> Result<(), CallError> {
    let text = contents.iter().fold(String::new(), |mut all, element| {
        all.push_str(&crate::cast::element_to_string(element));
        all.push('\n');
        all
    });
    let caller = ctx.caller();
    ctx.host().make_notecard(
        caller,
        &name,
        "Script generated notecard",
        &text,
        &[CHANGED_INVENTORY],
    );
    Ok(())
}

/// `osLoadedCreationDate`: the creation date of the OAR the region was
/// loaded from, `""` for a region no OAR was loaded into — every region of
/// the fake grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_loaded_creation_date(_ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(String::new())
}

/// `osLoadedCreationTime`: the creation time of the OAR the region was
/// loaded from, `""` as [`os_loaded_creation_date`].
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_loaded_creation_time(_ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(String::new())
}

/// `osLoadedCreationID`: the id of the OAR the region was loaded from, `""`
/// as [`os_loaded_creation_date`].
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_loaded_creation_id(_ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok(String::new())
}
