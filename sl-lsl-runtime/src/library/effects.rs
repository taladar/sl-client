//! Library tranche: what a prim does that no function reads back
//! (`server-lsl-lib-prim-state`, its sound and media group) — sounds,
//! particles, texture animation — and the face media a prim shows.
//!
//! A sound played once or preloaded goes to the host as a [`SoundCue`]; what
//! the prim plays on, its particles and its texture animation are the
//! prim's, written back with it — and a loop, a stop and a change of volume
//! are cued to the host as well, for a grid that tells the viewers of them in
//! a message of their own (`effects-sounds`, `gridspec/avatars.md` § A
//! script's sounds). The grids' rules are the `prim/effects`
//! probe's (book chapter `gridspec/lsl.md`, § Sound, particles, texture
//! animation and media): Second Life names a sound or texture that is not
//! there on the debug channel and OpenSim is silent; OpenSim throws on a
//! particle rule missing its value, where Second Life carries on.

use sl_types::lsl::Vector;

use crate::library::prim_params::set::{INVENTORY_SOUND, asset_named, set_media, texture_named};
use crate::library::prim_params::{self, Changes, LINK_THIS, set_targets, unit};
use crate::library::prim_state::{change_linked, own, write};
use crate::library::{CallError, Exception};
use crate::value::{Element, NULL_KEY};
use crate::vm::prim::{
    AttachedSound, Media, ParticleSystem, ParticleValue, Prim, SoundCue, TextureAnim,
};
use crate::vm::{EngineFlavour, ScriptCtx};

/// `SOUND_LOOP`: `llLinkPlaySound` plays on.
const SOUND_LOOP: i32 = 1;
/// `SOUND_TRIGGER`: `llLinkPlaySound` triggers the sound where the prim is.
const SOUND_TRIGGER: i32 = 2;
/// `SOUND_SYNC`: `llLinkPlaySound` waits for the object's sync master.
const SOUND_SYNC: i32 = 4;

/// The sound a sound argument names: a key, or the sound of that name in
/// the prim. Second Life names one it cannot find on the debug channel —
/// `NULL_KEY` and `""` among them — and the complaint costs the script a
/// second (`errors/flood-paths`' `cost`); OpenSim is silent (`prim/effects`).
fn sound_named(ctx: &mut ScriptCtx<'_>, given: &str) -> Option<String> {
    let found = asset_named(ctx, given, INVENTORY_SOUND).filter(|key| key != NULL_KEY);
    if found.is_none() && !ctx.flavour().is_opensim() {
        ctx.refuse(format!("Could not find sound '{given}'."));
        ctx.error_pause();
    }
    found
}

/// Sends `cue` from the caller's own prim.
fn cue(ctx: &mut ScriptCtx<'_>, sound: &SoundCue) {
    if let Some(key) = prim_params::own_key(ctx) {
        let caller = ctx.caller();
        ctx.host().sound(caller, &key, sound);
    }
}

/// Puts the prims `link` names on `playing` — [`None`] to stop them, which
/// a prim that played on states as stopped.
fn play_on(ctx: &mut ScriptCtx<'_>, link: i32, playing: Option<&AttachedSound>) {
    change_linked(ctx, link, |_ctx, prim, _changes| {
        let was = prim.sound.playing.is_some();
        prim.sound.playing = playing.cloned();
        prim.sound.stopped = playing.is_none() && (was || prim.sound.stopped);
    });
}

/// `llPlaySound`: plays `sound` once from the prim, at `volume` (clamped).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_play_sound(ctx: &mut ScriptCtx<'_>, sound: String, volume: f32) -> Result<(), CallError> {
    if let Some(sound) = sound_named(ctx, &sound) {
        cue(
            ctx,
            &SoundCue::Play {
                sound,
                volume: unit(volume),
                slave: false,
            },
        );
        // A sound played after a stop ends the stop the prim stated.
        if own(ctx).is_some_and(|prim| prim.sound.stopped) {
            change_linked(ctx, LINK_THIS, |_ctx, prim, _changes| {
                prim.sound.stopped = false;
            });
        }
    }
    Ok(())
}

/// `llPlaySoundSlave`: [`ll_play_sound`], in step with the object's sync
/// master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_play_sound_slave(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    if let Some(sound) = sound_named(ctx, &sound) {
        cue(
            ctx,
            &SoundCue::Play {
                sound,
                volume: unit(volume),
                slave: true,
            },
        );
    }
    Ok(())
}

