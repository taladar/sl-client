//! In-world spatial sounds (`viewer-in-world-sounds`): the `llTriggerSound`
//! one-shots, looped / attached object sounds, and the sound-gain and preload
//! side-signals the simulator already sends but the viewer never consumed.
//!
//! The receive protocol (`protocol-22`) is done and rides in on
//! [`SlEvent`](sl_client_bevy::SlEvent):
//!
//! - [`SlSessionEvent::SoundTrigger`] — a fire-and-forget spatial one-shot at a
//!   given region position and gain.
//! - [`SlSessionEvent::AttachedSound`] — a sound bound to an object, carrying
//!   [`SoundFlags`](sl_client_bevy::SoundFlags) (`LOOP` / `STOP` / the sync +
//!   queue bits); it follows the object as it moves and stops when the object is
//!   removed.
//! - [`SlSessionEvent::AttachedSoundGainChange`] — a live gain change for an
//!   object's attached sound, applied without restarting the loop.
//! - [`SlSessionEvent::PreloadSound`] — a hint to fetch a clip before it is
//!   triggered, so the trigger is not late.
//!
//! Everything plays through the one shared [`Mixer`] on its [`Bus::Sfx`], decoded
//! once by the [`SoundCache`](crate::sound_cache::SoundCache) and spatialised
//! against the listener the camera drives. The mixer's own source cap and
//! priority eviction (`sl_audio`'s [`VoicePool`](sl_audio::VoicePool)) handle the
//! "SL asks for more sounds than any device wants" problem, so this module never
//! has to cap voices itself.
//!
//! Muting is honoured up front: a sound whose owner **or** object is on the mute
//! list ([`MuteModel`](crate::world_api::MuteModel)) is never started.
//!
//! **Parcel-local sound** (`SOUND_LOCAL`) is honoured through
//! [`ParcelAudibility`], the reference viewer's `LLViewerParcelMgr::canHearSound`
//! reduced to the data we decode: a one-shot the agent cannot hear is dropped,
//! and an inaudible attached sound is driven to silence (kept time-coherent, it
//! returns when the agent re-enters the parcel).
//!
//! **Collision sounds** ([`ingest_collisions`]): a scripted `llCollisionSound`
//! already arrives as an ordinary [`SoundTrigger`](SlSessionEvent::SoundTrigger)
//! and needs nothing extra, so this module adds the viewer-*synthesized* layer —
//! a material-default sound when two physical prims meet, detected each frame by
//! parry-narrowphase contact tests over the moving-collider set
//! ([`DynamicColliders`], fired on the contact *edge*). Coverage is prim–prim only
//! (avatars and terrain carry no collider); impact-scaled gain is not attempted
//! because server-driven kinematic movers give no reliable contact velocity.
//!
//! Not yet done here, tracked on the roadmap task: sample-accurate **sync master
//! / slave** and **queue** semantics (looped attached sounds play, but not
//! phase-locked across objects), and the phase-2 **occlusion / HRTF** pass. A
//! one-shot / attached sound plays pan + distance + rolloff today, which is the
//! reference's floor.

use std::collections::{HashMap, HashSet};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sl_audio::{AudioMixer as _, Bus, Importance, Mixer, SpatialParams, VoiceId};
use sl_client_bevy::{
    AssetKey, MuteFlags, ObjectKey, ParcelFlags, RegionHandle, SlAgentParcel, SlEvent, SlIdentity,
    SlParcelOverlay, SlSessionEvent, Uuid,
};

use crate::coords::{bevy_to_sl_vec, region_offset_bevy, sl_to_bevy_vec};
use crate::derender::DerenderKind;
use crate::objects::{ObjectState, SceneObject};
use crate::raycast_index::DynamicColliders;
use crate::settings::ViewerSettings;
use crate::sound_cache::SoundCache;
use crate::world_api::MuteModel;

/// The persisted-settings section [`SETTING_COLLISION_SOUNDS`] lives under.
const AUDIO_SECTION: &[&str] = &["audio"];

