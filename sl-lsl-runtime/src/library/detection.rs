//! Library tranche: the detected block (`server-lsl-lib-detection-sensors`)
//! — `llDetected*` read the block of the event being handled, which the host
//! posted with it ([`Engine::post_detected`](crate::vm::Engine::post_detected)).
//!
//! An index past the block — or any index in an event with no block, since
//! such an event clears it — reads each type's zero value, measured on aditi
//! (2026-09-28): `NULL_KEY` for the keys, `0`, `ZERO_VECTOR`,
//! `ZERO_ROTATION` — except `llDetectedName`, which answers the `NULL_KEY`
//! *string*. The touch fields are zero as well, not the `TOUCH_INVALID_*`
//! markers: those are for a real touch the viewer sent no surface for.
//!
//! OpenSim answers its own past the block (`LSL_Api.llDetected*`,
//! `events/detected-cleared`): an empty name, key and owner, a touch face of
//! −1, touch coordinates of `<-1, -1, 0>`, and a rotation of four zeros (its
//! struct's default, not its constructor's identity).

use sl_types::lsl::{Rotation, Vector};

use crate::library::{CallError, Key};
use crate::value::{NULL_KEY, ZERO_ROTATION, ZERO_VECTOR};
use crate::vm::{Detected, ScriptCtx};

/// The field `read` of detection `index`, or `absent` past the block.
fn field<T>(ctx: &ScriptCtx<'_>, index: i32, read: impl FnOnce(&Detected) -> T, absent: T) -> T {
    ctx.detected(index).map_or(absent, read)
}

/// `llDetectedKey`: the detected avatar's or object's key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_key(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Key, CallError> {
    // Past the end: `NULL_KEY` on Second Life, an empty key on OpenSim
    // (`detection/sensor`).
    let none = absent_key(ctx);
    Ok(field(ctx, index, |d| Key(d.key.clone()), Key(none)))
}

/// `llDetectedName`: its name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_name(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<String, CallError> {
    let none = absent_key(ctx);
    Ok(field(ctx, index, |d| d.name.clone(), none))
}

/// What a key or a name reads past the block: `NULL_KEY` on Second Life,
/// empty on OpenSim.
fn absent_key(ctx: &ScriptCtx<'_>) -> String {
    if ctx.flavour().is_opensim() {
        String::new()
    } else {
        NULL_KEY.to_owned()
    }
}

/// What a touch's surface or texture coordinates read past the block: zero
/// on Second Life, `<-1, -1, 0>` on OpenSim.
const fn absent_touch_point(ctx: &ScriptCtx<'_>) -> Vector {
    if ctx.flavour().is_opensim() {
        Vector {
            x: -1.0,
            y: -1.0,
            z: 0.0,
        }
    } else {
        ZERO_VECTOR
    }
}

/// `llDetectedOwner`: its owner (an avatar owns itself).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_owner(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Key, CallError> {
    let none = absent_key(ctx);
    Ok(field(ctx, index, |d| Key(d.owner.clone()), Key(none)))
}

/// `llDetectedType`: its `AGENT` / `ACTIVE` / `PASSIVE` / `SCRIPTED` bits.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_type(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<i32, CallError> {
    Ok(field(ctx, index, |d| d.kind, 0))
}

/// `llDetectedGroup`: whether it shares the object's active group.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_group(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<i32, CallError> {
    Ok(field(ctx, index, |d| i32::from(d.same_group), 0))
}

/// `llDetectedLinkNumber`: the link of this object it touched or hit.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_link_number(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<i32, CallError> {
    Ok(field(ctx, index, |d| d.link_number, 0))
}

/// `llDetectedPos`: its region position.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_pos(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.position.clone(), ZERO_VECTOR))
}

/// `llDetectedRot`: its rotation.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_rot(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Rotation, CallError> {
    let none = if ctx.flavour().is_opensim() {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 0.0,
        }
    } else {
        ZERO_ROTATION
    };
    Ok(field(ctx, index, |d| d.rotation.clone(), none))
}

/// `llDetectedVel`: its velocity.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_vel(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.velocity.clone(), ZERO_VECTOR))
}

/// `llDetectedGrab`: the grab offset of a `touch`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_grab(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.grab.clone(), ZERO_VECTOR))
}

/// `llDetectedTouchFace`: the face touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_face(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<i32, CallError> {
    let none = if ctx.flavour().is_opensim() { -1 } else { 0 };
    Ok(field(ctx, index, |d| d.touch.face, none))
}

/// `llDetectedTouchST`: the surface coordinates touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_st(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    let none = absent_touch_point(ctx);
    Ok(field(ctx, index, |d| d.touch.st.clone(), none))
}

/// `llDetectedTouchUV`: the texture coordinates touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_uv(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    let none = absent_touch_point(ctx);
    Ok(field(ctx, index, |d| d.touch.uv.clone(), none))
}

/// `llDetectedTouchPos`: the region position touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_pos(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.touch.position.clone(), ZERO_VECTOR))
}

/// `llDetectedTouchNormal`: the surface normal touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_normal(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.touch.normal.clone(), ZERO_VECTOR))
}

/// `llDetectedTouchBinormal`: the surface binormal touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_binormal(
    ctx: &mut ScriptCtx<'_>,
    index: i32,
) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.touch.binormal.clone(), ZERO_VECTOR))
}

/// `llPassTouches`: whether a touch on the prim the script is in also reaches
/// its linkset's root — `PASS_IF_NOT_HANDLED` (0), `PASS_ALWAYS` (1) or
/// `PASS_NEVER` (2). The setting is the prim's, so a reset leaves it, and it
/// is handed to the host as the script gave it: what a value means is the
/// grid's (OpenSim passes for any value but 0, `PASS_NEVER` included;
/// `object-touch-grab`, 2026-10-12).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_pass_touches(ctx: &mut ScriptCtx<'_>, pass: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().pass_touches(caller, pass);
    Ok(())
}
