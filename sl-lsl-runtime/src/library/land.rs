//! Library tranche: the land, the region and the estate
//! (`server-lsl-lib-land-region-env`) — parcel queries and the parcel's
//! access lists, the region and its neighbours, `llGetEnv`, the ground,
//! water, wind and clouds, the day cycle with the sun and the moon, and
//! Second Life's `llGetSimStats`, `llGetEnvironment` and nav points.
//!
//! The host describes the land ([`Parcel`], [`crate::vm::land::Region`],
//! [`crate::vm::land::Environment`]);
//! every rule a grid applies to it is here, measured by the `land/*` probes
//! (`book/src/gridspec/lsl.md` § Parcel, region, estate and environment
//! queries).

use sl_types::lsl::{Rotation, Vector};

use crate::library::{BuiltinId, CallError, Exception, Key};
use crate::num::to_f32;
use crate::value::{Element, NULL_KEY, ZERO_VECTOR};
use crate::vm::ScriptCtx;
use crate::vm::land::{LandChange, Parcel};

/// `PARCEL_FLAG_ALLOW_DAMAGE`.
const PARCEL_FLAG_ALLOW_DAMAGE: u32 = 0x20;

/// `AGENT_LIST_PARCEL`.
const AGENT_LIST_PARCEL: i32 = 1;
/// `AGENT_LIST_PARCEL_OWNER`.
const AGENT_LIST_PARCEL_OWNER: i32 = 2;
/// `AGENT_LIST_REGION`.
const AGENT_LIST_REGION: i32 = 4;

/// The `REGION_FLAG_*` bits Second Life hands a script of its region flags.
const REGION_FLAGS: u32 =
    0x1 | 0x10 | 0x40 | 0x100 | 0x1000 | 0x4000 | 0x8_0000 | 0x10_0000 | 0x40_0000 | 0x800_0000;

/// The caller's prim's position.
fn here(ctx: &mut ScriptCtx<'_>) -> Vector {
    let caller = ctx.caller();
    ctx.host().placement(caller).position
}

/// The region's size, in metres, as floats.
fn size(ctx: &mut ScriptCtx<'_>) -> (f32, f32) {
    let caller = ctx.caller();
    let (x, y) = ctx.host().region_size(caller);
    let metres = |value: u32| u16::try_from(value).map_or(256.0, f32::from);
    (metres(x), metres(y))
}

/// Whether `at` lies outside the region, its height aside.
fn outside(ctx: &mut ScriptCtx<'_>, at: &Vector) -> bool {
    let (x, y) = size(ctx);
    at.x < 0.0 || at.y < 0.0 || at.x >= x || at.y >= y
}

/// `at` held inside the region: what Second Life reads a position off it as.
fn clamped(ctx: &mut ScriptCtx<'_>, at: &Vector) -> Vector {
    let (x, y) = size(ctx);
    Vector {
        x: at.x.clamp(0.0, (x - 0.01).max(0.0)),
        y: at.y.clamp(0.0, (y - 0.01).max(0.0)),
        z: at.z,
    }
}

/// The parcel `at` names as the flavour reads it: Second Life the parcel at
/// the edge a position off the region is clamped to, OpenSim none there.
fn parcel_of(ctx: &mut ScriptCtx<'_>, at: &Vector) -> Option<Parcel> {
    let at = if ctx.flavour().is_opensim() {
        if outside(ctx, at) {
            return None;
        }
        at.clone()
    } else {
        clamped(ctx, at)
    };
    let caller = ctx.caller();
    ctx.host().parcel_at(caller, at.x, at.y)
}

/// Whether the caller's owner owns `parcel`.
fn owns(ctx: &mut ScriptCtx<'_>, parcel: &Parcel) -> bool {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    parcel.owner.eq_ignore_ascii_case(&owner)
}

/// An integer as `llGetEnv` writes it, or a float with its fraction.
fn number(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        crate::num::float_to_int(value).to_string()
    } else {
        format!("{value:.6}")
    }
}

/// `llGetParcelFlags`: the parcel's flags. Off the region Second Life reads
/// the edge parcel's; OpenSim throws (`NullReferenceException`), which stops
/// the script.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a position off the region.
pub fn ll_get_parcel_flags(ctx: &mut ScriptCtx<'_>, at: Vector) -> Result<i32, CallError> {
    match parcel_of(ctx, &at) {
        Some(parcel) => Ok(parcel.flags.cast_signed()),
        None if ctx.flavour().is_opensim() => Err(CallError::Exception {
            id: BuiltinId::LlGetParcelFlags,
            exception: Exception::NullReference,
        }),
        None => Ok(0),
    }
}