/// The reference `EnableCollisionSounds` setting name: whether the
/// viewer-synthesized material collision sounds ([`ingest_collisions`]) play.
/// On by default, like the reference. Scripted `llCollisionSound`s arrive as
/// ordinary `SoundTrigger`s and are deliberately not gated by this.
pub(crate) const SETTING_COLLISION_SOUNDS: &str = "EnableCollisionSounds";

/// The parcel-local (`SOUND_LOCAL`) audibility check, grouped so the sound
/// systems take one param rather than three. Mirrors the reference viewer's
/// `LLViewerParcelMgr::canHearSound`.
#[derive(SystemParam)]
pub(crate) struct ParcelAudibility<'w> {
    /// The decoded parcel-overlay grids (per region) carrying the per-square
    /// `sound_local` bit.
    overlay: Res<'w, SlParcelOverlay>,
    /// The agent's current parcel (its membership bitmap and flags).
    agent_parcel: Res<'w, SlAgentParcel>,
    /// The agent's identity, read for its current region handle, so a sound in a
    /// different region is known not to be in the agent's parcel.
    identity: Res<'w, SlIdentity>,
}

impl ParcelAudibility<'_> {
    /// The agent's current region handle, if known.
    fn agent_region(&self) -> Option<RegionHandle> {
        self.identity.region_handle
    }

    /// Whether a sound at region-local (`x`, `y`) in `sound_region` is audible to
    /// the agent under parcel-local (`SOUND_LOCAL`) clamping.
    fn audible(&self, sound_region: RegionHandle, x: f32, y: f32) -> bool {
        let agent_parcel = self.agent_parcel.current.as_ref();
        let in_agent_parcel = self.agent_region() == Some(sound_region)
            && agent_parcel.is_some_and(|parcel| parcel.contains_point(x, y));
        let agent_parcel_sound_local =
            agent_parcel.is_some_and(|parcel| parcel.flags().contains(ParcelFlags::SOUND_LOCAL));
        let source_sound_local = self
            .overlay
            .grid_of(sound_region)
            .and_then(|grid| grid.cell_at_region_local(x, y))
            .is_some_and(|cell| cell.sound_local);
        audible_from_flags(
            in_agent_parcel,
            agent_parcel_sound_local,
            source_sound_local,
        )
    }
}

/// The reference viewer's `canHearSound` decision reduced to its three booleans:
/// a sound in the agent's own parcel is always heard; otherwise a sound-local
/// agent parcel hears nothing outside it, and a sound-local *source* parcel is
/// not heard from outside; anything else is audible.
const fn audible_from_flags(
    in_agent_parcel: bool,
    agent_parcel_sound_local: bool,
    source_sound_local: bool,
) -> bool {
    if in_agent_parcel {
        return true;
    }
    if agent_parcel_sound_local {
        return false;
    }
    !source_sound_local
}

/// How long a queued one-shot waits for its clip to decode before it is dropped
/// as too late to be worth playing (seconds). A trigger whose asset has not
/// arrived within this window is stale — the moment it marked has passed.
const ONESHOT_MAX_WAIT_SECONDS: f32 = 4.0;

/// A `SoundTrigger` one-shot whose clip is still being fetched / decoded, held
/// until the clip is ready (then played) or it goes stale ([`ONESHOT_MAX_WAIT_SECONDS`]).
struct PendingOneShot {
    /// The sound asset to play.
    sound: AssetKey,
    /// The world (scene-space) position to play it at.
    position: Vec3,
    /// The linear gain in `[0.0, 1.0]`.
    gain: f32,
    /// The wall-clock time ([`Time::elapsed_secs`]) the trigger arrived, for the
    /// staleness cutoff.
    enqueued: f32,
}

/// A sound attached to an object: it follows the object's world position and
/// keeps playing (looped) until the object stops it or is removed.
struct AttachedSoundVoice {
    /// The sound asset bound to the object.
    sound: AssetKey,
    /// The current linear gain in `[0.0, 1.0]` (an `AttachedSoundGainChange`
    /// updates this live).
    gain: f32,
    /// Whether the sound loops (the `LOOP` flag); a non-looping attached sound is
    /// removed once it finishes.
    looped: bool,
    /// The playing voice in the mixer, or `None` while the clip is still being
    /// fetched / decoded.
    voice: Option<VoiceId>,
}

