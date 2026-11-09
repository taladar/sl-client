//! Library tranche: sitting, the link numbers sitters take, and attachments
//! (`server-world-sit-and-attach`) — what a script sets of its prim as a
//! seat, who it finds sitting there, how many links the object has once
//! somebody sits on it, wearing the object and taking it off, and where it
//! stands — and `llSetPos`, which moves it, ahead of the rest of the
//! prim-state tranche (`server-lsl-lib-prim-state`) because the update
//! scheduler needed a script that moves a prim.
//!
//! Who sits where, and what is worn by whom, is the region's: the host
//! answers it. The rules here are the grids' as measured by the `avatar/sit`,
//! `events/attach`, `prim/identity` and `prim/placement` probes (book chapter
//! `gridspec/lsl.md`) and, where nothing measured them, `LSL_Api.cs`'s.

use sl_types::lsl::{Rotation, Vector};

use crate::library::experiences::single_prim;
use crate::library::{CallError, Key};
use crate::value::NULL_KEY;
use crate::vm::ScriptCtx;
use crate::vm::permissions::PERMISSION_ATTACH;
use crate::vm::seating::{Seat, SitTarget};

/// `LINK_ROOT`.
const LINK_ROOT: i32 = 1;
/// `LINK_SET`.
const LINK_SET: i32 = -1;
/// `LINK_ALL_OTHERS`.
const LINK_ALL_OTHERS: i32 = -2;
/// `LINK_ALL_CHILDREN`.
const LINK_ALL_CHILDREN: i32 = -3;

/// How many characters of a pie menu's sit or touch text a prim keeps
/// (`LSL_Api.llSetSitText`; unmeasured on Second Life, whose pie shows no
/// more).
const PIE_TEXT_CHARACTERS: usize = 9;

/// Whether `vector` is zero in each of its components.
fn is_zero(vector: &Vector) -> bool {
    vector.x == 0.0 && vector.y == 0.0 && vector.z == 0.0
}

/// The sit target `llSitTarget(offset, rotation)` sets, or [`None`] for one
/// that clears it. Second Life clears it for a zero offset whatever the turn
/// (the wiki; `avatar/sit` cleared it with both zero); OpenSim keeps a target
/// whose offset is zero but whose turn is not the identity
/// (`SceneObjectPart.IsSitTargetSet`), and reads `<0, 0, 0, 0>` as the
/// identity.
fn target_of(ctx: &ScriptCtx<'_>, offset: Vector, rotation: Rotation) -> Option<SitTarget> {
    if ctx.flavour().is_opensim() {
        let rotation = if rotation.x == 0.0 && rotation.y == 0.0 && rotation.z == 0.0 {
            Rotation {
                s: if rotation.s == 0.0 { 1.0 } else { rotation.s },
                ..rotation
            }
        } else {
            rotation
        };
        let identity = rotation.x == 0.0
            && rotation.y == 0.0
            && rotation.z == 0.0
            && (rotation.s.abs() - 1.0).abs() <= f32::EPSILON;
        (!is_zero(&offset) || !identity).then_some(SitTarget { offset, rotation })
    } else {
        (!is_zero(&offset)).then_some(SitTarget { offset, rotation })
    }
}

/// The seat settings of the prim `link` (a number, never a constant) of the
/// caller's linkset.
fn seat_of<'ctx>(ctx: &'ctx mut ScriptCtx<'_>, link: i32) -> Option<&'ctx mut Seat> {
    let caller = ctx.caller();
    ctx.host().seat(caller, link)
}

/// The caller's own link number.
fn this_link(ctx: &mut ScriptCtx<'_>) -> i32 {
    let caller = ctx.caller();
    ctx.host().links(caller).this
}

/// `llSitTarget`: the caller's prim seats an avatar at `offset`, turned by
/// `rotation`; a zero offset clears the target (on OpenSim only with the
/// identity turn). An avatar already seated stays where it is.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sit_target(
    ctx: &mut ScriptCtx<'_>,
    offset: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    let target = target_of(ctx, offset, rotation);
    let link = this_link(ctx);
    if let Some(seat) = seat_of(ctx, link) {
        seat.target = target;
    }
    Ok(())
}