/// `llGetParcelDetails`: one element per code, in the order asked. The codes
/// past 6 are each grid's own (Second Life: prim capacity, prims used,
/// landing point and look-at, teleport routing, flags, script danger;
/// OpenSim: the two sound flags, the claim date and 64 the dwell, every
/// other code 0). An unknown code is left out on Second Life with a
/// complaint; off the region OpenSim answers one 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_parcel_details(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    codes: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let opensim = ctx.flavour().is_opensim();
    let Some(parcel) = parcel_of(ctx, &at) else {
        return Ok(vec![Element::Integer(0)]);
    };
    let danger = i32::from(script_danger(ctx, &parcel));
    let mut out = Vec::new();
    for code in &codes {
        let code = match code {
            Element::Integer(code) => *code,
            _ => -1,
        };
        let element = match code {
            0 => Element::String(parcel.name.clone()),
            1 => Element::String(parcel.description.clone()),
            2 => Element::Key(parcel.owner.clone()),
            3 => Element::Key(parcel.group.clone()),
            4 => Element::Integer(parcel.area),
            5 => Element::Key(parcel.id.clone()),
            6 => Element::Integer(i32::from(parcel.see_avatars)),
            7 if opensim => Element::Integer(i32::from(parcel.sounds.0)),
            8 if opensim => Element::Integer(i32::from(parcel.sounds.1)),
            10 if opensim => Element::Integer(parcel.claim_date),
            64 if opensim => Element::Integer(crate::num::float_to_int(parcel.dwell)),
            _ if opensim => Element::Integer(0),
            7 => Element::Integer(parcel.capacity.0),
            8 => Element::Integer(parcel.capacity.1),
            9 => Element::Vector(parcel.landing.0.clone()),
            10 => Element::Vector(parcel.landing.1.clone()),
            11 => Element::Integer(parcel.tp_routing),
            12 => Element::Integer(parcel.flags.cast_signed()),
            13 => Element::Integer(danger),
            other => {
                // Second Life stops at the first code it does not know
                // (`5, 99, 2, 2` answered the id alone).
                ctx.refuse(format!(
                    "llGetParcelDetails error: Unknown parameter '{other}'"
                ));
                break;
            }
        };
        out.push(element);
    }
    Ok(out)
}

/// `llGetParcelMaxPrims`: the prims the parcel (or with `sim_wide` the
/// region) may hold; 0 off the region on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_parcel_max_prims(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    sim_wide: i32,
) -> Result<i32, CallError> {
    Ok(parcel_of(ctx, &at).map_or(0, |parcel| {
        if sim_wide != 0 {
            parcel.max_prims.1
        } else {
            parcel.max_prims.0
        }
    }))
}

/// `llGetParcelPrimCount`: the prims of a `PARCEL_COUNT_*` category on the
/// parcel or (`sim_wide`) the region. OpenSim counts no selected or
/// temporary prims, and region-wide no owner's or other's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_parcel_prim_count(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    category: i32,
    sim_wide: i32,
) -> Result<i32, CallError> {
    let opensim = ctx.flavour().is_opensim();
    let Some(parcel) = parcel_of(ctx, &at) else {
        return Ok(0);
    };
    let counts = if sim_wide != 0 {
        parcel.counts.1
    } else {
        parcel.counts.0
    };
    Ok(match category {
        0 => counts.total,
        1 | 3 if opensim && sim_wide != 0 => 0,
        1 => counts.owner,
        2 => counts.group,
        3 => counts.other,
        4 if !opensim => counts.selected,
        5 if !opensim => counts.temporary,
        _ => 0,
    })
}

/// `llGetParcelPrimOwners`: each owner of prims on the parcel and its count —
/// on Second Life only for land the script's owner owns (`[]` otherwise),
/// on OpenSim always.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_parcel_prim_owners(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
) -> Result<Vec<Element>, CallError> {
    let Some(parcel) = parcel_of(ctx, &at) else {
        return Ok(Vec::new());
    };
    if !ctx.flavour().is_opensim() && !owns(ctx, &parcel) {
        return Ok(Vec::new());
    }
    Ok(parcel
        .prim_owners
        .into_iter()
        .flat_map(|(owner, count)| [Element::Key(owner), Element::Integer(count)])
        .collect())
}

/// `llGetLandOwnerAt`: the parcel's owner (its group for group land);
/// `NULL_KEY` off the region on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_land_owner_at(ctx: &mut ScriptCtx<'_>, at: Vector) -> Result<Key, CallError> {
    Ok(Key(parcel_of(ctx, &at).map_or_else(
        || NULL_KEY.to_owned(),
        |parcel| parcel.owner,
    )))
}

/// The avatar keyed `agent` in the caller's region and where it stands.
fn avatar(ctx: &mut ScriptCtx<'_>, agent: &str) -> Option<Vector> {
    let caller = ctx.caller();
    ctx.host()
        .sense(caller)
        .into_iter()
        .find(|sensed| sensed.agent && sensed.key.eq_ignore_ascii_case(agent))
        .map(|sensed| sensed.position)
}