/// Puts the prim on `sound`, looping, as master or slave as given.
fn loop_sound(ctx: &mut ScriptCtx<'_>, sound: &str, volume: f32, master: bool, slave: bool) {
    let Some(sound) = sound_named(ctx, sound) else {
        return;
    };
    let playing = AttachedSound {
        sound,
        volume: unit(volume),
        looped: true,
        master,
        slave,
    };
    play_on(ctx, LINK_THIS, Some(&playing));
    cue(ctx, &SoundCue::Loop(playing));
}

/// `llLoopSound`: plays `sound` from the prim until stopped.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_loop_sound(ctx: &mut ScriptCtx<'_>, sound: String, volume: f32) -> Result<(), CallError> {
    loop_sound(ctx, &sound, volume, false, false);
    Ok(())
}

/// `llLoopSoundMaster`: [`ll_loop_sound`], the object's sync master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_loop_sound_master(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    loop_sound(ctx, &sound, volume, true, false);
    Ok(())
}

/// `llLoopSoundSlave`: [`ll_loop_sound`], in step with the sync master.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_loop_sound_slave(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    loop_sound(ctx, &sound, volume, false, true);
    Ok(())
}

/// `llTriggerSound`: plays `sound` once where the prim is, not following it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_trigger_sound(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
) -> Result<(), CallError> {
    if let Some(sound) = sound_named(ctx, &sound) {
        cue(
            ctx,
            &SoundCue::Trigger {
                sound,
                volume: unit(volume),
                bounds: None,
            },
        );
    }
    Ok(())
}

/// `llTriggerSoundLimited`: [`ll_trigger_sound`], heard only inside the box
/// between `top_north_east` and `bottom_south_west`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_trigger_sound_limited(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
    top_north_east: Vector,
    bottom_south_west: Vector,
) -> Result<(), CallError> {
    if let Some(sound) = sound_named(ctx, &sound) {
        cue(
            ctx,
            &SoundCue::Trigger {
                sound,
                volume: unit(volume),
                bounds: Some((bottom_south_west, top_north_east)),
            },
        );
    }
    Ok(())
}

/// `llStopSound`: stops what the prim plays on; nothing when it plays
/// nothing (`prim/effects`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_stop_sound(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    let mut stopped = false;
    change_linked(ctx, LINK_THIS, |_ctx, prim, _changes| {
        if prim.sound.playing.take().is_some() {
            prim.sound.stopped = true;
            stopped = true;
        }
    });
    if stopped {
        cue(ctx, &SoundCue::Stop);
    }
    Ok(())
}

/// `llPreloadSound`: has the viewers in earshot fetch `sound`. OpenSim
/// takes no sleep for a sound it cannot find.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_preload_sound(ctx: &mut ScriptCtx<'_>, sound: String) -> Result<(), CallError> {
    if let Some(sound) = sound_named(ctx, &sound) {
        cue(ctx, &SoundCue::Preload { sound });
    } else if ctx.flavour().is_opensim() {
        ctx.delay(0.0);
    }
    Ok(())
}

/// `llSoundPreload`: the old name of [`ll_preload_sound`].
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sound_preload(ctx: &mut ScriptCtx<'_>, sound: String) -> Result<(), CallError> {
    ll_preload_sound(ctx, sound)
}

/// `llSound`: the old sound call — loop or play once, queued or not.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sound(
    ctx: &mut ScriptCtx<'_>,
    sound: String,
    volume: f32,
    queue: i32,
    looped: i32,
) -> Result<(), CallError> {
    ll_set_sound_queueing(ctx, queue)?;
    if looped != 0 {
        ll_loop_sound(ctx, sound, volume)
    } else {
        ll_play_sound(ctx, sound, volume)
    }
}

/// `llSetSoundQueueing`: whether a sound waits for the one playing.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_sound_queueing(ctx: &mut ScriptCtx<'_>, queue: i32) -> Result<(), CallError> {
    ll_link_set_sound_queueing(ctx, LINK_THIS, queue)
}

/// `llLinkSetSoundQueueing`: [`ll_set_sound_queueing`] for the prims `link`
/// names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_set_sound_queueing(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    queue: i32,
) -> Result<(), CallError> {
    change_linked(ctx, link, |_ctx, prim, _changes| {
        prim.sound.queueing = queue != 0;
    });
    Ok(())
}

