//! OpenSim's own functions that shadow the prim tranche's
//! (`server-lsl-lib-prim-state`, from `server-lsl-lib-ossl`): the rule lists
//! of other prims, links forced without the permission, projectors,
//! particles and the `os*Sound*` family.
//!
//! Each is `OSSL_Api.cs`'s, which mostly hands on to the `ll*` function it
//! shadows: the same rules apply, on the OpenSim flavours alone (Second Life
//! does not compile them). The region's `[OSSL]` settings have already
//! admitted the call (`vm::ossl::admit`) by the time one runs.

use sl_types::lsl::Vector;

use crate::library::effects::ll_link_particle_system_as;
use crate::library::prim_params::set::{INVENTORY_SOUND, asset_named};
use crate::library::prim_params::{self, LINK_THIS, unit};
use crate::library::prim_state::{change_linked, own, write};
use crate::library::{BuiltinId, CallError, Key};
use crate::value::{Element, NULL_KEY};
use crate::vm::ScriptCtx;
use crate::vm::comms::Linked;
use crate::vm::prim::{AttachedSound, Prim, SoundCue};

/// `osGetPrimitiveParams`: the answer to `rules` of the prim keyed `prim`,
/// any prim of the region (`LSL_Api.GetPrimitiveParamsEx`).
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws: a face it cannot index.
pub fn os_get_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    prim: Key,
    rules: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    prim_params::answer(ctx, "osGetPrimitiveParams", &[prim.0], &rules)
}

/// `osSetPrimitiveParams`: applies `rules` to the prim keyed `prim`, which
/// its owner must also own (`LSL_Api.SetPrimitiveParamsEx`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    prim: Key,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let owner = ctx.host().owner(caller);
    let owned = ctx
        .host()
        .object_details(caller, &prim.0)
        .is_some_and(|details| !details.avatar && details.owner.eq_ignore_ascii_case(&owner));
    if owned {
        prim_params::apply_rules(ctx, "osSetPrimitiveParams", &[prim.0], &rules);
    }
    Ok(())
}

/// `osGetLinkPrimitiveParams`: the answer to `rules` of the prims `link`
/// names, every prim of a set in link order.
///
/// # Errors
///
/// [`CallError::Exception`] where OpenSim throws.
pub fn os_get_link_primitive_params(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    rules: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    prim_params::link_get_params(ctx, "osGetLinkPrimitiveParams", link, &rules)
}

/// `osGetLinkNumber`: the link number of the first prim or sitter of the
/// linkset named `name`, −1 for none (`SceneObjectGroup.GetLinkNumber`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_get_link_number(ctx: &mut ScriptCtx<'_>, name: String) -> Result<i32, CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    if let Some(prim) = links.prims.iter().find(|prim| prim.name == name) {
        return Ok(prim.link);
    }
    let prims = i32::try_from(links.prims.len()).unwrap_or(i32::MAX);
    Ok(links
        .sitters
        .iter()
        .position(|sitter| sitter.name == name)
        .and_then(|index| i32::try_from(index).ok())
        .map_or(-1, |index| prims.saturating_add(index).saturating_add(1)))
}

/// `osForceCreateLink`: `llCreateLink` without `PERMISSION_CHANGE_LINKS`; a
/// target that is no prim the owner may link is nothing, silently.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_create_link(
    ctx: &mut ScriptCtx<'_>,
    target: String,
    parent: i32,
) -> Result<(), CallError> {
    if !crate::library::dotnet::is_uuid(&target) {
        return Ok(());
    }
    let caller = ctx.caller();
    let _refused = ctx.host().create_link(caller, &target, parent != 0);
    Ok(())
}

/// `osForceBreakLink`: `llBreakLink` without `PERMISSION_CHANGE_LINKS`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_break_link(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<(), CallError> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    if links.prims.len() < 2 {
        return Ok(());
    }
    if let Some(Linked::Prim(prim)) = links.numbered(link) {
        ctx.host().break_link(caller, &prim.key);
    }
    Ok(())
}

/// `osForceBreakAllLinks`: `llBreakAllLinks` without
/// `PERMISSION_CHANGE_LINKS`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_force_break_all_links(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let caller = ctx.caller();
    if ctx.host().links(caller).prims.len() > 1 {
        ctx.host().break_all_links(caller);
    }
    Ok(())
}

/// `osClearObjectAnimations`: stops the animations the object's animated
/// mesh plays and answers how many. Nothing on the fake grid starts one yet
/// (`llStartObjectAnimation` is the avatar tranche's), so it is 0.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn os_clear_object_animations(_ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(0)
}

