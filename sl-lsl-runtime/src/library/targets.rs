//! Position and rotation targets: `llTarget`, `llRotTarget` and their
//! removals. The engine checks a script's targets on its own beat and raises
//! `at_target` / `not_at_target` and `at_rot_target` / `not_at_rot_target`
//! (`physics/targets`); none of it needs physics, since a prim a script moves
//! reaches a target as a physical one does.

use sl_types::lsl::{Rotation, Vector};

use crate::library::CallError;
use crate::vm::ScriptCtx;

/// `llTarget`: a target `range` metres round `position`, its handle — one
/// past the script's last, from 1, shared with rotation targets. A script
/// keeps the newest eight.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_target(ctx: &mut ScriptCtx<'_>, position: Vector, range: f32) -> Result<i32, CallError> {
    Ok(ctx.add_position_target(position, range))
}

/// `llTargetRemove`: drops the position target `handle`; any other number
/// does nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_target_remove(ctx: &mut ScriptCtx<'_>, handle: i32) -> Result<(), CallError> {
    ctx.remove_position_target(handle);
    Ok(())
}

/// `llRotTarget`: a target `error` radians round `rotation`, its handle, from
/// the counter position targets take theirs from. A script keeps the newest
/// eight.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot_target(
    ctx: &mut ScriptCtx<'_>,
    rotation: Rotation,
    error: f32,
) -> Result<i32, CallError> {
    Ok(ctx.add_rotation_target(rotation, error))
}

/// `llRotTargetRemove`: drops the rotation target `handle`; any other number
/// does nothing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot_target_remove(ctx: &mut ScriptCtx<'_>, handle: i32) -> Result<(), CallError> {
    ctx.remove_rotation_target(handle);
    Ok(())
}
