//! Library tranche: OpenSim's land, terrain, wind and environment writers
//! (`server-lsl-lib-ossl-land-writers`) — the ground's heights and textures,
//! the water line and the sun, the wind models' parameters, the region's,
//! a parcel's and an avatar's environment and LightShare's view of it, the
//! parcel joins, cuts and details, the region's statistics, notices and
//! restarts. Ported from `OSSL_Api.cs`, `LS_Api.cs` and the OpenSim modules
//! they call (`LandManagementModule`, `EnvironmentModule`, `RestartModule`,
//! `PermissionsModule`).
//!
//! Only an OpenSim flavour compiles these, and every call has passed the
//! region's permission check by the time it gets here (`vm::ossl::admit`);
//! the checks below are the second ones OpenSim makes in the function itself,
//! against its permissions module: an estate manager, a god, a parcel's owner
//! or its group's member with the power named.

use core::fmt::Write as _;

use sl_types::lsl::{Rotation, Vector};

use crate::cast::element_to_string_for;
use crate::library::dotnet::canonical_uuid;
use crate::library::money_permissions::opensim_integer_item;
use crate::library::ossl::{dotnet_format, runtime_error};
use crate::library::{BuiltinId, CallError, Exception, Key, ThreatLevel};
use crate::num::float_to_int;
use crate::value::{Element, NULL_KEY};
use crate::vm::land::{
    DayCycle, EnvironmentChange, EnvironmentScope, LandChange, LightShare, Parcel, ParcelDetails,
    RegionEdit, WindParamError,
};
use crate::vm::ossl::warn;
use crate::vm::{Allowed, ScriptCtx};

/// `ParcelFlags.AllowTerraform`.
const PARCEL_FLAG_ALLOW_TERRAFORM: u32 = 0x10;
/// `GroupPowers.LandDivideJoin`.
const POWER_LAND_DIVIDE_JOIN: u64 = 1 << 15;
/// `GroupPowers.LandOptions`.
const POWER_LAND_OPTIONS: u64 = 1 << 22;
/// `GroupPowers.AllowEditLand`.
const POWER_ALLOW_EDIT_LAND: u64 = 1 << 23;
/// `GroupPowers.AllowEnvironment`.
const POWER_ALLOW_ENVIRONMENT: u64 = 1 << 46;

/// `PARCEL_DETAILS_NAME`.
const PARCEL_DETAILS_NAME: i32 = 0;
/// `PARCEL_DETAILS_DESC`.
const PARCEL_DETAILS_DESC: i32 = 1;
/// `PARCEL_DETAILS_OWNER`.
const PARCEL_DETAILS_OWNER: i32 = 2;
/// `PARCEL_DETAILS_GROUP`.
const PARCEL_DETAILS_GROUP: i32 = 3;
/// `PARCEL_DETAILS_SEE_AVATARS`.
const PARCEL_DETAILS_SEE_AVATARS: i32 = 6;
/// `PARCEL_DETAILS_ANY_AVATAR_SOUNDS`.
const PARCEL_DETAILS_ANY_AVATAR_SOUNDS: i32 = 7;
/// `PARCEL_DETAILS_GROUP_SOUNDS`.
const PARCEL_DETAILS_GROUP_SOUNDS: i32 = 8;
/// `PARCEL_DETAILS_CLAIMDATE`.
const PARCEL_DETAILS_CLAIMDATE: i32 = 10;

/// The edge of one parcel block, in metres: a join samples the box at it.
const BLOCK: usize = 4;

/// The seconds below which `osRegionRestart` calls a restart off.
const RESTART_FLOOR: f32 = 15.0;

// ----------------------------------------------------------------------
// Who may: OpenSim's `PermissionsModule`.
// ----------------------------------------------------------------------

/// `IsEstateManager`: the estate's owner or one of its managers.
fn estate_manager(ctx: &ScriptCtx<'_>) -> bool {
    let standing = ctx.standing();
    standing.is(Allowed::EstateOwner) || standing.is(Allowed::EstateManager)
}

/// `IsAdministrator`: a god — the estate owner as `region_owner_is_god` makes
/// one, or a grid god.
fn administrator(ctx: &ScriptCtx<'_>) -> bool {
    ctx.standing().is(Allowed::God)
}

/// `CanIssueEstateCommand`: a god, or for an owner's command the estate's
/// owner, for any other its owner or a manager.
fn estate_command(ctx: &ScriptCtx<'_>, owner_command: bool) -> bool {
    if administrator(ctx) {
        return true;
    }
    if owner_command {
        return ctx.standing().is(Allowed::EstateOwner);
    }
    estate_manager(ctx)
}

/// `IsGroupMember`: whether `agent` is in the group keyed `group` holding
/// every power of `powers` — or, for none, any power at all.
fn group_member(ctx: &mut ScriptCtx<'_>, group: &str, agent: &str, powers: u64) -> bool {
    match ctx.host().group_powers(group, agent) {
        Some(held) => (held != 0 && powers == 0) || held & powers == powers,
        None => false,
    }
}

/// `GenericParcelOwnerPermission` without the estate managers, which
/// `CanEditParcelProperties` leaves out whatever it is asked: the parcel's
/// owner, a member of the group owning it with `powers`, or a god.
fn edits_parcel(ctx: &mut ScriptCtx<'_>, parcel: &Parcel, powers: u64) -> bool {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    if parcel.owner.eq_ignore_ascii_case(&owner) {
        return true;
    }
    if parcel.group_owned && group_member(ctx, &parcel.group.clone(), &owner, powers) {
        return true;
    }
    administrator(ctx)
}

/// `CanTerraformLand` at `(x, y)`: an estate manager or a god anywhere, and
/// on a parcel that lets anyone terraform it, its owner, or a member of the
/// group owning it who may edit land.
fn terraforms(ctx: &mut ScriptCtx<'_>, x: u32, y: u32) -> bool {
    if estate_manager(ctx) || administrator(ctx) {
        return true;
    }
    let caller = ctx.caller();
    let metres = |value: u32| u16::try_from(value).map_or(f32::MAX, f32::from);
    let Some(parcel) = ctx.host().parcel_at(caller, metres(x), metres(y)) else {
        return false;
    };
    if parcel.flags & PARCEL_FLAG_ALLOW_TERRAFORM != 0 {
        return true;
    }
    let owner = ctx.host().owner(caller);
    if parcel.owner.eq_ignore_ascii_case(&owner) {
        return true;
    }
    parcel.group_owned
        && parcel.group != NULL_KEY
        && group_member(ctx, &parcel.group.clone(), &owner, POWER_ALLOW_EDIT_LAND)
}

// ----------------------------------------------------------------------
// The ground.
// ----------------------------------------------------------------------

/// `osSetTerrainHeight`: the ground at the whole-metre point `(x, y)` set to
/// `height`, where the script's owner may terraform; 1 where it did, 0 where
/// it may not. The region tells its avatars of the patch after the tick.
///
/// # Errors
///
/// An OSSL run-time error for a point off the region.
pub fn os_set_terrain_height(
    ctx: &mut ScriptCtx<'_>,
    x: i32,
    y: i32,
    height: f32,
) -> Result<i32, CallError> {
    set_terrain_height(ctx, x, y, height, BuiltinId::OsSetTerrainHeight)
}

/// `osTerrainSetHeight`: the deprecated [`os_set_terrain_height`], whose
/// error names the new function.
///
/// # Errors
///
/// As [`os_set_terrain_height`].
pub fn os_terrain_set_height(
    ctx: &mut ScriptCtx<'_>,
    x: i32,
    y: i32,
    height: f32,
) -> Result<i32, CallError> {
    set_terrain_height(ctx, x, y, height, BuiltinId::OsTerrainSetHeight)
}

/// The two terrain writers' shared body (`SetTerrainHeight`).
fn set_terrain_height(
    ctx: &mut ScriptCtx<'_>,
    x: i32,
    y: i32,
    height: f32,
    id: BuiltinId,
) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let (width, depth) = ctx.host().region_size(caller);
    let inside = |coordinate: i32, size: u32| {
        u32::try_from(coordinate)
            .ok()
            .filter(|coordinate| *coordinate < size)
    };
    let (Some(x), Some(y)) = (inside(x, width), inside(y, depth)) else {
        return Err(runtime_error(
            id,
            "osSetTerrainHeight: Coordinate out of bounds",
        ));
    };
    if !terraforms(ctx, x, y) {
        return Ok(0);
    }
    let _set = ctx
        .host()
        .edit_region(caller, &RegionEdit::TerrainHeight { x, y, height });
    Ok(1)
}

