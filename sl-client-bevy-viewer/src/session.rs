//! The ECS session driver: folds the `SlClientPlugin` event stream into viewer
//! actions.
//!
//! This is the Phase 1 slice — enough to prove the session is live and drive a
//! clean shutdown:
//!
//! - on `RegionHandshakeComplete`, ask the sim to stream content by setting the
//!   draw distance;
//! - snap the fly-camera to the agent's own login position the first time the
//!   agent's avatar object arrives;
//! - on a quit key (`Esc` / `Q`), request a clean logout, then exit once the
//!   grid acknowledges it (or after a short grace, so a lost `LogoutReply` can
//!   never wedge the window open);
//! - exit on any `LoggedOut` / `Disconnected`.
//!
//! Rendering the scene (terrain, prims, meshes, sculpts, avatars, chat) lands
//! in later phases.

use bevy::prelude::*;
use sl_client_bevy::{Command, Distance, SlCommand, SlEvent, SlIdentity, SlSessionEvent};

use crate::camera::FlyCamera;
use crate::coords::sl_to_bevy_vec;

/// The draw distance requested once the region handshake completes, in metres.
///
/// The sim only streams object/terrain updates within the agent's interest
/// radius, so the viewer must announce one before any content arrives.
const DRAW_DISTANCE_METRES: f64 = 128.0;

/// How long, in seconds, to wait for a clean `LoggedOut` after a quit request
/// before forcing the exit anyway.
const QUIT_GRACE_SECS: f32 = 3.0;

/// Viewer-side session bookkeeping not already tracked by the plugin.
#[derive(Resource, Default)]
pub(crate) struct ViewerSession {
    /// Whether the camera has been snapped to the agent's login position yet.
    camera_positioned: bool,
    /// The wall-clock deadline (`Time::elapsed_secs`) at which a pending quit
    /// forces an exit even without a `LoggedOut`; `None` until quit is
    /// requested.
    quit_deadline: Option<f32>,
}

/// Request a clean logout on the quit key (`Esc` / `Q`).
///
/// The logout command is queued once; the actual `AppExit` is driven by
/// [`drive_session`] (on `LoggedOut` / `Disconnected`) or by
/// [`enforce_quit_deadline`] as a fallback.
pub(crate) fn handle_quit_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut session: ResMut<ViewerSession>,
    mut commands: MessageWriter<SlCommand>,
) {
    if session.quit_deadline.is_some() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Escape) || keyboard.just_pressed(KeyCode::KeyQ) {
        info!("quit requested; logging out");
        commands.write(SlCommand(Command::Logout));
        session.quit_deadline = Some(time.elapsed_secs() + QUIT_GRACE_SECS);
    }
}

/// Force the app to exit once the post-quit grace period has elapsed, in case a
/// `LoggedOut` never arrives.
pub(crate) fn enforce_quit_deadline(
    time: Res<Time>,
    session: Res<ViewerSession>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(deadline) = session.quit_deadline
        && time.elapsed_secs() >= deadline
    {
        warn!("logout not acknowledged within grace period; exiting anyway");
        exit.write(AppExit::Success);
    }
}

/// Fold the session event stream into viewer actions: draw distance on
/// handshake, camera placement on the agent's first appearance, and a clean
/// exit on logout/disconnect.
pub(crate) fn drive_session(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut session: ResMut<ViewerSession>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
    mut commands: MessageWriter<SlCommand>,
    mut exit: MessageWriter<AppExit>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionHandshakeComplete => {
                info!("region handshake complete; requesting draw distance");
                commands.write(SlCommand(Command::SetDrawDistance(Distance::new(
                    DRAW_DISTANCE_METRES,
                ))));
            }
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                if !session.camera_positioned
                    && identity
                        .agent_id
                        .is_some_and(|agent| agent.uuid() == object.full_id.uuid())
                {
                    let position = sl_to_bevy_vec(&object.motion.position);
                    for mut transform in &mut cameras {
                        transform.translation = position;
                    }
                    session.camera_positioned = true;
                    info!("placed camera at agent login position {position:?}");
                }
            }
            SlSessionEvent::LoggedOut => {
                info!("logged out cleanly; exiting");
                exit.write(AppExit::Success);
            }
            SlSessionEvent::Disconnected(reason) => {
                warn!("disconnected ({reason:?}); exiting");
                exit.write(AppExit::Success);
            }
            _other => {}
        }
    }
}