/// `llOverMyLand`: whether the avatar stands on land the script's owner
/// owns.
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(
    clippy::module_name_repetitions,
    reason = "the library function is named after its LSL call"
)]
pub fn ll_over_my_land(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<i32, CallError> {
    let Some(at) = avatar(ctx, &agent.0) else {
        return Ok(0);
    };
    let caller = ctx.caller();
    let parcel = ctx.host().parcel_at(caller, at.x, at.y);
    Ok(i32::from(parcel.is_some_and(|parcel| owns(ctx, &parcel))))
}

/// Whether a script would be in danger on `parcel` (`llScriptDanger`): land
/// that does not let everybody build and enter, or runs no outside scripts —
/// Second Life also over land nobody owns, OpenSim's `LSLScriptDanger` letting
/// a script of the parcel's group run where group scripts may.
pub(crate) fn script_danger(ctx: &mut ScriptCtx<'_>, parcel: &Parcel) -> bool {
    use crate::vm::contents::{
        PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY, PARCEL_FLAG_ALLOW_CREATE_OBJECTS,
        PARCEL_FLAG_ALLOW_GROUP_SCRIPTS, PARCEL_FLAG_ALLOW_SCRIPTS,
    };
    let entry = PARCEL_FLAG_ALLOW_CREATE_OBJECTS | PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY;
    if parcel.flags & entry != entry {
        return true;
    }
    let opensim = ctx.flavour().is_opensim();
    // Second Life also counts land that allows damage (`land/parcel`'s west
    // edge parcel, which lets everybody build, enter and run scripts).
    if !opensim && parcel.flags & PARCEL_FLAG_ALLOW_DAMAGE != 0 {
        return true;
    }
    if parcel.flags & PARCEL_FLAG_ALLOW_SCRIPTS != 0 {
        return !opensim && parcel.owner == NULL_KEY;
    }
    if opensim && parcel.flags & PARCEL_FLAG_ALLOW_GROUP_SCRIPTS != 0 {
        let caller = ctx.caller();
        let object = ctx.host().whereabouts(caller).object;
        let group = ctx
            .host()
            .object_details(caller, &object)
            .map(|details| details.group);
        if group.as_deref() == Some(parcel.group.as_str()) {
            return false;
        }
    }
    true
}

/// `llScriptDanger`: whether a script of the caller's object would be in
/// danger at `at`; off the region always, on both grids.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_script_danger(ctx: &mut ScriptCtx<'_>, at: Vector) -> Result<i32, CallError> {
    if outside(ctx, &at) {
        return Ok(1);
    }
    let caller = ctx.caller();
    let Some(parcel) = ctx.host().parcel_at(caller, at.x, at.y) else {
        return Ok(1);
    };
    Ok(i32::from(script_danger(ctx, &parcel)))
}

/// `llParcelMediaQuery`: the parcel's media settings asked for — `[]` on
/// land the script's owner does not own, on both grids, which OpenSim
/// returns before its sleep; on its own land the URL, texture, type, size,
/// description and loop each code asks for.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_parcel_media_query(
    ctx: &mut ScriptCtx<'_>,
    codes: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let at = here(ctx);
    if !owns_here(ctx, &at) {
        return Ok(Vec::new());
    }
    let caller = ctx.caller();
    let media = ctx.host().parcel_media(caller, &at);
    let mut out = Vec::new();
    for code in codes {
        match code {
            Element::Integer(5) => out.push(Element::String(media.url.clone())),
            Element::Integer(4) => out.push(Element::Key(media.texture.clone())),
            Element::Integer(10) => out.push(Element::String(media.mime.clone())),
            Element::Integer(11) => {
                out.push(Element::Integer(media.size.0));
                out.push(Element::Integer(media.size.1));
            }
            Element::Integer(12) => out.push(Element::String(media.description.clone())),
            Element::Integer(13) => out.push(Element::Integer(i32::from(media.looping))),
            _ => return Ok(Vec::new()),
        }
    }
    Ok(out)
}

/// `llParcelMediaCommandList`: on the script's owner's land, set the
/// parcel's media (`URL`, `TEXTURE`, `TYPE`, `SIZE`, `DESC`, `LOOP_SET`) and
/// tell the avatars on it to play, stop, pause, unload or seek it; nothing on
/// anybody else's land. The command is sent as both grids send it, to the
/// avatar `PARCEL_MEDIA_COMMAND_AGENT` names or to every avatar on the
/// parcel. Both grids hold the script two seconds, but OpenSim not on land
/// it does not own.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_parcel_media_command_list(
    ctx: &mut ScriptCtx<'_>,
    commands: Vec<Element>,
) -> Result<(), CallError> {
    let at = here(ctx);
    if !owns_here(ctx, &at) {
        return Ok(());
    }
    let caller = ctx.caller();
    let mut media = ctx.host().parcel_media(caller, &at);
    let mut command = None;
    let mut agent = None;
    let mut time = 0.0_f32;
    let mut index = 0_usize;
    while let Some(Element::Integer(code)) = commands.get(index) {
        let arg = |offset: usize| commands.get(index.saturating_add(offset));
        let width = match *code {
            5 => {
                media.url = arg(1)
                    .map(crate::cast::element_to_string)
                    .unwrap_or_default();
                2
            }
            4 => {
                media.texture = arg(1)
                    .map(crate::cast::element_to_string)
                    .unwrap_or_default();
                2
            }
            10 => {
                media.mime = arg(1)
                    .map(crate::cast::element_to_string)
                    .unwrap_or_default();
                2
            }
            12 => {
                media.description = arg(1)
                    .map(crate::cast::element_to_string)
                    .unwrap_or_default();
                2
            }
            11 => {
                let int = |value: Option<&Element>| match value {
                    Some(Element::Integer(value)) => *value,
                    _ => 0,
                };
                media.size = (int(arg(1)), int(arg(2)));
                3
            }
            13 => {
                media.looping = matches!(arg(1), Some(Element::Integer(value)) if *value != 0);
                2
            }
            7 => {
                agent = arg(1).map(crate::cast::element_to_string);
                2
            }
            6 => {
                time = match arg(1) {
                    Some(Element::Float(value)) => *value,
                    Some(Element::Integer(value)) => crate::num::int_to_float(*value),
                    _ => 0.0,
                };
                command = Some(6);
                2
            }
            other @ (0 | 1 | 2 | 3 | 8 | 9) => {
                command = Some(other);
                1
            }
            _ => break,
        };
        index = index.saturating_add(width);
    }
    let _changed = ctx.host().change_land(
        caller,
        &at,
        &LandChange::Media {
            media,
            command,
            agent,
            time,
        },
    );
    Ok(())
}