/// `osTerrainFlush`: every patch of the ground sent again to every avatar in
/// the region (`TaintTerrain`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_terrain_flush(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    let _flushed = ctx.host().edit_region(caller, &RegionEdit::TerrainFlush);
    Ok(())
}

/// `osSetTerrainTexture`: the detail texture of `corner` (0–3), for a script
/// whose owner is a god; nothing for any other, a corner that is none or a
/// key that is none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_terrain_texture(
    ctx: &mut ScriptCtx<'_>,
    corner: i32,
    texture: Key,
) -> Result<(), CallError> {
    if !administrator(ctx) {
        return Ok(());
    }
    let Some(corner) = usize::try_from(corner).ok().filter(|corner| *corner <= 3) else {
        return Ok(());
    };
    let Some(texture) = canonical_uuid(&texture.0).filter(|texture| texture != NULL_KEY) else {
        return Ok(());
    };
    let caller = ctx.caller();
    let _set = ctx
        .host()
        .edit_region(caller, &RegionEdit::TerrainTexture { corner, texture });
    Ok(())
}

/// `osSetTerrainTextureHeight`: where `corner`'s (0–3) textures blend, for
/// a script whose owner is a god.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_terrain_texture_height(
    ctx: &mut ScriptCtx<'_>,
    corner: i32,
    low: f32,
    high: f32,
) -> Result<(), CallError> {
    if !administrator(ctx) {
        return Ok(());
    }
    let Some(corner) = usize::try_from(corner).ok().filter(|corner| *corner <= 3) else {
        return Ok(());
    };
    let caller = ctx.caller();
    let _set = ctx.host().edit_region(
        caller,
        &RegionEdit::TerrainTextureHeights { corner, low, high },
    );
    Ok(())
}

// ----------------------------------------------------------------------
// The water and the sun.
// ----------------------------------------------------------------------

/// `osSetRegionWaterHeight`: the region's water line, which every avatar in
/// it is told of in a `RegionInfo`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_region_water_height(ctx: &mut ScriptCtx<'_>, height: f32) -> Result<(), CallError> {
    let caller = ctx.caller();
    let _set = ctx
        .host()
        .edit_region(caller, &RegionEdit::WaterHeight(height));
    Ok(())
}

/// `osSetRegionSunSettings`: whether the region follows the estate's sun,
/// whether its own stands still and where, `hour` wrapped into 0–24 and kept
/// as Linden's sun hour, six more. The region keeps them and tells nobody:
/// the sun update OpenSim then raises has no listener since the day cycles
/// came.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_region_sun_settings(
    ctx: &mut ScriptCtx<'_>,
    estate_sun: i32,
    fixed: i32,
    hour: f32,
) -> Result<(), CallError> {
    let mut hour = f64::from(hour);
    while hour > 24.0 {
        hour -= 24.0;
    }
    while hour < 0.0 {
        hour += 24.0;
    }
    let caller = ctx.caller();
    let _set = ctx.host().edit_region(
        caller,
        &RegionEdit::Sun {
            estate_sun: estate_sun != 0,
            fixed: fixed != 0,
            hour: crate::num::to_f32(hour + 6.0),
        },
    );
    Ok(())
}

/// `osSetEstateSunSettings`: nothing — its body is commented out in
/// OpenSim's source, its permission check with it.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_set_estate_sun_settings(
    _ctx: &mut ScriptCtx<'_>,
    _fixed: i32,
    _hour: f32,
) -> Result<(), CallError> {
    Ok(())
}

/// `osSetSunParam`: nothing — it hands the parameter to the region's sun
/// module, which OpenSim no longer has.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_sun_param(
    _ctx: &mut ScriptCtx<'_>,
    _param: String,
    _value: f32,
) -> Result<(), CallError> {
    Ok(())
}

/// `osSunSetParam`: the deprecated [`os_set_sun_param`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_sun_set_param(
    _ctx: &mut ScriptCtx<'_>,
    _param: String,
    _value: f32,
) -> Result<(), CallError> {
    Ok(())
}

// ----------------------------------------------------------------------
// The wind.
// ----------------------------------------------------------------------

/// `osGetWindParam`: the parameter `param` of the region's wind model
/// `plugin` — `SimpleRandomWind`'s `strength`, `ConfigurableWind`'s
/// `avgStrength`, `avgDirection`, `varStrength`, `varDirection` and
/// `rateChange`.
///
/// # Errors
///
/// [`Exception::NoWindPlugin`] for a model the region has not got and
/// [`Exception::NoWindParam`] for a parameter the model has not got: OpenSim
/// lets the model's exception out of the call.
pub fn os_get_wind_param(
    ctx: &mut ScriptCtx<'_>,
    plugin: String,
    param: String,
) -> Result<f32, CallError> {
    let caller = ctx.caller();
    ctx.host()
        .wind_param(caller, &plugin, &param)
        .map_err(|error| CallError::Exception {
            id: BuiltinId::OsGetWindParam,
            exception: match error {
                WindParamError::NoPlugin => Exception::NoWindPlugin(plugin),
                WindParamError::NoParam { plugin } => Exception::NoWindParam { plugin, param },
            },
        })
}

/// `osSetWindParam`: the parameter `param` of the region's wind model
/// `plugin`; nothing for a model or a parameter it has not got, which OpenSim
/// catches.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_wind_param(
    ctx: &mut ScriptCtx<'_>,
    plugin: String,
    param: String,
    value: f32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let _set = ctx.host().edit_region(
        caller,
        &RegionEdit::WindParam {
            plugin,
            param,
            value,
        },
    );
    Ok(())
}

// ----------------------------------------------------------------------
// Parcels.
// ----------------------------------------------------------------------

/// The box `pos1` and `pos2` span as OpenSim reads it: each corner's
/// coordinates truncated, the west and south first.
fn box_of(pos1: &Vector, pos2: &Vector) -> ((i32, i32), (i32, i32)) {
    (
        (
            float_to_int(pos1.x.min(pos2.x)),
            float_to_int(pos1.y.min(pos2.y)),
        ),
        (
            float_to_int(pos1.x.max(pos2.x)),
            float_to_int(pos1.y.max(pos2.y)),
        ),
    )
}

/// The parcel at the whole-metre point `(x, y)` (`GetLandObject(int, int)`).
///
/// # Errors
///
/// [`Exception::NoParcelAt`] for a point off the region, which OpenSim's land
/// module throws.
fn parcel_at_point(
    ctx: &mut ScriptCtx<'_>,
    (x, y): (i32, i32),
    id: BuiltinId,
) -> Result<Option<Parcel>, CallError> {
    let caller = ctx.caller();
    let (width, depth) = ctx.host().region_size(caller);
    let inside = |coordinate: i32, size: u32| {
        u32::try_from(coordinate)
            .ok()
            .filter(|coordinate| *coordinate < size)
            .and_then(|coordinate| u16::try_from(coordinate).ok())
    };
    let (Some(east), Some(north)) = (inside(x, width), inside(y, depth)) else {
        return Err(CallError::Exception {
            id,
            exception: Exception::NoParcelAt(x, y),
        });
    };
    Ok(ctx
        .host()
        .parcel_at(caller, f32::from(east), f32::from(north)))
}