/// `llLinkSitTarget`: [`ll_sit_target`] for the prim `link` — `LINK_ROOT`,
/// `LINK_THIS` or a link number; a constant naming several prims, and a
/// number naming none, set nothing (`LSL_Api.llLinkSitTarget`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_sit_target(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    offset: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    let target = target_of(ctx, offset, rotation);
    let Some(link) = single_prim(ctx, link) else {
        return Ok(());
    };
    if let Some(seat) = seat_of(ctx, link) {
        seat.target = target;
    }
    Ok(())
}

/// `llAvatarOnSitTarget`: the avatar on the caller's prim's sit target, or
/// `NULL_KEY` — for a prim without a target, and for an avatar seated on the
/// prim beside the one on its target (`avatar/sit`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_avatar_on_sit_target(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    let link = this_link(ctx);
    let caller = ctx.caller();
    Ok(Key(ctx
        .host()
        .avatar_on_sit_target(caller, link)
        .unwrap_or_else(|| NULL_KEY.to_owned())))
}

/// `llAvatarOnLinkSitTarget`: the avatar on the sit target of the prim
/// `link`; `NULL_KEY` for 0 and for the constants naming several prims, as
/// `LSL_Api.llAvatarOnLinkSitTarget` answers them (unmeasured).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_avatar_on_link_sit_target(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<Key, CallError> {
    let null = || Key(NULL_KEY.to_owned());
    if matches!(link, 0 | LINK_SET | LINK_ALL_OTHERS | LINK_ALL_CHILDREN) {
        return Ok(null());
    }
    let Some(link) = single_prim(ctx, link) else {
        return Ok(null());
    };
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .avatar_on_sit_target(caller, link)
        .map_or_else(null, Key))
}

/// `llGetNumberOfPrims`: the prims of the caller's object and the avatars
/// sitting on it (`avatar/sit`: 2 for a lone prim with its sitter).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_number_of_prims(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let count = links.prims.len().saturating_add(links.sitters.len());
    Ok(i32::try_from(count).unwrap_or(i32::MAX))
}

/// `llGetLinkNumber`: the caller's prim's link number — 0 for a prim linked
/// to nothing and with nobody sitting on it (`prim/identity`), 1 for a root.
/// OpenSim counts prims alone, so a lone prim is 0 with a sitter too
/// (`LSL_Api.llGetLinkNumber`, `prim/link-refusals`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_number(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    if ctx.flavour().is_opensim() {
        let caller = ctx.caller();
        if ctx.host().links(caller).prims.len() < 2 {
            return Ok(0);
        }
    }
    Ok(this_link(ctx))
}

/// `llGetLinkKey`: the key of the prim or the sitter numbered `link`.
///
/// `LINK_THIS` is the caller's prim on both grids, and the numbers past the
/// last prim's are the sitters', in the order they sat (`avatar/sit`: the
/// last link is the sitter). Where the grids part (`prim/identity`,
/// `prim/link`): on Second Life a prim linked to nothing is link 0 and has no
/// link 1, and in a linkset 0 is `NULL_KEY` to the root and the root to a
/// child (`prim/link`'s `linked` and `as-child`); OpenSim answers the root
/// for any number under 2. Any other constant is `NULL_KEY`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_key(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let null = || NULL_KEY.to_owned();
    let root = || {
        links
            .prims
            .first()
            .map_or_else(null, |root| root.key.clone())
    };
    let opensim_root = ctx.flavour().is_opensim() && (0..2).contains(&link);
    let from_a_child = link == 0 && links.prims.len() > 1 && links.this > 1;
    let key = if opensim_root || from_a_child {
        root()
    } else {
        links
            .numbered(link)
            .map_or_else(null, |linked| linked.key().to_owned())
    };
    Ok(Key(key))
}

