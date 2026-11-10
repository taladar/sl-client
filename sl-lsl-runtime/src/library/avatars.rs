//! Library tranche: avatars (`server-lsl-lib-avatar-control`) — what a script
//! reads of an avatar (its size, state, language and names, over the
//! `dataserver` too), the animations it plays on one and the object's own, the
//! animation overrides, the controls it takes, the camera it sets and reads,
//! the teleports it sends one on, and what one wears.
//!
//! Each function checks the permission its call needs (`ScriptCtx::permissions`)
//! and says Second Life's complaint when it is missing; the host keeps the
//! avatars and sends their viewers what was decided. The rules are measured by
//! the `avatar/*` probes (`book/src/gridspec/lsl.md` § Animations, sitting,
//! controls and camera).

use sl_types::lsl::{Rotation, Vector};

use crate::library::{CallError, Key};
use crate::value::{Element, NULL_KEY, ZERO_ROTATION, ZERO_VECTOR};
use crate::vm::ScriptCtx;
use crate::vm::avatars::{self, Destination};
use crate::vm::contents::INVENTORY_ANIMATION;
use crate::vm::permissions::{
    PERMISSION_CONTROL_CAMERA, PERMISSION_OVERRIDE_ANIMATIONS, PERMISSION_TAKE_CONTROLS,
    PERMISSION_TELEPORT, PERMISSION_TRACK_CAMERA, PERMISSION_TRIGGER_ANIMATION,
};

/// `DATA_ONLINE`.
const DATA_ONLINE: i32 = 1;
/// `DATA_NAME`.
const DATA_NAME: i32 = 2;
/// `DATA_BORN`.
const DATA_BORN: i32 = 3;
/// `DATA_RATING`.
const DATA_RATING: i32 = 4;
/// `DATA_PAYINFO`.
const DATA_PAYINFO: i32 = 8;

/// OpenSim's `DATA_USERLEVEL`, which has no constant.
const DATA_USERLEVEL: i32 = 7;

/// Second Life's complaint for an animation call without its permission.
const NO_ANIMATION_PERMISSION: &str =
    "Script trying to trigger animations but PERMISSION_TRIGGER_ANIMATION permission not set";
/// Second Life's complaint for `llTakeControls` without its permission.
const NO_CONTROLS_PERMISSION: &str =
    "Script trying to take controls from owner but PERMISSION_TAKE_CONTROLS permission not set!";
/// Second Life's complaint for `llSetCameraParams` without its permission.
const NO_CAMERA_PERMISSION: &str =
    "Script trying to set camera parameters but PERMISSION_CONTROL_CAMERA permission not set!";
/// Second Life's complaint for a teleport by a script that holds no
/// `PERMISSION_TELEPORT` (`experience/powers`).
const NO_TELEPORT_PERMISSION: &str = "Script trying to teleport agent without current permission!";
/// `AGENT_SITTING`: OpenSim teleports no seated avatar.
const AGENT_SITTING: i32 = 0x10;

/// The avatar who granted the script `bits`, if it holds them all.
fn granted(ctx: &ScriptCtx<'_>, bits: i32) -> Option<String> {
    let permissions = ctx.permissions();
    permissions
        .holds(bits)
        .then(|| permissions.granter().map(str::to_owned))
        .flatten()
}

/// Say Second Life's `complaint` on `DEBUG_CHANNEL`; OpenSim says nothing.
fn complain(ctx: &mut ScriptCtx<'_>, complaint: &str) {
    if !ctx.flavour().is_opensim() {
        ctx.refuse(complaint.to_owned());
    }
}

/// The asset of the animation `name` a script names: an animation in its
/// prim's contents, or a built-in one by its name or key.
fn animation_asset(ctx: &mut ScriptCtx<'_>, name: &str) -> Option<String> {
    let caller = ctx.caller();
    if let Some(item) = ctx
        .host()
        .contents(caller)
        .into_iter()
        .find(|item| item.kind == INVENTORY_ANIMATION && item.name == name)
    {
        return Some(item.asset);
    }
    if let Some(builtin) = sl_anim::registry::builtin_animation_by_name(name) {
        return Some(builtin.id.to_string());
    }
    crate::library::dotnet::is_uuid(name).then(|| name.to_ascii_lowercase())
}