/// `osParcelJoin`: every parcel the box from `pos1` to `pos2` touches —
/// sampled every four metres from its south-west corner — joined into the
/// largest, the first found of the largest, if they are two or more, the
/// script's owner may divide and join the largest and one owner holds them
/// all; nothing otherwise.
///
/// # Errors
///
/// [`Exception::NoParcelAt`] for a box reaching off the region.
pub fn os_parcel_join(
    ctx: &mut ScriptCtx<'_>,
    pos1: Vector,
    pos2: Vector,
) -> Result<(), CallError> {
    let ((start_x, start_y), (end_x, end_y)) = box_of(&pos1, &pos2);
    let mut found: Vec<((i32, i32), Parcel)> = Vec::new();
    let mut largest: Option<usize> = None;
    let mut largest_area = 0;
    for x in (i64::from(start_x)..i64::from(end_x)).step_by(BLOCK) {
        for y in (i64::from(start_y)..i64::from(end_y)).step_by(BLOCK) {
            let point = (
                i32::try_from(x).unwrap_or(i32::MAX),
                i32::try_from(y).unwrap_or(i32::MAX),
            );
            let Some(parcel) = parcel_at_point(ctx, point, BuiltinId::OsParcelJoin)? else {
                continue;
            };
            if found.iter().any(|(_point, held)| held.id == parcel.id) {
                continue;
            }
            if parcel.area > largest_area {
                largest_area = parcel.area;
                largest = Some(found.len());
            }
            found.push((point, parcel));
        }
    }
    let Some(largest) = largest.filter(|_largest| found.len() >= 2) else {
        return Ok(());
    };
    let master = found.remove(largest);
    if !edits_parcel(ctx, &master.1, POWER_LAND_DIVIDE_JOIN) {
        return Ok(());
    }
    if found
        .iter()
        .any(|(_point, parcel)| !parcel.owner.eq_ignore_ascii_case(&master.1.owner))
    {
        return Ok(());
    }
    let mut points = vec![master.0];
    points.extend(found.iter().map(|(point, _parcel)| *point));
    let caller = ctx.caller();
    let at = point_vector(master.0);
    let _joined = ctx
        .host()
        .change_land(caller, &at, &LandChange::Join(points));
    Ok(())
}

/// A whole-metre point as a vector on the ground plane.
fn point_vector((x, y): (i32, i32)) -> Vector {
    let metres = |value: i32| i16::try_from(value).map_or(0.0, f32::from);
    Vector {
        x: metres(x),
        y: metres(y),
        z: 0.0,
    }
}

/// `osParcelSubdivide`: the box from `pos1` to `pos2` cut off the parcel at
/// its south-west corner as a parcel of its own — the whole four-metre blocks
/// it covers, which may be none — if the script's owner may divide and join
/// that parcel and every metre of the box lies on it; nothing otherwise, a box
/// reaching off the region past its corner among them.
///
/// # Errors
///
/// [`Exception::NoParcelAt`] for a south-west corner off the region.
pub fn os_parcel_subdivide(
    ctx: &mut ScriptCtx<'_>,
    pos1: Vector,
    pos2: Vector,
) -> Result<(), CallError> {
    let (start, end) = box_of(&pos1, &pos2);
    let Some(parcel) = parcel_at_point(ctx, start, BuiltinId::OsParcelSubdivide)? else {
        return Ok(());
    };
    if !edits_parcel(ctx, &parcel, POWER_LAND_DIVIDE_JOIN) {
        return Ok(());
    }
    if start.0 < end.0 && start.1 < end.1 {
        let caller = ctx.caller();
        let (width, depth) = ctx.host().region_size(caller);
        let past = |end: i32, size: u32| u32::try_from(end).map_or(true, |end| end > size);
        // OpenSim reads every metre of the box and gives up at one off the
        // region; one read per block it touches says the same of the land.
        if past(end.0, width) || past(end.1, depth) {
            return Ok(());
        }
        let block = |metre: i32| metre.div_euclid(4);
        for block_x in block(start.0)..=block(end.0.saturating_sub(1)) {
            for block_y in block(start.1)..=block(end.1.saturating_sub(1)) {
                let point = (
                    block_x.saturating_mul(4).max(start.0),
                    block_y.saturating_mul(4).max(start.1),
                );
                let here = parcel_at_point(ctx, point, BuiltinId::OsParcelSubdivide)?;
                if here.is_none_or(|here| here.id != parcel.id) {
                    return Ok(());
                }
            }
        }
    }
    let caller = ctx.caller();
    let at = point_vector(start);
    let _cut = ctx
        .host()
        .change_land(caller, &at, &LandChange::Subdivide(start, end));
    Ok(())
}

/// .NET's `Convert.ToInt32` of a string: an optionally signed decimal
/// integer with white space around it.
///
/// # Errors
///
/// [`Exception::FormatInvalid`] for anything else and
/// [`Exception::Overflow`] for a number past the integer's range.
fn convert_to_int32(text: &str) -> Result<i32, Exception> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return Err(Exception::FormatInvalid);
    }
    trimmed
        .strip_prefix('+')
        .unwrap_or(trimmed)
        .parse()
        .map_err(|_error| Exception::Overflow)
}

/// `osSetParcelDetails`: the parcel at `pos` given the details `rules` lists,
/// code then value — its name, description, owner, group, claim date, whether
/// those outside see its avatars and whether avatar sounds are heard on it —
/// where the script's owner may change its options; an owner or a claim date
/// only for an estate manager, a group the parcel's owner is a member of only
/// for the parcel's owner or an estate manager. Every refusal is shouted on
/// `DEBUG_CHANNEL`, a second's pause each, and the rest goes on.
///
/// # Errors
///
/// [`Exception::NoParcelAt`] for a position off the region, an
/// `InvalidCastException` for a code that is no integer, an
/// `IndexOutOfRangeException` for a code with no value and
/// `Convert.ToInt32`'s exceptions for a claim date or a switch that is no
/// integer.
pub fn os_set_parcel_details(
    ctx: &mut ScriptCtx<'_>,
    pos: Vector,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    set_parcel_details(ctx, &pos, &rules, BuiltinId::OsSetParcelDetails)
}

/// `osParcelSetDetails`: the deprecated [`os_set_parcel_details`].
///
/// # Errors
///
/// As [`os_set_parcel_details`].
pub fn os_parcel_set_details(
    ctx: &mut ScriptCtx<'_>,
    pos: Vector,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    set_parcel_details(ctx, &pos, &rules, BuiltinId::OsParcelSetDetails)
}

/// The two parcel detail setters' shared body (`SetParcelDetails`).
#[expect(
    clippy::too_many_lines,
    reason = "one rule per arm, as OpenSim's switch reads them"
)]
fn set_parcel_details(
    ctx: &mut ScriptCtx<'_>,
    pos: &Vector,
    rules: &[Element],
    id: BuiltinId,
) -> Result<(), CallError> {
    let thrown = |exception| CallError::Exception { id, exception };
    let point = (float_to_int(pos.x), float_to_int(pos.y));
    let Some(parcel) = parcel_at_point(ctx, point, id)? else {
        warn(ctx, "There is no land at that location".to_owned());
        return Ok(());
    };
    if !edits_parcel(ctx, &parcel, POWER_LAND_OPTIONS) {
        warn(
            ctx,
            "script owner does not have permission to modify the parcel".to_owned(),
        );
        return Ok(());
    }
    let caller = ctx.caller();
    let script_owner = ctx.host().owner(caller);
    let mut owner = parcel.owner.clone();
    let mut details = ParcelDetails::default();
    let mut index = 0_usize;
    while index < rules.len() {
        let code = rules
            .get(index)
            .ok_or(Exception::IndexOutOfRange)
            .and_then(opensim_integer_item)
            .map_err(thrown)?;
        let arg = rules
            .get(index.saturating_add(1))
            .map(|element| element_to_string_for(element, true))
            .ok_or_else(|| thrown(Exception::IndexOutOfRange))?;
        index = index.saturating_add(2);
        match code {
            PARCEL_DETAILS_NAME => {
                if parcel.name != arg {
                    details.name = Some(arg);
                }
            }
            PARCEL_DETAILS_DESC => {
                if parcel.description != arg {
                    details.description = Some(arg);
                }
            }
            PARCEL_DETAILS_OWNER => {
                if !estate_manager(ctx) {
                    warn(
                        ctx,
                        "script owner does not have permission to modify the parcel owner"
                            .to_owned(),
                    );
                } else if let Some(new) = canonical_uuid(&arg)
                    && !new.eq_ignore_ascii_case(&owner)
                {
                    owner.clone_from(&new);
                    details.owner = Some(new);
                    details.group = Some(NULL_KEY.to_owned());
                }
            }
            PARCEL_DETAILS_GROUP => {
                if script_owner.eq_ignore_ascii_case(&owner) || estate_manager(ctx) {
                    let current = details
                        .group
                        .clone()
                        .unwrap_or_else(|| parcel.group.clone());
                    if let Some(new) = canonical_uuid(&arg)
                        && !new.eq_ignore_ascii_case(&current)
                    {
                        if new == NULL_KEY {
                            details.group = Some(new);
                        } else if ctx.host().group_powers(&new, &owner).is_none() {
                            warn(
                                ctx,
                                "land owner is not member of the new group for parcel".to_owned(),
                            );
                        } else {
                            details.group = Some(new);
                        }
                    }
                } else {
                    warn(
                        ctx,
                        "script owner does not have permission to modify the parcel group"
                            .to_owned(),
                    );
                }
            }
            PARCEL_DETAILS_CLAIMDATE => {
                if estate_manager(ctx) {
                    let mut date = convert_to_int32(&arg).map_err(thrown)?;
                    if date == 0 {
                        date = i32::try_from(ctx.unix_micros().div_euclid(1_000_000))
                            .unwrap_or(i32::MAX);
                    }
                    let current = details.claim_date.unwrap_or(parcel.claim_date);
                    if current != date {
                        details.claim_date = Some(date);
                    }
                } else {
                    warn(
                        ctx,
                        "script owner does not have permission to modify the parcel CLAIM DATE"
                            .to_owned(),
                    );
                }
            }
            PARCEL_DETAILS_SEE_AVATARS => {
                let see = convert_to_int32(&arg).map_err(thrown)? != 0;
                if details.see_avatars.unwrap_or(parcel.see_avatars) != see {
                    details.see_avatars = Some(see);
                }
            }
            PARCEL_DETAILS_ANY_AVATAR_SOUNDS => {
                let heard = convert_to_int32(&arg).map_err(thrown)? != 0;
                if details.any_sounds.unwrap_or(parcel.sounds.0) != heard {
                    details.any_sounds = Some(heard);
                }
            }
            PARCEL_DETAILS_GROUP_SOUNDS => {
                let heard = convert_to_int32(&arg).map_err(thrown)? != 0;
                if details.group_sounds.unwrap_or(parcel.sounds.1) != heard {
                    details.group_sounds = Some(heard);
                }
            }
            _ => {}
        }
    }
    if details != ParcelDetails::default() {
        let at = point_vector(point);
        let _set = ctx
            .host()
            .change_land(caller, &at, &LandChange::Details(details));
    }
    Ok(())
}

