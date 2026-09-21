//! Bevy glue for the shared [`sl_audio`] mixer.
//!
//! This owns the **one** [`Mixer`] the whole viewer feeds (device + graph +
//! buses), pumps it once per frame off the render-critical path (in the `Last`
//! schedule, after every producer has had its turn in `Update`), and keeps the
//! mixer's listener following the camera so spatial sounds are placed correctly.
//!
//! There is deliberately nothing here that *produces* sound: in-world sounds, UI
//! sounds, the parcel-stream / media hand-offs and voice are separate tasks that
//! each grab the [`Mixer`] (a `NonSend` resource, because the audio device
//! stream is not `Send`) and call its `play_*` / `open_stream` methods. This
//! module is only the device lifecycle, the per-frame pump, and the listener.
//!
//! Two user preferences live here because this module is where they apply
//! (surfaced on the preferences audio tab, `viewer-preferences-audio-tab`):
//!
//! - **Ear position** ([`SETTING_EAR_LOCATION`], the reference
//!   `MediaSoundsEarLocation`): whether the listener's *position* is the
//!   camera's or the avatar's. The reference switches only the position — the
//!   orientation always stays the camera's (`llvieweraudio.cpp`,
//!   `audio_update_listener`), and `resolve_listener` mirrors that. The
//!   avatar position is the body-root anchor's current-frame local
//!   [`Transform`] (a top-level entity whose local *is* its world pose — the
//!   `camera` `own_avatar_pose` idiom; the frame-late
//!   `GlobalTransform` would trail a moving avatar).
//! - **Output device** ([`SETTING_OUTPUT_DEVICE`]): the device the mixer's
//!   stream opens, by name, empty for the system default. A change rebuilds
//!   the graph on the new device ([`Mixer::rebuild_and_restart`] — sources
//!   are re-created, the buses keep their levels); a name that fails to open
//!   falls back to the system default explicitly, since the mixer's own
//!   automatic fallback only covers a *running* device disappearing.
//!
//!   The fallback does not rewrite the setting — an unplugged headset is
//!   expected back, and silently demoting the user's choice to "system
//!   default" would lose it. Instead the discrepancy is published as
//!   [`OutputDeviceStatus`] (what was asked for, and whether it is actually
//!   playing), which the preferences audio tab surfaces rather than showing the
//!   preference as though it were the truth, and the named device is retried
//!   every five seconds — only once it is enumerable again, so a
//!   device that comes back is picked up without a restart and one that does
//!   not costs a host enumeration rather than a failed stream open.
//!
//!   Every such enumeration goes through [`OutputDeviceEnumerator`], the
//!   resource this plugin inserts. Listing the machine's devices opens them,
//!   and only an app that asked for audio may do that — see that type for why
//!   a `cargo nextest` run must not.

use bevy::prelude::*;

use sl_audio::{AudioMixer as _, DeviceSelection, EarMode, Listener, Mixer, MixerConfig};
use sl_settings::SettingValue;

use crate::settings::ViewerSettings;
use crate::world_api::AvatarState;
use crate::world_api::ViewerCamera;
use sl_client_bevy::SlIdentity;

/// The persisted-settings section this module's keys live under (`[audio]`).
const AUDIO_SECTION: &[&str] = &["audio"];

/// The reference `MediaSoundsEarLocation` setting name: where the ears are —
/// `0` the camera position (the default), `1` the avatar position.
pub const SETTING_EAR_LOCATION: &str = "MediaSoundsEarLocation";
/// The default ear location: the camera (the reference default).
const DEFAULT_EAR_LOCATION: u32 = 0;

/// The output-device setting name: the audio output device to open, by its
/// reported name, or empty for the system default.
pub const SETTING_OUTPUT_DEVICE: &str = "AudioOutputDevice";

