//! Library tranche: a script's own memory — what it uses, what is left, its
//! limit and its profile (`server-lsl-memory-and-limits`). Each answers from
//! the instance's accounting through the [`ScriptCtx`], in the words of the
//! grid the region imitates (book chapter `gridspec/lsl.md`, § The memory
//! functions).

use crate::library::CallError;
use crate::vm::ScriptCtx;

/// A count of bytes as an LSL integer.
fn integer(bytes: u32) -> i32 {
    i32::try_from(bytes).unwrap_or(i32::MAX)
}

/// `llGetUsedMemory`: what the script uses — on Second Life its code and what
/// its globals hold, on YEngine its heap; `65536` on XEngine.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_used_memory(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(integer(ctx.used_memory()))
}

/// `llGetFreeMemory`: on Second Life the limit less the most the script has
/// used since the limit was set, a low-water mark; on YEngine the 1 MiB heap
/// less what it uses now; `65536` on XEngine.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_free_memory(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(integer(ctx.free_memory()))
}

/// `llGetMemoryLimit`: `65536`, or what `llSetMemoryLimit` set.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_get_memory_limit(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(ctx.memory_limit().cast_signed())
}

/// `llSetMemoryLimit`: `TRUE` and the limit is `limit` when it is from what
/// the script uses up to `65536`, on Second Life; `FALSE` and nothing changes
/// otherwise, and always on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_memory_limit(ctx: &mut ScriptCtx<'_>, limit: i32) -> Result<i32, CallError> {
    Ok(i32::from(ctx.set_memory_limit(limit)))
}

/// `llScriptProfiler`: `PROFILE_SCRIPT_MEMORY` starts recording the most the
/// script uses, `PROFILE_NONE` stops; nothing on OpenSim. The reference also
/// runs a profiled script slower, which is not modelled.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_script_profiler(ctx: &mut ScriptCtx<'_>, flags: i32) -> Result<(), CallError> {
    ctx.profile(flags);
    Ok(())
}

/// `llGetSPMaxMemory`: the most the script used while profiled, `0` before
/// any profiling, on Second Life; `65536` on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_sp_max_memory(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(integer(ctx.profiled_memory()))
}