/// `llSetSoundRadius`: how far the prim's sounds carry; nothing is said of
/// one past its range (`prim/effects`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_sound_radius(ctx: &mut ScriptCtx<'_>, radius: f32) -> Result<(), CallError> {
    ll_link_set_sound_radius(ctx, LINK_THIS, radius)
}

/// `llLinkSetSoundRadius`: [`ll_set_sound_radius`] for the prims `link`
/// names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_set_sound_radius(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    radius: f32,
) -> Result<(), CallError> {
    change_linked(ctx, link, |_ctx, prim, _changes| {
        prim.sound.radius = radius.max(0.0);
    });
    Ok(())
}

/// `llAdjustSoundVolume`: the volume of what the prim plays on, clamped.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_adjust_sound_volume(ctx: &mut ScriptCtx<'_>, volume: f32) -> Result<(), CallError> {
    let mut playing = false;
    change_linked(ctx, LINK_THIS, |_ctx, prim, _changes| {
        if let Some(sound) = prim.sound.playing.as_mut() {
            sound.volume = unit(volume);
            playing = true;
        }
    });
    if playing {
        cue(
            ctx,
            &SoundCue::Adjust {
                volume: unit(volume),
            },
        );
    }
    Ok(())
}

/// `llLinkAdjustSoundVolume`: [`ll_adjust_sound_volume`] for the prims
/// `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_adjust_sound_volume(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    volume: f32,
) -> Result<(), CallError> {
    change_linked(ctx, link, |_ctx, prim, _changes| {
        if let Some(playing) = prim.sound.playing.as_mut() {
            playing.volume = unit(volume);
        }
    });
    Ok(())
}

/// `llLinkPlaySound`: plays `sound` from each prim `link` names, as `flags`
/// say: once, looping (`SOUND_LOOP`), triggered where the prim is
/// (`SOUND_TRIGGER`), in step with the sync master (`SOUND_SYNC`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_play_sound(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    sound: String,
    volume: f32,
    flags: i32,
) -> Result<(), CallError> {
    let Some(sound) = sound_named(ctx, &sound) else {
        return Ok(());
    };
    let volume = unit(volume);
    if flags & SOUND_LOOP != 0 {
        let playing = AttachedSound {
            sound,
            volume,
            looped: true,
            master: false,
            slave: flags & SOUND_SYNC != 0,
        };
        play_on(ctx, link, Some(&playing));
        return Ok(());
    }
    let caller = ctx.caller();
    for key in set_targets(ctx, link) {
        let played = if flags & SOUND_TRIGGER != 0 {
            SoundCue::Trigger {
                sound: sound.clone(),
                volume,
                bounds: None,
            }
        } else {
            SoundCue::Play {
                sound: sound.clone(),
                volume,
                slave: flags & SOUND_SYNC != 0,
            }
        };
        ctx.host().sound(caller, &key, &played);
    }
    Ok(())
}

/// `llLinkStopSound`: [`ll_stop_sound`] for the prims `link` names.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_link_stop_sound(ctx: &mut ScriptCtx<'_>, link: i32) -> Result<(), CallError> {
    play_on(ctx, link, None);
    Ok(())
}

/// `llCollisionSound`: the sound the prim makes when it collides — `""` the
/// grid's own, `NULL_KEY` none. A name that is not in the prim is kept
/// silently on both grids (`prim/effects`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_collision_sound(
    ctx: &mut ScriptCtx<'_>,
    impact_sound: String,
    impact_volume: f32,
) -> Result<(), CallError> {
    let sound = if impact_sound.is_empty() {
        None
    } else {
        Some(asset_named(ctx, &impact_sound, INVENTORY_SOUND).unwrap_or(impact_sound))
    };
    let Some(mut prim) = own(ctx) else {
        return Ok(());
    };
    prim.sound.collision = sound.map(|sound| (sound, unit(impact_volume)));
    write(ctx, &prim, &Changes::default());
    Ok(())
}