/// Whether the script's owner owns the parcel at `at`. OpenSim's calls that
/// ask return before their sleep when not, so the call takes no forced delay
/// there.
fn owns_here(ctx: &mut ScriptCtx<'_>, at: &Vector) -> bool {
    let owned = parcel_of(ctx, at).is_some_and(|parcel| owns(ctx, &parcel));
    if !owned && ctx.flavour().is_opensim() {
        ctx.delay(0.0);
    }
    owned
}

/// `llGetParcelMusicURL`: the parcel's stream.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_parcel_music_url(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let at = here(ctx);
    Ok(parcel_of(ctx, &at)
        .map(|parcel| parcel.music_url)
        .unwrap_or_default())
}

/// Make `change` to the parcel under the caller's prim, when the script's
/// owner owns it; on anybody else's land nothing happens and nothing is said,
/// on both grids.
fn change_own_land(ctx: &mut ScriptCtx<'_>, change: &LandChange) -> bool {
    let at = here(ctx);
    let Some(parcel) = parcel_of(ctx, &at) else {
        return false;
    };
    if !owns(ctx, &parcel) {
        return false;
    }
    let caller = ctx.caller();
    ctx.host().change_land(caller, &at, change)
}

/// `llSetParcelMusicURL`: the parcel's stream, on the owner's land; OpenSim
/// takes no sleep on anybody else's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_parcel_music_url(ctx: &mut ScriptCtx<'_>, url: String) -> Result<(), CallError> {
    let at = here(ctx);
    if owns_here(ctx, &at) {
        let _set = change_own_land(ctx, &LandChange::MusicUrl(url));
    }
    Ok(())
}

/// `llEjectFromLand`: send the avatar off the owner's parcel.
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(
    clippy::module_name_repetitions,
    reason = "the library function is named after its LSL call"
)]
pub fn ll_eject_from_land(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<(), CallError> {
    let _ejected = change_own_land(ctx, &LandChange::Eject(agent.0));
    Ok(())
}

/// Whether OpenSim keeps the entry `agent` already has on the owner's
/// parcel's ban list (`ban`) or access list rather than the one for `hours`:
/// an entry for good, or one lasting longer than the new one would. It then
/// changes nothing and returns before its sleep. Second Life replaces it.
fn opensim_keeps(ctx: &mut ScriptCtx<'_>, agent: &str, hours: f32, ban: bool) -> bool {
    if !ctx.flavour().is_opensim() {
        return false;
    }
    let Some(agent) = crate::library::dotnet::canonical_uuid(agent) else {
        return false;
    };
    let at = here(ctx);
    let Some(parcel) = parcel_of(ctx, &at) else {
        return false;
    };
    if !owns(ctx, &parcel) {
        return false;
    }
    let list = if ban { &parcel.bans } else { &parcel.passes };
    let Some(held) = list
        .iter()
        .find(|entry| entry.agent.eq_ignore_ascii_case(&agent))
    else {
        return false;
    };
    // `Util.UnixTimeSinceEpoch() + (int)(3600.0 * hours)`, 0 for none.
    let expires = if hours == 0.0 {
        0
    } else {
        (ctx.unix_micros() / 1_000_000)
            .saturating_add(i64::from(crate::num::float_to_int(hours * 3600.0)))
    };
    held.expires == 0 || (expires != 0 && expires < held.expires)
}

/// `llAddToLandPassList`: a pass to the owner's parcel for `hours` (0: for
/// good). OpenSim keeps a pass already there for good or for longer, and then
/// takes no sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_add_to_land_pass_list(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    hours: f32,
) -> Result<(), CallError> {
    if opensim_keeps(ctx, &agent.0, hours, false) {
        ctx.delay(0.0);
        return Ok(());
    }
    let _added = change_own_land(ctx, &LandChange::Pass(agent.0, hours));
    Ok(())
}

/// `llAddToLandBanList`: a ban from the owner's parcel for `hours` (0: for
/// good). OpenSim keeps a ban already there for good or for longer, and then
/// takes no sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_add_to_land_ban_list(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    hours: f32,
) -> Result<(), CallError> {
    if opensim_keeps(ctx, &agent.0, hours, true) {
        ctx.delay(0.0);
        return Ok(());
    }
    let _added = change_own_land(ctx, &LandChange::Ban(agent.0, hours));
    Ok(())
}

/// `llRemoveFromLandPassList`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_remove_from_land_pass_list(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<(), CallError> {
    let _removed = change_own_land(ctx, &LandChange::Unpass(agent.0));
    Ok(())
}

/// `llRemoveFromLandBanList`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_remove_from_land_ban_list(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<(), CallError> {
    let _removed = change_own_land(ctx, &LandChange::Unban(agent.0));
    Ok(())
}

/// `llResetLandPassList`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_reset_land_pass_list(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let _reset = change_own_land(ctx, &LandChange::ResetPasses);
    Ok(())
}

/// `llResetLandBanList`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_reset_land_ban_list(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let _reset = change_own_land(ctx, &LandChange::ResetBans);
    Ok(())
}

/// `llManageEstateAccess`: change the estate's access lists
/// (`ESTATE_ACCESS_*`), for a script whose owner is the estate's owner or a
/// manager; whether it was changed.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_manage_estate_access(
    ctx: &mut ScriptCtx<'_>,
    action: i32,
    who: Key,
) -> Result<i32, CallError> {
    let standing = ctx.standing();
    let may = standing.roles.iter().any(|role| {
        matches!(
            role,
            crate::vm::Allowed::EstateOwner | crate::vm::Allowed::EstateManager
        )
    });
    if !may || !matches!(action, 4 | 8 | 16 | 32 | 64 | 128) {
        return Ok(0);
    }
    let at = here(ctx);
    let caller = ctx.caller();
    Ok(i32::from(ctx.host().change_land(
        caller,
        &at,
        &LandChange::Estate(action, who.0),
    )))
}

