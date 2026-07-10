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
    AnimationKey, Camera, Command, Distance, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
};

use crate::camera::{CameraStart, FlyCamera};
use crate::coords::{bevy_to_sl_vec, sl_to_bevy_object_rotation, sl_to_bevy_vec};

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
    /// Whether the agent's own avatar object has arrived, i.e. the agent is
    /// in-world with a live circuit to carry an `AgentUpdate`. Set independently
    /// of [`camera_positioned`](Self::camera_positioned): a fixed `--camera-position`
    /// suppresses the login camera-snap (so `camera_positioned` stays false), but
    /// the agent is still in-world and its interest camera must still be reported —
    /// otherwise a screenshot / fixed-camera run never streams content toward the
    /// framed viewpoint (R22b).
    agent_in_world: bool,
    /// Whether the `--play-animation` debug animation has been triggered yet, so
    /// it fires once on the first region handshake rather than on every one.
    play_on_login_done: bool,
    /// The wall-clock deadline (`Time::elapsed_secs`) at which a pending quit
    /// forces an exit even without a `LoggedOut`; `None` until quit is
    /// requested.
    quit_deadline: Option<f32>,
}

/// Debug animations to play on the agent's **own** avatar once it lands (the
/// `--play-animation <uuid>` flag, repeatable), so the P18.3 skeleton driver and
/// P18.4 priority blending can be exercised with a single login rather than
/// needing a second avatar to animate. Empty (the default) plays nothing; more
/// than one layers them so the blend of concurrent motions can be watched.
#[derive(Resource, Default)]
pub(crate) struct PlayOnLogin {
    /// The animations to start on the agent's own avatar (empty plays none).
    pub(crate) animations: Vec<AnimationKey>,
    /// Whether to keep re-issuing the animation on a short cadence (the
    /// `--repeat-animation` flag), so it is still playing once the avatar has
    /// finished loading — useful for an unattended screenshot capture where a
    /// one-shot play would have expired before the body is fully on screen.
    pub(crate) repeat: bool,
}

/// Re-issue the `--play-animation` debug animation on a fixed cadence when
/// `--repeat-animation` is set, so a short or non-looping motion keeps playing
/// long enough for the (slower) avatar load / bake to finish. Idempotent for a
/// looping motion (the sim just refreshes its start), and a no-op until the
/// animation has first been kicked off on the region handshake.
pub(crate) fn repeat_debug_animation(
    time: Res<Time>,
    session: Res<ViewerSession>,
    play_on_login: Res<PlayOnLogin>,
    mut next_at: Local<f32>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !play_on_login.repeat || !session.play_on_login_done {
        return;
    }
    if play_on_login.animations.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    // Re-issue every ~2 s. A bare re-`Play` of an animation the sim already lists
    // is a no-op (no fresh `AvatarAnimation` broadcast, so the local playback
    // clock never restarts), so first `Stop` it to drop it from the list, then
    // `Play` it again — the drop + re-add gives a new sequence id, which restarts
    // the motion and re-poses the skeleton. This keeps a short / non-looping
    // debug motion visibly moving long after a one-shot play would have expired.
    *next_at = now + 2.0;
    for &animation in &play_on_login.animations {
        commands.write(SlCommand(Command::StopAnimation(animation)));
        commands.write(SlCommand(Command::PlayAnimation(animation)));
    }
}

/// How often, in seconds, the fly-camera's viewpoint is reported to the simulator
/// as the agent's interest camera (R22). The simulator streams full object /
/// avatar updates within the interest radius of this viewpoint, so a few times a
/// second is ample — `set_camera` sends an `AgentUpdate` on each call, and the
/// session re-advertises the viewpoint on every keep-alive between them.
const CAMERA_INTEREST_INTERVAL_SECS: f32 = 0.5;