/// The avatar `agent` if it is in the region.
fn agent_of(ctx: &mut ScriptCtx<'_>, agent: &Key) -> Option<avatars::Agent> {
    let caller = ctx.caller();
    ctx.host().agent(caller, &agent.0)
}

/// `llGetAgentSize`: the avatar's size, `ZERO_VECTOR` when it is not in the
/// region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_agent_size(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<Vector, CallError> {
    Ok(agent_of(ctx, &agent).map_or(ZERO_VECTOR, |agent| agent.size))
}

/// `llGetAgentInfo`: the avatar's `AGENT_*` bits, 0 when it is not in the
/// region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_agent_info(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<i32, CallError> {
    Ok(agent_of(ctx, &agent).map_or(0, |agent| agent.info))
}

/// `llGetAgentLanguage`: the language the avatar's viewer reports.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_agent_language(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<String, CallError> {
    Ok(agent_of(ctx, &agent)
        .map(|agent| agent.language)
        .unwrap_or_default())
}

/// `llKey2Name`: an avatar's legacy name or an object's name, of what is in
/// the region; empty for anything else.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_key2_name(ctx: &mut ScriptCtx<'_>, id: Key) -> Result<String, CallError> {
    if let Some(agent) = agent_of(ctx, &id) {
        return Ok(agent.legacy_name);
    }
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .object_details(caller, &id.0)
        .map(|details| details.name)
        .unwrap_or_default())
}

/// `llGetDisplayName`: the display name of an avatar in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_display_name(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<String, CallError> {
    Ok(agent_of(ctx, &agent)
        .map(|agent| agent.display_name)
        .unwrap_or_default())
}

/// `llGetUsername`: the user name of an avatar in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_username(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<String, CallError> {
    Ok(agent_of(ctx, &agent)
        .map(|agent| agent.username)
        .unwrap_or_default())
}

/// `llName2Key`: the key of the avatar in the region with the legacy or user
/// name `name`, or `NULL_KEY`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_name2_key(ctx: &mut ScriptCtx<'_>, name: String) -> Result<Key, CallError> {
    let caller = ctx.caller();
    Ok(Key(ctx
        .host()
        .agent_named(caller, &name)
        .filter(|(_, here)| *here)
        .map_or_else(|| NULL_KEY.to_owned(), |(key, _)| key)))
}

/// `llRequestAgentData`: one fact of any avatar of the grid, over
/// `dataserver` — online, name, birth date, rating (always zeroes) and payment
/// information; nothing for a key no account has or a fact that is none.
/// OpenSim answers a fact it does not know (`DATA_USERLEVEL`, 7, aside) with
/// an empty key at once, before its sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_agent_data(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    data: i32,
) -> Result<Key, CallError> {
    if ctx.flavour().is_opensim()
        && !matches!(
            data,
            DATA_ONLINE | DATA_NAME | DATA_BORN | DATA_RATING | DATA_USERLEVEL | DATA_PAYINFO
        )
    {
        ctx.delay(0.0);
        return Ok(Key(String::new()));
    }
    let caller = ctx.caller();
    let record = ctx.host().agent_record(caller, &agent.0);
    let id = ctx.fresh_key();
    let answer = record.and_then(|record| match data {
        DATA_ONLINE => Some(i32::from(record.online).to_string()),
        DATA_NAME => Some(record.legacy_name),
        DATA_BORN => Some(record.born),
        DATA_RATING => Some("0,0,0,0,0,0".to_owned()),
        DATA_PAYINFO => Some(record.payment.to_string()),
        _ => None,
    });
    if let Some(answer) = answer {
        ctx.raise_dataserver(id.clone(), answer);
    }
    Ok(Key(id))
}