/// Register this module's persisted settings. Called from
/// [`ViewerSettings`]'s `load`.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        AUDIO_SECTION,
        SETTING_EAR_LOCATION,
        SettingValue::U32(DEFAULT_EAR_LOCATION),
        "setting-desc-MediaSoundsEarLocation",
    );
    settings.register_in(
        AUDIO_SECTION,
        SETTING_OUTPUT_DEVICE,
        SettingValue::String(String::new()),
        "setting-desc-AudioOutputDevice",
    );
}

/// The viewer's audio plugin: creates the shared [`Mixer`] (opening the default
/// output device), keeps the listener on the camera (or the avatar, per the
/// ear-position preference), applies the output-device preference, and pumps
/// the mixer once per frame. If the device cannot be opened the viewer still
/// runs — every audio system guards on the mixer being present.
#[derive(Debug)]
pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    /// Create the mixer (device + graph) and wire the per-frame pump. The
    /// device applier runs before the pump so a rebuilt graph gets its
    /// listener and `update()` the same frame.
    fn build(&self, app: &mut App) {
        match Mixer::new(&MixerConfig::default()) {
            Ok(mut mixer) => {
                if let Err(e) = mixer.start(&DeviceSelection::Default) {
                    warn!("audio device could not be started ({e}); running without audio");
                }
                app.insert_non_send(mixer);
            }
            Err(e) => {
                warn!("audio mixer could not be created ({e}); running without audio");
            }
        }
        app.init_resource::<OutputDeviceStatus>()
            // Only an app that asked for audio may enumerate the machine's
            // devices; see [`OutputDeviceEnumerator`].
            .insert_resource(OutputDeviceEnumerator(Mixer::output_devices))
            .add_systems(Last, (apply_output_device, drive_audio).chain());
    }
}

/// The stored ear location resolved to an [`EarMode`] (an unknown stored
/// value reads as the camera default).
fn ear_mode(settings: Option<&ViewerSettings>) -> EarMode {
    let stored = settings.map_or(DEFAULT_EAR_LOCATION, |settings| {
        settings
            .store()
            .get_u32(SETTING_EAR_LOCATION)
            .unwrap_or(DEFAULT_EAR_LOCATION)
    });
    if stored == 1 {
        EarMode::AvatarHead
    } else {
        EarMode::Camera
    }
}

/// The listener pose for `mode`: the camera pose, or — ears at the avatar —
/// the avatar's position with the **camera's** orientation (the reference
/// switches only the position; see the module doc). No avatar in the scene
/// falls back to the camera pose.
fn resolve_listener(
    mode: EarMode,
    camera_position: Vec3,
    camera_forward: Vec3,
    camera_up: Vec3,
    avatar_position: Option<Vec3>,
) -> Listener {
    let position = match (mode, avatar_position) {
        (EarMode::AvatarHead, Some(avatar)) => avatar,
        (EarMode::AvatarHead, None) | (EarMode::Camera, _) => camera_position,
    };
    Listener::new(
        position.to_array(),
        camera_forward.to_array(),
        camera_up.to_array(),
    )
}

/// The own avatar's current-frame world position: the body-root anchor's
/// local [`Transform`] (a top-level entity — its local is its world pose,
/// this frame's value; see the module doc).
fn own_avatar_position(
    identity: Option<&SlIdentity>,
    avatars: Option<&AvatarState>,
    anchors: &Query<&Transform>,
) -> Option<Vec3> {
    let agent = identity?.agent_id?;
    let anchor = avatars?.body_root_of(agent)?;
    anchors
        .get(anchor)
        .ok()
        .map(|transform| transform.translation)
}

