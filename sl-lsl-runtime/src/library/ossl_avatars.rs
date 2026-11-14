//! Library tranche: OpenSim's NPC and avatar functions
//! (`server-lsl-lib-ossl-avatars`) — the NPCs a script creates, drives and
//! removes (`osNpc*`), and OpenSim's twins of the avatar tranche's `ll*`
//! functions: animations, attachments, sitting and stand targets, teleports,
//! names, appearance saves, health and speed, group invitations and ejections,
//! and kicks. Ported from `OSSL_Api.cs` and OpenSim's `NPCModule.cs`.
//!
//! Only an OpenSim flavour compiles these, and every call has passed the
//! region's permission check by the time it gets here (`vm::ossl::admit`).
//! The NPC module is imitated as the stock `[NPC]` section configures it:
//! unowned NPCs, NPCs sensed as agents and clones of other avatars allowed,
//! and no `- NPC -` title.

use sl_types::chat::ChatVolume;
use sl_types::lsl::{Rotation, Vector};

use crate::library::comms::{speak, waits_for_throttle};
use crate::library::dotnet::canonical_uuid;
use crate::library::inventory::CHANGED_INVENTORY;
use crate::library::notecards::load_notecard;
use crate::library::ossl::runtime_error;
use crate::library::{BuiltinId, CallError, Exception, Key};
use crate::value::{Element, NULL_KEY, ZERO_VECTOR};
use crate::vm::ScriptCtx;
use crate::vm::avatars::Destination;
use crate::vm::contents::{INVENTORY_ANIMATION, INVENTORY_OBJECT};
use crate::vm::npcs::{Appearance, DropAt, Npc, NpcAction, NpcRefusal, NpcSpec, NpcSpeech, Vitals};
use crate::vm::ossl::warn;
use crate::vm::permissions::{PERMISSION_ATTACH, PERMISSION_TELEPORT};

/// `OS_NPC_NO_FLY`.
const OS_NPC_NO_FLY: i32 = 1;
/// `OS_NPC_LAND_AT_TARGET`.
const OS_NPC_LAND_AT_TARGET: i32 = 2;
/// `OS_NPC_RUNNING`.
const OS_NPC_RUNNING: i32 = 4;
/// `OS_NPC_NOT_OWNED`.
const OS_NPC_NOT_OWNED: i32 = 2;
/// `OS_NPC_SENSE_AS_AGENT`.
const OS_NPC_SENSE_AS_AGENT: i32 = 4;
/// `OS_NPC_OBJECT_GROUP`.
const OS_NPC_OBJECT_GROUP: i32 = 8;
/// `NPCLOOKAT_CLEAR`, the last look-at type.
const NPCLOOKAT_CLEAR: i32 = 10;
/// `OS_ATTACH_MSG_ALL`.
const OS_ATTACH_MSG_ALL: i32 = -0xFFFF;
/// `OS_ATTACH_MSG_INVERT_POINTS`.
const OS_ATTACH_MSG_INVERT_POINTS: i32 = 1;
/// `OS_ATTACH_MSG_OBJECT_CREATOR`.
const OS_ATTACH_MSG_OBJECT_CREATOR: i32 = 2;
/// `OS_ATTACH_MSG_SCRIPT_CREATOR`.
const OS_ATTACH_MSG_SCRIPT_CREATOR: i32 = 4;
/// `OS_LTPAG_USEVEL`.
const OS_LTPAG_USEVEL: i32 = 0x1;
/// `OS_LTPAG_USELOOKAT`.
const OS_LTPAG_USELOOKAT: i32 = 0x2;
/// `OS_LTPAG_ALGNLV`: look along the velocity.
const OS_LTPAG_ALGNLV: i32 = 0x4;
/// `OS_LTPAG_FORCEFLY`.
const OS_LTPAG_FORCEFLY: i32 = 0x8;
/// What `ScenePresence.LocalTeleport` reads as "force no fly" (OpenSim's
/// `OS_LTPAG_FORCENOFLY` constant is `0x16`, but the move tests bit 16).
const LOCAL_TELEPORT_NO_FLY: i32 = 16;
/// The flags that make a local teleport one the viewer is told of.
const LOCAL_TELEPORT_ANNOUNCED: i32 = 0x1e;
/// `LINK_THIS`.
const LINK_THIS: i32 = -4;
/// `PARCEL_FLAG_ALLOW_DAMAGE`.
const PARCEL_FLAG_ALLOW_DAMAGE: u32 = 0x20;
/// The group power to invite (`GroupPowers.Invite`).
const POWER_INVITE: u64 = 1 << 1;
/// The group power to eject (`GroupPowers.Eject`).
const POWER_EJECT: u64 = 1 << 2;
/// The farthest a sit active range reaches.
const MAX_SIT_RANGE: f32 = 128.0;
/// The fastest `osSetOwnerSpeed` makes its owner.
const MAX_OWNER_SPEED: f32 = 4.0;
/// What `osGetLinkSitActiveRange` answers for a link that is none
/// (`int.MinValue` as a float).
const NO_LINK_RANGE: f32 = -2_147_483_648.0;
/// The side of a grid cell, in metres.
const CELL: f64 = 256.0;

// ----------------------------------------------------------------------
// Keys, NPCs and avatars.
// ----------------------------------------------------------------------

/// `key` in its canonical form, or [`None`] for text that is no key
/// (OpenSim's `UUID.TryParse`).
fn parsed(key: &str) -> Option<String> {
    canonical_uuid(key)
}

/// `key` in its canonical form as OpenMetaverse's `new UUID(key)` reads it:
/// a key that is none throws.
fn constructed(id: BuiltinId, key: &str) -> Result<String, CallError> {
    canonical_uuid(key).ok_or(CallError::Exception {
        id,
        exception: Exception::InvalidUuid,
    })
}

/// The NPC keyed `key`, of the caller's region.
fn npc_of(ctx: &mut ScriptCtx<'_>, key: &str) -> Option<Npc> {
    let caller = ctx.caller();
    ctx.host()
        .npcs(caller)
        .into_iter()
        .find(|npc| npc.key.eq_ignore_ascii_case(key))
}

/// The NPC keyed `key` if the script's owner may drive it
/// (`NPCModule.CheckPermissions`): one that is unowned, owned by the script's
/// owner, or is the owner itself. [`None`] for a key no NPC has.
fn driven(ctx: &mut ScriptCtx<'_>, key: &str) -> Option<Npc> {
    let npc = npc_of(ctx, key)?;
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let allowed = npc
        .owner
        .as_ref()
        .is_none_or(|by| by.eq_ignore_ascii_case(&owner))
        || npc.key.eq_ignore_ascii_case(&owner);
    allowed.then_some(npc)
}

/// Whether `key` is an avatar in the caller's region, an NPC among them
/// (OpenSim's `GetScenePresence`).
fn present(ctx: &mut ScriptCtx<'_>, key: &str) -> bool {
    let caller = ctx.caller();
    ctx.host().agent(caller, key).is_some() || npc_of(ctx, key).is_some()
}

/// The key of the avatar in the region, an NPC among them, named exactly
/// `first last` (`GetScenePresence(first, last)`).
fn present_named(ctx: &mut ScriptCtx<'_>, first: &str, last: &str) -> Option<String> {
    let name = format!("{first} {last}");
    let caller = ctx.caller();
    if let Some(npc) = ctx
        .host()
        .npcs(caller)
        .into_iter()
        .find(|npc| npc.name == name)
    {
        return Some(npc.key);
    }
    let (key, here) = ctx.host().agent_named(caller, &name)?;
    if !here {
        return None;
    }
    // `agent_named` matches without regard to case; the presence's name is
    // compared exactly.
    let agent = ctx.host().agent(caller, &key)?;
    (agent.legacy_name == name).then_some(key)
}

/// Where `agent`, an avatar or an NPC in the region, stands.
fn position_of(ctx: &mut ScriptCtx<'_>, agent: &str) -> Option<Vector> {
    if let Some(npc) = npc_of(ctx, agent) {
        return Some(npc.position);
    }
    let caller = ctx.caller();
    ctx.host()
        .object_details(caller, agent)
        .map(|details| details.position)
}

/// The key of the caller's own prim.
fn own_prim(ctx: &mut ScriptCtx<'_>) -> String {
    let caller = ctx.caller();
    ctx.host().whereabouts(caller).prim
}

