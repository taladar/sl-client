//! Library tranche: a script linking and delinking its own object
//! (`server-world-link-sets`) — `llCreateLink`, `llBreakLink` and
//! `llBreakAllLinks`, each behind `PERMISSION_CHANGE_LINKS`.
//!
//! What a link makes of the order and which prim becomes a root is the
//! region's, the same as a viewer's link and delink of the prims: the host
//! does it. What is here is what each grid checks first and says when it
//! refuses, as the `prim/link` probe measured it (book chapter
//! `gridspec/lsl.md`, § Linking) and, where it did not, `LSL_Api.cs`'s.

use crate::library::{CallError, Key, dotnet};
use crate::vm::ScriptCtx;
use crate::vm::comms::Linked;
use crate::vm::permissions::PERMISSION_CHANGE_LINKS;

/// Whether the script holds `PERMISSION_CHANGE_LINKS`; when it does not, the
/// refusal is said in `ctx`'s flavour's words — `opensim` OpenSim's, else
/// Second Life's.
fn may_change_links(ctx: &mut ScriptCtx<'_>, second_life: &str, opensim: &str) -> bool {
    if ctx.permissions().holds(PERMISSION_CHANGE_LINKS) {
        return true;
    }
    let text = if ctx.flavour().is_opensim() {
        opensim
    } else {
        second_life
    };
    ctx.refuse(text.to_owned());
    false
}

/// `llCreateLink`: link the object `target` names with the script's own —
/// the script's the root when `parent` is true, `target`'s when false
/// (`prim/link`: the target became the root and the script's prim link 2).
/// Both prims hear `changed(CHANGED_LINK)`; the forced second of sleep is the
/// table's.
///
/// Refused without `PERMISSION_CHANGE_LINKS`, on Second Life with
/// `Script trying to link but PERMISSION_CHANGE_LINKS permission not set!`
/// and on OpenSim with `llCreateLink: PERMISSION_CHANGE_LINKS permission not
/// set` — though OpenSim first drops a target that is no key without a word.
/// A target that is no prim the owner may link (`NULL_KEY`, a key nothing
/// has, the owner's own) is `Script trying to link but target is invalid.` on
/// Second Life and nothing on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_create_link(ctx: &mut ScriptCtx<'_>, target: Key, parent: i32) -> Result<(), CallError> {
    if ctx.flavour().is_opensim() && !dotnet::is_uuid(&target.0) {
        return Ok(());
    }
    if !may_change_links(
        ctx,
        "Script trying to link but PERMISSION_CHANGE_LINKS permission not set!",
        "llCreateLink: PERMISSION_CHANGE_LINKS permission not set",
    ) {
        return Ok(());
    }
    let caller = ctx.caller();
    let linked = ctx.host().create_link(caller, &target.0, parent != 0);
    if linked.is_err() && !ctx.flavour().is_opensim() {
        ctx.refuse("Script trying to link but target is invalid.".to_owned());
    }
    Ok(())
}

/// `llBreakLink`: take the prim numbered `link` out of the script's linkset
/// (`prim/link`: `llBreakLink(2)` and `llBreakLink(1)` of a set of two each
/// leave both prims free, and both hear `CHANGED_LINK`). `LINK_THIS` is the
/// script's own prim; a number past the last prim's, 0 in a linkset, a
/// sitter's number and every other constant take nothing out. Whoever sits on
/// the set is stood up first, on both grids (`prim/link-refusals`; the host
/// does it).
///
/// Refused without `PERMISSION_CHANGE_LINKS`: `llBreakLink:
/// PERMISSION_CHANGE_LINKS permission not set` on OpenSim, and on Second
/// Life `Script trying to break links but PERMISSION_CHANGE_LINKS permission
/// not set!` (`prim/link-refusals`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_break_link(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<(), CallError> {
    if !may_change_links(
        ctx,
        "Script trying to break links but PERMISSION_CHANGE_LINKS permission not set!",
        "llBreakLink: PERMISSION_CHANGE_LINKS permission not set",
    ) {
        return Ok(());
    }
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    if links.prims.len() < 2 {
        return Ok(());
    }
    if let Some(Linked::Prim(prim)) = links.numbered(link) {
        ctx.host().break_link(caller, &prim.key);
    }
    Ok(())
}

/// `llBreakAllLinks`: take every prim of the script's linkset out of it
/// (`prim/link`: from a child too). Nothing for a prim on its own. Second
/// Life stands up whoever sits on the set first, OpenSim leaves them sitting
/// on the root (`prim/link-refusals`; the host does it).
///
/// Refused without `PERMISSION_CHANGE_LINKS` — the same words as
/// `llBreakLink`'s on Second Life, and on OpenSim `llBreakAllLinks: Script
/// trying to link but PERMISSION_CHANGE_LINKS permission not set!`
/// (`prim/link-refusals`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_break_all_links(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    if !may_change_links(
        ctx,
        "Script trying to break links but PERMISSION_CHANGE_LINKS permission not set!",
        "llBreakAllLinks: Script trying to link but PERMISSION_CHANGE_LINKS permission not set!",
    ) {
        return Ok(());
    }
    let caller = ctx.caller();
    if ctx.host().links(caller).prims.len() > 1 {
        ctx.host().break_all_links(caller);
    }
    Ok(())
}
