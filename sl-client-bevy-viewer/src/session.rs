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
use sl_client_bevy::{
    AnimationKey, Command, Distance, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
};

use crate::camera::FlyCamera;
use crate::coords::{sl_to_bevy_object_rotation, sl_to_bevy_vec};

/// The draw distance requested once the region handshake completes, in metres.
///
/// The sim only streams object/terrain updates within the agent's interest
/// radius, so the viewer must announce one before any content arrives. A full
/// region is 256 m; a draw distance past that lets the sim announce the
/// neighbouring regions (opening child circuits) so their terrain streams too.
const DRAW_DISTANCE_METRES: f64 = 512.0;

/// How long, in seconds, to wait for a clean `LoggedOut` after a quit request
/// before forcing the exit anyway.
const QUIT_GRACE_SECS: f32 = 3.0;

/// Viewer-side session bookkeeping not already tracked by the plugin.
#[derive(Resource, Default)]
pub(crate) struct ViewerSession {
    /// Whether the camera has been snapped to the agent's login position yet.
    camera_positioned: bool,
    /// Whether the `--play-animation` debug animation has been triggered yet, so
    /// it fires once on the first region handshake rather than on every one.
    play_on_login_done: bool,
    /// The wall-clock deadline (`Time::elapsed_secs`) at which a pending quit
    /// forces an exit even without a `LoggedOut`; `None` until quit is
    /// requested.
    quit_deadline: Option<f32>,
}

/// A debug animation to play on the agent's **own** avatar once it lands (the
/// `--play-animation <uuid>` flag), so the P18.3 skeleton driver can be exercised
/// with a single login rather than needing a second avatar to animate. `None`
/// (the default) plays nothing.
#[derive(Resource, Default)]
pub(crate) struct PlayOnLogin {
    /// The animation to start on the agent's own avatar, or `None` to play none.
    pub(crate) animation: Option<AnimationKey>,
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
    play_on_login: Res<PlayOnLogin>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
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
                // Kick off the `--play-animation` debug animation on the agent's
                // own avatar, once, so its skeleton is driven (P18.3) — the sim
                // broadcasts the agent's own `AvatarAnimation` back, which the
                // animation manager fetches / decodes and the driver poses from.
                if let Some(animation) = play_on_login.animation
                    && !session.play_on_login_done
                {
                    info!("playing debug animation {animation} on own avatar");
                    commands.write(SlCommand(Command::PlayAnimation(animation)));
                    session.play_on_login_done = true;
                }
            }
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                if !session.camera_positioned
                    && identity
                        .agent_id
                        .is_some_and(|agent| agent.uuid() == object.full_id.uuid())
                {
                    let position = sl_to_bevy_vec(&object.motion.position);
                    // Seat the fly-camera a few metres in front of the agent (along
                    // the way it faces), slightly above pelvis height, looking back
                    // at it — so the avatar is fully framed head-on on login rather
                    // than the camera sitting inside it (first person) or off to the
                    // side. A Second Life avatar faces its local +X, so its forward
                    // in Bevy world is that rotation applied to `X`. WASD/mouse-look
                    // take over from here. Per-component `f32` maths keeps clear of
                    // the workspace `arithmetic_side_effects` lint (it does not apply
                    // to plain floating-point), which `Vec3`'s operators trip.
                    let forward = sl_to_bevy_object_rotation(&object.motion.rotation)
                        .mul_vec3(Vec3::X)
                        .normalize_or_zero();
                    let distance = 4.0_f32;
                    let camera_pos = Vec3::new(
                        position.x + forward.x * distance,
                        position.y + forward.y * distance + 0.3,
                        position.z + forward.z * distance,
                    );
                    let look = Vec3::new(
                        position.x - camera_pos.x,
                        position.y - camera_pos.y,
                        position.z - camera_pos.z,
                    );
                    for (mut transform, mut camera) in &mut cameras {
                        transform.translation = camera_pos;
                        camera.aim_along(look);
                    }
                    session.camera_positioned = true;
                    info!("placed camera facing agent at {camera_pos:?}");
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