/// The group the caller's prim is set to, `NULL_KEY` for none.
fn own_group(ctx: &mut ScriptCtx<'_>) -> String {
    let caller = ctx.caller();
    let prim = own_prim(ctx);
    ctx.host()
        .object_details(caller, &prim)
        .map_or_else(|| NULL_KEY.to_owned(), |details| details.group)
}

/// The asset of the animation item named `name` in the caller's prim.
fn animation_item(ctx: &mut ScriptCtx<'_>, name: &str) -> Option<String> {
    let caller = ctx.caller();
    ctx.host()
        .contents(caller)
        .into_iter()
        .find(|item| item.kind == INVENTORY_ANIMATION && item.name == name)
        .map(|item| item.asset)
}

/// The built-in animation called `name` (`DefaultAvatarAnimations`).
fn builtin_animation(name: &str) -> Option<String> {
    sl_anim::registry::builtin_animation_by_name(name).map(|builtin| builtin.id.to_string())
}

/// What `Animator.AddAnimation` plays for `animation`: the animation item of
/// that name in the caller's prim, else the built-in of that name; nothing
/// for anything else, a key included.
fn played(ctx: &mut ScriptCtx<'_>, animation: &str) -> Option<String> {
    animation_item(ctx, animation).or_else(|| builtin_animation(animation))
}

/// What `Animator.RemoveAnimation` stops for `animation`: a key as itself,
/// else the animation item of that name, else the built-in of that name.
fn stopped(ctx: &mut ScriptCtx<'_>, animation: &str) -> Option<String> {
    parsed(animation)
        .or_else(|| animation_item(ctx, animation))
        .or_else(|| builtin_animation(animation))
}

/// The default avatar's appearance, as OpenSim's `AvatarAppearance` starts.
fn default_appearance() -> Appearance {
    Appearance {
        serial: 0,
        height: 1.9,
        hover: 0.0,
        textures: vec![crate::vm::npcs::DEFAULT_AVATAR_TEXTURE.to_owned(); 45],
        extra_bakes: None,
        visual_params: crate::vm::npcs::DEFAULT_VISUAL_PARAMS.to_vec(),
        attachments: Vec::new(),
    }
}

// ----------------------------------------------------------------------
// NPCs.
// ----------------------------------------------------------------------

/// `osIsNpc`: whether the key is an NPC's in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_is_npc(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<i32, CallError> {
    Ok(i32::from(npc_of(ctx, &npc.0).is_some()))
}

/// `osNpcCreate` with OpenSim's default options: an unowned NPC, as the stock
/// `[NPC] AllowNotOwned` makes it.
///
/// # Errors
///
/// As [`os_npc_create_with`].
pub fn os_npc_create(
    ctx: &mut ScriptCtx<'_>,
    first: String,
    last: String,
    position: Vector,
    notecard: String,
) -> Result<Key, CallError> {
    npc_create(
        ctx,
        BuiltinId::OsNpcCreate2,
        (first, last),
        position,
        &notecard,
        OS_NPC_NOT_OWNED,
    )
}

/// `osNpcCreate` with options: owned by the script's owner unless
/// `OS_NPC_NOT_OWNED`, sensed as an agent with `OS_NPC_SENSE_AS_AGENT`, in
/// the prim's group with `OS_NPC_OBJECT_GROUP`.
///
/// # Errors
///
/// An OSSL run-time error, which ends the event, when the owner may not rez
/// at the position, the owner is not of the prim's group it is to join, or
/// the appearance notecard cannot be found or read.
pub fn os_npc_create_with(
    ctx: &mut ScriptCtx<'_>,
    first: String,
    last: String,
    position: Vector,
    notecard: String,
    options: i32,
) -> Result<Key, CallError> {
    npc_create(
        ctx,
        BuiltinId::OsNpcCreate,
        (first, last),
        position,
        &notecard,
        options,
    )
}

/// `osNpcCreate`'s shared body (`NpcCreate`).
fn npc_create(
    ctx: &mut ScriptCtx<'_>,
    id: BuiltinId,
    (first, last): (String, String),
    position: Vector,
    notecard: &str,
    options: i32,
) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let mut group = None;
    let mut group_title = String::new();
    let host_group = own_group(ctx);
    if options & OS_NPC_OBJECT_GROUP != 0 && host_group != NULL_KEY {
        if ctx.host().group_powers(&host_group, &owner).is_none() {
            return Err(runtime_error(
                id,
                "osNpcCreate: the object owner is not member of the object group",
            ));
        }
        // The stock `NoNPCGroup` titles it with the group's name.
        group_title = ctx.host().group_name(&host_group).unwrap_or_default();
        group = Some(host_group);
    }
    // `AllowCloneOtherAvatars`: a key of an avatar in the region is its look.
    let mut appearance = parsed(notecard).and_then(|key| ctx.host().appearance(caller, &key));
    if appearance.is_none() && !notecard.trim().is_empty() {
        let Some(text) = load_notecard(ctx, notecard) else {
            return Err(runtime_error(
                id,
                &format!("osNpcCreate: Notecard '{notecard}' not found."),
            ));
        };
        let Some(read) = Appearance::from_notecard(&text, &default_appearance()) else {
            return Err(runtime_error(
                id,
                &format!("osNpcCreate: Error processing notecard '{notecard}'"),
            ));
        };
        appearance = Some(read);
    }
    let spec = NpcSpec {
        first,
        last,
        position,
        owner: (options & OS_NPC_NOT_OWNED == 0).then_some(owner),
        sense_as_agent: options & OS_NPC_SENSE_AS_AGENT != 0,
        group,
        group_title,
        appearance,
    };
    match ctx.host().create_npc(caller, &spec) {
        Ok(key) => Ok(Key(key)),
        Err(NpcRefusal::NoRez) => Err(runtime_error(
            id,
            "no permission to rez NPC at requested location",
        )),
        Err(NpcRefusal::Full) => Ok(Key(NULL_KEY.to_owned())),
    }
}

/// `osNpcSaveAppearance`: the NPC's appearance, HUDs left out, saved to a
/// notecard of the caller's prim; the notecard's asset key, or `NULL_KEY`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_save_appearance(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    notecard: String,
) -> Result<Key, CallError> {
    npc_save_appearance(ctx, &npc.0, &notecard, false)
}

/// `osNpcSaveAppearance` with HUDs kept when `include_huds` is not 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_save_appearance_huds(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    notecard: String,
    include_huds: i32,
) -> Result<Key, CallError> {
    npc_save_appearance(ctx, &npc.0, &notecard, include_huds != 0)
}

/// `osNpcSaveAppearance`'s shared body.
fn npc_save_appearance(
    ctx: &mut ScriptCtx<'_>,
    npc: &str,
    notecard: &str,
    include_huds: bool,
) -> Result<Key, CallError> {
    let Some(npc) = parsed(npc).and_then(|key| driven(ctx, &key)) else {
        return Ok(Key(NULL_KEY.to_owned()));
    };
    Ok(save_appearance(ctx, &npc.key, notecard, include_huds))
}

/// OpenSim's `SaveAppearanceToNotecard`: the appearance of `agent`, an
/// avatar or NPC in the region, written to the notecard `notecard` of the
/// caller's prim in place of one of that name; its asset key, or `NULL_KEY`
/// for an avatar not in the region.
fn save_appearance(
    ctx: &mut ScriptCtx<'_>,
    agent: &str,
    notecard: &str,
    include_huds: bool,
) -> Key {
    let caller = ctx.caller();
    let Some(appearance) = ctx.host().appearance(caller, agent) else {
        return Key(NULL_KEY.to_owned());
    };
    let text = appearance.to_notecard(include_huds);
    // `AddInventoryItemExclusive`: an item of the name is removed first, a
    // `changed(CHANGED_INVENTORY)` of its own before the add's.
    let replaced = ctx
        .host()
        .contents(caller)
        .iter()
        .any(|item| item.name == notecard);
    let changed: &[i32] = if replaced {
        &[CHANGED_INVENTORY, CHANGED_INVENTORY]
    } else {
        &[CHANGED_INVENTORY]
    };
    Key(ctx
        .host()
        .make_notecard(caller, notecard, "Avatar Appearance", &text, changed, true))
}

