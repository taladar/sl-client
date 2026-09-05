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
//! - on a quit request — Avatar ▸ Quit (picked, or reached by the `Ctrl+Q`
//!   accelerator drawn against it), the window's close button, or a termination
//!   signal — request a clean logout, then exit once the grid acknowledges it
//!   (or after a short grace, so a lost `LogoutReply` can never wedge the window
//!   open);
//! - exit on any `LoggedOut` / `Disconnected`.
//!
//! Rendering the scene (terrain, prims, meshes, sculpts, avatars, chat) lands
//! in later phases.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowCloseRequested};
use sl_client_bevy::{
    AnimationKey, Camera, Command, Distance, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
};
use sl_settings::SettingValue;

use crate::coords::bevy_to_sl_vec;
use crate::settings::ViewerSettings;
use crate::world_api::ViewerCamera;

/// The persisted-settings section the draw-distance setting lives under.
const RENDER_SECTION: &[&str] = &["render"];

/// The draw-distance setting key: how far, in metres, the simulator streams
/// objects and terrain toward the agent. The reference viewer's `RenderFarClip`;
/// surfaced in the quick-preferences panel (`crate::quick_preferences`) and
/// applied live by [`apply_draw_distance`].
pub const SETTING_DRAW_DISTANCE: &str = "RenderFarClip";

/// The default draw distance, in metres.
///
/// The sim only streams object/terrain updates within the agent's interest
/// radius, so the viewer must announce one before any content arrives. A full
/// region is 256 m; a draw distance past that lets the sim announce the
/// neighbouring regions (opening child circuits) so their terrain streams too.
const DEFAULT_DRAW_DISTANCE_METRES: f32 = 512.0;

/// Declare the persisted draw-distance setting (the quick-preferences panel and
/// any future graphics tab bind to it; [`apply_draw_distance`] consumes it).
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        RENDER_SECTION,
        SETTING_DRAW_DISTANCE,
        SettingValue::F32(DEFAULT_DRAW_DISTANCE_METRES),
        "Draw distance in metres: how far the simulator streams objects and \
         terrain toward the agent (a larger value opens child circuits to \
         neighbouring regions)",
    );
}

/// How long, in seconds, to wait for a clean `LoggedOut` after a quit request
/// before forcing the exit anyway.
const QUIT_GRACE_SECS: f32 = 3.0;

