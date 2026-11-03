//! Library tranche: what a script says about who may change its prim's
//! contents (`server-world-changed-raisers`) — `llAllowInventoryDrop`.
//!
//! The rest of the contents family — reading, giving and removing items — is
//! `server-lsl-lib-task-inventory`'s.

use crate::library::CallError;
use crate::vm::ScriptCtx;

/// `llAllowInventoryDrop`: whether anybody may drop an item into the
/// script's prim, as well as those who may modify it. The prim's flag; it
/// raises no `changed` on either grid (`prim/changed`), and a drop it lets in
/// raises `CHANGED_ALLOWED_DROP` (`events/changed-contents`, the region's).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_allow_inventory_drop(ctx: &mut ScriptCtx<'_>, allow: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().allow_inventory_drop(caller, allow != 0);
    Ok(())
}