/// Writes a projector to `prim` (`OSSL_Api.SetProjectionParams`): on when
/// `projection` is not 0, projecting `texture` with the field of view, focus
/// and ambience given.
fn project(prim: &mut Prim, projection: i32, texture: &str, values: [f32; 3]) {
    let [fov, focus, ambience] = values;
    prim.projector.on = projection != 0;
    texture.clone_into(&mut prim.projector.texture);
    prim.projector.fov = fov;
    prim.projector.focus = focus;
    prim.projector.ambience = ambience;
}

/// `osSetProjectionParams(prim, …)`: a projector on the prim keyed `prim`,
/// the caller's own for `NULL_KEY`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_projection_params(
    ctx: &mut ScriptCtx<'_>,
    prim: Key,
    projection: i32,
    texture: Key,
    fov: f32,
    focus: f32,
    amb: f32,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let found = if prim.0 == NULL_KEY {
        own(ctx)
    } else {
        ctx.host().prim(caller, &prim.0)
    };
    if let Some(mut found) = found {
        project(&mut found, projection, &texture.0, [fov, focus, amb]);
        write(ctx, &found, &prim_params::Changes::default());
    }
    Ok(())
}

/// `osSetProjectionParams(link, …)`: a projector on the prim numbered
/// `link`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_link_projection_params(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    projection: i32,
    texture: Key,
    fov: f32,
    focus: f32,
    amb: f32,
) -> Result<(), CallError> {
    let link = if link == LINK_THIS || link < 2 {
        LINK_THIS
    } else {
        link
    };
    change_linked(ctx, link, |_ctx, prim, _changes| {
        project(prim, projection, &texture.0, [fov, focus, amb]);
    });
    Ok(())
}

/// `osSetProjectionParams(projection, …)`: a projector on the caller's own
/// prim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_own_projection_params(
    ctx: &mut ScriptCtx<'_>,
    projection: i32,
    texture: Key,
    fov: f32,
    focus: f32,
    amb: f32,
) -> Result<(), CallError> {
    os_set_link_projection_params(ctx, LINK_THIS, projection, texture, fov, focus, amb)
}

/// `osParticleSystem`: `llParticleSystem`.
///
/// # Errors
///
/// [`CallError::Exception`] for a rule missing its value.
pub fn os_particle_system(ctx: &mut ScriptCtx<'_>, rules: Vec<Element>) -> Result<(), CallError> {
    ll_link_particle_system_as(ctx, BuiltinId::OsParticleSystem, LINK_THIS, &rules)
}

/// `osLinkParticleSystem`: `llLinkParticleSystem`.
///
/// # Errors
///
/// [`CallError::Exception`] for a rule missing its value.
pub fn os_link_particle_system(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    ll_link_particle_system_as(ctx, BuiltinId::OsLinkParticleSystem, link, &rules)
}

/// The key of the single prim `link` names (`GetSingleLinkPart`): the
/// caller's own for `LINK_THIS`, the root for `LINK_ROOT`, the prim of that
/// number.
fn single(ctx: &mut ScriptCtx<'_>, link: i32) -> Option<String> {
    let caller = ctx.caller();
    let links = ctx.host().links(caller);
    match links.numbered(link) {
        Some(Linked::Prim(prim)) => Some(prim.key.clone()),
        _ if link == prim_params::LINK_ROOT || (link == 0 && links.prims.len() == 1) => {
            links.prims.first().map(|prim| prim.key.clone())
        }
        _ => None,
    }
}

/// The sound `sound` names for the prim keyed `prim`: a key, or a sound of
/// that name in the prim; silently nothing otherwise, as OpenSim is.
fn os_sound(ctx: &mut ScriptCtx<'_>, sound: &str) -> Option<String> {
    asset_named(ctx, sound, INVENTORY_SOUND).filter(|key| key != NULL_KEY)
}

/// Sends `cue` from the prim `link` names.
fn os_cue(ctx: &mut ScriptCtx<'_>, link: i32, cue: impl FnOnce(String) -> SoundCue, sound: &str) {
    let Some(prim) = single(ctx, link) else {
        return;
    };
    let Some(sound) = os_sound(ctx, sound) else {
        return;
    };
    let caller = ctx.caller();
    ctx.host().sound(caller, &prim, &cue(sound));
}

/// `osPlaySound`: plays `sound` once from the prim numbered `link`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_play_sound(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_cue(
        ctx,
        link,
        |sound| SoundCue::Play {
            sound,
            volume: unit(volume),
            slave: false,
        },
        &sound,
    );
    Ok(())
}

/// `osPlaySoundSlave`: [`os_play_sound`] in step with the sync master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_play_sound_slave(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_cue(
        ctx,
        link,
        |sound| SoundCue::Play {
            sound,
            volume: unit(volume),
            slave: true,
        },
        &sound,
    );
    Ok(())
}