/// `llRequestDisplayName`: an avatar's display name over `dataserver`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_display_name(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let record = ctx.host().agent_record(caller, &agent.0);
    let id = ctx.fresh_key();
    if let Some(record) = record {
        ctx.raise_dataserver(id.clone(), record.display_name);
    }
    Ok(Key(id))
}

/// `llRequestUsername`: an avatar's user name over `dataserver`. OpenSim
/// answers an avatar in the region at once, without its sleep, and a key that
/// is none or `NULL_KEY` with an empty key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_username(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<Key, CallError> {
    let caller = ctx.caller();
    if ctx.flavour().is_opensim() {
        if !crate::library::dotnet::is_uuid(&agent.0) || agent.0 == NULL_KEY {
            ctx.delay(0.0);
            return Ok(Key(String::new()));
        }
        if ctx.host().agent(caller, &agent.0).is_some() {
            ctx.delay(0.0);
        }
    }
    let record = ctx.host().agent_record(caller, &agent.0);
    let id = ctx.fresh_key();
    if let Some(record) = record {
        ctx.raise_dataserver(id.clone(), record.username);
    }
    Ok(Key(id))
}

/// `llRequestUserKey`: the key of the avatar of the grid with the user name
/// `name`, over `dataserver`; `NULL_KEY` for none. OpenSim returns `NULL_KEY`
/// for a name of fewer than two parts without asking, and answers an avatar
/// in the region at once; neither takes its sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_user_key(ctx: &mut ScriptCtx<'_>, name: String) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let opensim = ctx.flavour().is_opensim();
    if opensim
        && name
            .split([' ', '.'])
            .filter(|part| !part.trim().is_empty())
            .count()
            < 2
    {
        ctx.delay(0.0);
        return Ok(Key(NULL_KEY.to_owned()));
    }
    let found = ctx.host().agent_named(caller, &name);
    if opensim && found.as_ref().is_some_and(|(_, here)| *here) {
        ctx.delay(0.0);
    }
    let id = ctx.fresh_key();
    ctx.raise_dataserver(
        id.clone(),
        found.map_or_else(|| NULL_KEY.to_owned(), |(key, _)| key),
    );
    Ok(Key(id))
}

/// Start or stop the animation `name` on the avatar that granted
/// `PERMISSION_TRIGGER_ANIMATION`; Second Life complains of a missing
/// permission or animation, OpenSim is silent.
fn animate(ctx: &mut ScriptCtx<'_>, name: &str, start: bool) {
    let Some(agent) = granted(ctx, PERMISSION_TRIGGER_ANIMATION) else {
        complain(ctx, NO_ANIMATION_PERMISSION);
        return;
    };
    let Some(asset) = animation_asset(ctx, name) else {
        complain(ctx, &format!("Could not find animation '{name}'"));
        return;
    };
    let caller = ctx.caller();
    ctx.host().animate(caller, &agent, &asset, start);
}

/// `llStartAnimation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_start_animation(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    animate(ctx, &name, true);
    Ok(())
}

/// `llStopAnimation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_animation(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    animate(ctx, &name, false);
    Ok(())
}

/// `llGetAnimation`: the avatar's movement state, empty when it is not in
/// the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_animation(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<String, CallError> {
    Ok(agent_of(ctx, &agent)
        .map(|agent| agent.state)
        .unwrap_or_default())
}

/// `llGetAnimationList`: the keys of the animations playing on the avatar.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_animation_list(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
) -> Result<Vec<Element>, CallError> {
    Ok(agent_of(ctx, &agent)
        .map(|agent| agent.animations.into_iter().map(Element::Key).collect())
        .unwrap_or_default())
}

/// `llSetAnimationOverride`: play `animation` for the granting avatar's
/// movement state `state`, with `PERMISSION_OVERRIDE_ANIMATIONS`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_animation_override(
    ctx: &mut ScriptCtx<'_>,
    state: String,
    animation: String,
) -> Result<(), CallError> {
    let Some(agent) = granted(ctx, PERMISSION_OVERRIDE_ANIMATIONS) else {
        return Ok(());
    };
    let Some(asset) = animation_asset(ctx, &animation) else {
        complain(ctx, &format!("Could not find animation '{animation}'"));
        return Ok(());
    };
    let caller = ctx.caller();
    ctx.host()
        .set_animation_override(caller, &agent, &state, Some((&animation, &asset)));
    Ok(())
}