/// `osNpcLoadAppearance`: the NPC looks as the notecard says.
///
/// # Errors
///
/// An OSSL run-time error for a notecard that cannot be found, and
/// [`Exception::InvalidCast`] for one that is no appearance (OpenSim's cast of
/// what it read to a map; text that is no LLSD at all throws Mono's XML
/// parser's exception there, which this does not name).
pub fn os_npc_load_appearance(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    notecard: String,
) -> Result<(), CallError> {
    let Some(npc) = parsed(&npc.0).and_then(|key| driven(ctx, &key)) else {
        return Ok(());
    };
    let Some(text) = load_notecard(ctx, &notecard) else {
        return Err(runtime_error(
            BuiltinId::OsNpcLoadAppearance,
            &format!("osNpcCreate: Notecard reference '{notecard}' not found."),
        ));
    };
    let Some(appearance) = Appearance::from_notecard(&text, &default_appearance()) else {
        return Err(CallError::Exception {
            id: BuiltinId::OsNpcLoadAppearance,
            exception: Exception::InvalidCast,
        });
    };
    let caller = ctx.caller();
    ctx.host().npc_act(
        caller,
        &npc.key,
        &NpcAction::Appearance(Box::new(appearance)),
    );
    Ok(())
}

/// `osNpcGetOwner`: the NPC's owner, or — for an unowned NPC and for any key
/// that is no NPC's — the key itself; `NULL_KEY` for text that is no key.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_get_owner(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<Key, CallError> {
    if parsed(&npc.0).is_none() {
        return Ok(Key(NULL_KEY.to_owned()));
    }
    Ok(npc_of(ctx, &npc.0)
        .and_then(|found| found.owner)
        .map_or(npc, Key))
}

/// `osNpcGetPos`: where the NPC stands, `ZERO_VECTOR` for one the script may
/// not drive.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_get_pos(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<Vector, CallError> {
    Ok(parsed(&npc.0)
        .and_then(|key| driven(ctx, &key))
        .map_or(ZERO_VECTOR, |npc| npc.position))
}

/// `osNpcGetRot`: how the NPC is turned, `ZERO_ROTATION` for one the script
/// may not drive.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_get_rot(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<Rotation, CallError> {
    Ok(parsed(&npc.0)
        .and_then(|key| driven(ctx, &key))
        .map_or(crate::value::ZERO_ROTATION, |npc| npc.rotation))
}

/// Make the NPC keyed `npc` do `action` if the script may drive it.
fn act(ctx: &mut ScriptCtx<'_>, npc: &str, action: &NpcAction) {
    if let Some(npc) = driven(ctx, npc) {
        let caller = ctx.caller();
        ctx.host().npc_act(caller, &npc.key, action);
    }
}

/// `osNpcMoveTo`: the NPC walks, or flies, to the position and lands there.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_move_to(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    position: Vector,
) -> Result<(), CallError> {
    if let Some(key) = parsed(&npc.0) {
        act(
            ctx,
            &key,
            &NpcAction::MoveTo {
                target: position,
                no_fly: false,
                land_at_target: true,
                running: false,
            },
        );
    }
    Ok(())
}

/// `osNpcMoveToTarget`: the NPC goes to the target as the `OS_NPC_*` options
/// say.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_move_to_target(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    target: Vector,
    options: i32,
) -> Result<(), CallError> {
    if let Some(key) = parsed(&npc.0) {
        act(
            ctx,
            &key,
            &NpcAction::MoveTo {
                target,
                no_fly: options & OS_NPC_NO_FLY != 0,
                land_at_target: options & OS_NPC_LAND_AT_TARGET != 0,
                running: options & OS_NPC_RUNNING != 0,
            },
        );
    }
    Ok(())
}

/// `osNpcSetRot`: the NPC turns.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_set_rot(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    rotation: Rotation,
) -> Result<(), CallError> {
    if let Some(key) = parsed(&npc.0) {
        act(ctx, &key, &NpcAction::Turn(rotation));
    }
    Ok(())
}

/// `osNpcStopMoveToTarget`: the NPC stops where it is.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_stop_move_to_target(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcStopMoveToTarget, &npc.0)?;
    act(ctx, &key, &NpcAction::StopMoving);
    Ok(())
}

/// `osNpcSetProfileAbout`: the "about" text of the NPC's profile.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_set_profile_about(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    about: String,
) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcSetProfileAbout, &npc.0)?;
    act(ctx, &key, &NpcAction::About(about));
    Ok(())
}

/// `osNpcSetProfileImage`: the picture of the NPC's profile — a texture of
/// the caller's prim by name, or a key; anything else is nothing.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for an NPC key that is none.
pub fn os_npc_set_profile_image(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    image: String,
) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcSetProfileImage, &npc.0)?;
    if driven(ctx, &key).is_none() {
        return Ok(());
    }
    let caller = ctx.caller();
    let texture = ctx
        .host()
        .inventory_key(caller, &image, crate::vm::contents::INVENTORY_TEXTURE)
        .or_else(|| parsed(&image));
    if let Some(texture) = texture {
        act(ctx, &key, &NpcAction::Image(texture));
    }
    Ok(())
}

/// Have the NPC keyed `npc` say `speech` if the script may drive it, offering
/// the line to the scripts in earshot.
fn npc_speak(ctx: &mut ScriptCtx<'_>, npc: &str, speech: &NpcSpeech) {
    let Some(npc) = driven(ctx, npc) else {
        return;
    };
    let caller = ctx.caller();
    let overheard = ctx.host().npc_say(caller, &npc.key, speech);
    ctx.overhear(overheard);
}

/// `osNpcSay` on channel 0.
///
/// # Errors
///
/// As [`os_npc_say_on`].
pub fn os_npc_say(ctx: &mut ScriptCtx<'_>, npc: Key, message: String) -> Result<(), CallError> {
    npc_said(
        ctx,
        BuiltinId::OsNpcSay2,
        &npc.0,
        0,
        message,
        ChatVolume::Say,
    )
}

/// `osNpcSay`: the NPC says the message on the channel, past OpenSim's
/// channel-0 throttle of the calling script.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_say_on(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    channel: i32,
    message: String,
) -> Result<(), CallError> {
    npc_said(
        ctx,
        BuiltinId::OsNpcSay,
        &npc.0,
        channel,
        message,
        ChatVolume::Say,
    )
}

/// `osNpcShout`: the NPC shouts, past the channel-0 throttle.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_shout(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    channel: i32,
    message: String,
) -> Result<(), CallError> {
    npc_said(
        ctx,
        BuiltinId::OsNpcShout,
        &npc.0,
        channel,
        message,
        ChatVolume::Shout,
    )
}

/// The NPC says a line at `volume`, `osNpcSay` and `osNpcShout` past the
/// script's channel-0 throttle.
fn npc_said(
    ctx: &mut ScriptCtx<'_>,
    id: BuiltinId,
    npc: &str,
    channel: i32,
    text: String,
    volume: ChatVolume,
) -> Result<(), CallError> {
    let key = constructed(id, npc)?;
    if driven(ctx, &key).is_none() || waits_for_throttle(ctx, channel) {
        return Ok(());
    }
    npc_speak(
        ctx,
        &key,
        &NpcSpeech {
            volume,
            channel,
            text,
            to: None,
        },
    );
    Ok(())
}

/// `osNpcWhisper`: the NPC whispers; no throttle.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_whisper(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    channel: i32,
    message: String,
) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcWhisper, &npc.0)?;
    npc_speak(
        ctx,
        &key,
        &NpcSpeech {
            volume: ChatVolume::Whisper,
            channel,
            text: message,
            to: None,
        },
    );
    Ok(())
}

/// `osNpcSayTo`: the NPC says the message to one avatar or prim, wherever in
/// the region, past the channel-0 throttle.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_say_to(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    target: Key,
    channel: i32,
    message: String,
) -> Result<(), CallError> {
    let (Some(key), Some(target)) = (parsed(&npc.0), parsed(&target.0)) else {
        return Ok(());
    };
    if driven(ctx, &key).is_none() || waits_for_throttle(ctx, channel) {
        return Ok(());
    }
    npc_speak(
        ctx,
        &key,
        &NpcSpeech {
            volume: ChatVolume::Say,
            channel,
            text: message,
            to: Some(target),
        },
    );
    Ok(())
}

