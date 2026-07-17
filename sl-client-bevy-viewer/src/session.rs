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
use bevy::window::PrimaryWindow;
use sl_client_bevy::{
    AnimationKey, Camera, Command, Distance, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    Throttle,
};

use crate::camera::ViewerCamera;
use crate::coords::bevy_to_sl_vec;
use crate::settings::ViewerSettings;

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
    /// Whether the agent's own avatar object has arrived, i.e. the agent is
    /// in-world with a live circuit to carry an `AgentUpdate`. Once set, the
    /// interest camera is reported so content streams toward the viewpoint (R22b).
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
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
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

/// The vertical field of view (radians) advertised to the simulator if the
/// camera's projection can't be read — the Bevy perspective default the viewer
/// camera is built with.
const DEFAULT_VERTICAL_FOV: f32 = core::f32::consts::FRAC_PI_4;

/// Report the viewer's viewport size (`AgentHeightWidth`) and vertical field of
/// view (`AgentFOV`) to the simulator, resent whenever either changes (R22b).
///
/// The simulator builds the agent's interest list from a **view frustum** — the
/// camera position and look axis (sent in `AgentUpdate`, see
/// [`report_camera_interest`]) *plus* the field of view and viewport aspect it can
/// only learn from these two messages. The reference viewer sends both on login and
/// on every window reshape. Without them the sim falls back to a default frustum, so
/// the camera-interest report alone never pulls a distant avatar into the interest
/// list — it stays a coarse "blue sphere" however close the camera flies, and edge-of-
/// range objects cull by the wrong direction. Advertising the real viewport + FOV is
/// what makes the directional, camera-driven interest list behave like the reference
/// viewer's.
///
/// Gated on [`ViewerSession::agent_in_world`] (a live circuit must exist) and sent
/// only on change, so it is idle once the window settles.
pub(crate) fn report_agent_viewport(
    session: Res<ViewerSession>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Projection, With<ViewerCamera>>,
    mut last: Local<Option<(u16, u16, u32)>>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !session.agent_in_world {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let width = u16::try_from(window.resolution.physical_width()).unwrap_or(u16::MAX);
    let height = u16::try_from(window.resolution.physical_height()).unwrap_or(u16::MAX);
    let fov = match cameras.single() {
        Ok(Projection::Perspective(perspective)) => perspective.fov,
        _ => DEFAULT_VERTICAL_FOV,
    };
    // Resend only when the viewport or FOV actually changes (the FOV compared by its
    // bit pattern, since `f32` is not `Eq`); otherwise this is a per-frame no-op.
    let key = (width, height, fov.to_bits());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    if std::env::var("SL_VIEWER_LOG_AVATAR_INTEREST").as_deref() == Ok("1") {
        info!("R22b report viewport {width}x{height} vertical_fov={fov} rad");
    }
    commands.write(SlCommand(Command::SetAgentSize { height, width }));
    commands.write(SlCommand(Command::SetAgentFov {
        vertical_angle: fov,
    }));
}

/// Request a clean logout on the quit chord (`Ctrl+Q`, matching the reference).
///
/// `Escape` is deliberately *not* the quit key: in the world it resets the camera
/// ([`crate::camera::reset_camera_view`]) and in a focused UI it releases focus
/// ([`crate::input_context`]), both of which a quit-on-`Escape` would pre-empt. The
/// logout command is queued once; the actual `AppExit` is driven by
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
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl && keyboard.just_pressed(KeyCode::KeyQ) {
        info!("quit requested; logging out");
        request_logout(&mut session, &mut commands, time.elapsed_secs());
    }
}

/// Request a clean grid logout and arm the quit deadline (idempotent): queue a
/// [`Command::Logout`] and record the wall-clock time by which
/// [`enforce_quit_deadline`] forces the exit if no `LoggedOut` arrives. Shared by
/// the quit key ([`handle_quit_input`]) and the screenshot harness so both leave the
/// avatar cleanly logged out — an abrupt process exit strands the grid session and
/// blocks the next login.
pub(crate) fn request_logout(
    session: &mut ViewerSession,
    commands: &mut MessageWriter<SlCommand>,
    now: f32,
) {
    if session.quit_deadline.is_some() {
        return;
    }
    commands.write(SlCommand(Command::Logout));
    session.quit_deadline = Some(now + QUIT_GRACE_SECS);
}

/// Persist the settings store once, when a logout is first requested, so a tuned
/// value (e.g. a SpaceNavigator sensitivity) survives to the next session.
///
/// Keyed off the quit deadline being armed rather than the `LoggedOut` event, so
/// the save happens even if the grid never acknowledges the logout and
/// [`enforce_quit_deadline`] forces the exit.
pub(crate) fn save_settings_on_logout(
    session: Res<ViewerSession>,
    settings: Res<ViewerSettings>,
    mut saved: Local<bool>,
) {
    if *saved || session.quit_deadline.is_none() {
        return;
    }
    *saved = true;
    settings.save();
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
/// handshake, marking the agent in-world on its first appearance, and a clean
/// exit on logout/disconnect.
///
/// The camera is no longer placed here: third-person
/// ([`crate::camera::position_camera`]) follows the avatar the moment it arrives,
/// so there is nothing to snap. The `SL_VIEWER_CAMERA_*` framing knobs the old
/// snap read now seed the third-person orbit
/// ([`CameraRig::seed_orbit_from_env`](crate::camera::CameraRig)).
pub(crate) fn drive_session(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut session: ResMut<ViewerSession>,
    play_on_login: Res<PlayOnLogin>,
    mut commands: MessageWriter<SlCommand>,
    mut exit: MessageWriter<AppExit>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionHandshakeComplete => {
                info!("region handshake complete; requesting draw distance + throttle");
                commands.write(SlCommand(Command::SetDrawDistance(Distance::new(
                    DRAW_DISTANCE_METRES,
                ))));
                // Advertise a generous bandwidth throttle (R22b). Without an
                // `AgentThrottle` the simulator streams objects at conservative
                // defaults, so it spends the tiny budget on the highest-priority
                // (nearest) objects and never reaches lower-priority ones — a
                // same-region avatar 150 m off stays a coarse "blue sphere" for the
                // whole session however close the camera flies, because interest-list
                // sends are bandwidth-priority-ordered. The reference viewer always
                // advertises its throttle; the 1000 kbps preset matches its generous
                // end and is ample to stream the full scene.
                commands.write(SlCommand(Command::SetThrottle(Throttle::preset_1000())));
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
                // `AgentUpdate` (R22b). The camera then follows the avatar of its
                // own accord (`position_camera`), so there is nothing to snap here.
                if is_own_avatar {
                    session.agent_in_world = true;
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