/// `llGetRegionName`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_name(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().region(caller).name)
}

/// `llGetRegionCorner`: the region's south-west corner on the grid, in
/// metres.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_corner(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let caller = ctx.caller();
    let (x, y) = ctx.host().region(caller).corner;
    Ok(Vector {
        x: to_f32(x),
        y: to_f32(y),
        z: 0.0,
    })
}

/// `llGetRegionFlags`: Second Life the `REGION_FLAG_*` bits alone; OpenSim
/// the raw region flags.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_flags(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let flags = ctx.host().region(caller).flags;
    Ok(if ctx.flavour().is_opensim() {
        flags.cast_signed()
    } else {
        (flags & REGION_FLAGS).cast_signed()
    })
}

/// `llGetRegionAgentCount`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_agent_count(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().region(caller).agents)
}

/// `llGetSimulatorHostname`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_simulator_hostname(ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().region(caller).hostname)
}

/// `llGetEnv`: what each grid answers of `name` (`land/env`). Second Life's
/// keys are case-sensitive and include the region's name, rating and grid,
/// damage and the agents reserved; OpenSim lowers the key, answers its
/// uptime, a constant `0` for pathfinding and `OpenSim` for the SKU.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_env(ctx: &mut ScriptCtx<'_>, name: String) -> Result<String, CallError> {
    let opensim = ctx.flavour().is_opensim();
    let key = if opensim { name.to_lowercase() } else { name };
    let caller = ctx.caller();
    let region = ctx.host().region(caller);
    let simulator = ctx.host().simulator();
    let now = ctx.now().0;
    let unix = ctx.unix_micros().div_euclid(1_000_000);
    let both = match key.as_str() {
        "sim_channel" => Some(region.channel.clone()),
        "sim_version" => Some(simulator.version.clone()),
        "estate_id" => Some(region.estate.0.to_string()),
        "estate_name" => Some(region.estate.1.clone()),
        "region_max_prims" => Some(region.capacity.0.to_string()),
        "region_object_bonus" => Some(number(region.capacity.1)),
        "agent_limit" => Some(region.agent_limit.to_string()),
        "whisper_range" => Some("10".to_owned()),
        "chat_range" => Some("20".to_owned()),
        "shout_range" => Some("100".to_owned()),
        "region_start_time" => Some(region.start_time.to_string()),
        "region_cpu_ratio" => Some("1".to_owned()),
        "region_idle" => Some("0".to_owned()),
        "simulator_hostname" => Some(region.hostname.clone()),
        _ => None,
    };
    if let Some(answer) = both {
        return Ok(answer);
    }
    Ok(if opensim {
        match key.as_str() {
            "region_product_name" => region.product.0,
            "region_product_sku" => "OpenSim".to_owned(),
            "dynamic_pathfinding" => "0".to_owned(),
            // OpenSim counts its frames at a fifth of its frame rate.
            "frame_number" => (now / 5).to_string(),
            "region_up_time" => unix.saturating_sub(region.start_time).to_string(),
            _ => String::new(),
        }
    } else {
        match key.as_str() {
            "region_product_name" => region.product.0,
            "region_product_sku" => region.product.1,
            "region_rating" => region.rating,
            "dynamic_pathfinding" => "enabled".to_owned(),
            "frame_number" => now.to_string(),
            "grid" => simulator.grid_nick,
            "allow_damage_adjust" => "1".to_owned(),
            "damage_limit" => "0".to_owned(),
            "region_name" => region.name,
            "agent_reserved" => "5".to_owned(),
            _ => String::new(),
        }
    })
}

/// `llEdgeOfWorld`: whether a line from `at` along `direction` leaves the
/// region where no region adjoins it — OpenSim's port of Second Life's rule:
/// a direction with no level part is an edge; Second Life answers 0 from a
/// position off the region, OpenSim clamps it in.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_edge_of_world(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    direction: Vector,
) -> Result<i32, CallError> {
    if direction.x == 0.0 && direction.y == 0.0 {
        return Ok(1);
    }
    if !ctx.flavour().is_opensim() && outside(ctx, &at) {
        return Ok(0);
    }
    let (width, depth) = size(ctx);
    let px = at.x.clamp(0.5, width - 0.5);
    let py = at.y.clamp(0.5, depth - 0.5);
    let length = direction.x.hypot(direction.y);
    let (dx, dy) = (direction.x / length, direction.y / length);
    let along = |position: f32, step: f32, extent: f32| {
        if step > 0.0 {
            (extent + 1.0 - position) / step
        } else if step < 0.0 {
            -(position + 1.0) / step
        } else {
            f32::INFINITY
        }
    };
    let t = along(px, dx, width).min(along(py, dy, depth));
    let (ex, ey) = (px + t * dx, py + t * dy);
    let caller = ctx.caller();
    let [east, north, west, south] = ctx.host().region(caller).neighbours;
    let adjoins =
        (ex > width && east) || (ey > depth && north) || (ex < 0.0 && west) || (ey < 0.0 && south);
    Ok(i32::from(!adjoins))
}