/// `llGetLinkName`: the name of the prim or the sitter numbered `link`,
/// resolved as [`ll_get_link_key`] resolves it on Second Life — and on
/// OpenSim too, which answers a lone prim's link 1 with `NULL_KEY` here
/// though its `llGetLinkKey(1)` is the prim (`prim/identity`). What names
/// nothing answers `NULL_KEY` as text, on both grids.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_name(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<String, CallError> {
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .links(caller)
        .numbered(link)
        .map_or_else(|| NULL_KEY.to_owned(), |linked| linked.name().to_owned()))
}

/// `llGetObjectName`: the name of the prim the script is in — the prim's,
/// not its linkset root's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .links(caller)
        .this_prim()
        .map(|prim| prim.name.clone())
        .unwrap_or_default())
}

/// `llGetObjectPrimCount`: how many prims the object holding the prim
/// `object` has, its sitters left out (`avatar/sit`: 1 with an avatar on
/// it); 0 for a key no prim in the region has.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_prim_count(ctx: &mut ScriptCtx<'_>, object: Key) -> Result<i32, CallError> {
    Ok(ctx.host().prim_count(&object.0).unwrap_or(0))
}

/// `llUnSit`: stand `agent` up, when it sits on the object or the object's
/// owner may move it off the land it sits over (the host's rule).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_un_sit(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().unsit(caller, &agent.0);
    Ok(())
}

/// `text` cut to what a pie menu keeps of it.
fn pie_text(text: &str) -> String {
    text.chars().take(PIE_TEXT_CHARACTERS).collect()
}

/// `llSetSitText`: what the pie menu says for sitting on the prim, cut to
/// nine characters.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_sit_text(ctx: &mut ScriptCtx<'_>, text: String) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().set_pie_text(caller, true, &pie_text(&text));
    Ok(())
}

/// `llSetTouchText`: what the pie menu says for touching the prim, cut to
/// nine characters.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_touch_text(ctx: &mut ScriptCtx<'_>, text: String) -> Result<(), CallError> {
    let caller = ctx.caller();
    ctx.host().set_pie_text(caller, false, &pie_text(&text));
    Ok(())
}

/// Sets one of the prim's sitter camera offsets with `set`, and the root's
/// too where the root has none yet (`LSL_Api.llSetCameraEyeOffset`).
fn set_camera(
    ctx: &mut ScriptCtx<'_>,
    offset: &Vector,
    read: fn(&Seat) -> &Vector,
    set: fn(&mut Seat, Vector),
) {
    let link = this_link(ctx);
    if let Some(seat) = seat_of(ctx, link) {
        set(seat, offset.clone());
    }
    let root = if link == 0 { 0 } else { LINK_ROOT };
    if root != link
        && let Some(seat) = seat_of(ctx, root)
        && is_zero(read(seat))
    {
        set(seat, offset.clone());
    }
}

/// `llSetCameraEyeOffset`: where the camera of an avatar sitting on the prim
/// is, from the prim; the sit response carries it (`avatar/sit`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_camera_eye_offset(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<(), CallError> {
    set_camera(
        ctx,
        &offset,
        |seat| &seat.camera_eye,
        |seat, offset| seat.camera_eye = offset,
    );
    Ok(())
}

/// `llSetCameraAtOffset`: where that camera looks, from the prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_camera_at_offset(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<(), CallError> {
    set_camera(
        ctx,
        &offset,
        |seat| &seat.camera_at,
        |seat, offset| seat.camera_at = offset,
    );
    Ok(())
}

/// `llForceMouselook`: whether an avatar sitting on the prim is put in
/// mouselook.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_force_mouselook(ctx: &mut ScriptCtx<'_>, mouselook: i32) -> Result<(), CallError> {
    let link = this_link(ctx);
    if let Some(seat) = seat_of(ctx, link) {
        seat.force_mouselook = mouselook != 0;
    }
    Ok(())
}

/// `llGetAttached`: the attachment point the object is worn on, 0 when it is
/// not worn (`events/attach`: 5 on the left hand).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_attached(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().attached(caller))
}