/// The value type a particle rule takes, and its name for Second Life's
/// complaint.
const fn particle_rule(code: i32) -> Option<(ParticleKind, &'static str)> {
    use ParticleKind::{Float, Integer, Key, Vector};
    Some(match code {
        0 => (Integer, "PSYS_PART_FLAGS"),
        1 => (Vector, "PSYS_PART_START_COLOR"),
        2 => (Float, "PSYS_PART_START_ALPHA"),
        3 => (Vector, "PSYS_PART_END_COLOR"),
        4 => (Float, "PSYS_PART_END_ALPHA"),
        5 => (Vector, "PSYS_PART_START_SCALE"),
        6 => (Vector, "PSYS_PART_END_SCALE"),
        7 => (Float, "PSYS_PART_MAX_AGE"),
        8 => (Vector, "PSYS_SRC_ACCEL"),
        9 => (Integer, "PSYS_SRC_PATTERN"),
        10 => (Float, "PSYS_SRC_INNERANGLE"),
        11 => (Float, "PSYS_SRC_OUTERANGLE"),
        12 => (Key, "PSYS_SRC_TEXTURE"),
        13 => (Float, "PSYS_SRC_BURST_RATE"),
        15 => (Integer, "PSYS_SRC_BURST_PART_COUNT"),
        16 => (Float, "PSYS_SRC_BURST_RADIUS"),
        17 => (Float, "PSYS_SRC_BURST_SPEED_MIN"),
        18 => (Float, "PSYS_SRC_BURST_SPEED_MAX"),
        19 => (Float, "PSYS_SRC_MAX_AGE"),
        20 => (Key, "PSYS_SRC_TARGET_KEY"),
        21 => (Vector, "PSYS_SRC_OMEGA"),
        22 => (Float, "PSYS_SRC_ANGLE_BEGIN"),
        23 => (Float, "PSYS_SRC_ANGLE_END"),
        24 => (Integer, "PSYS_PART_BLEND_FUNC_SOURCE"),
        25 => (Integer, "PSYS_PART_BLEND_FUNC_DEST"),
        26 => (Float, "PSYS_PART_START_GLOW"),
        27 => (Float, "PSYS_PART_END_GLOW"),
        _ => return None,
    })
}

/// The type a particle rule's value has.
#[derive(Debug, Clone, Copy)]
enum ParticleKind {
    /// An integer.
    Integer,
    /// A float.
    Float,
    /// A vector.
    Vector,
    /// A key or a name.
    Key,
}

/// Reads a particle system from `rules`: each code followed by its value. An
/// unknown code is skipped with its value; a value of the wrong type is
/// refused on Second Life with `PSYS error: Rule <n> error, <name> needs
/// float data.` and skipped silently on OpenSim; a texture is found by name.
/// A code without its value is nothing more on Second Life and throws on
/// OpenSim (`prim/effects`).
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a rule missing its value.
fn particles_of(
    ctx: &mut ScriptCtx<'_>,
    id: crate::library::BuiltinId,
    rules: &[Element],
) -> Result<ParticleSystem, CallError> {
    let opensim_flavour = ctx.flavour().is_opensim();
    let mut system = ParticleSystem::default();
    for (index, pair) in rules.chunks(2).enumerate() {
        let (Some(code), Some(value)) = (pair.first(), pair.get(1)) else {
            if opensim_flavour {
                return Err(CallError::Exception {
                    id,
                    exception: Exception::IndexOutOfRange,
                });
            }
            break;
        };
        let Element::Integer(code) = code else {
            continue;
        };
        let Some((kind, name)) = particle_rule(*code) else {
            continue;
        };
        let read = match (kind, value) {
            (ParticleKind::Integer, Element::Integer(integer)) => {
                Some(ParticleValue::Integer(*integer))
            }
            (ParticleKind::Float, Element::Float(float)) => Some(ParticleValue::Float(*float)),
            (ParticleKind::Float, Element::Integer(integer)) => {
                Some(ParticleValue::Float(crate::num::int_to_float(*integer)))
            }
            (ParticleKind::Vector, Element::Vector(vector)) => {
                Some(ParticleValue::Vector(vector.clone()))
            }
            (ParticleKind::Key, Element::String(text) | Element::Key(text)) => {
                let key = if *code == 12 {
                    texture_named(ctx, text).unwrap_or_else(|| NULL_KEY.to_owned())
                } else {
                    text.clone()
                };
                Some(ParticleValue::Key(key))
            }
            _ => None,
        };
        let Some(read) = read else {
            if !opensim_flavour {
                let wanted = match kind {
                    ParticleKind::Integer => "integer",
                    ParticleKind::Float => "float",
                    ParticleKind::Vector => "vector",
                    ParticleKind::Key => "key",
                };
                let rule = index.saturating_add(1);
                ctx.refuse_to_owner(format!(
                    "PSYS error: Rule {rule} error, {name} needs {wanted} data."
                ));
            }
            continue;
        };
        system.rules.push((*code, read));
    }
    Ok(system)
}