/// `llGetAgentList`: the avatars of a scope — the parcel the caller's prim
/// is on (`AGENT_LIST_PARCEL`), the parcels of its owner
/// (`AGENT_LIST_PARCEL_OWNER`), the region (`AGENT_LIST_REGION`) — any other
/// scope `["INVALID_SCOPE"]`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_agent_list(
    ctx: &mut ScriptCtx<'_>,
    scope: i32,
    _options: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    if !matches!(
        scope,
        AGENT_LIST_PARCEL | AGENT_LIST_PARCEL_OWNER | AGENT_LIST_REGION
    ) {
        return Ok(vec![Element::String("INVALID_SCOPE".to_owned())]);
    }
    let at = here(ctx);
    let caller = ctx.caller();
    let mine = ctx.host().parcel_at(caller, at.x, at.y);
    let avatars: Vec<(String, Vector)> = ctx
        .host()
        .sense(caller)
        .into_iter()
        .filter(|sensed| sensed.agent)
        .map(|sensed| (sensed.key, sensed.position))
        .collect();
    let mut out = Vec::new();
    for (key, position) in avatars {
        let theirs = ctx.host().parcel_at(caller, position.x, position.y);
        let admitted = match scope {
            AGENT_LIST_PARCEL => theirs
                .as_ref()
                .zip(mine.as_ref())
                .is_some_and(|(theirs, mine)| theirs.id == mine.id),
            AGENT_LIST_PARCEL_OWNER => theirs
                .as_ref()
                .zip(mine.as_ref())
                .is_some_and(|(theirs, mine)| theirs.owner == mine.owner),
            _ => true,
        };
        if admitted {
            out.push(Element::Key(key));
        }
    }
    Ok(out)
}

/// A region position as `llRequestSimulatorData` writes it on Second Life:
/// `<x,y,0>`, integers, no spaces.
fn second_life_position(corner: (f64, f64)) -> String {
    format!(
        "<{},{},0>",
        crate::num::double_to_long(corner.0),
        crate::num::double_to_long(corner.1)
    )
}

/// A vector as LSL writes it in a list: six decimals.
fn six(x: f64, y: f64) -> String {
    format!("<{x:.6}, {y:.6}, 0.000000>")
}

/// `llRequestSimulatorData`: ask of the grid's region `name` its position
/// (`DATA_SIM_POS` 5), status (6), rating (7) or release (128, which Second
/// Life refuses with `NULL_KEY`); the answer comes over `dataserver`. Second
/// Life never answers the position of a region no grid has, and calls the
/// empty name's status `crashed`; OpenSim answers its own region's position
/// in regions, not metres, when named exactly.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_simulator_data(
    ctx: &mut ScriptCtx<'_>,
    name: String,
    data: i32,
) -> Result<Key, CallError> {
    let opensim = ctx.flavour().is_opensim();
    if !matches!(data, 5..=7) && !(opensim && data == 128) {
        return Ok(Key(NULL_KEY.to_owned()));
    }
    let caller = ctx.caller();
    let own = ctx.host().region(caller);
    let found = ctx.host().sim_data(caller, &name);
    let id = ctx.fresh_key();
    let answer = match (data, found) {
        (128, _) => Some("OpenSim".to_owned()),
        (5, Some(region)) => Some(if !opensim {
            second_life_position(region.corner)
        } else if name == own.name {
            six(region.corner.0 / 256.0, region.corner.1 / 256.0)
        } else {
            six(region.corner.0, region.corner.1)
        }),
        (6, Some(region)) => Some(region.status),
        (7, Some(region)) => Some(region.rating),
        (5, None) if opensim => Some("unknown".to_owned()),
        (5, None) => None,
        (_, None) if !opensim && name.is_empty() => Some("crashed".to_owned()),
        (_, None) => Some("unknown".to_owned()),
        _ => None,
    };
    if let Some(answer) = answer {
        ctx.raise_dataserver(id.clone(), answer);
    }
    Ok(Key(id))
}

/// The point `offset` from the caller's prim, held inside the region.
fn under(ctx: &mut ScriptCtx<'_>, offset: &Vector) -> Vector {
    let at = here(ctx);
    let point = Vector {
        x: at.x + offset.x,
        y: at.y + offset.y,
        z: at.z + offset.z,
    };
    clamped(ctx, &point)
}

/// The ground's height at `(x, y)`.
fn height(ctx: &mut ScriptCtx<'_>, x: f32, y: f32) -> f32 {
    let caller = ctx.caller();
    ctx.host().ground(caller, x, y)
}

/// `llGround`: the ground's height under `offset` from the prim (its height
/// aside), held inside the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ground(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<f32, CallError> {
    let at = under(ctx, &offset);
    Ok(height(ctx, at.x, at.y))
}

/// The ground's gradient under `offset`: its rise a metre east and north.
fn gradient(ctx: &mut ScriptCtx<'_>, offset: &Vector) -> (f32, f32) {
    let at = under(ctx, offset);
    let (width, depth) = size(ctx);
    let east = height(ctx, (at.x + 0.5).min(width - 0.01), at.y);
    let west = height(ctx, (at.x - 0.5).max(0.0), at.y);
    let north = height(ctx, at.x, (at.y + 0.5).min(depth - 0.01));
    let south = height(ctx, at.x, (at.y - 0.5).max(0.0));
    (east - west, north - south)
}

/// `v` made unit length; zero stays zero.
fn unit(v: &Vector) -> Vector {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length <= 0.0 {
        return ZERO_VECTOR;
    }
    Vector {
        x: v.x / length,
        y: v.y / length,
        z: v.z / length,
    }
}

/// `llGroundNormal`: the ground's normal under `offset`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ground_normal(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<Vector, CallError> {
    let (dx, dy) = gradient(ctx, &offset);
    Ok(unit(&Vector {
        x: 0.0 - dx,
        y: 0.0 - dy,
        z: 1.0,
    }))
}