/// Viewer-side session bookkeeping not already tracked by the plugin.
#[derive(Debug, Resource, Default)]
pub struct ViewerSession {
    /// Whether the agent's own avatar object has arrived, i.e. the agent is
    /// in-world with a live circuit to carry an `AgentUpdate`. Once set, the
    /// interest camera is reported so content streams toward the viewpoint (R22b).
    agent_in_world: bool,
    /// Whether the `--play-animation` debug animation has been triggered yet, so
    /// it fires once on the first region handshake rather than on every one.
    play_on_login_done: bool,
    /// Whether the login-time offline-instant-message drain has been requested, so
    /// it fires once (on the first region handshake) rather than on every region
    /// cross.
    offline_messages_requested: bool,
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
#[derive(Debug, Resource, Default)]
pub struct PlayOnLogin {
    /// The animations to start on the agent's own avatar (empty plays none).
    pub animations: Vec<AnimationKey>,
    /// Whether to keep re-issuing the animation on a short cadence (the
    /// `--repeat-animation` flag), so it is still playing once the avatar has
    /// finished loading — useful for an unattended screenshot capture where a
    /// one-shot play would have expired before the body is fully on screen.
    pub repeat: bool,
}

/// Re-issue the `--play-animation` debug animation on a fixed cadence when
/// `--repeat-animation` is set, so a short or non-looping motion keeps playing
/// long enough for the (slower) avatar load / bake to finish. Idempotent for a
/// looping motion (the sim just refreshes its start), and a no-op until the
/// animation has first been kicked off on the region handshake.
pub fn repeat_debug_animation(
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

/// The shortest interval, in seconds, between interest-camera `AgentUpdate`s while
/// the view is moving (≈45 Hz). The simulator paces the interest-list object /
/// avatar update stream off the agent's `AgentUpdate` cadence, so a moving vehicle
/// (or any moving object) only renders smoothly when the camera is reported near
/// the display rate — the reference viewer sends `AgentUpdate` at up to
/// `MAX_AGENT_UPDATES_PER_SECOND` (125, `llviewermessage.cpp`) whenever the camera
/// or controls change. The earlier 0.5 s (2 Hz) reporting starved the stream: the
/// sim streamed a driven kart at ~14 Hz, so extrapolation between those sparse
/// samples showed as visible jerk even going straight
/// ([[viewer-physical-object-motion-not-smooth]]). Capped at ~45 Hz (the sim's own
/// physics rate — sending faster cannot yield fresher object data). A **still**
/// view sends nothing here and falls back to the 1 Hz keep-alive `AgentUpdate`.
const CAMERA_INTEREST_MIN_PERIOD_SECS: f32 = 1.0 / 45.0;

/// The camera must move at least this far (metres) since the last interest report
/// before another is sent within the min period — so a static view relies on the
/// keep-alive rather than spamming identical viewpoints, mirroring the reference
/// viewer's send-on-significant-change behaviour.
const CAMERA_INTEREST_MOVE_EPS_M: f32 = 0.02;

/// The camera's look axis must change by at least this (one minus the dot product
/// of successive forward vectors, ~2.6°) before another interest report is sent
/// within the min period, the rotation counterpart of [`CAMERA_INTEREST_MOVE_EPS_M`].
const CAMERA_INTEREST_LOOK_EPS: f32 = 1.0e-3;

/// Report the fly-camera's world viewpoint to the simulator as the agent's
/// interest camera, throttled to `CAMERA_INTEREST_INTERVAL_SECS` (R22).
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
///
/// Reads the camera's **`Transform`**, not its `GlobalTransform`: the camera is a
/// top-level entity (spawned with no parent), so its `Transform` is its world pose,
/// and `GlobalTransform` is only recomputed by propagation in `PostUpdate` — a
/// frame old by the time this reads it. At the ~45 Hz cadence below that frame is a
/// whole report interval, so the sim's interest list would trail the viewpoint by
/// one report the entire time the camera is moving. Scheduled
/// `.after(WorldPhase::CameraPositioned)` so the pose it reads is this frame's.
pub fn report_camera_interest(
    time: Res<Time>,
    mut since_last: Local<f32>,
    mut last_view: Local<Option<(Vec3, Vec3)>>,
    session: Res<ViewerSession>,
    camera: Query<&Transform, With<ViewerCamera>>,
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
    // Rate-limit to ~45 Hz: sending faster than the sim's physics rate cannot yield
    // fresher object data, and it matches the reference's high-but-bounded cadence.
    *since_last += time.delta_secs();
    if *since_last < CAMERA_INTEREST_MIN_PERIOD_SECS {
        return;
    }
    let Ok(transform) = camera.single() else {
        return;
    };
    let eye = transform.translation;
    // A point one metre ahead along the camera's forward (Bevy `-Z`) gives the
    // look axis `Camera::looking_at` needs; the distance is irrelevant to it.
    // Per-component `f32` maths keeps clear of the workspace
    // `arithmetic_side_effects` lint, which `Vec3`'s `+` operator trips.
    let forward = transform.forward().as_vec3();
    // Only report when the view actually moved since the last send; a static view
    // relies on the 1 Hz keep-alive `AgentUpdate` instead of re-sending an identical
    // viewpoint every 22 ms (the reference viewer likewise sends on significant
    // change). While driving, the camera moves every frame, so this fires at the full
    // ~45 Hz cap — which is what keeps the sim's interest-list object stream dense.
    let moved = last_view.is_none_or(|(last_eye, last_forward)| {
        eye.distance(last_eye) > CAMERA_INTEREST_MOVE_EPS_M
            || forward.dot(last_forward) < 1.0 - CAMERA_INTEREST_LOOK_EPS
    });
    if !moved {
        return;
    }
    *since_last = 0.0;
    *last_view = Some((eye, forward));
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
/// Gated on `ViewerSession::agent_in_world` (a live circuit must exist) and sent
/// only on change, so it is idle once the window settles.
pub fn report_agent_viewport(
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

/// A viewer-level request to quit — the menu ▸ Quit action (a [`QuitRequested`]
/// message) or the window's close button / a compositor close
/// ([`WindowCloseRequested`]). Both route here so the quit goes through a
/// **graceful** `request_logout` rather than an abrupt `AppExit`: an abrupt
/// exit strands the grid session (which can block the next login) and can leave
/// an in-flight network request hanging the process teardown. The actual
/// `AppExit` still follows from [`drive_session`] on `LoggedOut`, with
/// [`enforce_quit_deadline`] as the grace-period fallback.
#[derive(Debug, Message, Default)]
pub struct QuitRequested;

/// Set by the `SIGTERM` / `SIGINT` handler, read by [`quit_on_termination_signal`].
///
/// An [`AtomicBool`](core::sync::atomic::AtomicBool) rather than anything that
/// allocates or locks, because it is written from inside a signal handler; in an
/// [`Arc`](std::sync::Arc) because that is what `signal_hook`'s flag registration
/// holds on to.
#[cfg(unix)]
static TERMINATION_REQUESTED: std::sync::LazyLock<std::sync::Arc<core::sync::atomic::AtomicBool>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(core::sync::atomic::AtomicBool::new(false)));

/// Ask the operating system to raise a flag on `SIGTERM` and `SIGINT` instead of
/// killing the process, so [`quit_on_termination_signal`] can turn either into
/// the same graceful logout the menu's Quit takes.
///
/// This is what lets a **driving harness ask this viewer to quit**. A cross-check
/// runner that must escalate a stuck run has only signals to escalate with, and
/// the default `SIGTERM` disposition strands the grid session — which does not
/// merely lose that run: the next login is rejected until the simulator times the
/// stale presence out, and *that* failure looks exactly like a viewer bug. The
/// runner still escalates to `SIGKILL` in the end; this is what makes the step
/// before it worth taking.
///
/// `SIGINT` comes with it because a run watched from a terminal is stopped with
/// `Ctrl-C`, and that should log out too rather than strand the session.
///
/// A no-op off Unix, where there are no such signals to install.
///
/// # Errors
///
/// Returns the registration error. The caller logs it and carries on: a viewer
/// that could not install the handler still runs, it just dies abruptly when
/// signalled, exactly as it did before.
pub fn install_termination_handler() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        let _registration =
            signal_hook::flag::register(signal, std::sync::Arc::clone(&TERMINATION_REQUESTED))?;
    }
    Ok(())
}

/// Turn a `SIGTERM` / `SIGINT` into the same graceful logout the menu's Quit
/// takes, once per run.
///
/// Polled from `Update` rather than acted on in the handler itself: a signal
/// handler may not touch the ECS (or allocate, or lock), and the quit path is a
/// message and a deadline.
pub fn quit_on_termination_signal(
    time: Res<Time>,
    mut session: ResMut<ViewerSession>,
    mut commands: MessageWriter<SlCommand>,
) {
    #[cfg(unix)]
    {
        if !TERMINATION_REQUESTED.load(core::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if session.quit_deadline.is_some() {
            return;
        }
        info!("termination signal received; logging out");
        request_logout(&mut session, &mut commands, time.elapsed_secs());
    }
}

/// Route quit requests — menu ▸ Quit and the window close button (which, on
/// Wayland, includes a compositor-initiated close) — into a graceful
/// `request_logout`. The window's default close-to-exit is disabled
/// (`WindowPlugin { close_when_requested: false }`) so this handler owns the
/// close and can log out first.
pub fn handle_quit_requests(
    mut quit: MessageReader<QuitRequested>,
    mut closed: MessageReader<WindowCloseRequested>,
    time: Res<Time>,
    mut session: ResMut<ViewerSession>,
    mut commands: MessageWriter<SlCommand>,
) {
    let menu_quit = quit.read().count() > 0;
    let window_closed = closed.read().count() > 0;
    if menu_quit || window_closed {
        info!("quit requested; logging out");
        request_logout(&mut session, &mut commands, time.elapsed_secs());
    }
}

/// Request a clean grid logout and arm the quit deadline (idempotent): queue a
/// [`Command::Logout`] and record the wall-clock time by which
/// [`enforce_quit_deadline`] forces the exit if no `LoggedOut` arrives. Shared by
/// the quit request ([`handle_quit_requests`]) and the screenshot harness so both
/// leave the avatar cleanly logged out — an abrupt process exit strands the grid session and
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
pub fn save_settings_on_logout(
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
pub fn enforce_quit_deadline(
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

/// Announce the (user-tunable) draw distance to the simulator, re-sending it
/// whenever the [`SETTING_DRAW_DISTANCE`] setting changes and on every region
/// handshake.
///
/// The sim builds the agent's interest list around this radius, so it must be
/// (re)announced for a fresh region and updated live when the quick-preferences
/// panel (`crate::quick_preferences`) moves the draw-distance slider — the
/// reference viewer's `RenderFarClip` → `AgentSetAppearance`/interest behaviour.
/// The camera's far clip plane is deliberately *not* tied to this: the sky dome
/// and stars render at the fixed far plane (see `sl_viewer_world_scene::sky`), so shrinking it to
/// the draw distance would clip the sky. Only the streaming radius follows the
/// setting.
pub fn apply_draw_distance(
    settings: Option<Res<ViewerSettings>>,
    mut events: MessageReader<SlEvent>,
    mut applied: Local<Option<f32>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(settings) = settings else {
        return;
    };
    // A fresh region must be told the draw distance again, so drop the memo of
    // what was last announced (to the old region) on every handshake.
    if events
        .read()
        .any(|event| matches!(event.0, SlSessionEvent::RegionHandshakeComplete))
    {
        *applied = None;
    }
    let Ok(distance) = settings.store().get_f32(SETTING_DRAW_DISTANCE) else {
        return;
    };
    if *applied == Some(distance) {
        return;
    }
    *applied = Some(distance);
    info!("announcing draw distance {distance} m to the simulator");
    commands.write(SlCommand(Command::SetDrawDistance(Distance::new(
        f64::from(distance),
    ))));
}

/// Fold the session event stream into viewer actions: marking the agent in-world
/// on its first appearance, and a clean exit on logout/disconnect.
///
/// The camera is no longer placed here: third-person
/// (`crate::camera::position_camera`) follows the avatar the moment it arrives,
/// so there is nothing to snap. The `SL_VIEWER_CAMERA_*` framing knobs the old
/// snap read now seed the third-person orbit
/// ([`CameraRig::seed_orbit_from_env`](crate::world_api::CameraRig)).
pub fn drive_session(
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
                info!("region handshake complete");
                // The draw distance is announced by `apply_draw_distance`, and
                // the bandwidth throttle by the network & cache tab's
                // `apply_throttle` (`crate::preferences_network_cache`) — both
                // re-send their (user-tunable) setting on every handshake.
                // Drain the agent's stored offline instant messages, once — the
                // reference `LLIMProcessing::requestOfflineMessages`, which the
                // simulator otherwise holds and re-delivers on every login until
                // retrieved (OpenSim deletes them as it hands them over). They
                // arrive as ordinary `InstantMessageReceived` events with `offline`
                // set, folding into the conversations, offers and group-notice
                // surfaces like any live IM. The legacy UDP `RetrieveInstantMessages`
                // is used (not the modern `ReadOfflineMsgs` cap) because it carries
                // the per-message transaction ids our UDP accept paths need — the
                // cap path drops them, which is why the reference only uses the cap
                // when the AcceptFriendship / AcceptGroupInvite caps are also
                // present (a path we do not wire yet).
                if !session.offline_messages_requested {
                    info!("requesting stored offline instant messages");
                    commands.write(SlCommand(Command::RetrieveInstantMessages));
                    session.offline_messages_requested = true;
                }
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

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Command, SlCommand, SlEvent, SlSessionEvent};
    use sl_settings::{Scope, SettingValue};

    use super::{SETTING_DRAW_DISTANCE, apply_draw_distance, register_settings};
    use crate::settings::ViewerSettings;

    /// A boxed error so tests can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// Collects the `SetDrawDistance` commands `apply_draw_distance` emits.
    #[derive(Resource, Default)]
    struct Sent {
        /// How many draw-distance announcements were sent.
        count: usize,
        /// The metres of the most recent announcement.
        last: Option<f64>,
    }

    /// Drain `SlCommand`s, recording each draw-distance announcement.
    fn collect(mut reader: MessageReader<SlCommand>, mut out: ResMut<Sent>) {
        for command in reader.read() {
            if let Command::SetDrawDistance(distance) = &command.0 {
                out.count = out.count.saturating_add(1);
                out.last = Some(distance.meters());
            }
        }
    }

    /// A headless app with the draw-distance setting registered and the
    /// apply + collect systems chained.
    fn app() -> App {
        let mut settings = ViewerSettings::from_store_for_test(sl_settings::SettingsStore::new());
        register_settings(&mut settings);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SlCommand>()
            .add_message::<SlEvent>()
            .insert_resource(settings)
            .init_resource::<Sent>()
            .add_systems(Update, (apply_draw_distance, collect).chain());
        app
    }

    /// The draw distance is announced once, re-sent on change, deduped otherwise,
    /// and re-announced on every region handshake.
    #[test]
    fn draw_distance_announced_and_deduped() -> Result<(), TestError> {
        let mut app = app();

        // First frame: the default 512 m is announced once.
        app.update();
        assert_eq!(app.world().resource::<Sent>().count, 1);
        assert_eq!(app.world().resource::<Sent>().last, Some(512.0));

        // No change: nothing re-sent.
        app.update();
        assert_eq!(app.world().resource::<Sent>().count, 1);

        // A slider move (a store change) re-announces the new value.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_DRAW_DISTANCE,
            SettingValue::F32(256.0),
        );
        app.update();
        assert_eq!(app.world().resource::<Sent>().count, 2);
        assert_eq!(app.world().resource::<Sent>().last, Some(256.0));

        // A fresh region re-announces the (unchanged) draw distance.
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::RegionHandshakeComplete));
        app.update();
        assert_eq!(app.world().resource::<Sent>().count, 3);
        assert_eq!(app.world().resource::<Sent>().last, Some(256.0));
        Ok(())
    }