/// `osNpcSit`: the NPC sits on the prim as its own sit request would; the
/// options are read by nothing.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for an NPC or a target key that is none.
pub fn os_npc_sit(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    target: Key,
    _options: i32,
) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcSit, &npc.0)?;
    let Some(npc) = driven(ctx, &key) else {
        return Ok(());
    };
    let target = constructed(BuiltinId::OsNpcSit, &target.0)?;
    let caller = ctx.caller();
    ctx.host().sit(caller, &npc.key, &target, false);
    Ok(())
}

/// `osNpcStand`: the NPC stands up.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_npc_stand(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsNpcStand, &npc.0)?;
    act(ctx, &key, &NpcAction::Stand);
    Ok(())
}

/// `osNpcRemove`: the NPC leaves the region. A key that is none is nothing:
/// OpenSim swallows what its own parse throws.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_remove(ctx: &mut ScriptCtx<'_>, npc: Key) -> Result<(), CallError> {
    if let Some(key) = parsed(&npc.0) {
        act(ctx, &key, &NpcAction::Remove);
    }
    Ok(())
}

/// `osNpcPlayAnimation`: the NPC plays an animation of the caller's prim, or
/// a built-in one, by name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_play_animation(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    animation: String,
) -> Result<(), CallError> {
    let Some(npc) = parsed(&npc.0).and_then(|key| driven(ctx, &key)) else {
        return Ok(());
    };
    if let Some(asset) = played(ctx, &animation) {
        let caller = ctx.caller();
        ctx.host().animate(caller, &npc.key, &asset, true);
    }
    Ok(())
}

/// `osNpcStopAnimation`: the NPC stops an animation, by key, by the name of
/// an animation of the caller's prim, or by a built-in's name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_stop_animation(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    animation: String,
) -> Result<(), CallError> {
    let Some(npc) = parsed(&npc.0).and_then(|key| driven(ctx, &key)) else {
        return Ok(());
    };
    if let Some(asset) = stopped(ctx, &animation) {
        let caller = ctx.caller();
        ctx.host().animate(caller, &npc.key, &asset, false);
    }
    Ok(())
}

/// `osNpcTouch`: the NPC touches a prim — the one keyed, its root for link 0
/// or 1, or the prim of that link number in its linkset; nothing for a prim
/// the region does not have.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_touch(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    object: Key,
    link: i32,
) -> Result<(), CallError> {
    let Some(npc) = parsed(&npc.0).and_then(|key| driven(ctx, &key)) else {
        return Ok(());
    };
    let Some(object) = parsed(&object.0) else {
        return Ok(());
    };
    let linkset = ctx.host().linkset_of(&object);
    if linkset.is_empty() {
        return Ok(());
    }
    let prim = if link == LINK_THIS {
        Some(object)
    } else if link == 0 || link == 1 {
        linkset.first().cloned()
    } else {
        usize::try_from(link)
            .ok()
            .and_then(|link| link.checked_sub(1))
            .and_then(|index| linkset.get(index))
            .cloned()
    };
    if let Some(prim) = prim {
        let caller = ctx.caller();
        ctx.host()
            .npc_act(caller, &npc.key, &NpcAction::Touch(prim));
    }
    Ok(())
}

/// `osNpcLookAt`: the NPC looks at a prim or an avatar, by the `LookAt`
/// viewer effect sent every viewer. 0 when it is sent — or when nobody is in
/// the region to send it to — -1 for a type that is none, -2 for an NPC key
/// that is none, -3 for no NPC, -4 for a target key that is none, -5 for a
/// target the region does not have.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_npc_look_at(
    ctx: &mut ScriptCtx<'_>,
    npc: Key,
    kind: i32,
    target: Key,
    offset: Vector,
) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let anybody = ctx.host().sense(caller).iter().any(|sensed| sensed.agent);
    if !anybody {
        return Ok(0);
    }
    if !(0..=NPCLOOKAT_CLEAR).contains(&kind) {
        return Ok(-1);
    }
    let Some(key) = parsed(&npc.0) else {
        return Ok(-2);
    };
    let Some(npc) = npc_of(ctx, &key) else {
        return Ok(-3);
    };
    let Some(target) = parsed(&target.0) else {
        return Ok(-4);
    };
    if target != NULL_KEY && ctx.host().object_details(caller, &target).is_none() {
        return Ok(-5);
    }
    ctx.host().npc_act(
        caller,
        &npc.key,
        &NpcAction::LookAt {
            kind,
            target,
            offset,
        },
    );
    Ok(0)
}

/// `osGetNPCList`: every NPC of the region, as its key (a string), where it
/// stands and its name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_npc_list(ctx: &mut ScriptCtx<'_>) -> Result<Vec<Element>, CallError> {
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .npcs(caller)
        .into_iter()
        .flat_map(|npc| {
            [
                Element::String(npc.key),
                Element::Vector(npc.position),
                Element::String(npc.name),
            ]
        })
        .collect())
}

// ----------------------------------------------------------------------
// Avatars: names and kinds.
// ----------------------------------------------------------------------

/// `osKey2Name`: the name of the avatar or NPC in the region, else of the
/// grid's account, keyed so; empty for anything else.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_key2_name(ctx: &mut ScriptCtx<'_>, id: String) -> Result<String, CallError> {
    let Some(key) = parsed(&id) else {
        return Ok(String::new());
    };
    if let Some(npc) = npc_of(ctx, &key) {
        return Ok(npc.name);
    }
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .agent_record(caller, &key)
        .map(|record| record.legacy_name)
        .unwrap_or_default())
}

/// `osAvatarName2Key`: the key of the avatar or NPC in the region, else of
/// the grid's account, named so; `NULL_KEY` for nobody.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_avatar_name2_key(
    ctx: &mut ScriptCtx<'_>,
    first: String,
    last: String,
) -> Result<String, CallError> {
    if let Some(key) = present_named(ctx, &first, &last) {
        return Ok(key);
    }
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .agent_named(caller, &format!("{first} {last}"))
        .map_or_else(|| NULL_KEY.to_owned(), |(key, _here)| key))
}

/// `osAvatarType` by key: 1 for an avatar in the region, 2 for an NPC, 0 for
/// neither, -1 for a key that is none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_avatar_type(ctx: &mut ScriptCtx<'_>, avatar: Key) -> Result<i32, CallError> {
    let Some(key) = parsed(&avatar.0) else {
        return Ok(-1);
    };
    Ok(avatar_type(ctx, &key))
}

/// `osAvatarType` by name: 1 for an avatar in the region, 2 for an NPC, 0
/// for neither.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_avatar_type_named(
    ctx: &mut ScriptCtx<'_>,
    first: String,
    last: String,
) -> Result<i32, CallError> {
    Ok(present_named(ctx, &first, &last).map_or(0, |key| avatar_type(ctx, &key)))
}

/// What `osAvatarType` answers of the key `key`.
fn avatar_type(ctx: &mut ScriptCtx<'_>, key: &str) -> i32 {
    if npc_of(ctx, key).is_some() {
        return 2;
    }
    let caller = ctx.caller();
    i32::from(ctx.host().agent(caller, key).is_some())
}

/// `osGetGender`: `male` or `female` by the avatar's shape, `unknown` for an
/// avatar or NPC not in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_gender(ctx: &mut ScriptCtx<'_>, avatar: Key) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let male = parsed(&avatar.0)
        .and_then(|key| ctx.host().appearance(caller, &key))
        .and_then(|appearance| appearance.is_male());
    Ok(Key(match male {
        Some(true) => "male",
        Some(false) => "female",
        None => "unknown",
    }
    .to_owned()))
}

// ----------------------------------------------------------------------
// Avatars: animations and appearance.
// ----------------------------------------------------------------------

/// `osAvatarPlayAnimation`: any avatar or NPC in the region plays an
/// animation of the caller's prim, or a built-in one, by name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_avatar_play_animation(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    animation: String,
) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar.0).filter(|key| present(ctx, key)) else {
        return Ok(());
    };
    if let Some(asset) = played(ctx, &animation) {
        let caller = ctx.caller();
        ctx.host().animate(caller, &key, &asset, true);
    }
    Ok(())
}

/// `osAvatarStopAnimation`: any avatar or NPC in the region stops an
/// animation, by key, by the name of one of the caller's prim, or a
/// built-in's.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_avatar_stop_animation(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    animation: String,
) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar.0).filter(|key| present(ctx, key)) else {
        return Ok(());
    };
    if let Some(asset) = stopped(ctx, &animation) {
        let caller = ctx.caller();
        ctx.host().animate(caller, &key, &asset, false);
    }
    Ok(())
}