/// `osSetParcelSIPAddress`: the voice address of the parcel under the prim,
/// for the parcel's owner.
///
/// # Errors
///
/// An OSSL run-time error for a script whose owner does not own the parcel,
/// and for a region with no voice module that takes a SIP address — the fake
/// grid's voice is WebRTC, which has none, as the local OpenSim runs no voice
/// module.
pub fn os_set_parcel_sip_address(
    ctx: &mut ScriptCtx<'_>,
    address: String,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let at = ctx.host().placement(caller).position;
    let Some(parcel) = ctx.host().parcel_at(caller, at.x, at.y) else {
        return Err(CallError::Exception {
            id: BuiltinId::OsSetParcelSIPAddress,
            exception: Exception::NullReference,
        });
    };
    let owner = ctx.host().owner(caller);
    if !parcel.owner.eq_ignore_ascii_case(&owner) {
        return Err(runtime_error(
            BuiltinId::OsSetParcelSIPAddress,
            "osSetParcelSIPAddress: Sorry, you need to own the land to use this function",
        ));
    }
    if !ctx.host().parcel_sip_address(caller, &address) {
        return Err(runtime_error(
            BuiltinId::OsSetParcelSIPAddress,
            "osSetParcelSIPAddress: No voice module enabled for this land",
        ));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// The region: statistics, notices and restarts.
// ----------------------------------------------------------------------

/// `osGetRegionStats`: the region's last statistics, OpenSim's 48 in its
/// `StatsIndex` order.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_region_stats(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .region_stats(caller)
        .into_iter()
        .map(Element::Float)
        .collect())
}

/// `osRegionNotice(agent, message)`: `message` and a newline as a non-modal
/// alert to `agent`, an avatar in the region and no NPC, for a script whose
/// owner may issue estate commands; nothing otherwise.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_region_notice_to(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    message: String,
) -> Result<(), CallError> {
    if !estate_command(ctx, false) {
        return Ok(());
    }
    let Some(agent) = canonical_uuid(&agent.0) else {
        return Ok(());
    };
    if !root_avatar(ctx, &agent) {
        return Ok(());
    }
    let caller = ctx.caller();
    let _sent = ctx.host().edit_region(
        caller,
        &RegionEdit::Notice {
            agent: Some(agent),
            text: format!("{message}\n"),
        },
    );
    Ok(())
}

/// Whether `agent` is an avatar in the region and no NPC.
fn root_avatar(ctx: &mut ScriptCtx<'_>, agent: &str) -> bool {
    let caller = ctx.caller();
    if ctx.host().agent(caller, agent).is_none() {
        return false;
    }
    !ctx.host()
        .npcs(caller)
        .iter()
        .any(|npc| npc.key.eq_ignore_ascii_case(agent))
}

/// `osRegionNotice(message)`: `message` and a newline as an alert to every
/// avatar in the region, for a script whose owner may issue estate commands.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_region_notice(ctx: &mut ScriptCtx<'_>, message: String) -> Result<(), CallError> {
    if !estate_command(ctx, false) {
        return Ok(());
    }
    let caller = ctx.caller();
    let _sent = ctx.host().edit_region(
        caller,
        &RegionEdit::Notice {
            agent: None,
            text: format!("{message}\n"),
        },
    );
    Ok(())
}

/// `osRegionRestart(seconds, message)`: a restart of the region in `seconds`,
/// announced with `message` in place of OpenSim's own; see
/// [`os_region_restart`].
///
/// # Errors
///
/// As [`os_region_restart`].
pub fn os_region_restart_with(
    ctx: &mut ScriptCtx<'_>,
    seconds: f32,
    message: String,
) -> Result<i32, CallError> {
    region_restart(ctx, seconds, &message, BuiltinId::OsRegionRestart)
}

/// `osRegionRestart(seconds)`: for a script whose owner may issue estate
/// commands, a restart of the region in `seconds`, its first notice to every
/// avatar now and the next ever closer — every two minutes above five
/// minutes, every minute above two, every half minute above one, then every
/// fifteen seconds — or under 15 seconds a scheduled restart called off; 1.
/// 0 for any other script.
///
/// # Errors
///
/// A `FormatException` for a message that is no .NET format of one item.
pub fn os_region_restart(ctx: &mut ScriptCtx<'_>, seconds: f32) -> Result<i32, CallError> {
    region_restart(ctx, seconds, "", BuiltinId::OsRegionRestart2)
}

/// The two restart overloads' shared body (`RegionRestart`, `ScheduleRestart`
/// and the first `DoOneNotice`).
fn region_restart(
    ctx: &mut ScriptCtx<'_>,
    seconds: f32,
    message: &str,
    id: BuiltinId,
) -> Result<i32, CallError> {
    if !estate_command(ctx, false) {
        return Ok(0);
    }
    let caller = ctx.caller();
    if seconds < RESTART_FLOOR {
        let _aborted = ctx.host().edit_region(caller, &RegionEdit::AbortRestart);
        return Ok(1);
    }
    let mut alerts = Vec::new();
    let mut left = f64::from(seconds);
    while left > 0.0 {
        alerts.push(crate::num::double_to_int(left));
        left -= if left > 300.0 {
            120.0
        } else if left > 120.0 {
            60.0
        } else if left > 60.0 {
            30.0
        } else {
            15.0
        };
    }
    let template = if message.is_empty() {
        let region = ctx.host().region(caller).name;
        format!(
            "Region: {region} is about to restart.\n\nIf you stay here you will be logged \
             out.\n\n\nTime remaining: {{0}}.\n"
        )
    } else {
        format!("{message}\n\nTime remaining: {{0}}.\n")
    };
    let first = alerts.first().copied().unwrap_or(0);
    let notice = dotnet_format(&template, &[Element::String(time_remaining(first))])
        .map_err(|exception| CallError::Exception { id, exception })?;
    let _scheduled = ctx
        .host()
        .edit_region(caller, &RegionEdit::Restart { alerts, notice });
    Ok(1)
}