/// The in-world sound state: one-shots waiting on their clip, the attached-sound
/// voices by object, and voices to stop next frame.
#[derive(Resource, Default)]
pub(crate) struct WorldSounds {
    /// `SoundTrigger` one-shots whose clip has not decoded yet.
    pending_oneshots: Vec<PendingOneShot>,
    /// Attached sounds by object id.
    attached: HashMap<ObjectKey, AttachedSoundVoice>,
    /// Voices to stop on the next drive pass (an object's sound was replaced or
    /// explicitly stopped); the mixer is not available in the event-ingest
    /// system, so the stop is deferred to [`drive_world_sounds`].
    stopping: Vec<VoiceId>,
    /// The last time (seconds) a collision sound played for an unordered pair of
    /// collider entities (keyed by their sorted bit ids), so a jittering resting
    /// contact does not machine-gun. Pruned each pass to stay bounded.
    collision_cooldowns: HashMap<(u64, u64), f32>,
    /// The unordered prim pairs that were touching last frame, so a sound fires
    /// only on the *start* of a contact (the edge), matching avian's
    /// `CollisionStart` — a continuously-resting pair stays silent.
    touching_pairs: HashSet<(u64, u64)>,
}

/// Ingest the four sound events into [`WorldSounds`] and prefetch their clips.
///
/// This has no access to the [`Mixer`] (an ingest system, not the audio pump);
/// it only records intent and requests decodes. [`drive_world_sounds`] turns that
/// into actual voices once the clips are ready and the mixer is in hand.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the event stream, the \
              clock, the clip cache, the sound state, the object mirror a position needs, and \
              the three gates a sound passes (mute list, asset blacklist, parcel audibility)"
)]
pub(crate) fn ingest_world_sound_events(
    mut events: MessageReader<SlEvent>,
    time: Res<Time>,
    mut cache: ResMut<SoundCache>,
    mut sounds: ResMut<WorldSounds>,
    state: Res<ObjectState>,
    mutes: Res<MuteModel>,
    derender: Res<crate::derender::DerenderList>,
    parcel: ParcelAudibility,
) {
    let now = time.elapsed_secs();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::SoundTrigger {
                sound_id,
                owner_id,
                object_id,
                position,
                region_handle,
                gain,
                ..
            } => {
                if muted(&mutes, *owner_id, object_id.uuid()) {
                    continue;
                }
                // A blacklisted sound asset is never played, whoever triggers it
                // (`viewer-derender-blacklist`, the reference's
                // `isBlacklisted(sound_id, AT_SOUND)` in `process_sound_trigger`).
                if derender.blacklists(*sound_id, DerenderKind::Sound) {
                    continue;
                }
                // Parcel-local (`SOUND_LOCAL`) clamp: a one-shot the agent cannot
                // hear from where they stand is dropped outright (it is
                // instantaneous, so re-evaluation never matters).
                if !parcel.audible(*region_handle, position.x, position.y) {
                    continue;
                }
                let sound = AssetKey::from(*sound_id);
                cache.request(sound);
                // Region-local position into absolute scene space, component-wise
                // (a whole-`Vec3` `+` trips `arithmetic_side_effects`).
                let local = sl_to_bevy_vec(position);
                let offset = region_offset_bevy(*region_handle, state.origin());
                let scene = Vec3::new(local.x + offset.x, local.y + offset.y, local.z + offset.z);
                sounds.pending_oneshots.push(PendingOneShot {
                    sound,
                    position: scene,
                    gain: *gain,
                    enqueued: now,
                });
            }
            SlSessionEvent::AttachedSound {
                sound_id,
                object_id,
                owner_id,
                gain,
                flags,
            } => {
                // A STOP flag (or a null sound) removes the object's attached
                // sound rather than starting one.
                if flags.is_stop() || sound_id.is_nil() {
                    if let Some(previous) = sounds.attached.remove(object_id)
                        && let Some(voice) = previous.voice
                    {
                        sounds.stopping.push(voice);
                    }
                    continue;
                }
                if muted(&mutes, *owner_id, object_id.uuid())
                    || derender.blacklists(*sound_id, DerenderKind::Sound)
                {
                    continue;
                }
                let sound = AssetKey::from(*sound_id);
                cache.request(sound);
                let looped = flags.is_loop();
                match sounds.attached.get_mut(object_id) {
                    // Same sound on the same object: just adopt the new gain /
                    // loop flag, keeping the running voice time-coherent.
                    Some(existing) if existing.sound == sound => {
                        existing.gain = *gain;
                        existing.looped = looped;
                    }
                    // A different sound replaces the old one: stop the old voice
                    // and start fresh.
                    _replaced => {
                        if let Some(previous) = sounds.attached.insert(
                            *object_id,
                            AttachedSoundVoice {
                                sound,
                                gain: *gain,
                                looped,
                                voice: None,
                            },
                        ) && let Some(voice) = previous.voice
                        {
                            sounds.stopping.push(voice);
                        }
                    }
                }
            }
            SlSessionEvent::AttachedSoundGainChange { object_id, gain } => {
                if let Some(existing) = sounds.attached.get_mut(object_id) {
                    existing.gain = *gain;
                }
            }
            SlSessionEvent::PreloadSound { sounds: preloads } => {
                for preload in preloads {
                    // No point warming a clip that will never be played.
                    if derender.blacklists(preload.sound_id, DerenderKind::Sound) {
                        continue;
                    }
                    cache.request(AssetKey::from(preload.sound_id));
                }
            }
            _other => {}
        }
    }
}