/// `osOwnerSaveAppearance`: the owner's appearance, HUDs left out, saved to a
/// notecard of the caller's prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_owner_save_appearance(
    ctx: &mut ScriptCtx<'_>,
    notecard: String,
) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    Ok(save_appearance(ctx, &owner, &notecard, false))
}

/// `osOwnerSaveAppearance` with HUDs kept when `include_huds` is not 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_owner_save_appearance_huds(
    ctx: &mut ScriptCtx<'_>,
    notecard: String,
    include_huds: i32,
) -> Result<Key, CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    Ok(save_appearance(ctx, &owner, &notecard, include_huds != 0))
}

/// `osAgentSaveAppearance`: any avatar's appearance, HUDs left out.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_agent_save_appearance(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    notecard: String,
) -> Result<Key, CallError> {
    Ok(parsed(&avatar.0).map_or_else(
        || Key(NULL_KEY.to_owned()),
        |key| save_appearance(ctx, &key, &notecard, false),
    ))
}

/// `osAgentSaveAppearance` with HUDs kept when `include_huds` is not 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_agent_save_appearance_huds(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    notecard: String,
    include_huds: i32,
) -> Result<Key, CallError> {
    Ok(parsed(&avatar.0).map_or_else(
        || Key(NULL_KEY.to_owned()),
        |key| save_appearance(ctx, &key, &notecard, include_huds != 0),
    ))
}

// ----------------------------------------------------------------------
// Attachments.
// ----------------------------------------------------------------------

/// `osForceAttachToAvatar`: `llAttachToAvatar` with no permission asked —
/// worn on the owner, when the owner is in the region and the object is not
/// worn already.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_attach_to_avatar(ctx: &mut ScriptCtx<'_>, point: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 {
        return Ok(());
    }
    let owner = ctx.host().owner(caller);
    if ctx.host().agent(caller, &owner).is_some() {
        ctx.host().attach(caller, point);
    }
    Ok(())
}

/// `osForceDetachFromAvatar`: `llDetachFromAvatar` with no permission asked.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_detach_from_avatar(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().attached(caller) != 0 {
        ctx.host().detach(caller);
    }
    Ok(())
}

/// `osForceAttachToAvatarFromInventory`: the owner wears an object of the
/// caller's prim, moved into the owner's inventory.
///
/// # Errors
///
/// As [`os_force_attach_to_other_avatar_from_inventory`].
pub fn os_force_attach_to_avatar_from_inventory(
    ctx: &mut ScriptCtx<'_>,
    item: String,
    point: i32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    attach_from_inventory(
        ctx,
        BuiltinId::OsForceAttachToAvatarFromInventory,
        &owner,
        &item,
        point,
    )
}

/// `osForceAttachToOtherAvatarFromInventory`: any avatar in the region wears
/// an object of the caller's prim, moved into its inventory.
///
/// # Errors
///
/// [`Exception::ItemNotFound`] for an item the prim does not hold and
/// [`Exception::ItemNotObject`] for one that is no object, each after the
/// object says so on channel 0.
pub fn os_force_attach_to_other_avatar_from_inventory(
    ctx: &mut ScriptCtx<'_>,
    avatar: String,
    item: String,
    point: i32,
) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar) else {
        return Ok(());
    };
    attach_from_inventory(
        ctx,
        BuiltinId::OsForceAttachToOtherAvatarFromInventory,
        &key,
        &item,
        point,
    )
}

/// `ForceAttachToAvatarFromInventory`'s body.
fn attach_from_inventory(
    ctx: &mut ScriptCtx<'_>,
    id: BuiltinId,
    agent: &str,
    item: &str,
    point: i32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let held = ctx
        .host()
        .contents(caller)
        .into_iter()
        .find(|held| held.name == item);
    let Some(held) = held else {
        speak(
            ctx,
            0,
            &format!("Could not find object '{item}'"),
            ChatVolume::Say,
        );
        return Err(CallError::Exception {
            id,
            exception: Exception::ItemNotFound(item.to_owned()),
        });
    };
    if held.kind != INVENTORY_OBJECT {
        speak(
            ctx,
            0,
            &format!("Unable to attach, item '{item}' is not an object."),
            ChatVolume::Say,
        );
        return Err(CallError::Exception {
            id,
            exception: Exception::ItemNotObject(item.to_owned()),
        });
    }
    if ctx.host().agent(caller, agent).is_some() {
        ctx.host().attach_from_inventory(caller, agent, item, point);
    }
    Ok(())
}

/// Whether the script may drop its object (`ShoutErrorOnLackingOwnerPerms`):
/// the owner granted `PERMISSION_ATTACH`; else a shout says which is
/// missing, with its second's pause.
fn may_drop(ctx: &mut ScriptCtx<'_>) -> bool {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let granter = ctx.permissions().granter().map(str::to_owned);
    if granter.as_deref() != Some(owner.as_str()) {
        warn(
            ctx,
            "Cannot drop attachment. Permissions not granted to owner.".to_owned(),
        );
        return false;
    }
    if !ctx.permissions().holds(PERMISSION_ATTACH) {
        warn(
            ctx,
            "Cannot drop attachment. Permissions not granted.".to_owned(),
        );
        return false;
    }
    true
}

/// Drop the caller's worn object to the ground, or at `at`, its owner in the
/// region.
fn drop_attachment(ctx: &mut ScriptCtx<'_>, at: Option<&DropAt>) {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    if ctx.host().agent(caller, &owner).is_some() {
        ctx.host().drop_attachment(caller, at);
    }
}

/// `osDropAttachment`: the object, worn, is put down where its owner stands,
/// the owner having granted `PERMISSION_ATTACH`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_drop_attachment(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    if may_drop(ctx) {
        drop_attachment(ctx, None);
    }
    Ok(())
}

/// `osForceDropAttachment`: [`os_drop_attachment`] with no permission asked.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_drop_attachment(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    drop_attachment(ctx, None);
    Ok(())
}

/// `osDropAttachmentAt`: the object, worn, is put down at the position and
/// rotation, the owner having granted `PERMISSION_ATTACH`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_drop_attachment_at(
    ctx: &mut ScriptCtx<'_>,
    position: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    if may_drop(ctx) {
        drop_attachment(ctx, Some(&DropAt { position, rotation }));
    }
    Ok(())
}

/// `osForceDropAttachmentAt`: [`os_drop_attachment_at`] with no permission
/// asked.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_drop_attachment_at(
    ctx: &mut ScriptCtx<'_>,
    position: Vector,
    rotation: Rotation,
) -> Result<(), CallError> {
    drop_attachment(ctx, Some(&DropAt { position, rotation }));
    Ok(())
}

/// A list element as OpenSim's `listObjToInt` reads it: an integer, or a
/// string that is one.
fn element_integer(element: &Element) -> Option<i32> {
    match element {
        Element::Integer(value) => Some(*value),
        Element::String(text) | Element::Key(text) => text.parse().ok(),
        _ => None,
    }
}

/// `osGetNumberOfAttachments`: each point of the list that reads as an
/// integer, followed by how many objects the avatar wears there (0 for a
/// point not above 0); empty for an avatar not in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_number_of_attachments(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    points: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let caller = ctx.caller();
    let Some(worn) = parsed(&avatar.0).and_then(|key| ctx.host().attachments(caller, &key)) else {
        return Ok(Vec::new());
    };
    Ok(points
        .iter()
        .filter_map(element_integer)
        .flat_map(|point| {
            let count = if point <= 0 {
                0
            } else {
                worn.iter().filter(|(at, _key)| *at == point).count()
            };
            [
                Element::Integer(point),
                Element::Integer(i32::try_from(count).unwrap_or(i32::MAX)),
            ]
        })
        .collect())
}

