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

use bevy::prelude::*;

use sl_audio::{AudioMixer as _, DeviceSelection, Listener, Mixer, MixerConfig};

use crate::camera::ViewerCamera;

/// The viewer's audio plugin: creates the shared [`Mixer`] (opening the default
/// output device), keeps the listener on the camera, and pumps the mixer once
/// per frame. If the device cannot be opened the viewer still runs — every
/// audio system guards on the mixer being present.
///
/// The listener follows the camera (the reference viewer's default). The
/// alternative "ears at the avatar's head" preference ([`sl_audio::EarMode`] is
/// already modelled in the mixer) is wired when the avatar-anchored sound
/// producers land, since that is where the avatar head pose becomes available.
pub(crate) struct AudioPlugin;

impl Plugin for AudioPlugin {
    /// Create the mixer (device + graph) and wire the per-frame pump.
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
        app.add_systems(Last, drive_audio);
    }
}

/// Update the listener from the camera pose and commit the mixer's queued work.
///
/// Runs in `Last` so any sound a producer triggered during `Update` is committed
/// once, after the scene has settled — a single graph recompile per frame no
/// matter how many sounds started.
fn drive_audio(
    mixer: Option<NonSendMut<Mixer>>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
) {
    let Some(mut mixer) = mixer else {
        return;
    };
    if let Ok(camera_transform) = camera.single() {
        // Use the transform's direction accessors (no `Quat * Vec3` multiply).
        let translation = camera_transform.translation();
        let forward = Vec3::from(camera_transform.forward());
        let up = Vec3::from(camera_transform.up());
        mixer.set_listener(Listener::new(
            translation.to_array(),
            forward.to_array(),
            up.to_array(),
        ));
    }
    mixer.update();
}