/// `llResetAnimationOverride`: `state` — or with `ALL` every state — plays
/// as it does by default again.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_reset_animation_override(
    ctx: &mut ScriptCtx<'_>,
    state: String,
) -> Result<(), CallError> {
    let Some(agent) = granted(ctx, PERMISSION_OVERRIDE_ANIMATIONS) else {
        return Ok(());
    };
    let caller = ctx.caller();
    if state == "ALL" {
        for every in avatars::OVERRIDE_STATES {
            ctx.host()
                .set_animation_override(caller, &agent, every, None);
        }
    } else {
        ctx.host()
            .set_animation_override(caller, &agent, &state, None);
    }
    Ok(())
}

/// `llGetAnimationOverride`: what plays for the granting avatar's movement
/// state `state` — the override's name, or with none Second Life's default
/// animation's name (`stand`) and OpenSim's the state's own (`Standing`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_animation_override(
    ctx: &mut ScriptCtx<'_>,
    state: String,
) -> Result<String, CallError> {
    let Some(agent) = granted(ctx, PERMISSION_OVERRIDE_ANIMATIONS)
        .or_else(|| granted(ctx, PERMISSION_TRIGGER_ANIMATION))
    else {
        return Ok(String::new());
    };
    let caller = ctx.caller();
    let opensim = ctx.flavour().is_opensim();
    Ok(ctx
        .host()
        .animation_override(caller, &agent, &state)
        .unwrap_or_else(|| {
            if opensim {
                state.clone()
            } else {
                avatars::default_animation(&state).to_owned()
            }
        }))
}

/// Start or stop the object animation `name`, which must be an animation of
/// the prim's contents.
fn object_animate(ctx: &mut ScriptCtx<'_>, name: &str, start: bool) {
    let caller = ctx.caller();
    let Some(asset) = ctx
        .host()
        .contents(caller)
        .into_iter()
        .find(|item| item.kind == INVENTORY_ANIMATION && item.name == name)
        .map(|item| item.asset)
    else {
        complain(ctx, &format!("Could not find animation '{name}'"));
        return;
    };
    ctx.host().object_animate(caller, name, &asset, start);
}

/// `llStartObjectAnimation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_start_object_animation(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    object_animate(ctx, &name, true);
    Ok(())
}

/// `llStopObjectAnimation`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_object_animation(ctx: &mut ScriptCtx<'_>, name: String) -> Result<(), CallError> {
    object_animate(ctx, &name, false);
    Ok(())
}

/// `llGetObjectAnimationNames`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_object_animation_names(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .object_animations(caller)
        .into_iter()
        .map(Element::String)
        .collect())
}

/// `llTakeControls`: route the granting avatar's `controls` to the script's
/// `control` event, with `PERMISSION_TAKE_CONTROLS`; the viewer is sent the
/// flavour's `ScriptControlChange`s.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_take_controls(
    ctx: &mut ScriptCtx<'_>,
    controls: i32,
    accept: i32,
    pass_on: i32,
) -> Result<(), CallError> {
    let Some(agent) = granted(ctx, PERMISSION_TAKE_CONTROLS) else {
        complain(ctx, NO_CONTROLS_PERMISSION);
        // Second Life says it twice to a script that was granted by somebody
        // and no longer holds the bit — after `llReleaseControls`
        // (`avatar/controls`) — and once to one never granted
        // (`avatar/permissions`).
        if ctx.permissions().granter().is_some() {
            complain(ctx, NO_CONTROLS_PERMISSION);
        }
        return Ok(());
    };
    let flavour = ctx.flavour();
    let before = ctx.controls().as_ref().map(|taken| taken.held);
    let (changes, held) = avatars::take(flavour, before, controls, accept != 0, pass_on != 0);
    // Not accepting them, the script hears none of the controls.
    let taken = if accept != 0 { controls } else { 0 };
    *ctx.controls() = Some(crate::vm::TakenControls {
        agent: agent.clone(),
        held,
        events: avatars::Controls {
            taken,
            ..avatars::Controls::default()
        },
    });
    let caller = ctx.caller();
    ctx.host()
        .control_changes(caller, &agent, &changes, Some(taken));
    Ok(())
}