/// Turn the ingested intent into mixer voices each frame: stop the voices flagged
/// for removal, start any one-shot / attached sound whose clip has decoded, keep
/// attached voices on their object and at their current gain, and reap attached
/// sounds whose object is gone or whose one-shot went stale.
pub(crate) fn drive_world_sounds(
    mixer: Option<NonSendMut<Mixer>>,
    time: Res<Time>,
    cache: Res<SoundCache>,
    mut sounds: ResMut<WorldSounds>,
    state: Res<ObjectState>,
    globals: Query<&GlobalTransform>,
    parcel: ParcelAudibility,
) {
    let Some(mut mixer) = mixer else {
        // No audio device: do not let intent accumulate unboundedly.
        sounds.pending_oneshots.clear();
        sounds.stopping.clear();
        return;
    };

    for voice in sounds.stopping.drain(..) {
        mixer.stop_voice(voice);
    }

    realize_oneshots(&mut mixer, &cache, &mut sounds, time.elapsed_secs());
    drive_attached(&mut mixer, &cache, &mut sounds, &state, &globals, &parcel);
}

/// Whether an attached sound on an object at Bevy scene position `scene` is
/// audible under the parcel-local clamp: the object's scene position is mapped
/// back to region-local coordinates in the agent's region (attached sounds the
/// agent hears are almost always in-region; a genuinely cross-region object then
/// reads as "outside the agent's parcel", which is the safe answer). Without a
/// known region there is no clamp.
fn scene_audible(parcel: &ParcelAudibility, state: &ObjectState, scene: Vec3) -> bool {
    let Some(region) = parcel.agent_region() else {
        return true;
    };
    let offset = region_offset_bevy(region, state.origin());
    // Component-wise (a whole-`Vec3` `-` trips `arithmetic_side_effects`).
    let local = bevy_to_sl_vec(Vec3::new(
        scene.x - offset.x,
        scene.y - offset.y,
        scene.z - offset.z,
    ));
    parcel.audible(region, local.x, local.y)
}