/// `llParticleSystem`: the prim's particle system; an empty list stops it.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a rule missing its value.
pub fn ll_particle_system(ctx: &mut ScriptCtx<'_>, rules: Vec<Element>) -> Result<(), CallError> {
    ll_link_particle_system_as(
        ctx,
        crate::library::BuiltinId::LlParticleSystem,
        LINK_THIS,
        &rules,
    )
}

/// `llLinkParticleSystem`: [`ll_particle_system`] for the prims `link`
/// names; a link naming none is nothing (`prim/effects`).
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a rule missing its value.
pub fn ll_link_particle_system(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    rules: Vec<Element>,
) -> Result<(), CallError> {
    ll_link_particle_system_as(
        ctx,
        crate::library::BuiltinId::LlLinkParticleSystem,
        link,
        &rules,
    )
}

/// The particle system `rules` names, set on the prims `link` names, the
/// throw reported against `id`.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a rule missing its value.
pub(crate) fn ll_link_particle_system_as(
    ctx: &mut ScriptCtx<'_>,
    id: crate::library::BuiltinId,
    link: i32,
    rules: &[Element],
) -> Result<(), CallError> {
    let system = particles_of(ctx, id, rules)?;
    let particles = (!rules.is_empty()).then_some(system);
    change_linked(ctx, link, |_ctx, prim, _changes| {
        prim.particles.clone_from(&particles);
    });
    Ok(())
}

/// `llSetTextureAnim`: the prim's texture animation; a mode without
/// `ANIM_ON` stops it. Nothing is said of zero frames, a negative rate or a
/// face past the prim's (`prim/effects`).
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(clippy::too_many_arguments, reason = "the library's signature")]
pub fn ll_set_texture_anim(
    ctx: &mut ScriptCtx<'_>,
    mode: i32,
    face: i32,
    sizex: i32,
    sizey: i32,
    start: f32,
    length: f32,
    rate: f32,
) -> Result<(), CallError> {
    ll_set_link_texture_anim(
        ctx, LINK_THIS, mode, face, sizex, sizey, start, length, rate,
    )
}

/// `llSetLinkTextureAnim`: [`ll_set_texture_anim`] for the prims `link`
/// names.
///
/// # Errors
///
/// None; the signature is the library's.
#[expect(clippy::too_many_arguments, reason = "the library's signature")]
pub fn ll_set_link_texture_anim(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    mode: i32,
    face: i32,
    sizex: i32,
    sizey: i32,
    start: f32,
    length: f32,
    rate: f32,
) -> Result<(), CallError> {
    let anim = TextureAnim {
        mode,
        face,
        size_x: sizex,
        size_y: sizey,
        start,
        length,
        rate,
    };
    change_linked(ctx, link, |_ctx, prim, _changes| {
        prim.texture_anim = (mode & 1 != 0).then(|| anim.clone());
    });
    Ok(())
}

/// `STATUS_OK`.
const STATUS_OK: i32 = 0;
/// `STATUS_MALFORMED_PARAMS`.
const STATUS_MALFORMED_PARAMS: i32 = 1000;
/// `STATUS_BOUNDS_ERROR`.
const STATUS_BOUNDS_ERROR: i32 = 1002;
/// `STATUS_NOT_FOUND`.
const STATUS_NOT_FOUND: i32 = 1003;

/// `PRIM_MEDIA_MAX_URL_LENGTH`.
const MAX_URL_LENGTH: usize = 1024;
/// `PRIM_MEDIA_MAX_WIDTH_PIXELS` and `PRIM_MEDIA_MAX_HEIGHT_PIXELS`.
const MAX_PIXELS: i32 = 2048;

/// The face `face` of the prim, which media takes one at a time: the index,
/// or [`None`] past its faces. Second Life names a missing face on the
/// debug channel (`prim/effects`).
fn media_face(ctx: &mut ScriptCtx<'_>, prim: &Prim, face: i32) -> Option<usize> {
    let found = usize::try_from(face)
        .ok()
        .filter(|index| *index < prim.media.len());
    if found.is_none() && !ctx.flavour().is_opensim() {
        ctx.refuse(format!(
            "PRIM_MEDIA error: face {face} on object {} not found",
            prim.key
        ));
    }
    found
}