/// `osMessageAttachments`: `dataserver(<this prim>, message)` in the scripts
/// of the objects the avatar wears on the points — or on all of them for
/// `OS_ATTACH_MSG_ALL` or an empty list, or on every other with
/// `OS_ATTACH_MSG_INVERT_POINTS` — those made by the prim's creator alone
/// with `OS_ATTACH_MSG_OBJECT_CREATOR`, or by the script's with
/// `OS_ATTACH_MSG_SCRIPT_CREATOR`. A point that does not read as an integer
/// sends nothing at all.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_message_attachments(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    message: String,
    points: Vec<Element>,
    options: i32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let Some(key) = parsed(&avatar.0).filter(|key| key != NULL_KEY) else {
        return Ok(());
    };
    let Some(worn) = ctx.host().attachments(caller, &key) else {
        return Ok(());
    };
    if worn.is_empty() {
        return Ok(());
    }
    let mut invert = options & OS_ATTACH_MSG_INVERT_POINTS != 0;
    let mut all = false;
    let mut listed = Vec::new();
    for point in &points {
        let Some(point) = element_integer(point) else {
            return Ok(());
        };
        if point == OS_ATTACH_MSG_ALL {
            if invert {
                return Ok(());
            }
            all = true;
            break;
        }
        if point > 0 {
            listed.push(point);
        }
    }
    if listed.is_empty() {
        if !invert && !all {
            return Ok(());
        }
        all = true;
        invert = false;
    }
    let chosen: Vec<&(i32, String)> = worn
        .iter()
        .filter(|(point, _root)| all || invert || listed.contains(point))
        .filter(|(point, _root)| !(invert && listed.contains(point)))
        .collect();
    let own = own_prim(ctx);
    let host_creator = ctx
        .host()
        .prim(caller, &own)
        .map(|prim| prim.creator)
        .unwrap_or_default();
    let script_creator = ctx.standing().creator;
    for (_point, root) in chosen {
        let creator = ctx
            .host()
            .prim(caller, root)
            .map(|prim| prim.creator)
            .unwrap_or_default();
        if options & OS_ATTACH_MSG_OBJECT_CREATOR != 0 && creator != host_creator {
            continue;
        }
        if options & OS_ATTACH_MSG_SCRIPT_CREATOR != 0 && creator != script_creator {
            continue;
        }
        for prim in ctx.host().linkset_of(root) {
            if let Some(scripts) = ctx.host().prim_scripts(&prim)
                && !scripts.is_empty()
            {
                ctx.message_scripts(scripts, own.clone(), message.clone());
            }
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------
// Sitting and standing.
// ----------------------------------------------------------------------

/// `osForceOtherSit` onto the caller's prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_other_sit(ctx: &mut ScriptCtx<'_>, avatar: String) -> Result<(), CallError> {
    let prim = own_prim(ctx);
    force_sit(ctx, &avatar, &prim);
    Ok(())
}

/// `osForceOtherSit` onto any prim of the region.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a target key that is none.
pub fn os_force_other_sit_on(
    ctx: &mut ScriptCtx<'_>,
    avatar: String,
    target: String,
) -> Result<(), CallError> {
    let target = constructed(BuiltinId::OsForceOtherSit, &target)?;
    force_sit(ctx, &avatar, &target);
    Ok(())
}

/// `ForceSit`: seat the avatar, an NPC among them, on `prim` unless somebody
/// sits on its sit target.
fn force_sit(ctx: &mut ScriptCtx<'_>, avatar: &str, prim: &str) {
    let Some(agent) = parsed(avatar).filter(|key| present(ctx, key)) else {
        return;
    };
    let caller = ctx.caller();
    ctx.host().sit(caller, &agent, prim, true);
}

/// The link number of the caller's own prim, and its linkset's root's.
fn own_links(ctx: &mut ScriptCtx<'_>) -> (i32, i32) {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    let root = links.prims.first().map_or(0, |prim| prim.link);
    (links.this, root)
}

/// The prim a link number names for `osSetLinkStandTarget` and its readers:
/// the caller's own for `LINK_THIS`, the root for 0 and 1, the prim of that
/// number past them, nothing for another negative number or a number no prim
/// has.
fn link_named(ctx: &mut ScriptCtx<'_>, link: i32) -> Option<i32> {
    let (this, root) = own_links(ctx);
    if link == LINK_THIS {
        return Some(this);
    }
    if link < 0 {
        return None;
    }
    if link < 2 {
        return Some(root);
    }
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    links
        .prims
        .iter()
        .any(|prim| prim.link == link)
        .then_some(link)
}

/// `osSetSitActiveRange`: how far an avatar may be to sit on the caller's
/// prim, at most 128 m.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_sit_active_range(ctx: &mut ScriptCtx<'_>, range: f32) -> Result<(), CallError> {
    let (this, _root) = own_links(ctx);
    let caller = ctx.caller();
    if let Some(seat) = ctx.host().seat(caller, this) {
        seat.sit_active_range = range.min(MAX_SIT_RANGE);
    }
    Ok(())
}

/// `osSetLinkSitActiveRange`: the same of every prim the link number names
/// (`LINK_SET`, `LINK_ALL_OTHERS` and their kin too).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_link_sit_active_range(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    range: f32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let flavour = ctx.flavour();
    let links = ctx.host().links(caller);
    let named: Vec<i32> = links
        .named(link, flavour)
        .iter()
        .map(|prim| prim.link)
        .collect();
    for link in named {
        if let Some(seat) = ctx.host().seat(caller, link) {
            seat.sit_active_range = range.min(MAX_SIT_RANGE);
        }
    }
    Ok(())
}

/// `osGetSitActiveRange`: the caller's prim's sit active range.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_sit_active_range(ctx: &mut ScriptCtx<'_>) -> Result<f32, CallError> {
    let (this, _root) = own_links(ctx);
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .seat(caller, this)
        .map_or(0.0, |seat| seat.sit_active_range))
}

/// `osGetLinkSitActiveRange`: the sit active range of a prim of the linkset,
/// `int.MinValue` for a link that names none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_link_sit_active_range(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<f32, CallError> {
    let Some(link) = link_named(ctx, link) else {
        return Ok(NO_LINK_RANGE);
    };
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .seat(caller, link)
        .map_or(NO_LINK_RANGE, |seat| seat.sit_active_range))
}

/// `osSetStandTarget`: where an avatar standing up from the caller's prim is
/// put, from the prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_stand_target(ctx: &mut ScriptCtx<'_>, target: Vector) -> Result<(), CallError> {
    let (this, _root) = own_links(ctx);
    let caller = ctx.caller();
    if let Some(seat) = ctx.host().seat(caller, this) {
        seat.stand_target = target;
    }
    Ok(())
}

/// `osSetLinkStandTarget`: the same of a prim of the linkset.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_link_stand_target(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    target: Vector,
) -> Result<(), CallError> {
    if let Some(link) = link_named(ctx, link) {
        let caller = ctx.caller();
        if let Some(seat) = ctx.host().seat(caller, link) {
            seat.stand_target = target;
        }
    }
    Ok(())
}

/// `osGetStandTarget`: the caller's prim's stand target.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_stand_target(ctx: &mut ScriptCtx<'_>) -> Result<Vector, CallError> {
    let (this, _root) = own_links(ctx);
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .seat(caller, this)
        .map_or(ZERO_VECTOR, |seat| seat.stand_target.clone()))
}

/// `osGetLinkStandTarget`: the stand target of a prim of the linkset,
/// `ZERO_VECTOR` for a link that names none.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_link_stand_target(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<Vector, CallError> {
    let Some(link) = link_named(ctx, link) else {
        return Ok(ZERO_VECTOR);
    };
    let caller = ctx.caller();
    Ok(ctx
        .host()
        .seat(caller, link)
        .map_or(ZERO_VECTOR, |seat| seat.stand_target.clone()))
}

// ----------------------------------------------------------------------
// Teleports.
// ----------------------------------------------------------------------

/// `checkAllowAgentTPbyLandOwner`: whether the script may teleport `agent`,
/// who stands at `at` — the agent is its owner or granted it
/// `PERMISSION_TELEPORT`, or the land there is its owner's, its owner holds
/// estate powers, or the land is deeded to the prim's group.
fn may_teleport(ctx: &mut ScriptCtx<'_>, agent: &str, at: &Vector) -> bool {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    if owner.eq_ignore_ascii_case(agent) {
        return true;
    }
    if ctx
        .permissions()
        .granter()
        .is_some_and(|granter| granter.eq_ignore_ascii_case(agent))
        && ctx.permissions().holds(PERMISSION_TELEPORT)
    {
        return true;
    }
    let Some(parcel) = ctx.host().parcel_at(caller, at.x, at.y) else {
        return true;
    };
    if parcel.owner.eq_ignore_ascii_case(&owner) {
        return true;
    }
    let standing = ctx.standing();
    if standing.is(crate::vm::Allowed::EstateOwner)
        || standing.is(crate::vm::Allowed::EstateManager)
    {
        return true;
    }
    if !parcel.group_owned || parcel.group == NULL_KEY || parcel.group.is_empty() {
        return false;
    }
    parcel.group.eq_ignore_ascii_case(&own_group(ctx))
}