/// `osTriggerSound`: triggers `sound` where the prim numbered `link` is.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_trigger_sound(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_cue(
        ctx,
        link,
        |sound| SoundCue::Trigger {
            sound,
            volume: unit(volume),
            bounds: None,
        },
        &sound,
    );
    Ok(())
}

/// `osTriggerSoundLimited`: [`os_trigger_sound`] heard only inside the box.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_trigger_sound_limited(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
    top_north_east: Vector,
    bottom_south_west: Vector,
) -> Result<(), CallError> {
    os_cue(
        ctx,
        link,
        |sound| SoundCue::Trigger {
            sound,
            volume: unit(volume),
            bounds: Some((bottom_south_west, top_north_east)),
        },
        &sound,
    );
    Ok(())
}

/// `osPreloadSound`: has the viewers in earshot fetch `sound`; a second's
/// sleep, as `OSSL_Api` sleeps it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_preload_sound(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
) -> Result<(), CallError> {
    let preloaded = single(ctx, link).is_some() && os_sound(ctx, &sound).is_some();
    os_cue(ctx, link, |sound| SoundCue::Preload { sound }, &sound);
    if preloaded {
        ctx.sleep(1.0);
    }
    Ok(())
}

/// Puts the prim `link` names on `sound`, looping, master or slave as given.
fn os_loop(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: &str,
    volume: f32,
    master: bool,
    slave: bool,
) {
    let Some(key) = single(ctx, link) else {
        return;
    };
    let Some(sound) = os_sound(ctx, sound) else {
        return;
    };
    let caller = ctx.caller();
    let Some(mut prim) = ctx.host().prim(caller, &key) else {
        return;
    };
    prim.sound.playing = Some(AttachedSound {
        sound,
        volume: unit(volume),
        looped: true,
        master,
        slave,
    });
    write(ctx, &prim, &prim_params::Changes::default());
}

/// `osLoopSound`: the prim numbered `link` plays `sound` until stopped.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_loop_sound(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_loop(ctx, link, &sound, volume, false, false);
    Ok(())
}

/// `osLoopSoundMaster`: [`os_loop_sound`], the object's sync master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_loop_sound_master(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_loop(ctx, link, &sound, volume, true, false);
    Ok(())
}

/// `osLoopSoundSlave`: [`os_loop_sound`], in step with the sync master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_loop_sound_slave(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    os_loop(ctx, link, &sound, volume, false, true);
    Ok(())
}

/// `osStopSound`: stops what each prim `link` names plays on.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_stop_sound(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<(), CallError> {
    change_linked(ctx, link, |_ctx, prim, _changes| prim.sound.playing = None);
    Ok(())
}

/// `osSetSoundRadius`: how far the prim numbered `link`'s sounds carry.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_set_sound_radius(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    radius: f32,
) -> Result<(), CallError> {
    if let Some(key) = single(ctx, link) {
        let caller = ctx.caller();
        if let Some(mut prim) = ctx.host().prim(caller, &key) {
            prim.sound.radius = radius;
            write(ctx, &prim, &prim_params::Changes::default());
        }
    }
    Ok(())
}

/// `osAdjustSoundVolume`: the volume the prim numbered `link` plays on at.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_adjust_sound_volume(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    volume: f32,
) -> Result<(), CallError> {
    if let Some(key) = single(ctx, link) {
        let caller = ctx.caller();
        if let Some(mut prim) = ctx.host().prim(caller, &key) {
            if let Some(playing) = prim.sound.playing.as_mut() {
                playing.volume = unit(volume);
            }
            write(ctx, &prim, &prim_params::Changes::default());
        }
    }
    Ok(())
}

/// `osCollisionSound`: the prim's collision sound. An empty name with a
/// volume of 0 silences collisions, of 1 restores the grid's own sound; a
/// name that is no sound silences them too (`OSSL_Api.osCollisionSound`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn os_collision_sound(
    ctx: &mut ScriptCtx<'_>,
    impact_sound: String,
    impact_volume: f32,
) -> Result<(), CallError> {
    let collision = if impact_sound.is_empty() {
        impact_volume
            .total_cmp(&1.0)
            .is_ne()
            .then(|| (NULL_KEY.to_owned(), impact_volume))
    } else {
        Some(os_sound(ctx, &impact_sound).map_or_else(
            || (NULL_KEY.to_owned(), 0.0),
            |sound| (sound, impact_volume),
        ))
    };
    if let Some(mut prim) = own(ctx) {
        prim.sound.collision = collision;
        write(ctx, &prim, &prim_params::Changes::default());
    }
    Ok(())
}