/// Play every pending one-shot whose clip is ready, drop the ones whose asset
/// turned out unavailable or that waited too long, and keep the rest.
fn realize_oneshots(mixer: &mut Mixer, cache: &SoundCache, sounds: &mut WorldSounds, now: f32) {
    let mut still_pending = Vec::new();
    for pending in std::mem::take(&mut sounds.pending_oneshots) {
        if let Some(clip) = cache.clip(pending.sound) {
            let _voice = mixer.play_spatial(
                clip,
                SpatialParams {
                    bus: Bus::Sfx,
                    gain: pending.gain,
                    importance: Importance::OneShot,
                    looped: false,
                    position: pending.position.to_array(),
                },
            );
        } else if cache.is_unavailable(pending.sound)
            || now - pending.enqueued > ONESHOT_MAX_WAIT_SECONDS
        {
            // Failed to fetch, or too late to matter — drop it.
        } else {
            still_pending.push(pending);
        }
    }
    sounds.pending_oneshots = still_pending;
}

/// Keep each attached sound playing on its object: start it once the clip is
/// ready, follow the object's world position, apply the current gain, and remove
/// it when the object is gone or a non-looping sound has finished.
fn drive_attached(
    mixer: &mut Mixer,
    cache: &SoundCache,
    sounds: &mut WorldSounds,
    state: &ObjectState,
    globals: &Query<&GlobalTransform>,
    parcel: &ParcelAudibility,
) {
    let mut finished: Vec<ObjectKey> = Vec::new();
    for (&object_id, attached) in &mut sounds.attached {
        // Resolve the object's current world position; a missing entity means the
        // object was removed, so stop and forget the sound.
        let Some(entity) = state.entity_of(object_id) else {
            if let Some(voice) = attached.voice.take() {
                mixer.stop_voice(voice);
            }
            finished.push(object_id);
            continue;
        };
        let position = globals
            .get(entity)
            .map(|transform| transform.translation())
            .unwrap_or_default();

        // Parcel-local clamp: an inaudible attached sound is driven to silence
        // (gain 0) rather than stopped, so it stays time-coherent and comes back
        // when the agent re-enters the parcel.
        let effective_gain = if scene_audible(parcel, state, position) {
            attached.gain
        } else {
            0.0
        };

        match attached.voice {
            None => {
                if let Some(clip) = cache.clip(attached.sound) {
                    attached.voice = mixer.play_spatial(
                        clip,
                        SpatialParams {
                            bus: Bus::Sfx,
                            gain: effective_gain,
                            importance: Importance::Attached,
                            looped: attached.looped,
                            position: position.to_array(),
                        },
                    );
                } else if cache.is_unavailable(attached.sound) {
                    finished.push(object_id);
                }
            }
            Some(voice) => {
                if mixer.is_playing(voice) {
                    mixer.set_voice_position(voice, position.to_array());
                    mixer.set_voice_gain(voice, effective_gain);
                } else {
                    // A non-looping attached sound ran to its end (or the mixer
                    // evicted it under source pressure): forget it.
                    attached.voice = None;
                    if !attached.looped {
                        finished.push(object_id);
                    }
                }
            }
        }
    }
    for object_id in finished {
        let _removed = sounds.attached.remove(&object_id);
    }
}

/// Whether a sound from `owner` on `object` is muted (either the owner or the
/// object itself is on the agent's mute list) — honouring the per-entry
/// **object-sounds exception**, so a mute whose "Block Object Sounds" toggle
/// is off still lets that source be heard (the reference's
/// `LLMute::flagObjectSounds`).
fn muted(mutes: &MuteModel, owner: Uuid, object: Uuid) -> bool {
    mutes.is_muted_aspect(owner, MuteFlags::ALLOW_OBJECT_SOUNDS)
        || mutes.is_muted_aspect(object, MuteFlags::ALLOW_OBJECT_SOUNDS)
}

/// The minimum time (seconds) between collision sounds for the same pair of
/// objects, so a jittering resting contact does not machine-gun.
const COLLISION_COOLDOWN_SECONDS: f32 = 0.15;

/// The gain a synthesized collision sound plays at (the Sfx bus volume applies on
/// top). Impact-scaled gain is not attempted: our physical prims are kinematic
/// (server-driven), so there is no reliable contact velocity to scale by.
const COLLISION_GAIN: f32 = 1.0;