/// A zero vector as the flavour writes one of a level ground's slope or
/// contour: Second Life's carries a negative zero in x (`<-0.00000, 0.00000,
/// 0.00000>`), OpenSim's none.
const fn level(ctx: &ScriptCtx<'_>) -> Vector {
    Vector {
        x: if ctx.flavour().is_opensim() {
            0.0
        } else {
            -0.0
        },
        y: 0.0,
        z: 0.0,
    }
}

/// `llGroundSlope`: the way down the ground under `offset`, along it; zero
/// on level ground.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ground_slope(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<Vector, CallError> {
    let (dx, dy) = gradient(ctx, &offset);
    if dx == 0.0 && dy == 0.0 {
        return Ok(level(ctx));
    }
    Ok(unit(&Vector {
        x: -dx,
        y: -dy,
        z: -(dx * dx + dy * dy),
    }))
}

/// `llGroundContour`: the level way across the ground under `offset`; zero
/// on level ground.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_ground_contour(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<Vector, CallError> {
    let (dx, dy) = gradient(ctx, &offset);
    if dx == 0.0 && dy == 0.0 {
        return Ok(level(ctx));
    }
    Ok(unit(&Vector {
        x: -dy,
        y: dx,
        z: 0.0,
    }))
}

/// `llWater`: the region's water height, wherever the offset points.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_water(ctx: &mut ScriptCtx<'_>, _offset: Vector) -> Result<f32, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().region(caller).water)
}

/// `llWind`: the wind under `offset`, level.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_wind(ctx: &mut ScriptCtx<'_>, offset: Vector) -> Result<Vector, CallError> {
    let at = under(ctx, &offset);
    let caller = ctx.caller();
    let wind = ctx.host().wind(caller, &at);
    Ok(Vector {
        x: wind.x,
        y: wind.y,
        z: 0.0,
    })
}

/// `llCloud`: 0, on both grids, which keep no clouds a script can read.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_cloud(_ctx: &mut ScriptCtx<'_>, _offset: Vector) -> Result<f32, CallError> {
    Ok(0.0)
}

/// The environment over the caller's prim (or for `region` the region's) now.
fn environment(ctx: &mut ScriptCtx<'_>, region: bool) -> crate::vm::land::Environment {
    let at = here(ctx);
    let unix = ctx.unix_micros().div_euclid(1_000_000);
    let caller = ctx.caller();
    ctx.host()
        .environment(caller, (!region).then_some(&at), unix)
}

/// The seconds into its day `environment`'s day cycle is now.
pub(crate) fn time_of_day(ctx: &mut ScriptCtx<'_>, region: bool) -> f64 {
    let environment = environment(ctx, region);
    let micros = ctx.unix_micros();
    let length = f64::from(environment.day_length);
    if length <= 0.0 {
        return 0.0;
    }
    let seconds = crate::num::wide_to_double(i128::from(micros)) / 1_000_000.0
        + f64::from(environment.day_offset);
    seconds.rem_euclid(length)
}

/// `llGetDayLength`: the parcel's day's length, in seconds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_day_length(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(crate::num::float_to_int(environment(ctx, false).day_length))
}

/// `llGetDayOffset`: the parcel's day's offset, in seconds.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_day_offset(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(crate::num::float_to_int(environment(ctx, false).day_offset))
}

/// `llGetRegionDayLength`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_day_length(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(crate::num::float_to_int(environment(ctx, true).day_length))
}

/// `llGetRegionDayOffset`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_day_offset(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(crate::num::float_to_int(environment(ctx, true).day_offset))
}

/// The forward axis of `turn`.
fn forward(turn: &Rotation) -> Vector {
    crate::ops::rotate(
        &Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        turn,
    )
}

/// `llGetSunRotation`: how the parcel's sun is turned.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_sun_rotation(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(environment(ctx, false).sun)
}

/// `llGetMoonRotation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_moon_rotation(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(environment(ctx, false).moon)
}

/// `llGetRegionSunRotation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_sun_rotation(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(environment(ctx, true).sun)
}

/// `llGetRegionMoonRotation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_moon_rotation(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(environment(ctx, true).moon)
}

/// `llGetSunDirection`: the way to the parcel's sun — its rotation's forward
/// axis.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_sun_direction(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(forward(&environment(ctx, false).sun))
}

/// `llGetMoonDirection`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_moon_direction(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(forward(&environment(ctx, false).moon))
}

/// `llGetRegionSunDirection`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_sun_direction(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(forward(&environment(ctx, true).sun))
}

/// `llGetRegionMoonDirection`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_moon_direction(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(forward(&environment(ctx, true).moon))
}

/// `llGetRegionTimeOfDay`: the seconds into its day the region's day cycle
/// is. Second Life's, which OpenSim does not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_region_time_of_day(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    Ok(to_f32(time_of_day(ctx, true)))
}

/// `llGetSimStats`: the region statistic `stat` (`SIM_STAT_*`, 0–26); an
/// unknown one 0.0 and `Unknown stat.`. Second Life's, which OpenSim does
/// not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_sim_stats(ctx: &mut ScriptCtx<'_>, stat: i32) -> Result<f32, CallError> {
    let caller = ctx.caller();
    let stats = ctx.host().sim_stats(caller);
    match usize::try_from(stat).ok().and_then(|at| stats.get(at)) {
        Some(value) => Ok(*value),
        None => {
            ctx.refuse("Unknown stat.".to_owned());
            Ok(0.0)
        }
    }
}