/// `llReleaseControls`: give the controls back and drop
/// `PERMISSION_TAKE_CONTROLS`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_release_controls(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let flavour = ctx.flavour();
    if let Some(taken) = ctx.controls().take() {
        let changes = avatars::release(flavour, taken.held);
        let caller = ctx.caller();
        ctx.host()
            .control_changes(caller, &taken.agent, &changes, None);
    }
    ctx.revoke_permissions(PERMISSION_TAKE_CONTROLS);
    Ok(())
}

/// The value of a camera rule's argument: a number, or a vector's three.
const fn camera_value(value: Option<&Element>) -> (f32, f32, f32) {
    match value {
        Some(Element::Integer(value)) => (crate::num::int_to_float(*value), 0.0, 0.0),
        Some(Element::Float(value)) => (*value, 0.0, 0.0),
        Some(Element::Vector(vector)) => (vector.x, vector.y, vector.z),
        _ => (0.0, 0.0, 0.0),
    }
}

/// `llSetCameraParams`: the granting avatar's follow camera, with
/// `PERMISSION_CONTROL_CAMERA`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_camera_params(ctx: &mut ScriptCtx<'_>, rules: Vec<Element>) -> Result<(), CallError> {
    let Some(agent) = granted(ctx, PERMISSION_CONTROL_CAMERA) else {
        complain(ctx, NO_CAMERA_PERMISSION);
        return Ok(());
    };
    let read: Vec<(i32, f32, f32, f32)> = rules
        .chunks(2)
        .filter_map(|pair| match pair {
            [Element::Integer(rule), value] => {
                let (x, y, z) = camera_value(Some(value));
                Some((*rule, x, y, z))
            }
            _ => None,
        })
        .collect();
    let sent = avatars::camera_rules(ctx.flavour(), &read);
    let caller = ctx.caller();
    ctx.host().set_camera(caller, &agent, &sent);
    Ok(())
}

/// `llClearCameraParams`: the granting avatar's follow camera cleared.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_clear_camera_params(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let Some(agent) = granted(ctx, PERMISSION_CONTROL_CAMERA) else {
        return Ok(());
    };
    let caller = ctx.caller();
    ctx.host().clear_camera(caller, &agent);
    Ok(())
}

/// The granting avatar's camera, with `PERMISSION_TRACK_CAMERA`.
fn camera(ctx: &mut ScriptCtx<'_>) -> Option<(Vector, Rotation)> {
    let agent = granted(ctx, PERMISSION_TRACK_CAMERA)?;
    let caller = ctx.caller();
    ctx.host().camera(caller, &agent)
}

/// `llGetCameraPos`: `ZERO_VECTOR` without the permission.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_camera_pos(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    Ok(camera(ctx).map_or(ZERO_VECTOR, |(position, _)| position))
}

/// `llGetCameraRot`: `ZERO_ROTATION` without the permission.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_camera_rot(ctx: &mut ScriptCtx<'_>) -> Result<Rotation, CallError> {
    Ok(camera(ctx).map_or(ZERO_ROTATION, |(_, rotation)| rotation))
}