/// The reference viewer's default **same-material** collision sound for a prim of
/// material byte `material` (`LL_MCODE_*`), from Firestorm's `sound_ids.cpp`.
/// `LIGHT` (7) and anything unrecognised map to plastic, as the reference does.
///
/// This is a deliberate reduction of the reference's full material-**pair**
/// matrix to the primary object's own material, which covers the common
/// like-on-like case (wood on wood, stone on stone) and keeps the table small;
/// the cross-material entries are a known simplification.
const fn collision_sound_str(material: u8) -> &'static str {
    match material {
        0 => "9538f37c-456e-4047-81be-6435045608d4", // stone
        1 => "9e5c1297-6eed-40c0-825a-d9bcd86e3193", // metal
        2 => "6a45ba0b-5775-4ea8-8513-26008a17f873", // glass
        3 => "063c97d3-033a-4e9b-98d8-05c8074922cb", // wood
        4 => "dce5fdd4-afe4-4ea1-822f-dd52cac46b08", // flesh
        6 => "153c8bf7-fb89-4d89-b263-47e58b1b4774", // rubber
        _ => "0e24a717-b97e-4b77-9c94-b59a5a88b2da", // plastic / light / unknown
    }
}

/// A canonical (order-independent) key for a pair of collider entities.
const fn pair_key(a: Entity, b: Entity) -> (u64, u64) {
    let (a, b) = (a.to_bits(), b.to_bits());
    if a <= b { (a, b) } else { (b, a) }
}

/// Play a material-default collision sound when two physical prims begin
/// touching (`viewer-in-world-sounds`): each frame, [`DynamicColliders`] reports
/// the prim pairs currently in contact (parry narrowphase); for each pair that
/// was *not* touching last frame — the contact edge — map the colliders back to
/// their objects, pick the primary object's material default, and enqueue a
/// one-shot at the contact point, honouring mute, the parcel-local clamp and a
/// per-pair cooldown. Only prim–prim collisions fire (avatars and terrain carry no
/// collider), and scripted `llCollisionSound`s already arrive separately as
/// `SoundTrigger`s, so this is purely the viewer-synthesised default layer.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's params are its dependencies; the collision-sound driver needs the \
              moving colliders, the scene-object map, time, the sound cache + state, the mute \
              list, the parcel-audibility check and the enable setting"
)]
pub(crate) fn ingest_collisions(
    dynamic: Res<DynamicColliders>,
    scene_objects: Query<&SceneObject>,
    time: Res<Time>,
    mut cache: ResMut<SoundCache>,
    mut sounds: ResMut<WorldSounds>,
    state: Res<ObjectState>,
    mutes: Res<MuteModel>,
    parcel: ParcelAudibility,
    settings: Option<Res<ViewerSettings>>,
) {
    if !collision_sounds_enabled(settings.as_deref()) {
        // Forget any tracked contacts while disabled, so re-enabling does not fire
        // a burst of stale impacts (every still-touching pair reads as a new edge).
        sounds.touching_pairs.clear();
        return;
    }
    let now = time.elapsed_secs();
    let mut touching_now = HashSet::new();
    for (entity1, entity2, point) in dynamic.contact_pairs() {
        let key = pair_key(entity1, entity2);
        touching_now.insert(key);
        // Only fire on the *start* of a contact (the edge), and honour the per-pair
        // cooldown so a bouncing / re-touching pair does not machine-gun.
        if sounds.touching_pairs.contains(&key)
            || sounds
                .collision_cooldowns
                .get(&key)
                .is_some_and(|&last| now - last < COLLISION_COOLDOWN_SECONDS)
        {
            continue;
        }
        // Only prim–prim collisions: both colliders must map to scene objects.
        let (Ok(object1), Ok(object2)) = (scene_objects.get(entity1), scene_objects.get(entity2))
        else {
            continue;
        };
        // Mute: skip if either object is on the mute list.
        let muted_pair = [object1, object2].iter().any(|object| {
            state
                .full_key(&object.scoped_id)
                .is_some_and(|key| mutes.is_muted(key.uuid()))
        });
        if muted_pair {
            continue;
        }
        // Parcel-local clamp at the contact point.
        if !scene_audible(&parcel, &state, point) {
            continue;
        }
        // Primary object's material default sound (unknown → wood, as the
        // reference maps an unrecognised mcode).
        let material = state.material_by_scoped(&object1.scoped_id).unwrap_or(3);
        let Ok(uuid) = Uuid::parse_str(collision_sound_str(material)) else {
            continue;
        };
        let sound = AssetKey::from(uuid);
        cache.request(sound);
        let _previous = sounds.collision_cooldowns.insert(key, now);
        sounds.pending_oneshots.push(PendingOneShot {
            sound,
            position: point,
            gain: COLLISION_GAIN,
            enqueued: now,
        });
    }
    sounds.touching_pairs = touching_now;
    // Prune stale cooldown entries so the map cannot grow without bound.
    sounds
        .collision_cooldowns
        .retain(|_, last| now - *last < COLLISION_COOLDOWN_SECONDS * 8.0);
}