/// Why a media rule list was refused.
enum MediaRefusal {
    /// A code no media rule has, at the pair's index.
    Malformed(usize, i32),
    /// A value past its limit, at the pair's index.
    Bounds(usize, i32),
}

/// `media` with the rules `params` applied: each a `PRIM_MEDIA_*` code and
/// its value. Second Life refuses a URL past 1024 characters and a size past
/// 2048 pixels; OpenSim keeps them (`prim/effects`).
fn media_with(
    media: &Media,
    params: &[Element],
    opensim_flavour: bool,
) -> Result<Media, MediaRefusal> {
    let mut media = media.clone();
    for (index, pair) in params.chunks(2).enumerate() {
        let (Some(Element::Integer(code)), Some(value)) = (pair.first(), pair.get(1)) else {
            return Err(MediaRefusal::Malformed(index, 0));
        };
        let integer = match value {
            Element::Integer(integer) => *integer,
            _ => 0,
        };
        let text = crate::cast::element_to_string(value);
        let flag = integer != 0;
        let long_url = !opensim_flavour && text.chars().count() > MAX_URL_LENGTH;
        let big = !opensim_flavour && integer > MAX_PIXELS;
        match *code {
            0 => media.alt_image_enable = flag,
            1 => media.controls = integer,
            2 | 3 if long_url => return Err(MediaRefusal::Bounds(index, *code)),
            2 => media.current_url = text,
            3 => media.home_url = text,
            4 => media.auto_loop = flag,
            5 => media.auto_play = flag,
            6 => media.auto_scale = flag,
            7 => media.auto_zoom = flag,
            8 => media.first_click_interact = flag,
            9 | 10 if big => return Err(MediaRefusal::Bounds(index, *code)),
            9 => media.width = integer,
            10 => media.height = integer,
            11 => media.whitelist_enable = flag,
            12 => media.whitelist = text,
            13 => media.perms_interact = integer,
            14 => media.perms_control = integer,
            other => return Err(MediaRefusal::Malformed(index, other)),
        }
    }
    Ok(media)
}

/// Whether a set or a clear of media on `prim` raises `CHANGED_MEDIA` under
/// `flavour`: every set and every clear does, but a clear on OpenSim of a
/// prim that never had media, which finds no media list to clear
/// (`events/timer-reset`). XEngine raises it after the call's forced delay,
/// which it takes first, so a timer that fell due in the delay runs before
/// it.
const fn media_raises(flavour: EngineFlavour, prim: &Prim, clear: bool) -> bool {
    !(clear && flavour.is_opensim() && !prim.has_media_list)
}

/// Sets media on face `face` of the prim keyed `key`, or clears it.
fn media_set(
    ctx: &mut ScriptCtx<'_>,
    function: &str,
    key: &str,
    face: i32,
    params: Option<&[Element]>,
) -> i32 {
    let caller = ctx.caller();
    let Some(mut prim) = ctx.host().prim(caller, key) else {
        return STATUS_NOT_FOUND;
    };
    let Some(index) = media_face(ctx, &prim, face) else {
        return STATUS_NOT_FOUND;
    };
    let opensim_flavour = ctx.flavour().is_opensim();
    let media = match params {
        None => None,
        Some(params) => {
            let current = prim.media.get(index).cloned().flatten().unwrap_or_default();
            match media_with(&current, params, opensim_flavour) {
                Ok(media) => Some(media),
                Err(refusal) => {
                    let (status, kind, at, code) = match refusal {
                        MediaRefusal::Malformed(at, code) => {
                            (STATUS_MALFORMED_PARAMS, "Malformed parameters", at, code)
                        }
                        MediaRefusal::Bounds(at, code) => {
                            (STATUS_BOUNDS_ERROR, "Bounds error", at, code)
                        }
                    };
                    if !opensim_flavour {
                        ctx.refuse(format!(
                            "{function} error: {kind} near parameter #{at}, param={code}.  No values have been set."
                        ));
                    }
                    return status;
                }
            }
        }
    };
    let raises = media_raises(ctx.flavour(), &prim, params.is_none());
    let mut changes = Changes::default();
    set_media(&mut prim, face, media.as_ref(), &mut changes);
    let events: &[i32] = if raises {
        &[prim_params::CHANGED_MEDIA]
    } else {
        &[]
    };
    ctx.set_prim(&prim, events);
    STATUS_OK
}