/// Whether the script holds `PERMISSION_ATTACH`, granted by the object's
/// owner — what both attaching and detaching ask (`LSL_Api.llAttachToAvatar`;
/// `events/attach` attached once granted).
fn may_attach(ctx: &mut ScriptCtx<'_>) -> bool {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let permissions = ctx.permissions();
    permissions.holds(PERMISSION_ATTACH)
        && permissions
            .granter()
            .is_some_and(|granter| granter.eq_ignore_ascii_case(&owner))
}

/// `llAttachToAvatar`: wear the object on its owner's `point`, once the
/// owner granted `PERMISSION_ATTACH`; the object keeps its key, and the
/// region raises `attach` with the owner's key (`events/attach`). An object
/// already worn, or a script without the grant, does nothing (OpenSim's
/// code; Second Life's complaint, if it has one, is unmeasured).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_attach_to_avatar(ctx: &mut ScriptCtx<'_>, point: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 || !may_attach(ctx) {
        return Ok(());
    }
    ctx.host().attach(caller, point);
    Ok(())
}

/// `llAttachToAvatarTemp`: wear the object on its owner's `point` as
/// [`ll_attach_to_avatar`] does, as a temporary attachment: no item is made,
/// its `AttachItemID` is its own id, and a detach takes it out of the world
/// (`attachments`, 2026-10-20).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_attach_to_avatar_temp(ctx: &mut ScriptCtx<'_>, point: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 || !may_attach(ctx) {
        return Ok(());
    }
    ctx.host().attach_temp(caller, point);
    Ok(())
}

/// `llDetachFromAvatar`: take the worn object off into its owner's
/// inventory, with `PERMISSION_ATTACH` granted by the owner; nothing for an
/// object not worn.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_detach_from_avatar(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) == 0 || !may_attach(ctx) {
        return Ok(());
    }
    ctx.host().detach(caller);
    Ok(())
}

/// `llGetPos`: the prim's region position — a worn prim's is its wearer's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_pos(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().placement(caller).position)
}

/// How far one `llSetPos` moves a prim, in metres: a target farther away is
/// reached this far towards it, in one call (`prim/placement`).
const SET_POS_REACH_M: f32 = 10.0;

/// `to`, or the point [`SET_POS_REACH_M`] from `from` towards it where it is
/// farther (`LSL_Api.SetPosAdjust`).
fn within_reach(from: &Vector, to: Vector) -> Vector {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    let distance = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
    if distance <= SET_POS_REACH_M {
        return to;
    }
    let share = SET_POS_REACH_M / distance;
    Vector {
        x: dx.mul_add(share, from.x),
        y: dy.mul_add(share, from.y),
        z: dz.mul_add(share, from.z),
    }
}

/// `llSetPos`: moves the prim to `target`, its position from its parent — a
/// root's region position, a worn root's offset on its point, a child's
/// offset from its root (`prim/placement`, `prim/link`).
///
/// A move farther than 10 m goes 10 m towards the target, on both grids for
/// a root. A child's offset is not capped on Second Life and is capped 10 m
/// from where the child was on OpenSim. The host keeps a root out of the
/// ground. The 0.2 s the call costs is the library table's forced delay.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_pos(ctx: &mut ScriptCtx<'_>, target: Vector) -> Result<(), CallError> {
    let caller = ctx.caller();
    let child = ctx.host().links(caller).this >= 2;
    let to = if child && !ctx.flavour().is_opensim() {
        target
    } else {
        let here = ctx.host().placement(caller).local;
        within_reach(&here, target)
    };
    ctx.host().set_position(caller, to);
    Ok(())
}

/// `llGetLocalPos`: the prim's position from its parent — a root's region
/// position, a worn root's offset on its attachment point, a child's offset
/// from its root.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_local_pos(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().placement(caller).local)
}

/// `llGetRootPosition`: the region position of the object's root — a worn
/// object's is its wearer's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_root_position(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().placement(caller).root)
}