/// `seconds` as a restart notice spells it: `2 minutes and 5 seconds`,
/// `1 minute`, `15 seconds`.
fn time_remaining(seconds: i32) -> String {
    let minutes = seconds / 60;
    let rest = seconds % 60;
    let mut text = String::new();
    if minutes > 0 {
        if minutes == 1 {
            text.push_str("1 minute");
        } else {
            let _written = write!(text, "{minutes} minutes");
        }
        if rest != 0 {
            text.push_str(" and ");
        }
    }
    if rest != 0 {
        if rest == 1 {
            text.push_str("1 second");
        } else {
            let _written = write!(text, "{rest} seconds");
        }
    }
    text
}

// ----------------------------------------------------------------------
// Environments.
// ----------------------------------------------------------------------

/// The asset a day cycle named `day_cycle` stands for: the key it is, else
/// the asset of the item of that name in the prim
/// (`ScriptUtils.GetAssetIdFromKeyOrItemName`); [`None`] for neither, or the
/// null key.
fn day_cycle_asset(ctx: &mut ScriptCtx<'_>, day_cycle: &str) -> Option<String> {
    let asset = match canonical_uuid(day_cycle) {
        Some(key) => key,
        None => {
            let caller = ctx.caller();
            ctx.host()
                .contents(caller)
                .into_iter()
                .find(|item| item.name == day_cycle)?
                .asset
        }
    };
    (asset != NULL_KEY).then_some(asset)
}

/// Whether `day_cycle` names none: empty, or the null key.
fn no_day_cycle(day_cycle: &str) -> bool {
    day_cycle.is_empty() || day_cycle == NULL_KEY
}

/// `osReplaceAgentEnvironment`: `agent`'s own environment — which overrides
/// the region's and every parcel's for it — made the region's with the day
/// cycle `day_cycle` (an item of the prim or an asset key), or dropped for an
/// empty or null one, blended into over `transition` seconds. 1 when done;
/// -2 when the region's `[OSSL]` settings deny it, -4 for a key that is none
/// or no avatar of the region, -3 for a day cycle that is not one, -5 for one
/// the grid cannot read, which drops the avatar's environment too.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_replace_agent_environment(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    transition: i32,
    day_cycle: String,
) -> Result<i32, CallError> {
    let standing = ctx.standing();
    if ctx
        .ossl()
        .refusal(
            "osReplaceAgentEnvironment",
            ThreatLevel::Moderate,
            &standing,
        )
        .is_some()
    {
        return Ok(-2);
    }
    let Some(agent) = canonical_uuid(&agent.0) else {
        return Ok(-4);
    };
    if !root_avatar(ctx, &agent) {
        return Ok(-4);
    }
    let caller = ctx.caller();
    let scope = EnvironmentScope::Agent(agent);
    if no_day_cycle(&day_cycle) {
        let _reset =
            ctx.host()
                .change_environment(caller, &scope, &EnvironmentChange::Reset, transition);
        return Ok(1);
    }
    let Some(asset) = day_cycle_asset(ctx, &day_cycle) else {
        return Ok(-3);
    };
    match ctx.host().day_cycle(&asset) {
        DayCycle::Missing | DayCycle::NotDayCycle => Ok(-3),
        DayCycle::Unreadable => {
            let _reset = ctx.host().change_environment(
                caller,
                &scope,
                &EnvironmentChange::Reset,
                transition,
            );
            Ok(-5)
        }
        DayCycle::DayCycle => {
            let _set = ctx.host().change_environment(
                caller,
                &scope,
                &EnvironmentChange::Set {
                    day_cycle: Some(asset),
                    day_length: None,
                    day_offset: None,
                    altitudes: None,
                },
                transition,
            );
            Ok(1)
        }
    }
}

/// The day cycle a parcel or region replacement reads: [`None`] when none is
/// named, else the asset, or the code OpenSim answers — -4 for no settings
/// asset, -5 for one that is no day cycle or cannot be read.
fn replacement_cycle(ctx: &mut ScriptCtx<'_>, day_cycle: &str) -> Result<Option<String>, i32> {
    if no_day_cycle(day_cycle) {
        return Ok(None);
    }
    let asset = day_cycle_asset(ctx, day_cycle).ok_or(-4)?;
    match ctx.host().day_cycle(&asset) {
        DayCycle::Missing => Err(-4),
        DayCycle::NotDayCycle | DayCycle::Unreadable => Err(-5),
        DayCycle::DayCycle => Ok(Some(asset)),
    }
}

/// `osReplaceParcelEnvironment`: the environment of the parcel under the prim
/// given the day cycle `day_cycle`, on the parcel's own environment or a copy
/// of the region's, blended into over `transition` seconds; with no day cycle
/// nothing changes. 1 when done; -1 when the estate allows parcels no
/// environment of their own, -2 off any parcel, -3 for a script whose owner
/// may not change the parcel's environment, -4 for no settings asset, -5 for
/// one that is no day cycle.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_replace_parcel_environment(
    ctx: &mut ScriptCtx<'_>,
    transition: i32,
    day_cycle: String,
) -> Result<i32, CallError> {
    let caller = ctx.caller();
    if !ctx.host().region(caller).environment_override {
        return Ok(-1);
    }
    let at = ctx.host().placement(caller).position;
    let Some(parcel) = ctx.host().parcel_at(caller, at.x, at.y) else {
        return Ok(-2);
    };
    if !edits_parcel(ctx, &parcel, POWER_ALLOW_ENVIRONMENT) {
        return Ok(-3);
    }
    let asset = match replacement_cycle(ctx, &day_cycle) {
        Ok(asset) => asset,
        Err(code) => return Ok(code),
    };
    if asset.is_some() {
        let _set = ctx.host().change_environment(
            caller,
            &EnvironmentScope::Parcel(at),
            &EnvironmentChange::Set {
                day_cycle: asset,
                day_length: None,
                day_offset: None,
                altitudes: None,
            },
            transition,
        );
    }
    Ok(1)
}

/// Whether two altitudes differ, as OpenSim's exact `!=` reads them (an
/// admitted altitude is positive, so no zero or NaN is compared).
fn differs(a: f32, b: f32) -> bool {
    a.total_cmp(&b).is_ne()
}

/// `osReplaceRegionEnvironment`: the region's environment given the day
/// cycle `day_cycle`, a day of `day_hours` hours (4–168), offset
/// `offset_hours` from midnight (−11.5–11.5, one at or below zero read a day
/// later), and its sky tracks starting at the three altitudes (each above 0
/// and below 4000 m), each where given, blended into over `transition`
/// seconds, for a script whose owner owns the estate or is a god. 1 when
/// done; -3 for any other script, -4 for no settings asset, -5 for one that
/// is no day cycle.
///
/// OpenSim's quirks are kept: an offset is weighed against the day's length
/// to decide whether anything changed, and the third altitude is admitted by
/// the second's ceiling.
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(
    clippy::too_many_arguments,
    reason = "the library's signature: OpenSim's seven parameters"
)]
pub fn os_replace_region_environment(
    ctx: &mut ScriptCtx<'_>,
    transition: i32,
    day_cycle: String,
    day_hours: f32,
    offset_hours: f32,
    altitude1: f32,
    altitude2: f32,
    altitude3: f32,
) -> Result<i32, CallError> {
    if !estate_command(ctx, true) {
        return Ok(-3);
    }
    let asset = match replacement_cycle(ctx, &day_cycle) {
        Ok(asset) => asset,
        Err(code) => return Ok(code),
    };
    let mut changed = asset.is_some();
    let caller = ctx.caller();
    let unix = ctx.unix_micros().div_euclid(1_000_000);
    let current = ctx.host().environment(caller, None, unix);
    let mut length = float_to_int(current.day_length);
    let mut day_length = None;
    let hours = f64::from(day_hours);
    if (4.0..=24.0 * 7.0).contains(&hours) {
        let new = crate::num::double_to_int(hours * 3600.0);
        changed |= length != new;
        length = new;
        day_length = Some(new);
    }
    let mut day_offset = None;
    let mut offset = f64::from(offset_hours);
    if (-11.5..=11.5).contains(&offset) {
        if offset <= 0.0 {
            offset += 24.0;
        }
        let new = crate::num::double_to_int(offset * 3600.0);
        changed |= length != new;
        day_offset = Some(new);
    }
    let mut altitudes = current.altitudes;
    let mut sort = false;
    if altitude1 > 0.0 && altitude1 < 4000.0 && differs(altitudes[0], altitude1) {
        altitudes[0] = altitude1;
        sort = true;
    }
    if altitude2 > 0.0 && altitude2 < 4000.0 && differs(altitudes[1], altitude2) {
        altitudes[1] = altitude2;
        sort = true;
    }
    if altitude3 > 0.0 && altitude2 < 4000.0 && differs(altitudes[2], altitude3) {
        altitudes[2] = altitude3;
        sort = true;
    }
    if sort {
        altitudes.sort_by(f32::total_cmp);
        changed = true;
    }
    if changed {
        let _set = ctx.host().change_environment(
            caller,
            &EnvironmentScope::Region,
            &EnvironmentChange::Set {
                day_cycle: asset,
                day_length,
                day_offset,
                altitudes: sort.then_some(altitudes),
            },
            transition,
        );
    }
    Ok(1)
}