/// Whether the script may teleport `agent` (`experience/powers`).
///
/// Second Life: the agent granted `PERMISSION_TELEPORT` and, outside an
/// experience, owns the object; a script that holds no `PERMISSION_TELEPORT`
/// at all is told so on `DEBUG_CHANNEL`, one that holds another avatar's
/// grant is not.
///
/// OpenSim (`LSL_Api.llTeleportAgent`): an agent in the region and not
/// seated, who granted the permission — or, with none, the owner wearing the
/// object, or anyone when the object and the agent both stand on the
/// object's owner's land. `llTeleportAgentGlobalCoords` (`global`) is the
/// owner's alone, and only by the grant.
fn may_teleport(ctx: &mut ScriptCtx<'_>, agent: &str, global: bool) -> bool {
    let caller = ctx.caller();
    let grant = granted(ctx, PERMISSION_TELEPORT);
    if !ctx.flavour().is_opensim() {
        let Some(granter) = grant else {
            complain(ctx, NO_TELEPORT_PERMISSION);
            return false;
        };
        return granter == agent
            && (ctx.host().owner(caller) == agent || ctx.host().experience(caller).is_some());
    }
    let owner = ctx.host().owner(caller);
    if global && owner != agent {
        return false;
    }
    let Some(present) = ctx.host().agent(caller, agent) else {
        return false;
    };
    if present.info & AGENT_SITTING != 0 {
        return false;
    }
    if grant.as_deref() == Some(agent) {
        return true;
    }
    if global {
        return false;
    }
    if ctx.host().attached(caller) != 0 && owner == agent {
        return true;
    }
    let Some(standing) = ctx
        .host()
        .object_details(caller, agent)
        .map(|details| details.position)
    else {
        return false;
    };
    let here = ctx.host().placement(caller).position;
    let owns = |parcel: Option<crate::vm::land::Parcel>| parcel.is_some_and(|p| p.owner == owner);
    owns(ctx.host().parcel_at(caller, here.x, here.y))
        && owns(ctx.host().parcel_at(caller, standing.x, standing.y))
}

/// `llTeleportAgent`: send the avatar to the landmark `landmark` of the
/// prim's contents, or with an empty one to `position` in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_teleport_agent(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    landmark: String,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    if !may_teleport(ctx, &agent.0, false) {
        return Ok(());
    }
    let caller = ctx.caller();
    let destination = if landmark.is_empty() {
        Destination::Local { position, look_at }
    } else {
        let Some(asset) = ctx
            .host()
            .contents(caller)
            .into_iter()
            .find(|item| {
                item.kind == crate::vm::contents::INVENTORY_LANDMARK && item.name == landmark
            })
            .map(|item| item.asset)
        else {
            complain(ctx, &format!("Could not find landmark '{landmark}'"));
            return Ok(());
        };
        Destination::Landmark { asset, look_at }
    };
    ctx.host().teleport_agent(caller, &agent.0, &destination);
    Ok(())
}

/// `llTeleportAgentGlobalCoords`: send the avatar to `position` in the
/// region whose corner is `global`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_teleport_agent_global_coords(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    global: Vector,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    if !may_teleport(ctx, &agent.0, true) {
        return Ok(());
    }
    let caller = ctx.caller();
    let corner = (f64::from(global.x), f64::from(global.y));
    ctx.host().teleport_agent(
        caller,
        &agent.0,
        &Destination::Global {
            corner,
            position,
            look_at,
        },
    );
    Ok(())
}

/// `llTeleportAgentHome`: send the avatar home — one on the land of the
/// script's owner, as both grids allow it with no permission. It holds the
/// script 5 s; OpenSim not for an avatar that is not in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_teleport_agent_home(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let here = ctx.host().placement(caller).position;
    let on_owners_land = ctx
        .host()
        .parcel_at(caller, here.x, here.y)
        .is_some_and(|parcel| parcel.owner == owner);
    let present = ctx.host().agent(caller, &agent.0).is_some();
    if on_owners_land && present {
        ctx.host()
            .teleport_agent(caller, &agent.0, &Destination::Home);
    }
    // OpenSim returns before its sleep for an avatar not in the region.
    if !present && ctx.flavour().is_opensim() {
        ctx.delay(0.0);
    }
    Ok(())
}

/// `llGetAttachedList`: the keys of what the avatar wears that a script may
/// see; `["NOT FOUND"]` for an avatar not in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_attached_list(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    Ok(ctx.host().attached_list(caller, &agent.0).map_or_else(
        || vec![Element::String("NOT FOUND".to_owned())],
        |keys| keys.into_iter().map(Element::Key).collect(),
    ))
}