/// `llGetClosestNavPoint`: the nearest point of the navmesh to `at`. The
/// fake grid keeps no navmesh and reads all of its ground as walkable: the
/// point is `at` on the ground. Second Life's, which OpenSim does not
/// compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_closest_nav_point(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    _options: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let point = clamped(ctx, &at);
    let ground = height(ctx, point.x, point.y);
    Ok(vec![Element::Vector(Vector {
        x: point.x,
        y: point.y,
        z: ground,
    })])
}

/// `llGetEnvironment`: what the environment over `at` says of each rule —
/// `ENVIRONMENT_DAYINFO` the day's length, offset and time of day, each
/// `SKY_*` and `WATER_*` rule its setting's values — the textures refused to
/// a script whose owner does not own the setting, an unknown rule refused.
/// Second Life's, which OpenSim does not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_environment(
    ctx: &mut ScriptCtx<'_>,
    at: Vector,
    rules: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let mut out = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let number = index.saturating_add(1);
        let Element::Integer(rule) = rule else {
            ctx.refuse(format!(
                "llGetEnvironment Error running rule #{number}: rule id is not an integer."
            ));
            return Ok(Vec::new());
        };
        if *rule == 200 {
            let unix = ctx_unix(ctx);
            let environment = ctx.host().environment(caller, Some(&at), unix);
            out.push(Element::Integer(crate::num::float_to_int(
                environment.day_length,
            )));
            out.push(Element::Integer(crate::num::float_to_int(
                environment.day_offset,
            )));
            out.push(Element::Float(to_f32(time_of_day(ctx, false))));
            continue;
        }
        let texture = match rule {
            19 => Some("SKY_CLOUD_TEXTURE"),
            20 => Some("SKY_MOON_TEXTURE"),
            21 => Some("SKY_SUN_TEXTURE"),
            107 => Some("WATER_NORMAL_TEXTURE"),
            _ => None,
        };
        if let Some(name) = texture {
            ctx.refuse(format!(
                "llGetEnvironment Error running rule #{number} ({name}): Attempt to read \
                 unreadable value.."
            ));
            return Ok(Vec::new());
        }
        match ctx.host().environment_rule(caller, &at, *rule) {
            Some(values) => out.extend(values),
            None => {
                ctx.refuse(format!(
                    "llGetEnvironment Error running rule #{number}: unknown rule id {rule}."
                ));
                return Ok(Vec::new());
            }
        }
    }
    Ok(out)
}

/// The region's calendar time now, in whole seconds.
fn ctx_unix(ctx: &mut ScriptCtx<'_>) -> i64 {
    ctx.unix_micros().div_euclid(1_000_000)
}

/// `osGetParcelDwell`: the traffic of the parcel at `at`, truncated; 0 off
/// any parcel. OpenSim's, which Second Life does not compile.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_parcel_dwell(ctx: &mut ScriptCtx<'_>, at: Vector) -> Result<i32, CallError> {
    Ok(parcel_of(ctx, &at).map_or(0, |parcel| crate::num::float_to_int(parcel.dwell)))
}

/// `osSetParcelMusicURL`: the stream of the parcel under the prim. The
/// region's `[OSSL]` settings are the only gate — OpenSim checks no owner.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_parcel_music_url(ctx: &mut ScriptCtx<'_>, url: String) -> Result<(), CallError> {
    let at = here(ctx);
    let caller = ctx.caller();
    let _set = ctx
        .host()
        .change_land(caller, &at, &LandChange::MusicUrl(url));
    Ok(())
}

/// `osSetParcelMediaURL`: the media URL of the parcel under the prim, the
/// rest of its media kept; no command is sent. The region's `[OSSL]`
/// settings are the only gate.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_parcel_media_url(ctx: &mut ScriptCtx<'_>, url: String) -> Result<(), CallError> {
    let at = here(ctx);
    let caller = ctx.caller();
    let mut media = ctx.host().parcel_media(caller, &at);
    media.url = url;
    let _set = ctx.host().change_land(
        caller,
        &at,
        &LandChange::Media {
            media,
            command: None,
            agent: None,
            time: 0.0,
        },
    );
    Ok(())
}

/// `osGetMapTexture`: the key of the region's map texture.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_map_texture(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    let caller = ctx.caller();
    Ok(Key(ctx
        .host()
        .map_texture(caller, "")
        .unwrap_or_else(|| crate::value::NULL_KEY.to_owned())))
}

/// `osGetRegionMapTexture`: the key of the map texture of the region named
/// or keyed `region` — the script's own for an empty or blank name, without
/// a pause — or `NULL_KEY` for a region the grid does not know; a lookup holds
/// the script a second.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_region_map_texture(
    ctx: &mut ScriptCtx<'_>,
    region: String,
) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let blank = region.trim().is_empty();
    let name = if blank { "" } else { region.as_str() };
    let texture = ctx
        .host()
        .map_texture(caller, name)
        .unwrap_or_else(|| crate::value::NULL_KEY.to_owned());
    if !blank {
        ctx.delay(1.0);
    }
    Ok(Key(texture))
}

/// `osWindActiveModelPluginName`: the wind model the region runs — the
/// local OpenSim's `SimpleRandomWind`, which the fake grid's uniform wind
/// stands in for.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_wind_active_model_plugin_name(_ctx: &mut ScriptCtx<'_>) -> Result<String, CallError> {
    Ok("SimpleRandomWind".to_owned())
}