/// `osResetEnvironment`: the environment of the parcel under the prim
/// (`parcel_or_region` above 0) or of the region dropped, blended out of over
/// `transition` seconds. 1 when done; for a parcel -1 when the estate allows
/// parcels no environment of their own, -2 off any parcel, -3 for a script
/// whose owner may not change its environment; for the region -3 for a
/// script whose owner does not own the estate and is no god.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_reset_environment(
    ctx: &mut ScriptCtx<'_>,
    parcel_or_region: i32,
    transition: i32,
) -> Result<i32, CallError> {
    let caller = ctx.caller();
    if parcel_or_region > 0 {
        if !ctx.host().region(caller).environment_override {
            return Ok(-1);
        }
        let at = ctx.host().placement(caller).position;
        let Some(parcel) = ctx.host().parcel_at(caller, at.x, at.y) else {
            return Ok(-2);
        };
        if !edits_parcel(ctx, &parcel, POWER_ALLOW_ENVIRONMENT) {
            return Ok(-3);
        }
        let _reset = ctx.host().change_environment(
            caller,
            &EnvironmentScope::Parcel(at),
            &EnvironmentChange::Reset,
            transition,
        );
        return Ok(1);
    }
    if !estate_command(ctx, true) {
        return Ok(-3);
    }
    let _reset = ctx.host().change_environment(
        caller,
        &EnvironmentScope::Region,
        &EnvironmentChange::Reset,
        transition,
    );
    Ok(1)
}

// ----------------------------------------------------------------------
// LightShare.
// ----------------------------------------------------------------------

/// `WL_WATER_COLOR`.
const WL_WATER_COLOR: i32 = 0;
/// `WL_WATER_FOG_DENSITY_EXPONENT`.
const WL_WATER_FOG_DENSITY_EXPONENT: i32 = 1;
/// `WL_UNDERWATER_FOG_MODIFIER`.
const WL_UNDERWATER_FOG_MODIFIER: i32 = 2;
/// `WL_REFLECTION_WAVELET_SCALE`.
const WL_REFLECTION_WAVELET_SCALE: i32 = 3;
/// `WL_FRESNEL_SCALE`.
const WL_FRESNEL_SCALE: i32 = 4;
/// `WL_FRESNEL_OFFSET`.
const WL_FRESNEL_OFFSET: i32 = 5;
/// `WL_REFRACT_SCALE_ABOVE`.
const WL_REFRACT_SCALE_ABOVE: i32 = 6;
/// `WL_REFRACT_SCALE_BELOW`.
const WL_REFRACT_SCALE_BELOW: i32 = 7;
/// `WL_BLUR_MULTIPLIER`.
const WL_BLUR_MULTIPLIER: i32 = 8;
/// `WL_BIG_WAVE_DIRECTION`.
const WL_BIG_WAVE_DIRECTION: i32 = 9;
/// `WL_LITTLE_WAVE_DIRECTION`.
const WL_LITTLE_WAVE_DIRECTION: i32 = 10;
/// `WL_NORMAL_MAP_TEXTURE`.
const WL_NORMAL_MAP_TEXTURE: i32 = 11;
/// `WL_HORIZON`.
const WL_HORIZON: i32 = 12;
/// `WL_HAZE_HORIZON`.
const WL_HAZE_HORIZON: i32 = 13;
/// `WL_BLUE_DENSITY`.
const WL_BLUE_DENSITY: i32 = 14;
/// `WL_HAZE_DENSITY`.
const WL_HAZE_DENSITY: i32 = 15;
/// `WL_DENSITY_MULTIPLIER`.
const WL_DENSITY_MULTIPLIER: i32 = 16;
/// `WL_DISTANCE_MULTIPLIER`.
const WL_DISTANCE_MULTIPLIER: i32 = 17;
/// `WL_MAX_ALTITUDE`.
const WL_MAX_ALTITUDE: i32 = 18;
/// `WL_SUN_MOON_COLOR`.
const WL_SUN_MOON_COLOR: i32 = 19;
/// `WL_AMBIENT`.
const WL_AMBIENT: i32 = 20;
/// `WL_EAST_ANGLE`.
const WL_EAST_ANGLE: i32 = 21;
/// `WL_SUN_GLOW_FOCUS`.
const WL_SUN_GLOW_FOCUS: i32 = 22;
/// `WL_SUN_GLOW_SIZE`.
const WL_SUN_GLOW_SIZE: i32 = 23;
/// `WL_SCENE_GAMMA`.
const WL_SCENE_GAMMA: i32 = 24;
/// `WL_STAR_BRIGHTNESS`.
const WL_STAR_BRIGHTNESS: i32 = 25;
/// `WL_CLOUD_COLOR`.
const WL_CLOUD_COLOR: i32 = 26;
/// `WL_CLOUD_XY_DENSITY`.
const WL_CLOUD_XY_DENSITY: i32 = 27;
/// `WL_CLOUD_COVERAGE`.
const WL_CLOUD_COVERAGE: i32 = 28;
/// `WL_CLOUD_SCALE`.
const WL_CLOUD_SCALE: i32 = 29;
/// `WL_CLOUD_DETAIL_XY_DENSITY`.
const WL_CLOUD_DETAIL_XY_DENSITY: i32 = 30;
/// `WL_CLOUD_SCROLL_X`.
const WL_CLOUD_SCROLL_X: i32 = 31;
/// `WL_CLOUD_SCROLL_Y`.
const WL_CLOUD_SCROLL_Y: i32 = 32;
/// `WL_CLOUD_SCROLL_Y_LOCK`.
const WL_CLOUD_SCROLL_Y_LOCK: i32 = 33;
/// `WL_CLOUD_SCROLL_X_LOCK`.
const WL_CLOUD_SCROLL_X_LOCK: i32 = 34;
/// `WL_DRAW_CLASSIC_CLOUDS`.
const WL_DRAW_CLASSIC_CLOUDS: i32 = 35;
/// `WL_SUN_MOON_POSITION`.
const WL_SUN_MOON_POSITION: i32 = 36;

/// A two-component direction as LightShare hands it out: a vector with no
/// height.
const fn flat((x, y): (f32, f32)) -> Element {
    Element::Vector(Vector { x, y, z: 0.0 })
}