/// The in-world-sounds plugin: the [`WorldSounds`] state, the event ingest, and
/// the per-frame drive that plays / follows / reaps the voices. The drive runs
/// after the ingest so a sound triggered this frame is considered the same frame.
pub(crate) struct WorldSoundsPlugin;

impl Plugin for WorldSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldSounds>()
            .add_systems(Startup, register_world_sound_settings)
            .add_systems(
                Update,
                (
                    ingest_world_sound_events,
                    ingest_collisions,
                    drive_world_sounds,
                )
                    .chain(),
            );
    }
}

/// Startup: declare this module's persisted settings.
fn register_world_sound_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_settings(&mut settings);
}

/// Declare this module's persisted settings (split from the Startup system so
/// tests can register on a bare store).
fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        AUDIO_SECTION,
        SETTING_COLLISION_SOUNDS,
        sl_settings::SettingValue::Bool(true),
        "Play the viewer-synthesized material sound when physical objects collide",
    );
}

/// Whether the synthesized collision sounds are enabled (a missing settings
/// resource or entry reads as the on default).
fn collision_sounds_enabled(settings: Option<&ViewerSettings>) -> bool {
    settings.is_none_or(|settings| {
        settings
            .store()
            .get_bool(SETTING_COLLISION_SOUNDS)
            .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use sl_audio::MixerConfig;

    /// The collision-sound gate: on by default (including with no settings
    /// resource at all), off only when the stored setting says so.
    #[test]
    fn collision_gate_predicate() {
        assert!(collision_sounds_enabled(None), "no settings resource: on");
        let mut settings =
            crate::settings::ViewerSettings::from_store_for_test(sl_settings::SettingsStore::new());
        register_settings(&mut settings);
        assert!(collision_sounds_enabled(Some(&settings)), "default: on");
        settings.set(
            sl_settings::Scope::Global,
            SETTING_COLLISION_SOUNDS,
            sl_settings::SettingValue::Bool(false),
        );
        assert!(!collision_sounds_enabled(Some(&settings)), "stored off");
    }

    /// A sound is muted when its owner or its object is on the mute list —
    /// unless that entry carries the object-sounds exception.
    #[test]
    fn muted_covers_owner_and_object() {
        /// A blanket mute of `id` (no aspect exceptions).
        fn blanket(id: Uuid) -> sl_client_bevy::MuteEntry {
            sl_client_bevy::MuteEntry {
                id,
                name: String::new(),
                mute_type: sl_client_bevy::MuteType::Object,
                flags: MuteFlags::default(),
            }
        }

        let mut mutes = MuteModel::default();
        let owner = Uuid::from_u128(1);
        let object = Uuid::from_u128(2);
        let other = Uuid::from_u128(3);
        assert!(!muted(&mutes, owner, object));
        mutes.note_mute(blanket(owner));
        assert!(muted(&mutes, owner, object), "owner muted");
        assert!(!muted(&mutes, other, object), "unrelated owner not muted");
        mutes.note_mute(blanket(object));
        mutes.note_unmute(owner, "");
        assert!(muted(&mutes, owner, object), "object muted");

        // The object-sounds exception un-mutes just the sound.
        mutes.note_mute(sl_client_bevy::MuteEntry {
            flags: MuteFlags(MuteFlags::ALLOW_OBJECT_SOUNDS),
            ..blanket(object)
        });
        assert!(!muted(&mutes, owner, object), "object-sounds exception");
    }

    /// `realize_oneshots` drops a one-shot older than the cutoff (its moment has
    /// passed) and keeps a fresh one still waiting on its clip. Without a device
    /// the mixer plays nothing, so this exercises the drop / keep partition.
    #[test]
    fn stale_oneshot_is_dropped() {
        let Ok(mut mixer) = Mixer::new(&MixerConfig::default()) else {
            unreachable!("mixer graph builds without a device")
        };
        let cache = SoundCache::new();
        let mut sounds = WorldSounds::default();
        // One old (enqueued at t=0), one fresh (t=10), evaluated at now=10.
        sounds.pending_oneshots.push(PendingOneShot {
            sound: AssetKey::from(Uuid::from_u128(3)),
            position: Vec3::ZERO,
            gain: 1.0,
            enqueued: 0.0,
        });
        sounds.pending_oneshots.push(PendingOneShot {
            sound: AssetKey::from(Uuid::from_u128(4)),
            position: Vec3::ZERO,
            gain: 1.0,
            enqueued: 10.0,
        });
        realize_oneshots(&mut mixer, &cache, &mut sounds, 10.0);
        assert_eq!(
            sounds.pending_oneshots.len(),
            1,
            "the fresh one-shot survives, the stale one is dropped"
        );
    }

    /// Every material byte maps to a parseable default collision-sound UUID, with
    /// LIGHT (7) and unknown codes sharing plastic's sound (as the reference does).
    #[test]
    fn collision_sound_table() {
        for material in 0u8..=8 {
            assert!(
                Uuid::parse_str(collision_sound_str(material)).is_ok(),
                "material {material} has a parseable collision sound"
            );
        }
        assert_eq!(collision_sound_str(7), collision_sound_str(5));
        assert_eq!(collision_sound_str(99), collision_sound_str(5));
    }

    /// The parcel-local (`SOUND_LOCAL`) audibility rule: own parcel is always
    /// heard; a sound-local agent parcel hears nothing outside it; a sound-local
    /// source parcel is not heard from outside; anything else is audible.
    #[test]
    fn parcel_local_audibility_rule() {
        // In the agent's own parcel: always heard, regardless of flags.
        assert!(audible_from_flags(true, true, true));
        // Outside, agent parcel is sound-local: nothing external is heard.
        assert!(!audible_from_flags(false, true, false));
        // Outside, source parcel is sound-local: not heard.
        assert!(!audible_from_flags(false, false, true));
        // Outside, neither is sound-local: heard.
        assert!(audible_from_flags(false, false, false));
    }

    /// A different sound on the same object replaces the entry, and the old entry
    /// (whose voice, if any, must be stopped) is returned by the insert. A STOP
    /// removes it entirely.
    #[test]
    fn attached_replace_and_remove() {
        let mut sounds = WorldSounds::default();
        let object = ObjectKey::from(Uuid::from_u128(1));
        let first = AssetKey::from(Uuid::from_u128(2));
        let second = AssetKey::from(Uuid::from_u128(3));
        let _prev = sounds.attached.insert(
            object,
            AttachedSoundVoice {
                sound: first,
                gain: 1.0,
                looped: true,
                voice: None,
            },
        );
        let replaced = sounds.attached.insert(
            object,
            AttachedSoundVoice {
                sound: second,
                gain: 0.5,
                looped: true,
                voice: None,
            },
        );
        assert_eq!(
            replaced.map(|voice| voice.sound),
            Some(first),
            "the previous entry is returned so its voice can be stopped"
        );
        let _removed = sounds.attached.remove(&object);
        assert!(
            sounds.attached.is_empty(),
            "STOP forgets the object's sound"
        );
    }
}
