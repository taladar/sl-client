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

/// How far away Second Life's `llMoveToTarget` still moves towards a target:
/// one 70 m off did not move the prim, as the documented 65 m limit says
/// (`physics/targets`).
const MOVE_TO_TARGET_REACH: f32 = 65.0;

/// `llMoveToTarget`: the object is drawn to `target` with the time constant
/// `tau` while it is physical. A tau of 0 or less moves nothing, on every
/// grid; a target beyond 65 m moves nothing on Second Life and is gone to
/// all the same on OpenSim, 27 m in half a second towards one 70 m up
/// (`physics/targets`). On a prim that is not physical the target is kept and
/// nothing moves.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_move_to_target(
    ctx: &mut ScriptCtx<'_>,
    target: Vector,
    tau: f32,
) -> Result<(), CallError> {
    if tau.is_nan() || tau <= 0.0 {
        return Ok(());
    }
    if !ctx.flavour().is_opensim()
        && let Some(prim) = crate::library::prim_state::own(ctx)
    {
        let (dx, dy, dz) = (
            target.x - prim.position.x,
            target.y - prim.position.y,
            target.z - prim.position.z,
        );
        if (dx * dx + dy * dy + dz * dz).sqrt() > MOVE_TO_TARGET_REACH {
            return Ok(());
        }
    }
    let caller = ctx.caller();
    ctx.host()
        .move_to_target(caller, Some(crate::vm::prim::MoveTarget { target, tau }));
    Ok(())
}

/// `llStopMoveToTarget`: the object is no longer drawn to a target.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_move_to_target(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().move_to_target(caller, None);
    Ok(())
}