/// What `lsGetWindlightScene` answers for `rule` of `scene`, or [`None`] for
/// a rule that is none.
fn scene_value(scene: &LightShare, rule: i32) -> Option<Element> {
    let float = Element::Float;
    let rotation = |value: &Rotation| Element::Rotation(value.clone());
    let vector = |value: &Vector| Element::Vector(value.clone());
    let switch = |value: bool| Element::Integer(i32::from(value));
    Some(match rule {
        WL_AMBIENT => rotation(&scene.ambient),
        WL_BIG_WAVE_DIRECTION => flat(scene.big_wave_direction),
        WL_BLUE_DENSITY => rotation(&scene.blue_density),
        WL_BLUR_MULTIPLIER => float(scene.blur_multiplier),
        WL_CLOUD_COLOR => rotation(&scene.cloud_color),
        WL_CLOUD_COVERAGE => float(scene.cloud_coverage),
        WL_CLOUD_DETAIL_XY_DENSITY => vector(&scene.cloud_detail_xy_density),
        WL_CLOUD_SCALE => float(scene.cloud_scale),
        WL_CLOUD_SCROLL_X => float(scene.cloud_scroll_x),
        WL_CLOUD_SCROLL_X_LOCK => switch(scene.cloud_scroll_x_lock),
        WL_CLOUD_SCROLL_Y => float(scene.cloud_scroll_y),
        WL_CLOUD_SCROLL_Y_LOCK => switch(scene.cloud_scroll_y_lock),
        WL_CLOUD_XY_DENSITY => vector(&scene.cloud_xy_density),
        WL_DENSITY_MULTIPLIER => float(scene.density_multiplier),
        WL_DISTANCE_MULTIPLIER => float(scene.distance_multiplier),
        WL_DRAW_CLASSIC_CLOUDS => switch(scene.draw_classic_clouds),
        WL_EAST_ANGLE => float(scene.east_angle),
        WL_FRESNEL_OFFSET => float(scene.fresnel_offset),
        WL_FRESNEL_SCALE => float(scene.fresnel_scale),
        WL_HAZE_DENSITY => float(scene.haze_density),
        WL_HAZE_HORIZON => float(scene.haze_horizon),
        WL_HORIZON => rotation(&scene.horizon),
        WL_LITTLE_WAVE_DIRECTION => flat(scene.little_wave_direction),
        WL_MAX_ALTITUDE => Element::Integer(i32::from(scene.max_altitude)),
        WL_NORMAL_MAP_TEXTURE => Element::Key(scene.normal_map_texture.clone()),
        WL_REFLECTION_WAVELET_SCALE => vector(&scene.reflection_wavelet_scale),
        WL_REFRACT_SCALE_ABOVE => float(scene.refract_scale_above),
        WL_REFRACT_SCALE_BELOW => float(scene.refract_scale_below),
        WL_SCENE_GAMMA => float(scene.scene_gamma),
        WL_STAR_BRIGHTNESS => float(scene.star_brightness),
        WL_SUN_GLOW_FOCUS => float(scene.sun_glow_focus),
        WL_SUN_GLOW_SIZE => float(scene.sun_glow_size),
        WL_SUN_MOON_COLOR => rotation(&scene.sun_moon_color),
        WL_SUN_MOON_POSITION => float(scene.sun_moon_position),
        WL_UNDERWATER_FOG_MODIFIER => float(scene.underwater_fog_modifier),
        WL_WATER_COLOR => vector(&scene.water_color),
        WL_WATER_FOG_DENSITY_EXPONENT => float(scene.water_fog_density_exponent),
        _ => return None,
    })
}

/// `lsGetWindlightScene`: each rule of `rules` that is one followed by its
/// value in the region's environment as LightShare reads it — OpenSim's
/// defaults where the region keeps none of its own. A rule that is none is
/// left out.
///
/// # Errors
///
/// An `InvalidCastException` for a rule that is no integer.
pub fn ls_get_windlight_scene(
    ctx: &mut ScriptCtx<'_>,
    rules: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    let scene = ctx.host().lightshare(caller);
    let mut values = Vec::new();
    for rule in &rules {
        let rule = opensim_integer_item(rule).map_err(|exception| CallError::Exception {
            id: BuiltinId::LsGetWindlightScene,
            exception,
        })?;
        if let Some(value) = scene_value(&scene, rule) {
            values.push(Element::Integer(rule));
            values.push(value);
        }
    }
    Ok(values)
}

/// Whether the script's owner may set the region's LightShare scene: an
/// estate manager, or a god with god powers on in the region; any other is
/// told on `DEBUG_CHANNEL`, in the words OpenSim uses for both functions.
fn sets_scene(ctx: &mut ScriptCtx<'_>) -> bool {
    if estate_manager(ctx) || ctx.standing().is(Allowed::ActiveGod) {
        return true;
    }
    ctx.shout_error(
        "lsSetWindlightScene can only be used by estate managers or owners.".to_owned(),
    );
    false
}

/// The words a LightShare value of the wrong type is refused with.
fn refusal(name: &str, index: usize, kind: &str) -> String {
    format!("Error running rule {name}: arg #{index} - parameter 1 must be {kind}")
}

/// `GetLSLFloatItem`: an integer or a float as it is, a string as OpenSim
/// casts one; anything else refused.
fn float_item(element: &Element) -> Option<f32> {
    match element {
        Element::Integer(integer) => Some(crate::num::int_to_float(*integer)),
        Element::Float(float) => Some(*float),
        Element::String(text) | Element::Key(text) => Some(crate::parse::opensim_float(text)),
        Element::Vector(_) | Element::Rotation(_) => None,
    }
}

/// `lsSetWindlightScene`: the region's environment made one of LightShare's
/// settings — the scene as it reads now, with each rule of `rules` set to the
/// value after it — and every avatar in the region told; 1. For a script
/// whose owner is no estate manager, or for a value of the wrong type, 0 and
/// a shout on `DEBUG_CHANNEL`, and nothing set. A rule that is none is
/// skipped alone, and its value read as the next rule.
///
/// # Errors
///
/// An `IndexOutOfRangeException` for a rule with no value, and a
/// `FormatException` for a normal map that is no key.
pub fn ls_set_windlight_scene(
    ctx: &mut ScriptCtx<'_>,
    rules: Vec<Element>,
) -> Result<i32, CallError> {
    if !sets_scene(ctx) {
        return Ok(0);
    }
    let caller = ctx.caller();
    let mut scene = ctx.host().lightshare(caller);
    let refused = read_scene(&mut scene, &rules).map_err(|exception| CallError::Exception {
        id: BuiltinId::LsSetWindlightScene,
        exception,
    })?;
    if let Some(text) = refused {
        ctx.shout_error(text);
        return Ok(0);
    }
    let _set = ctx
        .host()
        .edit_region(caller, &RegionEdit::LightShare(Box::new(scene)));
    Ok(1)
}

/// Reads `rules` into `scene` (`getWindlightProfileFromRules`): [`None`]
/// when every value read, else the words a value of the wrong type is
/// refused with.
///
/// # Errors
///
/// [`Exception::IndexOutOfRange`] for a rule with no value and
/// [`Exception::InvalidUuid`] for a normal map that is no key.
fn read_scene(scene: &mut LightShare, rules: &[Element]) -> Result<Option<String>, Exception> {
    let mut index = 0_usize;
    while let Some(rule) = rules.get(index) {
        let Ok(rule) = opensim_integer_item(rule) else {
            return Ok(Some(format!(
                "Error running rule type: arg #{index} - parameter type must be integer"
            )));
        };
        // A rule the switch does not name moves on by itself alone.
        let Some(name) = rule_name(rule) else {
            index = index.saturating_add(1);
            continue;
        };
        index = index.saturating_add(1);
        let value = rules.get(index).ok_or(Exception::IndexOutOfRange)?;
        if rule == WL_NORMAL_MAP_TEXTURE {
            let text = element_to_string_for(value, true);
            scene.normal_map_texture = canonical_uuid(&text).ok_or(Exception::InvalidUuid)?;
        } else if let Err(text) = set_rule(scene, rule, name, index, value) {
            return Ok(Some(text));
        }
        index = index.saturating_add(1);
    }
    Ok(None)
}

