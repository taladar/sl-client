//! Library tranche: the calls that are the VM's own control flow — a script
//! suspending or restarting itself, its timer, its start parameter, and
//! whether a script beside it is running. Each goes
//! through the [`ScriptCtx`]; a sleep or reset takes effect once the call has
//! returned.

use crate::library::{CallError, Key};
use crate::vm::ScriptCtx;

/// `llSleep`: suspend the script for `seconds`, counted in ticks and rounded
/// up to a whole one — whole frames, as Second Life rounds a sleep
/// (`time/sleep-lengths`). Nothing else of the script runs meanwhile; its
/// events queue. A duration that is not positive does not suspend, but on
/// XEngine a negative one throws from `WaitHandle.WaitOne` and ends the event
/// (`time/sleep-negative`) — all but -1 ms, which .NET reads as forever and
/// this VM, which waits for no one forever, as no wait.
///
/// # Errors
///
/// [`CallError::Exception`] on XEngine for a negative duration.
pub fn ll_sleep(ctx: &mut ScriptCtx<'_>, seconds: f32) -> Result<(), CallError> {
    if ctx.flavour() == crate::vm::EngineFlavour::XEngine {
        let millis = (f64::from(seconds) * 1000.0).trunc();
        if millis < -1.0 {
            return Err(CallError::Exception {
                id: crate::library::BuiltinId::LlSleep,
                exception: crate::library::Exception::SleepTimeout,
            });
        }
    }
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

/// `llDie`: the object the script is in leaves the region, every prim of it
/// and every script in them. The script runs nothing after the call; the grid
/// removes the object once control is back with it. An attachment is left
/// where it is, and its script carries on, as on every grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_die(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().die(caller) {
        ctx.die();
    }
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

/// `llGetOwner`: the key of the object's owner, as the host knows it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_owner(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    let caller = ctx.caller();
    Ok(Key(ctx.host().owner(caller)))
}

/// `llGetScriptState`: whether the script called `name` in this prim is
/// running — `TRUE` for the caller itself, `FALSE` for a script stopped by
/// hand or held by a run-time error (on Second Life; YEngine reads a held
/// script as running), and `FALSE` for a name that is no script here.
///
/// A name that is no script is refused on `DEBUG_CHANNEL` in the flavour's
/// words and costs the script a second, on every grid (`errors/flood`: twelve
/// such calls took 12.2 seconds on aditi; OpenSim's `LSL_Api.Error` sleeps a
/// second).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_script_state(ctx: &mut ScriptCtx<'_>, name: String) -> Result<i32, CallError> {
    let Some(running) = ctx.script_running(&name) else {
        // Both grids charge a second; OpenSim's is every refusal's.
        if ctx.flavour().is_opensim() {
            ctx.refuse(format!("llGetScriptState: Can't find script '{name}'"));
        } else {
            ctx.refuse(format!("Could not find script '{name}'."));
            ctx.error_pause();
        }
        return Ok(0);
    };
    Ok(i32::from(running))
}

/// `llGetTime`: the script's time, in seconds — the region's ticks since the
/// script was reset or last called `llResetTime` or `llGetAndResetTime`,
/// counted whether it ran or not (a stopped script's time runs on, measured
/// on every grid). On aditi it moves only in whole simulator frames, as it
/// moves here in whole ticks.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_time(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(ctx.script_time())
}

/// `llResetTime`: the script's time starts again from zero.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_reset_time(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    ctx.reset_script_time();
    Ok(())
}

/// `llGetAndResetTime`: the script's time, which then starts again from zero.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_and_reset_time(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let time = ctx.script_time();
    ctx.reset_script_time();
    Ok(time)
}
