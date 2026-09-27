//! Library tranche: the calls that are the VM's own control flow — a script
//! suspending or restarting itself. Each asks the VM through the
//! [`ScriptCtx`] and takes effect once the call has returned.

use crate::library::CallError;
use crate::vm::ScriptCtx;

/// `llSleep`: suspend the script for `seconds`, counted in ticks and rounded
/// up to a whole one. Nothing else of the script runs meanwhile; its events
/// queue. A duration that is not positive does not suspend.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sleep(ctx: &mut ScriptCtx<'_>, seconds: f32) -> Result<(), CallError> {
    ctx.sleep(seconds);
    Ok(())
}

/// `llResetScript`: restart the script — globals back to their initialisers,
/// the stack and the event queue dropped, `default`'s `state_entry` next.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_reset_script(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    ctx.reset();
    Ok(())
}
