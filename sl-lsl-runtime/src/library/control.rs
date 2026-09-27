//! Library tranche: the calls that are the VM's own control flow — a script
//! suspending or restarting itself, its timer, its start parameter, and
//! whether a script beside it is running. Each goes
//! through the [`ScriptCtx`]; a sleep or reset takes effect once the call has
//! returned.

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

/// `llSetTimerEvent`: a `timer` event every `seconds`, counted in ticks and
/// rounded up to a whole one; zero or less stops it. Setting it again
/// restarts the count. A timer that falls due while its last event still
/// waits in the queue does not queue a second one.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_timer_event(ctx: &mut ScriptCtx<'_>, seconds: f32) -> Result<(), CallError> {
    ctx.set_timer(seconds);
    Ok(())
}

/// `llGetStartParameter`: what the object was rezzed with — `0` for one
/// rezzed from an avatar's inventory or never rezzed at all.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_get_start_parameter(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(ctx.start_parameter())
}

/// `llGetScriptState`: whether the script called `name` in this prim is
/// running — `TRUE` for the caller itself, `FALSE` for a script stopped by
/// hand or by a run-time error, and `FALSE` for a name that is no script
/// here.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_script_state(ctx: &mut ScriptCtx<'_>, name: String) -> Result<i32, CallError> {
    Ok(i32::from(ctx.script_running(&name)))
}