    /// The interest camera is the viewpoint the simulator builds the agent's
    /// object stream around, so it must be *this* frame's pose: the camera's
    /// `Transform`, not the `GlobalTransform` propagation only refreshes in
    /// `PostUpdate`. At the report's ~45 Hz cadence a frame is a whole report
    /// interval, so a stale read has the sim streaming toward where the camera was
    /// the entire time it is moving.
    ///
    /// Stage a camera whose two poses disagree and read back the reported centre.
    #[test]
    fn interest_camera_reports_the_current_frame_pose() -> Result<(), TestError> {
        use core::time::Duration;

        use super::{ViewerSession, report_camera_interest};
        use crate::world_api::ViewerCamera;

        /// The centre of the most recent `SetCamera` command.
        #[derive(Resource, Default)]
        struct Reported(Option<sl_client_bevy::Vector>);

        /// Drain `SlCommand`s, recording each interest-camera report.
        fn collect(mut reader: MessageReader<SlCommand>, mut out: ResMut<Reported>) {
            for command in reader.read() {
                if let Command::SetCamera(camera) = &command.0 {
                    out.0 = Some(camera.center.clone());
                }
            }
        }

        let mut app = App::new();
        // The report is gated on the agent being in-world.
        let session = ViewerSession {
            agent_in_world: true,
            ..ViewerSession::default()
        };
        app.add_message::<SlCommand>()
            .init_resource::<Time>()
            .init_resource::<Reported>()
            .insert_resource(session)
            .add_systems(Update, (report_camera_interest, collect).chain());

        // This frame's pose, as `position_camera` just wrote it, against last
        // frame's as propagation left the `GlobalTransform`.
        let current = Transform::from_xyz(40.0, 5.0, -60.0);
        let stale = Transform::from_xyz(-40.0, -5.0, 60.0);
        app.world_mut()
            .spawn((ViewerCamera, current, GlobalTransform::from(stale)));

        // Past the ~45 Hz rate limit, so this frame reports.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(100));
        app.update();

        // The report is in Second Life coordinates; converting the two candidate
        // poses the same way keeps the assertion about *which pose was read*.
        let reported = app
            .world()
            .resource::<Reported>()
            .0
            .clone()
            .ok_or("a moving camera in-world reports its viewpoint")?;
        assert_eq!(
            reported,
            crate::coords::bevy_to_sl_vec(current.translation),
            "the interest camera is this frame's pose, not the frame-old \
             GlobalTransform's ({:?})",
            crate::coords::bevy_to_sl_vec(stale.translation),
        );
        Ok(())
    }
}