/// The avatar `agent` names, if it is a root avatar of the region the script
/// may teleport; a refusal sleeps the script half a second.
fn teleportable(ctx: &mut ScriptCtx<'_>, agent: &str) -> Option<String> {
    let key = parsed(agent)?;
    let caller = ctx.caller();
    ctx.host().agent(caller, &key)?;
    let at = position_of(ctx, &key)?;
    if !may_teleport(ctx, &key, &at) {
        ctx.delay(0.5);
        return None;
    }
    Some(key)
}

/// Send `agent` to `destination`.
fn send(ctx: &mut ScriptCtx<'_>, agent: &str, destination: &Destination) {
    let caller = ctx.caller();
    ctx.host().teleport_agent(caller, agent, destination);
}

/// `osTeleportAgent` within or beyond the region by position alone: inside
/// it a teleport there, half a second's sleep; outside it to the grid's
/// region under that point, five seconds'. OpenSim checks no permission.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_agent(
    ctx: &mut ScriptCtx<'_>,
    agent: String,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    teleport_by_position(ctx, &agent, position, look_at);
    Ok(())
}

/// `osTeleportAgent(agent, position, lookat)`'s body, which
/// `osTeleportOwner(position, lookat)` shares.
fn teleport_by_position(ctx: &mut ScriptCtx<'_>, agent: &str, position: Vector, look_at: Vector) {
    let Some(key) = teleportable(ctx, agent) else {
        return;
    };
    let caller = ctx.caller();
    let (width, depth) = ctx.host().region_size(caller);
    let (x, y) = (f64::from(position.x), f64::from(position.y));
    if (0.0..f64::from(width)).contains(&x) && (0.0..f64::from(depth)).contains(&y) {
        send(ctx, &key, &Destination::Local { position, look_at });
        ctx.delay(0.5);
        return;
    }
    let region = ctx.host().region(caller);
    let (global_x, global_y) = (x + region.corner.0, y + region.corner.1);
    let (cell_x, cell_y) = ((global_x / CELL).trunc(), (global_y / CELL).trunc());
    send(
        ctx,
        &key,
        &Destination::Global {
            corner: (cell_x * CELL, cell_y * CELL),
            position: Vector {
                x: single(global_x - cell_x * CELL),
                y: single(global_y - cell_y * CELL),
                z: position.z,
            },
            look_at,
        },
    );
    ctx.delay(5.0);
}

/// A double as LSL's float.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "OpenSim's double becomes the value model's single precision float"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

/// `osTeleportAgent` to the grid's region of that name: a teleport within
/// the region for an empty name or its own, half a second's sleep; to the
/// other region, five seconds'.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_agent_region(
    ctx: &mut ScriptCtx<'_>,
    agent: String,
    region: String,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    teleport_by_name(ctx, &agent, &region, position, look_at);
    Ok(())
}

/// `osTeleportAgent` by region name's body (`TeleportAgent`).
fn teleport_by_name(
    ctx: &mut ScriptCtx<'_>,
    agent: &str,
    region: &str,
    position: Vector,
    look_at: Vector,
) {
    let Some(key) = teleportable(ctx, agent) else {
        return;
    };
    let caller = ctx.caller();
    let here = ctx.host().region(caller).name;
    if region.is_empty() || region.to_lowercase() == here.to_lowercase() {
        send(ctx, &key, &Destination::Local { position, look_at });
        ctx.delay(0.5);
    } else {
        send(
            ctx,
            &key,
            &Destination::Region {
                name: region.to_owned(),
                position,
                look_at,
            },
        );
        ctx.delay(5.0);
    }
}

/// `osTeleportAgent` to the region at the grid cell `(x, y)`, five seconds'
/// sleep.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_agent_cell(
    ctx: &mut ScriptCtx<'_>,
    agent: String,
    x: i32,
    y: i32,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    teleport_by_cell(ctx, &agent, (x, y), position, look_at);
    Ok(())
}

/// `osTeleportAgent` by grid cell's body.
fn teleport_by_cell(
    ctx: &mut ScriptCtx<'_>,
    agent: &str,
    (x, y): (i32, i32),
    position: Vector,
    look_at: Vector,
) {
    let Some(key) = teleportable(ctx, agent) else {
        return;
    };
    // `(uint)regionGridX`: a negative cell wraps as the grid's handle does.
    let cell = |value: i32| f64::from(value.cast_unsigned()) * CELL;
    send(
        ctx,
        &key,
        &Destination::Global {
            corner: (cell(x), cell(y)),
            position,
            look_at,
        },
    );
    ctx.delay(5.0);
}

/// `osTeleportOwner` to a position, within the region or beyond it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_owner(
    ctx: &mut ScriptCtx<'_>,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    teleport_by_position(ctx, &owner, position, look_at);
    Ok(())
}

/// `osTeleportOwner` to the grid's region of that name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_owner_region(
    ctx: &mut ScriptCtx<'_>,
    region: String,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    teleport_by_name(ctx, &owner, &region, position, look_at);
    Ok(())
}

/// `osTeleportOwner` to the region at a grid cell.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_teleport_owner_cell(
    ctx: &mut ScriptCtx<'_>,
    x: i32,
    y: i32,
    position: Vector,
    look_at: Vector,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    teleport_by_cell(ctx, &owner, (x, y), position, look_at);
    Ok(())
}

/// `osLocalTeleportAgent`: move an avatar within the region with no teleport
/// sequence (`ScenePresence.LocalTeleport`): kept a tenth of a metre inside
/// the region — a velocity out of it zeroed — and above the ground, given the
/// velocity with `OS_LTPAG_USEVEL`, and, with any of the look, align, fly
/// and no-fly flags, told of the move as a local teleport looking along the
/// look-at (`OS_LTPAG_USELOOKAT`) or the velocity (`OS_LTPAG_ALGNLV`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_local_teleport_agent(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    position: Vector,
    velocity: Vector,
    look_at: Vector,
    flags: i32,
) -> Result<(), CallError> {
    let Some(key) = parsed(&agent.0) else {
        return Ok(());
    };
    let caller = ctx.caller();
    if ctx.host().agent(caller, &key).is_none() {
        return Ok(());
    }
    let Some(at) = position_of(ctx, &key) else {
        return Ok(());
    };
    if !may_teleport(ctx, &key, &at) {
        return Ok(());
    }
    let (width, depth) = ctx.host().region_size(caller);
    let mut position = position;
    let mut velocity = velocity;
    let keep = |value: &mut f32, speed: &mut f32, size: u32| {
        let size = single(f64::from(size));
        if *value <= 0.0 {
            *value = 0.1;
            if *speed < 0.0 {
                *speed = 0.0;
            }
        } else if *value >= size {
            *value = size - 0.1;
            if *speed > 0.0 {
                *speed = 0.0;
            }
        }
    };
    keep(&mut position.x, &mut velocity.x, width);
    keep(&mut position.y, &mut velocity.y, depth);
    let floor = ctx
        .host()
        .ground(caller, position.x.trunc(), position.y.trunc());
    let size = ctx
        .host()
        .agent(caller, &key)
        .map_or(1.6, |agent| agent.size.z);
    let lowest = floor + size * 0.5 + 0.1;
    if position.z < lowest {
        position.z = lowest;
    }
    let use_velocity = flags & OS_LTPAG_USEVEL != 0;
    let announced = (flags & LOCAL_TELEPORT_ANNOUNCED != 0).then(|| {
        let fly = if flags & OS_LTPAG_FORCEFLY != 0 {
            Some(true)
        } else if flags & LOCAL_TELEPORT_NO_FLY != 0 {
            Some(false)
        } else {
            None
        };
        let along = if flags & OS_LTPAG_USELOOKAT != 0 {
            Some(look_at.clone())
        } else if flags & OS_LTPAG_ALGNLV != 0 && use_velocity {
            Some(velocity.clone())
        } else if flags & OS_LTPAG_ALGNLV != 0 {
            ctx.host()
                .object_details(caller, &key)
                .map(|details| details.velocity)
        } else {
            None
        };
        let look = along.and_then(|mut along| {
            along.z = 0.0;
            let length = along.x.hypot(along.y);
            if length > 0.0 {
                along.x /= length;
                along.y /= length;
            }
            (along.x.abs() > 0.001 || along.y.abs() > 0.001).then_some(along)
        });
        (look, fly)
    });
    send(
        ctx,
        &key,
        &Destination::Moved {
            position,
            velocity: use_velocity.then_some(velocity),
            announced,
        },
    );
    Ok(())
}