/// Sets `scene`'s rule `rule`, called `name`, to `value`, the `index`th
/// element of the rules.
///
/// # Errors
///
/// The words a value of the wrong type is refused with.
fn set_rule(
    scene: &mut LightShare,
    rule: i32,
    name: &str,
    index: usize,
    value: &Element,
) -> Result<(), String> {
    let float = || float_item(value).ok_or_else(|| refusal(name, index, "float"));
    let vector = || match value {
        Element::Vector(vector) => Ok(vector.clone()),
        _ => Err(refusal(name, index, "vector")),
    };
    let rotation = || match value {
        Element::Rotation(rotation) => Ok(rotation.clone()),
        _ => Err(refusal(name, index, "rotation")),
    };
    let integer = |shown: &str| {
        opensim_integer_item(value).map_err(|_exception| refusal(shown, index, "integer"))
    };
    match rule {
        WL_SUN_MOON_POSITION => scene.sun_moon_position = float()?,
        WL_AMBIENT => scene.ambient = rotation()?,
        WL_BIG_WAVE_DIRECTION => {
            let direction = vector()?;
            scene.big_wave_direction = (direction.x, direction.y);
        }
        WL_BLUE_DENSITY => scene.blue_density = rotation()?,
        WL_BLUR_MULTIPLIER => scene.blur_multiplier = float()?,
        WL_CLOUD_COLOR => scene.cloud_color = rotation()?,
        WL_CLOUD_COVERAGE => scene.cloud_coverage = float()?,
        WL_CLOUD_DETAIL_XY_DENSITY => scene.cloud_detail_xy_density = vector()?,
        WL_CLOUD_SCALE => scene.cloud_scale = float()?,
        WL_CLOUD_SCROLL_X => scene.cloud_scroll_x = float()?,
        // OpenSim's scroll-X lock names the Y lock in its refusal.
        WL_CLOUD_SCROLL_X_LOCK | WL_CLOUD_SCROLL_Y_LOCK => {
            let locked = integer("WL_CLOUD_SCROLL_Y_LOCK")? == 1;
            if rule == WL_CLOUD_SCROLL_X_LOCK {
                scene.cloud_scroll_x_lock = locked;
            } else {
                scene.cloud_scroll_y_lock = locked;
            }
        }
        WL_CLOUD_SCROLL_Y => scene.cloud_scroll_y = float()?,
        WL_CLOUD_XY_DENSITY => scene.cloud_xy_density = vector()?,
        WL_DENSITY_MULTIPLIER => scene.density_multiplier = float()?,
        WL_DISTANCE_MULTIPLIER => scene.distance_multiplier = float()?,
        WL_DRAW_CLASSIC_CLOUDS => scene.draw_classic_clouds = integer(name)? == 1,
        WL_EAST_ANGLE => scene.east_angle = float()?,
        WL_FRESNEL_OFFSET => scene.fresnel_offset = float()?,
        WL_FRESNEL_SCALE => scene.fresnel_scale = float()?,
        WL_HAZE_DENSITY => scene.haze_density = float()?,
        WL_HAZE_HORIZON => scene.haze_horizon = float()?,
        WL_HORIZON => scene.horizon = rotation()?,
        WL_LITTLE_WAVE_DIRECTION => {
            let direction = vector()?;
            scene.little_wave_direction = (direction.x, direction.y);
        }
        // `(ushort)` keeps the low sixteen bits.
        WL_MAX_ALTITUDE => {
            scene.max_altitude = u16::try_from(integer(name)? & 0xFFFF).unwrap_or(0);
        }
        WL_REFLECTION_WAVELET_SCALE => scene.reflection_wavelet_scale = vector()?,
        WL_REFRACT_SCALE_ABOVE => scene.refract_scale_above = float()?,
        WL_REFRACT_SCALE_BELOW => scene.refract_scale_below = float()?,
        WL_SCENE_GAMMA => scene.scene_gamma = float()?,
        WL_STAR_BRIGHTNESS => scene.star_brightness = float()?,
        WL_SUN_GLOW_FOCUS => scene.sun_glow_focus = float()?,
        WL_SUN_GLOW_SIZE => scene.sun_glow_size = float()?,
        WL_SUN_MOON_COLOR => scene.sun_moon_color = rotation()?,
        WL_UNDERWATER_FOG_MODIFIER => scene.underwater_fog_modifier = float()?,
        WL_WATER_COLOR => scene.water_color = vector()?,
        WL_WATER_FOG_DENSITY_EXPONENT => scene.water_fog_density_exponent = float()?,
        _ => {}
    }
    Ok(())
}

/// The name a LightShare rule goes by in a refusal, or [`None`] for a rule
/// OpenSim's setter does not name.
const fn rule_name(rule: i32) -> Option<&'static str> {
    Some(match rule {
        WL_WATER_COLOR => "WL_WATER_COLOR",
        WL_WATER_FOG_DENSITY_EXPONENT => "WL_WATER_FOG_DENSITY_EXPONENT",
        WL_UNDERWATER_FOG_MODIFIER => "WL_UNDERWATER_FOG_MODIFIER",
        WL_REFLECTION_WAVELET_SCALE => "WL_REFLECTION_WAVELET_SCALE",
        WL_FRESNEL_SCALE => "WL_FRESNEL_SCALE",
        WL_FRESNEL_OFFSET => "WL_FRESNEL_OFFSET",
        WL_REFRACT_SCALE_ABOVE => "WL_REFRACT_SCALE_ABOVE",
        WL_REFRACT_SCALE_BELOW => "WL_REFRACT_SCALE_BELOW",
        WL_BLUR_MULTIPLIER => "WL_BLUR_MULTIPLIER",
        WL_BIG_WAVE_DIRECTION => "WL_BIG_WAVE_DIRECTION",
        WL_LITTLE_WAVE_DIRECTION => "WL_LITTLE_WAVE_DIRECTION",
        WL_NORMAL_MAP_TEXTURE => "WL_NORMAL_MAP_TEXTURE",
        WL_HORIZON => "WL_HORIZON",
        WL_HAZE_HORIZON => "WL_HAZE_HORIZON",
        WL_BLUE_DENSITY => "WL_BLUE_DENSITY",
        WL_HAZE_DENSITY => "WL_HAZE_DENSITY",
        WL_DENSITY_MULTIPLIER => "WL_DENSITY_MULTIPLIER",
        WL_DISTANCE_MULTIPLIER => "WL_DISTANCE_MULTIPLIER",
        WL_MAX_ALTITUDE => "WL_MAX_ALTITUDE",
        WL_SUN_MOON_COLOR => "WL_SUN_MOON_COLOR",
        WL_AMBIENT => "WL_AMBIENT",
        WL_EAST_ANGLE => "WL_EAST_ANGLE",
        WL_SUN_GLOW_FOCUS => "WL_SUN_GLOW_FOCUS",
        WL_SUN_GLOW_SIZE => "WL_SUN_GLOW_SIZE",
        WL_SCENE_GAMMA => "WL_SCENE_GAMMA",
        WL_STAR_BRIGHTNESS => "WL_STAR_BRIGHTNESS",
        WL_CLOUD_COLOR => "WL_CLOUD_COLOR",
        WL_CLOUD_XY_DENSITY => "WL_CLOUD_XY_DENSITY",
        WL_CLOUD_COVERAGE => "WL_CLOUD_COVERAGE",
        WL_CLOUD_SCALE => "WL_CLOUD_SCALE",
        WL_CLOUD_DETAIL_XY_DENSITY => "WL_CLOUD_DETAIL_XY_DENSITY",
        WL_CLOUD_SCROLL_X => "WL_CLOUD_SCROLL_X",
        WL_CLOUD_SCROLL_Y => "WL_CLOUD_SCROLL_Y",
        WL_CLOUD_SCROLL_Y_LOCK => "WL_CLOUD_SCROLL_Y_LOCK",
        WL_CLOUD_SCROLL_X_LOCK => "WL_CLOUD_SCROLL_X_LOCK",
        WL_DRAW_CLASSIC_CLOUDS => "WL_DRAW_CLASSIC_CLOUDS",
        WL_SUN_MOON_POSITION => "WL_SUN_MOON_POSITION",
        _ => return None,
    })
}

/// `lsClearWindlightScene`: the region's own environment dropped, for a
/// script whose owner is an estate manager; any other is told on
/// `DEBUG_CHANNEL`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ls_clear_windlight_scene(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    if !sets_scene(ctx) {
        return Ok(());
    }
    let caller = ctx.caller();
    let _cleared = ctx.host().edit_region(caller, &RegionEdit::ClearLightShare);
    Ok(())
}

/// `lsSetWindlightSceneTargeted`: 0, and `Function temporary not supported`
/// shouted on `DEBUG_CHANNEL` — OpenSim disabled its body.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ls_set_windlight_scene_targeted(
    ctx: &mut ScriptCtx<'_>,
    _rules: Vec<Element>,
    _target: Key,
) -> Result<i32, CallError> {
    ctx.shout_error("Function temporary not supported".to_owned());
    Ok(0)
}
