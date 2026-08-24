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
//!   [`crate::camera`] `own_avatar_pose` idiom; the frame-late
//!   `GlobalTransform` would trail a moving avatar).
//! - **Output device** ([`SETTING_OUTPUT_DEVICE`]): the device the mixer's
//!   stream opens, by name, empty for the system default. A change rebuilds
//!   the graph on the new device ([`Mixer::rebuild_and_restart`] — sources
//!   are re-created, the buses keep their levels); a name that fails to open
//!   falls back to the system default explicitly, since the mixer's own
//!   automatic fallback only covers a *running* device disappearing.

use bevy::prelude::*;

use sl_audio::{AudioMixer as _, DeviceSelection, EarMode, Listener, Mixer, MixerConfig};
use sl_settings::SettingValue;

use crate::avatars::AvatarState;
use crate::camera::ViewerCamera;
use crate::settings::ViewerSettings;
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
        "Where the ears are: 0 the camera position, 1 the avatar position",
    );
    settings.register_in(
        AUDIO_SECTION,
        SETTING_OUTPUT_DEVICE,
        SettingValue::String(String::new()),
        "The audio output device, by name (empty: the system default)",
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
        app.add_systems(Last, (apply_output_device, drive_audio).chain());
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

/// Apply [`SETTING_OUTPUT_DEVICE`] to the mixer: on a change (or a persisted
/// non-default device at startup), rebuild the graph on the selected device,
/// falling back to the system default when the named device cannot be opened.
/// The applied value is always recorded, so a broken name is not retried
/// every frame.
fn apply_output_device(
    settings: Option<Res<ViewerSettings>>,
    mixer: Option<NonSendMut<Mixer>>,
    mut last: Local<Option<String>>,
) {
    let (Some(settings), Some(mut mixer)) = (settings, mixer) else {
        return;
    };
    let stored = settings
        .store()
        .get_str(SETTING_OUTPUT_DEVICE)
        .unwrap_or("")
        .to_owned();
    if let Some(selection) = device_switch(&stored, last.as_deref())
        && let Err(e) = mixer.rebuild_and_restart(&selection)
    {
        warn!(
            "audio output device {stored:?} could not be started ({e}); \
             falling back to the system default"
        );
        if let Err(e) = mixer.rebuild_and_restart(&DeviceSelection::Default) {
            warn!("audio could not restart on the default device ({e}); running without audio");
        }
    }
    *last = Some(stored);
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_audio::{DeviceSelection, EarMode};
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        DEFAULT_EAR_LOCATION, SETTING_EAR_LOCATION, SETTING_OUTPUT_DEVICE, device_switch, ear_mode,
        resolve_listener,
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
}