/// Report the fly-camera's world viewpoint to the simulator as the agent's
/// interest camera, throttled to [`CAMERA_INTEREST_INTERVAL_SECS`] (R22).
///
/// The simulator builds the agent's interest list — which objects and avatars it
/// streams as full updates — around this viewpoint. Left at its
/// [`Camera::region_center`] default it never follows the fly-camera, so a distant
/// avatar the sim only ever announced as a coarse minimap dot stays a placeholder
/// sphere no matter how close the camera flies to it (and, conversely, a full
/// avatar is never culled back to a dot as the camera leaves). Feeding the
/// fly-camera in makes the interest list track the viewpoint, so avatars resolve
/// on approach and coarsen again on retreat, matching the reference viewer.
///
/// Reporting the camera does **not** move the agent — the `AgentUpdate` camera
/// fields are the viewpoint only; the agent moves solely via its control flags.
pub(crate) fn report_camera_interest(
    time: Res<Time>,
    mut since_last: Local<f32>,
    session: Res<ViewerSession>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    mut commands: MessageWriter<SlCommand>,
) {
    // Only once the agent is in-world (its avatar object has arrived, so a circuit
    // exists to carry the `AgentUpdate`); before then there is nothing to stream to.
    // Gated on `agent_in_world`, not `camera_positioned`: a fixed `--camera-position`
    // never fires the login camera-snap, but the fixed viewpoint must still drive the
    // interest list so a headless screenshot run streams content toward it (R22b).
    if !session.agent_in_world {
        return;
    }
    *since_last += time.delta_secs();
    if *since_last < CAMERA_INTEREST_INTERVAL_SECS {
        return;
    }
    *since_last = 0.0;
    let Ok(transform) = camera.single() else {
        return;
    };
    let eye = transform.translation();
    // A point one metre ahead along the camera's forward (Bevy `-Z`) gives the
    // look axis `Camera::looking_at` needs; the distance is irrelevant to it.
    // Per-component `f32` maths keeps clear of the workspace
    // `arithmetic_side_effects` lint, which `Vec3`'s `+` operator trips.
    let forward = transform.forward();
    let target = Vec3::new(eye.x + forward.x, eye.y + forward.y, eye.z + forward.z);
    let center = bevy_to_sl_vec(eye);
    // R22b diagnostic: surface the interest camera actually reported to the sim, so a
    // live run can confirm the viewpoint follows the fly-camera (and rule out the
    // "camera never reaching the sim" hypothesis). Gated on the avatars-interest flag
    // so it shares the one `SL_VIEWER_LOG_AVATAR_INTEREST=1` switch.
    if std::env::var("SL_VIEWER_LOG_AVATAR_INTEREST").as_deref() == Ok("1") {
        info!("R22b report interest camera center={center:?}");
    }
    let camera = Camera::looking_at(center, bevy_to_sl_vec(target));
    commands.write(SlCommand(Command::SetCamera(camera)));
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
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and queries; \
              placing / snapping the camera needs the event stream, identity, session \
              bookkeeping, play-on-login and camera-override config, the camera query, \
              and the command / exit writers together"
)]
pub(crate) fn drive_session(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut session: ResMut<ViewerSession>,
    play_on_login: Res<PlayOnLogin>,
    camera_start: Res<CameraStart>,
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
                // Kick off the `--play-animation` debug animations on the agent's
                // own avatar, once, so its skeleton is driven (P18.3 / P18.4) — the
                // sim broadcasts the agent's own `AvatarAnimation` back, which the
                // animation manager fetches / decodes and the driver poses from.
                if !play_on_login.animations.is_empty() && !session.play_on_login_done {
                    for &animation in &play_on_login.animations {
                        info!("playing debug animation {animation} on own avatar");
                        commands.write(SlCommand(Command::PlayAnimation(animation)));
                    }
                    session.play_on_login_done = true;
                }
            }
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                let is_own_avatar = identity
                    .agent_id
                    .is_some_and(|agent| agent.uuid() == object.full_id.uuid());
                // The agent is in-world the moment its own avatar object arrives —
                // a live circuit now exists to carry the interest-camera
                // `AgentUpdate`. Tracked separately from the login camera-snap so a
                // fixed `--camera-position` (which suppresses the snap) still reports
                // its viewpoint (R22b).
                if is_own_avatar {
                    session.agent_in_world = true;
                }
                if camera_start.position.is_none() && !session.camera_positioned && is_own_avatar {
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
                    // Debug affordances for rendering diagnosis (default: the
                    // head-on framing above). `SL_VIEWER_CAMERA_ORBIT_DEG` orbits
                    // the camera around the avatar's vertical axis (90 = a side
                    // view), `_ELEV_DEG` raises/lowers it (positive looks down),
                    // `_DISTANCE` sets the metres back (a smaller value zooms in),
                    // and `_TARGET_Z` lifts the look-at point above the pelvis (so
                    // a close-up can be aimed at the shoulders / head). Together
                    // they let the offline screenshot harness capture a spot the
                    // fixed head-on pose hides — needed to localise geometry
                    // artifacts like the shoulder spike (R13).
                    let env_f32 = |key: &str, default: f32| {
                        std::env::var(key)
                            .ok()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(default)
                    };
                    let orbit = env_f32("SL_VIEWER_CAMERA_ORBIT_DEG", 0.0).to_radians();
                    let elevation = env_f32("SL_VIEWER_CAMERA_ELEV_DEG", 0.0).to_radians();
                    let distance = env_f32("SL_VIEWER_CAMERA_DISTANCE", 4.0);
                    let target_up = env_f32("SL_VIEWER_CAMERA_TARGET_Z", 0.0);
                    // Orbit the (flattened) forward around Bevy up, then tilt it by
                    // the elevation, so the camera sits on a sphere around the
                    // avatar aimed inward.
                    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
                    let orbited = Quat::from_rotation_y(orbit).mul_vec3(flat);
                    let dir = Vec3::new(
                        orbited.x * elevation.cos(),
                        elevation.sin(),
                        orbited.z * elevation.cos(),
                    )
                    .normalize_or_zero();
                    let camera_pos = Vec3::new(
                        position.x + dir.x * distance,
                        position.y + dir.y * distance + 0.3 + target_up,
                        position.z + dir.z * distance,
                    );
                    let target = Vec3::new(position.x, position.y + target_up, position.z);
                    let look = Vec3::new(
                        target.x - camera_pos.x,
                        target.y - camera_pos.y,
                        target.z - camera_pos.z,
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