// ----------------------------------------------------------------------
// Health and speed.
// ----------------------------------------------------------------------

/// The vitals of `agent`, an avatar or NPC in the region.
fn vitals_of(ctx: &mut ScriptCtx<'_>, agent: &str) -> Option<Vitals> {
    let caller = ctx.caller();
    ctx.host().vitals(caller, agent)
}

/// Keep `vitals` for `agent`.
fn keep_vitals(ctx: &mut ScriptCtx<'_>, agent: &str, vitals: Vitals) {
    let caller = ctx.caller();
    ctx.host().set_vitals(caller, agent, vitals);
}

/// `osGetHealth`: the avatar's health, -1 for one not in the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_health(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<f32, CallError> {
    Ok(parsed(&agent.0)
        .filter(|key| key != NULL_KEY)
        .and_then(|key| vitals_of(ctx, &key))
        .map_or(-1.0, |vitals| vitals.health))
}

/// `osGetHealRate`: what the avatar heals by in a second, 0 for one not in
/// the region.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_heal_rate(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<f32, CallError> {
    Ok(parsed(&agent.0)
        .and_then(|key| vitals_of(ctx, &key))
        .map_or(0.0, |vitals| vitals.heal_rate))
}

/// `osCauseDamage`: the avatar loses health, where the land under the
/// caller's prim allows damage. At 0 an NPC is removed; an avatar is told
/// "You died!", healed to full and sent home.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_cause_damage(ctx: &mut ScriptCtx<'_>, avatar: Key, damage: f32) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar.0) else {
        return Ok(());
    };
    let Some(mut vitals) = vitals_of(ctx, &key) else {
        return Ok(());
    };
    let caller = ctx.caller();
    let here = ctx.host().placement(caller).position;
    let damaging = ctx
        .host()
        .parcel_at(caller, here.x, here.y)
        .is_some_and(|parcel| parcel.flags & PARCEL_FLAG_ALLOW_DAMAGE != 0);
    if !damaging {
        return Ok(());
    }
    vitals.health -= damage;
    keep_vitals(ctx, &key, vitals);
    if vitals.health > 0.0 {
        return Ok(());
    }
    if npc_of(ctx, &key).is_some() {
        ctx.host().npc_act(caller, &key, &NpcAction::Remove);
        return Ok(());
    }
    ctx.host().alert(caller, &key, "You died!", true);
    vitals.health = 100.0;
    keep_vitals(ctx, &key, vitals);
    send(ctx, &key, &Destination::Home);
    Ok(())
}

/// `osCauseHealing`: the avatar gains health, up to 100.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_cause_healing(
    ctx: &mut ScriptCtx<'_>,
    avatar: Key,
    healing: f32,
) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar.0) else {
        return Ok(());
    };
    if let Some(mut vitals) = vitals_of(ctx, &key) {
        vitals.health = (vitals.health + healing).min(100.0);
        keep_vitals(ctx, &key, vitals);
    }
    Ok(())
}

/// `osSetHealth`: the avatar's health, between 1 and 100.
///
/// # Errors
///
/// [`Exception::InvalidUuid`] for a key that is none.
pub fn os_set_health(ctx: &mut ScriptCtx<'_>, avatar: Key, health: f32) -> Result<(), CallError> {
    let key = constructed(BuiltinId::OsSetHealth, &avatar.0)?;
    if let Some(mut vitals) = vitals_of(ctx, &key) {
        vitals.health = health.clamp(1.0, 100.0);
        keep_vitals(ctx, &key, vitals);
    }
    Ok(())
}

/// `osSetHealRate`: what the avatar heals by in a second, between 0 and 100.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_heal_rate(ctx: &mut ScriptCtx<'_>, avatar: Key, rate: f32) -> Result<(), CallError> {
    let Some(key) = parsed(&avatar.0) else {
        return Ok(());
    };
    if let Some(mut vitals) = vitals_of(ctx, &key) {
        vitals.heal_rate = rate.clamp(0.0, 100.0);
        keep_vitals(ctx, &key, vitals);
    }
    Ok(())
}

/// `osSetSpeed`: the avatar moves the factor times as fast.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_speed(ctx: &mut ScriptCtx<'_>, avatar: String, factor: f32) -> Result<(), CallError> {
    if let Some(key) = parsed(&avatar).filter(|key| present(ctx, key)) {
        let caller = ctx.caller();
        ctx.host().set_speed(caller, &key, factor);
    }
    Ok(())
}

/// `osSetOwnerSpeed`: the owner moves the factor times as fast, at most 4.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_owner_speed(ctx: &mut ScriptCtx<'_>, factor: f32) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    if present(ctx, &owner) {
        ctx.host()
            .set_speed(caller, &owner, factor.min(MAX_OWNER_SPEED));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// Kicks and groups.
// ----------------------------------------------------------------------

/// `osKickAvatar` by key: the avatar is kicked from the region with the
/// alert; an NPC is removed.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_kick_avatar(ctx: &mut ScriptCtx<'_>, agent: Key, alert: String) -> Result<(), CallError> {
    let Some(key) = parsed(&agent.0).filter(|key| key != NULL_KEY) else {
        return Ok(());
    };
    if present(ctx, &key) {
        let caller = ctx.caller();
        ctx.host().kick(caller, &key, &alert);
    }
    Ok(())
}

/// `osKickAvatar` by name: every avatar of the region named exactly so.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_kick_avatar_named(
    ctx: &mut ScriptCtx<'_>,
    first: String,
    last: String,
    alert: String,
) -> Result<(), CallError> {
    if let Some(key) = present_named(ctx, &first, &last) {
        let caller = ctx.caller();
        ctx.host().kick(caller, &key, &alert);
    }
    Ok(())
}

/// The caller's prim's group, when it is set and the prim is not deeded to
/// it — what an invitation or an ejection is made in.
fn inviting_group(ctx: &mut ScriptCtx<'_>) -> Option<String> {
    let group = own_group(ctx);
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    (group != NULL_KEY && !group.eq_ignore_ascii_case(&owner)).then_some(group)
}

/// `osInviteToGroup`: invite an avatar in the region into the prim's group,
/// in the name of the prim's owner, who must hold the power to: 1 when it is
/// invited, 2 when it is a member already, 0 for anything else.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_invite_to_group(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<i32, CallError> {
    let Some(key) = parsed(&agent.0) else {
        return Ok(0);
    };
    let Some(group) = inviting_group(ctx) else {
        return Ok(0);
    };
    let caller = ctx.caller();
    if ctx.host().agent(caller, &key).is_none() || npc_of(ctx, &key).is_some() {
        return Ok(0);
    }
    if ctx.host().group_powers(&group, &key).is_some() {
        return Ok(2);
    }
    let owner = ctx.host().owner(caller);
    let powers = ctx.host().group_powers(&group, &owner).unwrap_or(0);
    if powers & POWER_INVITE == 0 {
        return Ok(0);
    }
    ctx.host().invite_to_group(caller, &group, &key);
    Ok(1)
}

/// `osEjectFromGroup`: eject an avatar from the prim's group, in the name of
/// the prim's owner, who must hold the power to: 1 when asked, else 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_eject_from_group(ctx: &mut ScriptCtx<'_>, agent: Key) -> Result<i32, CallError> {
    let Some(key) = parsed(&agent.0) else {
        return Ok(0);
    };
    let Some(group) = inviting_group(ctx) else {
        return Ok(0);
    };
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let powers = ctx.host().group_powers(&group, &owner).unwrap_or(0);
    if powers & POWER_EJECT == 0 {
        return Ok(0);
    }
    ctx.host().eject_from_group(caller, &group, &key);
    Ok(1)
}
