//! `llLinksetData*`: the linkset's key/value store, as a script reaches it.
//!
//! The store and every rule of it are
//! [`vm::linkset_data`](crate::vm::linkset_data)'s; these are the calls, each
//! on the caller's linkset's store through
//! [`ScriptCtx::linkset_data`] and [`ScriptCtx::change_linkset_data`], which
//! raises `linkset_data` for what a call changed. Second Life only: OpenSim's
//! engines compile none of them ([`Dialect::has_builtin`](crate::Dialect)).

use crate::library::CallError;
use crate::value::Element;
use crate::vm::ScriptCtx;

/// `llLinksetDataAvailable`: the bytes left of the store's 131 072.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_available(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(ctx.linkset_data().available())
}

/// `llLinksetDataCountKeys`: how many pairs the store holds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_count_keys(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(ctx.linkset_data().count_keys())
}

/// `llLinksetDataRead`: an ordinary pair's value; `""` for a protected or
/// missing one.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_read(ctx: &mut ScriptCtx<'_>, name: String) -> Result<String, CallError> {
    Ok(ctx.linkset_data().read(&name, ""))
}

/// `llLinksetDataReadProtected`: a pair's value when `password` is its
/// password; `""` otherwise.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_read_protected(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    password: String,
) -> Result<String, CallError> {
    Ok(ctx.linkset_data().read(&name, &password))
}

/// `llLinksetDataWrite`: write an ordinary pair, or delete it with an empty
/// value; a `LINKSETDATA_*` code.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_write(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    value: String,
) -> Result<i32, CallError> {
    Ok(ctx.change_linkset_data(|store| store.write(&name, &value, "")))
}

/// `llLinksetDataWriteProtected`: write a pair protected by `password`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_write_protected(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    value: String,
    password: String,
) -> Result<i32, CallError> {
    Ok(ctx.change_linkset_data(|store| store.write(&name, &value, &password)))
}

/// `llLinksetDataDelete`: delete an ordinary pair.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_delete(ctx: &mut ScriptCtx<'_>, name: String) -> Result<i32, CallError> {
    Ok(ctx.change_linkset_data(|store| store.delete(&name, "")))
}

/// `llLinksetDataDeleteProtected`: delete a pair protected by `password`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_delete_protected(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    password: String,
) -> Result<i32, CallError> {
    Ok(ctx.change_linkset_data(|store| store.delete(&name, &password)))
}

/// `llLinksetDataReset`: empty the store.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_reset(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    ctx.change_linkset_data(crate::vm::LinksetData::reset);
    Ok(())
}

/// The names as a list of strings.
fn strings(names: Vec<String>) -> Vec<Element> {
    names.into_iter().map(Element::String).collect()
}

/// `llLinksetDataListKeys`: the names in key order, `count` of them from
/// `start`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_list_keys(
    ctx: &mut ScriptCtx<'_>,
    start: i32,
    count: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(strings(ctx.linkset_data().list_keys(start, count)))
}

/// `llLinksetDataFindKeys`: the names a regular expression finds, `count` of
/// them from `start`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_find_keys(
    ctx: &mut ScriptCtx<'_>,
    pattern: String,
    start: i32,
    count: i32,
) -> Result<Vec<Element>, CallError> {
    Ok(strings(
        ctx.linkset_data().find_keys(&pattern, start, count),
    ))
}

/// `llLinksetDataCountFound`: how many names a regular expression finds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_count_found(
    ctx: &mut ScriptCtx<'_>,
    pattern: String,
) -> Result<i32, CallError> {
    Ok(ctx.linkset_data().count_found(&pattern))
}

/// `llLinksetDataDeleteFound`: delete the pairs a regular expression finds
/// that `password` may delete; `[deleted, not deleted]`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_linkset_data_delete_found(
    ctx: &mut ScriptCtx<'_>,
    pattern: String,
    password: String,
) -> Result<Vec<Element>, CallError> {
    let (deleted, kept) = ctx.change_linkset_data(|store| store.delete_found(&pattern, &password));
    Ok(vec![Element::Integer(deleted), Element::Integer(kept)])
}