/// Update the listener from the camera pose (position swapped to the avatar's
/// under the ear-position preference) and commit the mixer's queued work.
///
/// Runs in `Last` so any sound a producer triggered during `Update` is committed
/// once, after the scene has settled — a single graph recompile per frame no
/// matter how many sounds started.
fn drive_audio(
    mixer: Option<NonSendMut<Mixer>>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    settings: Option<Res<ViewerSettings>>,
    identity: Option<Res<SlIdentity>>,
    avatars: Option<Res<AvatarState>>,
    anchors: Query<&Transform>,
) {
    let Some(mut mixer) = mixer else {
        return;
    };
    if let Ok(camera_transform) = camera.single() {
        let mode = ear_mode(settings.as_deref());
        let avatar_position = if mode == EarMode::AvatarHead {
            own_avatar_position(identity.as_deref(), avatars.as_deref(), &anchors)
        } else {
            None
        };
        // Use the transform's direction accessors (no `Quat * Vec3` multiply).
        mixer.set_listener(resolve_listener(
            mode,
            camera_transform.translation(),
            Vec3::from(camera_transform.forward()),
            Vec3::from(camera_transform.up()),
            avatar_position,
        ));
    }
    mixer.update();
}

/// How often a named output device that failed to open is looked for again
/// (seconds). Enumeration opens the audio host, so this is not a per-frame
/// thing; it only runs at all while a device is known to be unavailable.
const DEVICE_RETRY_SECONDS: f32 = 5.0;

/// What the audio output is **actually** on, as against what
/// [`SETTING_OUTPUT_DEVICE`] asks for. The setting is the user's preference and
/// is never rewritten behind their back — an unplugged device is expected back —
/// so this resource carries the discrepancy instead, and the preferences audio
/// tab reads it rather than presenting the preference as the truth.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct OutputDeviceStatus {
    /// The device name the preference asks for (empty: the system default).
    pub requested: String,
    /// Whether [`requested`](Self::requested) names a device that could not be
    /// opened, so the mixer is running on the system default instead. It is
    /// retried every few seconds and clears when the device comes back (or the
    /// user picks another).
    pub unavailable: bool,
}

/// Permission — and the means — to enumerate the machine's audio output
/// devices, inserted by [`AudioPlugin`] and read by whatever surfaces the
/// device list (the preferences audio tab).
///
/// Enumeration is **hardware access**: the host opens every ALSA control device
/// to ask what it supports, which registers this process with the sound server
/// for as long as it takes. That is fine in a viewer the user launched to hear
/// something; it is not fine in a `cargo nextest` run, where the suite would be
/// reaching for a shared, mutable, machine-global resource it has no business
/// touching — and where the resulting clients are indistinguishable at a glance
/// from a viewer that failed to shut down.
///
/// So the enumerator is *carried*, not called statically: an app that did not
/// add [`AudioPlugin`] has no resource here, and every reader of it does
/// nothing rather than falling back to the host. The same seam lets a test
/// hand in a synthetic device list and assert on the surface it drives without
/// a sound card in the loop. (`sl-viewer-spacenav`'s `DeviceRead` is the same
/// idea for the 6-DOF puck, for the same reason.)
#[derive(Resource, Debug, Clone, Copy)]
pub struct OutputDeviceEnumerator(
    /// Enumerate the output devices, by name.
    pub fn() -> Vec<String>,
);

impl OutputDeviceEnumerator {
    /// The names of the output devices this machine has right now.
    #[must_use]
    pub fn devices(&self) -> Vec<String> {
        (self.0)()
    }
}

/// The output-device applier's own state, kept across frames in a `Local`.
#[derive(Debug, Default)]
struct DeviceApply {
    /// The setting value last looked at (`None` before the first look).
    applied: Option<String>,
    /// Whether [`applied`](Self::applied) named a device that failed to open, so
    /// the mixer fell back to the system default.
    unavailable: bool,
    /// When the next reappearance check is due, while `unavailable`.
    next_retry: f32,
}