/// The answer to `params` for face `face` of the prim keyed `key`: each
/// code's value, in the order asked; nothing for a face without media. An
/// unknown code answers nothing on Second Life and `1000` on OpenSim
/// (`prim/effects`).
fn media_get(ctx: &mut ScriptCtx<'_>, key: &str, face: i32, params: &[Element]) -> Vec<Element> {
    let caller = ctx.caller();
    let Some(prim) = ctx.host().prim(caller, key) else {
        return Vec::new();
    };
    let Some(index) = media_face(ctx, &prim, face) else {
        return Vec::new();
    };
    let opensim_flavour = ctx.flavour().is_opensim();
    let media = prim.media.get(index).cloned().flatten();
    let Some(media) = media else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for code in params {
        let Element::Integer(code) = code else {
            continue;
        };
        let flag = |value: bool| Element::Integer(i32::from(value));
        out.push(match code {
            0 => flag(media.alt_image_enable),
            1 => Element::Integer(media.controls),
            2 => Element::String(media.current_url.clone()),
            3 => Element::String(media.home_url.clone()),
            4 => flag(media.auto_loop),
            5 => flag(media.auto_play),
            6 => flag(media.auto_scale),
            7 => flag(media.auto_zoom),
            8 => flag(media.first_click_interact),
            9 => Element::Integer(media.width),
            10 => Element::Integer(media.height),
            11 => flag(media.whitelist_enable),
            12 => Element::String(media.whitelist.clone()),
            13 => Element::Integer(media.perms_interact),
            14 => Element::Integer(media.perms_control),
            _ if opensim_flavour => Element::Integer(STATUS_MALFORMED_PARAMS),
            _ => continue,
        });
    }
    out
}

/// The key of the prim numbered `link`, as the media link functions take a
/// single prim.
fn single(ctx: &mut ScriptCtx<'_>, link: i32) -> Option<String> {
    set_targets(ctx, link).into_iter().next()
}

/// `llSetPrimMediaParams`: sets media on face `face` from `params`; the
/// status (`STATUS_OK`, `STATUS_NOT_FOUND` for a face past the prim's, …).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_prim_media_params(
    ctx: &mut ScriptCtx<'_>,
    face: i32,
    params: Vec<Element>,
) -> Result<i32, CallError> {
    ll_set_link_media_as(ctx, "llSetPrimMediaParams", LINK_THIS, face, &params)
}

/// `llSetLinkMedia`: [`ll_set_prim_media_params`] for the prim numbered
/// `link`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_set_link_media(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    face: i32,
    params: Vec<Element>,
) -> Result<i32, CallError> {
    ll_set_link_media_as(ctx, "llSetLinkMedia", link, face, &params)
}

/// Media set on face `face` of the prim numbered `link`, refusals worded for
/// `function`.
fn ll_set_link_media_as(
    ctx: &mut ScriptCtx<'_>,
    function: &str,
    link: i32,
    face: i32,
    params: &[Element],
) -> Result<i32, CallError> {
    let Some(key) = single(ctx, link) else {
        return Ok(STATUS_NOT_FOUND);
    };
    Ok(media_set(ctx, function, &key, face, Some(params)))
}

/// `llGetPrimMediaParams`: face `face`'s media, as `params` asks.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_prim_media_params(
    ctx: &mut ScriptCtx<'_>,
    face: i32,
    params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    ll_get_link_media(ctx, LINK_THIS, face, params)
}

/// `llGetLinkMedia`: [`ll_get_prim_media_params`] for the prim numbered
/// `link`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_link_media(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    face: i32,
    params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let Some(key) = single(ctx, link) else {
        return Ok(Vec::new());
    };
    Ok(media_get(ctx, &key, face, &params))
}

/// `llClearPrimMedia`: clears face `face`'s media; `STATUS_NOT_FOUND` for a
/// face past the prim's (`prim/effects`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_clear_prim_media(ctx: &mut ScriptCtx<'_>, face: i32) -> Result<i32, CallError> {
    ll_clear_link_media(ctx, LINK_THIS, face)
}

/// `llClearLinkMedia`: [`ll_clear_prim_media`] for the prim numbered `link`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_clear_link_media(
    ctx: &mut ScriptCtx<'_>,
    link: i32,
    face: i32,
) -> Result<i32, CallError> {
    let Some(key) = single(ctx, link) else {
        return Ok(STATUS_NOT_FOUND);
    };
    Ok(media_set(ctx, "llClearLinkMedia", &key, face, None))
}
