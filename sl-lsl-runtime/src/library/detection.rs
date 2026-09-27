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
    Ok(field(
        ctx,
        index,
        |d| Key(d.key.clone()),
        Key(NULL_KEY.to_owned()),
    ))
}

/// `llDetectedName`: its name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_name(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<String, CallError> {
    Ok(field(ctx, index, |d| d.name.clone(), NULL_KEY.to_owned()))
}

/// `llDetectedOwner`: its owner (an avatar owns itself).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_owner(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Key, CallError> {
    Ok(field(
        ctx,
        index,
        |d| Key(d.owner.clone()),
        Key(NULL_KEY.to_owned()),
    ))
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
    Ok(field(ctx, index, |d| d.rotation.clone(), ZERO_ROTATION))
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
    Ok(field(ctx, index, |d| d.touch.face, 0))
}

/// `llDetectedTouchST`: the surface coordinates touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_st(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.touch.st.clone(), ZERO_VECTOR))
}

/// `llDetectedTouchUV`: the texture coordinates touched.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detected_touch_uv(ctx: &mut ScriptCtx<'_>, index: i32) -> Result<Vector, CallError> {
    Ok(field(ctx, index, |d| d.touch.uv.clone(), ZERO_VECTOR))
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