/// The device change to apply, if any: `stored` is the setting's current
/// value ("" = system default), `last` the last value this session applied
/// (`None` before the first look). Unchanged values — including the very
/// first sight of the default, which startup already opened — apply nothing.
fn device_switch(stored: &str, last: Option<&str>) -> Option<DeviceSelection> {
    if last == Some(stored) {
        return None;
    }
    if last.is_none() && stored.is_empty() {
        return None;
    }
    Some(if stored.is_empty() {
        DeviceSelection::Default
    } else {
        DeviceSelection::Named(stored.to_owned())
    })
}

/// The retry to make for a device that previously failed to open: the named
/// device, once it is back among the enumerated `devices`. `None` while it is
/// still absent — reopening a device that is not there would only fail again
/// and tear the graph down for nothing.
fn device_retry(stored: &str, devices: &[String]) -> Option<DeviceSelection> {
    (!stored.is_empty() && devices.iter().any(|name| name == stored))
        .then(|| DeviceSelection::Named(stored.to_owned()))
}

/// Apply [`SETTING_OUTPUT_DEVICE`] to the mixer: on a change (or a persisted
/// non-default device at startup), rebuild the graph on the selected device,
/// falling back to the system default when the named device cannot be opened.
///
/// A fallback is **not** the end of it. The setting keeps the user's choice
/// (their headset is expected back), [`OutputDeviceStatus`] records that the
/// choice is not what is playing, and the device is retried every
/// [`DEVICE_RETRY_SECONDS`] — but only once it reappears in the enumeration, so
/// the common case costs one host enumeration per five seconds and no failed
/// stream opens at all.
fn apply_output_device(
    time: Res<Time>,
    settings: Option<Res<ViewerSettings>>,
    mixer: Option<NonSendMut<Mixer>>,
    enumerator: Option<Res<OutputDeviceEnumerator>>,
    mut state: Local<DeviceApply>,
    mut status: ResMut<OutputDeviceStatus>,
) {
    let (Some(settings), Some(mut mixer)) = (settings, mixer) else {
        return;
    };
    let stored = settings
        .store()
        .get_str(SETTING_OUTPUT_DEVICE)
        .unwrap_or("")
        .to_owned();
    let now = time.elapsed_secs();

    // A setting change applies at once; failing that, a device that fell back to
    // the system default is retried once it is enumerable again.
    let selection = match device_switch(&stored, state.applied.as_deref()) {
        Some(selection) => Some(selection),
        None if state.unavailable && now >= state.next_retry => {
            state.next_retry = now + DEVICE_RETRY_SECONDS;
            // Every enumeration in the viewer goes through the carried
            // enumerator, so "who may touch the sound card" has one answer.
            enumerator.and_then(|devices| device_retry(&stored, &devices.devices()))
        }
        None => None,
    };

    if let Some(selection) = selection {
        match mixer.rebuild_and_restart(&selection) {
            Ok(()) => {
                if state.unavailable {
                    info!("audio output device {stored:?} is back; playing on it again");
                }
                state.unavailable = false;
            }
            Err(e) => {
                let named = selection != DeviceSelection::Default;
                warn!(
                    "audio output device {stored:?} could not be started ({e}); \
                     falling back to the system default"
                );
                // Only a *named* device can be waited for; a default that will
                // not open is not something a retry can fix.
                state.unavailable = named;
                state.next_retry = now + DEVICE_RETRY_SECONDS;
                if let Err(e) = mixer.rebuild_and_restart(&DeviceSelection::Default) {
                    warn!(
                        "audio could not restart on the default device ({e}); running without audio"
                    );
                }
            }
        }
    }
    state.applied = Some(stored.clone());

    // Publish the truth for the preferences combo. Guarded, so an unchanged
    // status does not dirty the resource every frame.
    let truth = OutputDeviceStatus {
        requested: stored,
        unavailable: state.unavailable,
    };
    if *status != truth {
        *status = truth;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_audio::{DeviceSelection, EarMode};
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        DEFAULT_EAR_LOCATION, SETTING_EAR_LOCATION, SETTING_OUTPUT_DEVICE, device_retry,
        device_switch, ear_mode, resolve_listener,
    };
    use crate::settings::ViewerSettings;

    /// A [`ViewerSettings`] with this module's settings registered.
    fn settings() -> ViewerSettings {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        super::register_settings(&mut settings);
        settings
    }

    #[test]
    fn registered_defaults_match_consts() {
        let settings = settings();
        let store = settings.store();
        assert_eq!(
            store.get_u32(SETTING_EAR_LOCATION).ok(),
            Some(DEFAULT_EAR_LOCATION)
        );
        assert_eq!(store.get_str(SETTING_OUTPUT_DEVICE).ok(), Some(""));
    }

    #[test]
    fn ear_mode_reads_the_stored_value() {
        let mut settings = settings();
        // No settings resource / the default: the camera.
        assert_eq!(ear_mode(None), EarMode::Camera);
        assert_eq!(ear_mode(Some(&settings)), EarMode::Camera);
        settings.set(Scope::Global, SETTING_EAR_LOCATION, SettingValue::U32(1));
        assert_eq!(ear_mode(Some(&settings)), EarMode::AvatarHead);
        // An unknown stored value maps to the camera default.
        settings.set(Scope::Global, SETTING_EAR_LOCATION, SettingValue::U32(7));
        assert_eq!(ear_mode(Some(&settings)), EarMode::Camera);
    }

    #[test]
    fn resolve_listener_modes() {
        let camera_position = Vec3::new(10.0, 2.0, 3.0);
        let forward = Vec3::NEG_Z;
        let up = Vec3::Y;
        let avatar = Vec3::new(-4.0, 1.0, 8.0);
        let camera_pose = sl_audio::Listener::new(
            camera_position.to_array(),
            forward.to_array(),
            up.to_array(),
        );
        // Camera mode: the camera pose, avatar present or not.
        assert_eq!(
            resolve_listener(EarMode::Camera, camera_position, forward, up, Some(avatar)),
            camera_pose
        );
        // Avatar mode: the avatar's position with the camera's orientation.
        assert_eq!(
            resolve_listener(
                EarMode::AvatarHead,
                camera_position,
                forward,
                up,
                Some(avatar)
            ),
            sl_audio::Listener::new(avatar.to_array(), forward.to_array(), up.to_array())
        );
        // Avatar mode without an avatar: fall back to the camera pose.
        assert_eq!(
            resolve_listener(EarMode::AvatarHead, camera_position, forward, up, None),
            camera_pose
        );
    }

    #[test]
    fn device_switch_cases() {
        // First sight of the default: startup already opened it — no rebuild.
        assert_eq!(device_switch("", None), None);
        // Unchanged values apply nothing.
        assert_eq!(device_switch("", Some("")), None);
        assert_eq!(device_switch("Speakers", Some("Speakers")), None);
        // A persisted device at startup, and any change, rebuilds.
        assert_eq!(
            device_switch("Speakers", None),
            Some(DeviceSelection::Named("Speakers".to_owned()))
        );
        assert_eq!(
            device_switch("Headset", Some("Speakers")),
            Some(DeviceSelection::Named("Headset".to_owned()))
        );
        // Back to the default after a named device.
        assert_eq!(
            device_switch("", Some("Speakers")),
            Some(DeviceSelection::Default)
        );
    }

    /// A device that failed to open is retried only once it is enumerable
    /// again: re-opening one that is still absent would fail and tear the graph
    /// down for nothing.
    #[test]
    fn device_retry_waits_for_the_device_to_reappear() {
        let absent = [String::from("Speakers")];
        let present = [String::from("Speakers"), String::from("Headset")];
        assert_eq!(device_retry("Headset", &absent), None, "still gone");
        assert_eq!(
            device_retry("Headset", &present),
            Some(DeviceSelection::Named("Headset".to_owned())),
            "back: re-open it"
        );
        // The system default is not a name that can be waited for.
        assert_eq!(device_retry("", &present), None);
    }
}
