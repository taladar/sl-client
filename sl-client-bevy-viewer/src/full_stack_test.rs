//! The **full-stack tier** ([[viewer-fake-grid-render-harness]]): the real
//! viewer against the real [`sl_fake_grid`] loopback grid, read back as pixels.
//!
//! # What this tier is for
//!
//! Every other tier cuts the pipeline somewhere. [`crate::world_test`] starts
//! at [`SlEvent`] and has no renderer; [`crate::render_readback`] starts at a
//! registered scene and has no grid; `sl-client-bevy`'s `fake_grid_login_smoke`
//! has both ends of the network but stops at the ECS state. Between an object
//! arriving on the wire and a lit pixel of it there is a whole chain nobody
//! else runs end to end: the capability announcement, the asset fetch, the
//! decode, the mesh build, the material, the camera. **This tier is the one
//! that renders what a grid actually said.**
//!
//! # Why it lives inside the crate
//!
//! An integration test (`tests/`) can only see the crate's public surface, and
//! everything this needs is deliberately private: the readback cell, the
//! settle loop, the pixel oracles, the plugin groups, the world queries. So it
//! is a `#[cfg(test)]` module of the library, exactly as
//! [`crate::render_readback`] is, and `sl-fake-grid` + `tokio` are
//! dev-dependencies.
//!
//! # What is asserted
//!
//! The same rule as the rest of the pixel tiers, for the same reason: **no
//! golden images**. What is decided here is decidable — that the sky is above
//! the sea is above the ground, that a subject's own disc is not the
//! background, that a killed object's disc *is* the background again. A driver
//! change moves none of those answers.
//!
//! # Skipping, not failing
//!
//! A machine with no GPU adapter cannot answer a question about pixels. Every
//! test here takes [`gpu_lock`] and returns `Ok` with a log line when
//! [`ViewerHarness::capture`] finds no adapter — the tier skips loudly rather
//! than failing, as the readback tier does.
//!
//! # Waiting, not sleeping
//!
//! Two clocks run here that a test cannot step: the grid's tokio runtime and
//! the client's network thread. So this tier steps frames against a **wall
//! clock deadline** and waits on observations — an event, a marker, a quiet
//! render — never on a duration. The one thing a duration decides is when to
//! give up, and giving up dumps the recorded event tail so a timeout says what
//! the session was doing rather than only that it stopped.
//!
//! [[viewer-fake-grid-render-harness]]: the roadmap task this implements.

use core::time::Duration;
use std::sync::MutexGuard;
use std::time::Instant;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::winit::WinitPlugin;
use tracing::subscriber::DefaultGuard;

use sl_client_bevy::{
    ChatLogConfig, ClientDirectories, Command, InventoryCacheConfig, LoginParams, LoginRequest,
    SlCapabilities, SlClientPlugin, SlCommand, SlEvent, SlSessionEvent, StartLocation,
};
use sl_fake_grid::{
    AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig, RegionFixture,
};

use crate::pixel_oracle::Frame;
use crate::render_readback::{
    Captured, FRAME, HOLD_FRAMES, PipelineStatusPlugin, Projected, SettleError, gpu_lock, settle,
};
use crate::render_test::{LogCapture, TestError, capture_logs};
use crate::viewer_camera::viewer_camera_bundle;
use crate::viewer_plugins::{ViewerInputPlugins, ViewerRenderPlugins, ViewerWorldPlugins};
use crate::world_api::{CameraMode, ViewerCamera};

/// The account every harness logs in as.
///
/// One name for the whole tier: the grid mints a fresh agent id per grid, so
/// two harnesses in one process are still two different avatars, and a fixed
/// name keeps a failure message readable. No real avatar name appears here.
const FIRST_NAME: &str = "Full";
/// The account's surname — see [`FIRST_NAME`].
const LAST_NAME: &str = "Stack";
/// The account's password on the loopback grid.
const PASSWORD: &str = "password";

/// The longest any single wait may run before the harness gives up and dumps
/// the recorded event tail.
///
/// Generous: a cold shader cache compiles pipelines the first time this tier
/// runs on a machine, and the whole point of a deadline here is to turn a hang
/// into a report, not to police how fast the machine is.
const WAIT: Duration = Duration::from_secs(60);

/// How long to sleep between frames while waiting on the network.
///
/// Nonzero because the frames this steps are not the work being waited for: a
/// login, an asset fetch and a CAPS long-poll all progress on other threads,
/// and spinning `update` as fast as possible starves them of the very core
/// they need.
const FRAME_PAUSE: Duration = Duration::from_millis(2);

/// The number of land patches a 256 m region streams — the whole ground.
///
/// A capture taken before they have all arrived shows holes, so
/// [`ViewerHarness::login`] waits for the set rather than for the first one.
const REGION_PATCHES: usize = 256;

/// Where the day is pinned for every capture, as a fraction of the day cycle.
///
/// The sky is a function of the clock, and the grid's clock runs; two captures
/// minutes apart would light the scene differently and a band classification
/// would be comparing times of day. `0.25` is the middle of the day track:
/// the sun is up, so the ground is lit and the sea is not black.
const DAY_POSITION: f32 = 0.25;

/// Every [`SlSessionEvent`] the plugin emitted, in order, plus the capability
/// maps it published.
///
/// Recorded rather than read live because a test asks its questions between
/// frames and an unread `MessageReader` would drop what it missed; a timeout
/// also dumps the tail from here.
#[derive(Resource, Default)]
struct Recorded {
    /// The session events, oldest first.
    events: Vec<SlSessionEvent>,
}

/// What the scene still owes, mirrored out of
/// [`SceneQuiescence`](sl_viewer_world_view::quiescence::SceneQuiescence) once a frame.
///
/// Mirrored rather than read directly because a `SystemParam` can only be read
/// inside a system, and the harness asks its questions from outside one.
#[derive(Resource, Default)]
struct SceneWork {
    /// Whether a region is up and nothing is outstanding.
    quiet: bool,
    /// Everything still in flight or queued across every asset store.
    outstanding: usize,
}

/// Mirrors the scene's outstanding work into [`SceneWork`].
fn note_scene_work(
    scene: sl_viewer_world_view::quiescence::SceneQuiescence,
    mut work: ResMut<SceneWork>,
) {
    work.quiet = scene.is_quiet();
    work.outstanding = scene.outstanding();
}

/// Appends this frame's events to [`Recorded`].
fn record(mut events: MessageReader<SlEvent>, mut recorded: ResMut<Recorded>) {
    for event in events.read() {
        recorded.events.push(event.0.clone());
    }
}

/// Drains the capability announcements so the channel does not grow unread.
///
/// The world group's `update_*_caps` systems are the real consumers; this only
/// exists so a `MessageReader`-less channel is not the reason a test's frame
/// budget goes on message bookkeeping.
fn drain_capabilities(mut capabilities: MessageReader<SlCapabilities>) {
    for _caps in capabilities.read() {}
}

/// The fixture that stands the **stock** region up as a [`RegionFixture`].
///
/// [`sl_fake_grid::Scenario::default`] is the stock region and it is not a
/// fixture, so a harness that only takes fixtures could not ask for it. This
/// carries the two halves that matter to a picture — the stock world (the
/// region-wide parcel and the scripted box) and the stock asset store — and
/// leaves the rest (the greeting chat, the UDP asset fixtures) behind, because
/// none of it is visible.
pub(crate) fn stock_fixture() -> RegionFixture {
    RegionFixture {
        world: sl_fake_grid::scenario::default_world(),
        assets: sl_fake_grid::scenario::default_assets(),
        ..RegionFixture::new()
    }
}

/// The real viewer, logged into a real (loopback) grid, rendering to a texture
/// this reads back.
///
/// Owns everything with a lifetime: the tokio runtime the grid's tasks run on
/// must outlive the grid, the grid must outlive the app that is talking to it,
/// and the GPU lock must outlive the app that holds an adapter.
pub(crate) struct ViewerHarness {
    /// The tokio runtime hosting the grid's tasks (must outlive the grid).
    runtime: tokio::runtime::Runtime,
    /// The grid; dropping it shuts every session and socket down.
    grid: FakeGrid,
    /// The viewer app.
    app: App,
    /// The cell each rendered frame is read back into.
    captured: Captured,
    /// The grid-side handle onto this agent's session, once logged in.
    agent: Option<FakeAgent>,
    /// Login notices, subscribed before the app starts.
    logins: tokio::sync::broadcast::Receiver<sl_fake_grid::LoginNotice>,
    /// Completed inter-region teleports, subscribed before the app starts.
    ///
    /// A teleport is **not** a login: the grid retires the source session and
    /// opens a destination one without any login of its own, so the login
    /// broadcast stays silent and only this names the session the agent now
    /// lives in.
    teleports: tokio::sync::broadcast::Receiver<sl_fake_grid::TeleportNotice>,
    /// Everything the rig logged at `WARN` or above, for a timeout report.
    ///
    /// Not asserted empty, unlike the readback tier's: a real session against a
    /// real grid legitimately warns (a retried fetch, an unimplemented cap),
    /// and a tier that failed on any of those would be a tier nobody runs.
    logs: LogCapture,
    /// Keeps the log capture installed for this harness's lifetime.
    _log_guard: DefaultGuard,
    /// Serialises this tier against the other GPU tiers in the same process.
    _gpu: MutexGuard<'static, ()>,
}

impl ViewerHarness {
    /// Start a grid serving `fixture` on the stock region, and build (but do
    /// not step) a viewer logging into it.
    pub(crate) fn start(fixture: RegionFixture) -> Result<Self, TestError> {
        Self::start_in(vec![fixture.into_region(RegionConfig::default())])
    }

    /// [`start`](Self::start) against a grid serving `regions`, the first of
    /// which the account starts in.
    ///
    /// The general form: a teleport needs a second region, and a region needs
    /// more than a fixture to describe (its name, its place on the grid, its
    /// water height).
    pub(crate) fn start_in(regions: Vec<RegionConfig>) -> Result<Self, TestError> {
        let _gpu = gpu_lock();
        let (logs, _log_guard) = capture_logs();
        let start_region = regions.first().ok_or("a grid needs at least one region")?;
        let start = StartLocation::region(
            start_region.name.clone(),
            sl_proto::RegionCoordinates::new(128.0, 128.0, 26.0),
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let mut builder = FakeGridBuilder::new()
            .account(AccountConfig::new(FIRST_NAME, LAST_NAME, PASSWORD))
            // A long hold, so the CAPS long-poll is not a busy loop competing
            // with the render for this machine's cores.
            .event_queue_hold(Duration::from_secs(2));
        for region in regions {
            builder = builder.region(region);
        }
        let grid = runtime.block_on(builder.start())?;
        let logins = grid.logins();
        let teleports = grid.teleports();
        let params = LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                FIRST_NAME,
                LAST_NAME,
                PASSWORD,
                start,
                "sl-fake-grid-full-stack",
                "0.0",
            ),
        };
        let (mut app, captured) = build_viewer_app(params);
        // `App::finish` / `cleanup` are what build the render app and publish
        // its `RenderDevice` into the main world; the plain `update` loop this
        // harness drives never calls them on its own, and without them Bevy's
        // own batching systems fail parameter validation on the very first
        // frame. If there is no adapter this is where it gives up, and the
        // no-adapter skip is decided later, by outcome, in `wait_quiet`.
        app.finish();
        app.cleanup();
        Ok(Self {
            runtime,
            grid,
            app,
            captured,
            agent: None,
            logins,
            teleports,
            logs,
            _log_guard,
            _gpu,
        })
    }

    /// Step frames until the session is in world: the circuit is up, the
    /// handshake is done, and the whole ground has arrived.
    ///
    /// Also picks up the grid-side [`FakeAgent`], which is what a test drives
    /// the other end of the conversation with.
    pub(crate) fn login(&mut self) -> Result<(), TestError> {
        let notice = self.run_until("the grid's login notice", |harness| {
            harness.logins.try_recv().ok()
        })?;
        self.agent = self.runtime.block_on(self.grid.agent(&notice));
        self.wait_event("RegionHandshakeComplete", |event| {
            matches!(event, SlSessionEvent::RegionHandshakeComplete).then_some(())
        })?;
        // The ground, all of it: a capture taken while patches are still
        // arriving shows holes where the terrain has not been built, and a
        // band classification would read one as sky.
        self.run_until("the region's whole ground", |harness| {
            let patches = harness
                .app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        SlSessionEvent::TerrainPatch(patch)
                            if patch.layer == sl_proto::TerrainLayerType::Land
                    )
                })
                .count();
            (patches >= REGION_PATCHES).then_some(())
        })?;
        Ok(())
    }

    /// Step frames until `done` answers `Some`, or fail after [`WAIT`] with the
    /// recorded event tail and the logged warnings.
    ///
    /// The one waiting primitive: [`wait_event`](Self::wait_event) and
    /// [`wait_marker`](Self::wait_marker) are both written in terms of it.
    pub(crate) fn run_until<T>(
        &mut self,
        what: &str,
        mut done: impl FnMut(&mut Self) -> Option<T>,
    ) -> Result<T, TestError> {
        let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
        loop {
            self.app.update();
            if let Some(value) = done(self) {
                return Ok(value);
            }
            if Instant::now() >= deadline {
                return Err(self.timeout_report(what).into());
            }
            std::thread::sleep(FRAME_PAUSE);
        }
    }

    /// What a timeout has to say for itself: how much the scene still owes, the
    /// last events the session saw, and the last warnings it logged.
    ///
    /// All three, because they answer different questions — the outstanding
    /// count says whether the session is working or stuck, the events say how
    /// far it got, and the warnings often say why it got no further.
    fn timeout_report(&self, what: &str) -> String {
        let outstanding = self.app.world().resource::<SceneWork>().outstanding;
        let events: Vec<String> = self
            .app
            .world()
            .resource::<Recorded>()
            .events
            .iter()
            .rev()
            .take(12)
            .map(|event| format!("{event:?}").chars().take(160).collect::<String>())
            .collect();
        let warnings: Vec<String> = self.logs.events().into_iter().rev().take(8).collect();
        format!(
            "timed out waiting for {what}\n  outstanding asset work: {outstanding}\n  last \
             events: {events:#?}\n  last warnings: {warnings:#?}"
        )
    }

    /// Step frames until an event matching `pick` has been recorded.
    ///
    /// Over the whole recorded history, not just this frame's: an event a test
    /// is about to wait for may already have arrived while it was waiting for
    /// something else.
    pub(crate) fn wait_event<T>(
        &mut self,
        what: &str,
        mut pick: impl FnMut(&SlSessionEvent) -> Option<T>,
    ) -> Result<T, TestError> {
        self.run_until(what, |harness| {
            harness
                .app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .find_map(&mut pick)
        })
    }

    /// Step frames until the grid's marker called `name` has reached the
    /// client.
    ///
    /// The way a test synchronises with grid-side work without sleeping: UDP
    /// delivery is ordered per circuit, so a marker sent after a `KillObject`
    /// arrives after it, and the client having seen the marker means it has
    /// seen the kill. See [`sl_fake_grid::marker`].
    pub(crate) fn wait_marker(&mut self, name: &str) -> Result<(), TestError> {
        let wanted = name.to_owned();
        self.wait_event(&format!("the `{name}` marker"), move |event| match event {
            SlSessionEvent::GenericMessage(generic) => {
                (sl_fake_grid::marker_name(generic).as_ref() == Some(&wanted)).then_some(())
            }
            _ => None,
        })
    }

    /// Step frames until the whole viewer is **quiet** — both halves of it.
    ///
    /// The *scene* is quiet when a region is up and every asset store has
    /// nothing in flight: the textures, meshes, wearables, animations and
    /// environment settings the arrival asked for have arrived and been built.
    /// The *render* is quiet when a frame has come back, no pipeline is queued
    /// or compiling, and every live reflection probe has captured.
    ///
    /// Both, and in that order, because they gate different things. A frame
    /// taken while a texture is still decoding shows an untextured face; a
    /// frame taken while a pipeline is still compiling shows nothing at all.
    ///
    /// Returns `false` when no frame ever came back, which means this machine
    /// has no usable GPU adapter and the caller should skip.
    pub(crate) fn wait_quiet(&mut self) -> Result<bool, TestError> {
        self.run_until("the scene's assets to arrive", |harness| {
            harness
                .app
                .world()
                .resource::<SceneWork>()
                .quiet
                .then_some(())
        })?;
        match settle(&mut self.app, &self.captured) {
            Ok(()) => Ok(true),
            Err(SettleError::NoAdapter) => Ok(false),
            Err(error) => Err(format!("the viewer never settled: {error}").into()),
        }
    }

    /// Settle the render and read the frame back.
    ///
    /// `None` on a machine with no GPU adapter — the caller logs and returns
    /// `Ok`, as every test in this tier does.
    pub(crate) fn capture(&mut self) -> Result<Option<Frame>, TestError> {
        if !self.wait_quiet()? {
            return Ok(None);
        }
        // A readback completes a frame or more after the render it belongs to,
        // so step past the settle before draining the slot: otherwise the frame
        // taken is the one rendered while the last pipeline was still compiling.
        for _frame in 0..HOLD_FRAMES {
            self.app.update();
        }
        let bytes = self
            .captured
            .take()
            .ok_or("the readback slot was empty after a settled render")?;
        Ok(Some(Frame::from_rgba8(bytes, FRAME, FRAME).ok_or(
            "the readback and the render target disagree about the frame size",
        )?))
    }

    /// Where `points` (in **Second Life region-local metres**, the frame the
    /// grid speaks) land on the frame, projected through the very camera that
    /// drew it.
    ///
    /// Region-local rather than Bevy world space because everything a test
    /// knows a position of — a fixture's row slot, the stock box — is stated in
    /// region metres by the grid. The scene origin is the region the session is
    /// rooted in, so the conversion is the plain axis map.
    pub(crate) fn project(&mut self, points: &[sl_proto::Vector]) -> Projected {
        let bevy: Vec<Vec3> = points
            .iter()
            .map(sl_viewer_kit::coords::sl_to_bevy_vec)
            .collect();
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
        cameras
            .single(self.app.world())
            .map(|(camera, transform)| {
                Projected(
                    bevy.iter()
                        .map(|point| camera.world_to_viewport(transform, *point).ok())
                        .collect(),
                )
            })
            .unwrap_or_default()
    }

    /// Place the camera at `eye` looking at `target`, both in Second Life
    /// region-local metres.
    ///
    /// A flycam pose, so nothing moves it afterwards: the third-person camera
    /// follows the avatar and would frame whatever the avatar happened to be
    /// doing, which is not a framing a test can state.
    pub(crate) fn look_from(&mut self, eye: sl_proto::Vector, target: sl_proto::Vector) {
        let eye = sl_viewer_kit::coords::sl_to_bevy_vec(&eye);
        let target = sl_viewer_kit::coords::sl_to_bevy_vec(&target);
        *self.app.world_mut().resource_mut::<CameraMode>() = CameraMode::Flycam;
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<&mut Transform, With<ViewerCamera>>();
        for mut transform in cameras.iter_mut(self.app.world_mut()) {
            *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        }
    }

    /// Run `future` on the harness's runtime — how a test drives the **grid**
    /// side of the conversation.
    ///
    /// Blocking is safe here and only here: everything a test asks the grid to
    /// do this way is local work on an already-running session, so it resolves
    /// without needing the viewer to step a frame. Anything that waits on the
    /// client (a teleport) goes through the client's own command path instead.
    pub(crate) fn grid<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// The viewer's world, for a claim about ECS state rather than pixels.
    ///
    /// Some of what this tier proves is not visible: that a mesh asset decoded,
    /// that a capability was announced. Those are read here, beside the picture
    /// they explain, rather than in a tier that could not also look at it.
    pub(crate) fn world(&self) -> &World {
        self.app.world()
    }

    /// The grid-side handle onto this session, after [`login`](Self::login).
    pub(crate) fn agent(&self) -> Result<FakeAgent, TestError> {
        self.agent
            .clone()
            .ok_or_else(|| "the harness has not logged in yet".into())
    }

    /// Send a marker from the grid, named `name`.
    ///
    /// The send half of [`wait_marker`](Self::wait_marker): a test calls this
    /// after the grid-side work it wants to wait for, then waits for the name.
    pub(crate) fn mark(&self, name: &str) -> Result<(), TestError> {
        let agent = self.agent()?;
        let now = agent.now();
        self.grid(
            agent.with_sim(|sim| sim.send_generic_message(&sl_fake_grid::marker(name), now)),
        )?;
        Ok(())
    }

    /// Write a command into the plugin's outbound stream.
    fn command(&mut self, command: Command) {
        self.app.world_mut().write_message(SlCommand(command));
    }

    /// Teleport the avatar to `region_name` at `position`, and wait until the
    /// destination region's ground has arrived.
    ///
    /// The client's own path (a `Teleport` command), not the grid's lure
    /// helper: what this tier exists to exercise is the viewer, and a teleport
    /// is one of the few things that replaces the whole scene.
    pub(crate) fn teleport_to(
        &mut self,
        region_name: &str,
        position: sl_proto::RegionCoordinates,
    ) -> Result<(), TestError> {
        let handle = self
            .grid
            .region_handle(region_name)
            .ok_or_else(|| format!("the grid serves no region called {region_name:?}"))?;
        let before = self
            .app
            .world()
            .resource::<Recorded>()
            .events
            .len()
            .saturating_sub(1);
        self.command(Command::Teleport {
            region_handle: handle,
            position,
            look_at: sl_proto::Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        });
        // From the command onward only: an earlier `RegionChanged` (the login's
        // own arrival) would answer this instantly.
        self.run_until(&format!("the arrival in {region_name}"), |harness| {
            harness
                .app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .skip(before)
                .any(|event| {
                    matches!(
                        event,
                        SlSessionEvent::RegionChanged { region_handle, .. }
                            if *region_handle == handle
                    )
                })
                .then_some(())
        })?;
        // The destination's ground streams after the arrival, patch by patch,
        // exactly as the login's did.
        self.run_until("the destination region's ground", |harness| {
            let patches = harness
                .app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .skip(before)
                .filter(|event| {
                    matches!(
                        event,
                        SlSessionEvent::TerrainPatch(patch)
                            if patch.layer == sl_proto::TerrainLayerType::Land
                                && patch.region_handle == handle
                    )
                })
                .count();
            (patches >= REGION_PATCHES).then_some(())
        })?;
        // The grid hands a teleported agent a fresh session and retires the old
        // one, so a test driving the grid afterwards must have the new handle.
        // It comes off the teleport broadcast, not the login one: nothing logs
        // in during a teleport.
        let destination = self.run_until("the destination session", |harness| {
            let notice = harness.teleports.try_recv().ok()?;
            harness
                .runtime
                .block_on(harness.grid.agent_by_seq(notice.to_seq))
        })?;
        self.agent = Some(destination);
        Ok(())
    }

    /// Log out cleanly and wait for the client to say so.
    pub(crate) fn logout(&mut self) -> Result<(), TestError> {
        self.command(Command::Logout);
        self.wait_event("LoggedOut", |event| {
            matches!(event, SlSessionEvent::LoggedOut).then_some(())
        })
    }
}

/// Build the headless viewer: the readback base (no window, no winit, no log,
/// no render thread), the viewer's own world, input and render groups, the real
/// client plugin, and a camera rendering into the texture this reads back.
///
/// The UI, shell and edit groups are deliberately absent. They own no pixel of
/// the world this tier looks at, they drag in CEF and the whole floater
/// scaffold, and the UI tier already covers them under a synthetic pointer.
fn build_viewer_app(params: LoginParams) -> (App, Captured) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                // Headless: no window at all, and the app must not exit for the
                // lack of one.
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            // No event loop: the harness drives `update` itself, so the frames
            // are counted rather than raced.
            .disable::<WinitPlugin>()
            // No render thread either: with the render app run inline, one
            // `update` is exactly one rendered frame and everything the render
            // world logs lands on this thread's log capture.
            .disable::<PipelinedRenderingPlugin>()
            // The harness owns the subscriber (`capture_logs`); two would clash.
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
    .add_plugins(PipelineStatusPlugin);

    // The render overrides, stated rather than read from the environment: this
    // is a test, and a developer with `SL_VIEWER_DISABLE_GLOW` exported in their
    // shell must not get a different answer from CI. The day is pinned so the
    // sky is the same one in every run.
    let overrides = crate::render_overrides::RenderOverrides {
        day_position: Some(DAY_POSITION),
        ..crate::render_overrides::RenderOverrides::default()
    };
    app.insert_resource(crate::environment::EnvironmentState::from_overrides(
        &overrides,
    ));
    app.insert_resource(overrides);

    // The login-parameter resources `run_session` inserts before the groups.
    app.insert_resource(crate::settings::ViewerSettings::declared_for_test(
        crate::REGISTRARS,
    ));
    app.insert_resource(crate::animations::AnimationManager::new());
    app.init_resource::<crate::camera::CameraStart>();
    app.init_resource::<crate::camera::CameraSpin>();

    // Resources the world and render groups *read* but do not own, whose
    // owners are in the groups this harness leaves out. Bevy fails a system's
    // parameter validation on a missing resource, and the message names
    // neither the system nor the resource without a debug rebuild — so these
    // are inserted deliberately, in their empty state, rather than discovered
    // one panic at a time. The same list the fixture world keeps, minus what
    // the input group brings with it.
    app.init_resource::<crate::world_api::SelectionSet>();
    app.init_resource::<crate::world_api::DerenderList>();
    app.init_resource::<crate::world_api::MatModeState>();
    app.init_resource::<crate::world_api::EditToolState>();
    app.init_resource::<crate::world_api::FriendsModel>();
    app.init_resource::<sl_viewer_world_avatar::avatar_complexity::AvatarComplexityModel>();
    app.init_resource::<crate::avatar_render_settings::AvatarRenderSettings>();
    app.init_resource::<sl_viewer_inventory::inventory::InventoryModel>();
    // The tracked map destination the in-world beacon draws from, owned by the
    // map floater.
    app.init_resource::<crate::world_api::MapTracking>();
    // There is no cursor to grab: an unattended run must never reach for the
    // desktop's pointer, the same reason screenshot mode says `false`.
    app.insert_resource(crate::input_context::CursorGrabAllowed(false));

    // The message vocabulary the world group writes into, registered wholesale
    // for the same reason as the resources above: an unregistered `Messages<T>`
    // fails a system's parameter validation the moment that system runs, which
    // in a full session is the moment a menu is dispatched or a sound is asked
    // for — not at startup, where it would be found.
    app.add_message::<sl_viewer_ui_core::ui_element::UiAction>();
    app.add_message::<sl_viewer_ui_core::ui_sounds::PlayUiSound>();
    app.add_message::<sl_viewer_notifications::ShowNotification>();
    app.add_message::<crate::derender::RequestDerender>();
    app.add_message::<crate::about_land::OpenAboutLand>();
    app.add_message::<crate::edit_contents::OpenObjectContents>();
    app.add_message::<crate::avatar_render_settings::RequestRenderException>();
    app.add_message::<crate::contact_sets_panel::OpenSetPseudonym>();
    app.add_message::<crate::world_api::OpenAvatarProfile>();
    app.add_message::<crate::world_api::OpenConversation>();
    app.add_message::<crate::world_api::OpenAddToContactSet>();
    app.add_message::<crate::world_api::MediaWorldClick>();
    app.add_message::<crate::world_api::OpenGroupProfile>();
    app.add_message::<crate::world_api::OpenAvatarPicker>();
    app.add_message::<crate::world_api::AvatarPicked>();
    app.add_message::<crate::world_api::OpenTexturePicker>();
    app.add_message::<crate::world_api::TexturePicked>();
    app.add_message::<crate::world_api::OpenWebBrowser>();
    app.add_message::<crate::world_api::BeginTeleportFlow>();
    app.add_message::<crate::world_api::ContentsMutated>();
    app.add_message::<crate::world_api::OpenNotecard>();
    app.add_message::<crate::world_api::OpenScript>();
    app.add_message::<crate::world_api::StartConference>();

    app.add_plugins(SlClientPlugin {
        params,
        diagnostics: false,
        chat_log_config: ChatLogConfig::default(),
        directories: ClientDirectories::default(),
        account_dirs: None,
        inventory_cache_config: InventoryCacheConfig::default(),
        background_inventory_fetch: false,
        fetch_server_chat_history: false,
        offline: false,
    })
    .add_plugins(ViewerRenderPlugins::default())
    .add_plugins(ViewerWorldPlugins::default())
    .add_plugins(ViewerInputPlugins::without_devices())
    .init_resource::<Recorded>()
    .init_resource::<SceneWork>()
    .init_resource::<Captured>()
    // After the plugin's `(drive, maintain_world)` chain, so a frame's world
    // state and its events are observed together.
    .add_systems(PostUpdate, (record, drain_capabilities, note_scene_work));

    let captured = app.world().resource::<Captured>().clone();

    // The render target: an ordinary image, plus `COPY_SRC` so the readback can
    // lift it back off the GPU.
    let mut target = Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);

    let readback_target = target.clone();
    app.add_systems(Startup, move |mut commands: Commands| {
        // The viewer's own camera bundle — its exposure, its HDR target, the
        // `ViewerCamera` marker every world phase reads — aimed into the
        // readback target instead of at a window that does not exist. A
        // provisional pose; each test frames its own subject with `look_from`.
        commands.spawn((
            viewer_camera_bundle(Transform::from_xyz(128.0, 30.0, -128.0)),
            crate::world_api::CameraRig::default(),
            RenderTarget::Image(readback_target.clone().into()),
            Name::new("full-stack-camera"),
        ));
        // The observer is attached to **this** readback entity rather than
        // registered globally. The full viewer has other readbacks running —
        // the GPU pick lifts its ID buffer back the same way, and the GPU
        // avatar pipeline its palettes — and a global `On<ReadbackComplete>`
        // would drain whichever fired last into the frame slot. The symptom is
        // not subtle but it is misleading: the buffer is the wrong length for
        // a 256² frame and the capture reads as "the readback and the render
        // target disagree about the frame size".
        commands
            .spawn(Readback::texture(readback_target.clone()))
            .observe(
                move |readback: On<ReadbackComplete>, captured: Res<Captured>| {
                    captured.set(readback.data.clone());
                },
            );
    });

    (app, captured)
}

#[cfg(test)]
mod tests {
    use super::{FRAME, ViewerHarness, stock_fixture};

    use bevy::prelude::*;
    use sl_fake_grid::RegionConfig;
    use sl_fake_grid::fixtures::catalogue::{CatalogueEntry, MESH_ASSET, ROW_Y, entry};
    use sl_proto::Vector;

    use crate::pixel_oracle::{
        Frame, Marker, Silhouette, band_mean, coverage, health, pixels_differ,
    };
    use crate::render_readback::Projected;
    use crate::render_test::TestError;

    /// How far south of a catalogue subject its camera stands, in metres.
    ///
    /// Close enough that a 1 m prim covers a few hundred pixels of a 256²
    /// frame, far enough that the neighbours 4 m to either side fall outside
    /// the 60° frame.
    const SUBJECT_DISTANCE: f32 = 6.0;

    /// The height a subject's camera stands at: the catalogue's ground.
    ///
    /// At ground level, aiming *up* past the subject, everything behind it is
    /// sky — so a disc taken on the subject is the subject against the sky and
    /// nothing else. The row's prims rest with their bottom faces on this
    /// plane, so the whole of one stands above the horizon.
    const SUBJECT_EYE_Z: f32 = 25.0;

    /// How far above the subject its camera aims, in metres: enough to put the
    /// horizon below the bottom of the subject.
    const SUBJECT_LOOK_UP: f32 = 1.0;

    /// A band mean darker than this in every channel is black, and a black band
    /// is one where nothing was drawn at all.
    const NOT_BLACK: f32 = 1.0 / 255.0;

    /// How much of its own disc a checkered face must paint in each of its two
    /// colours before the checker counts as having arrived.
    ///
    /// A quarter rather than a half: the disc is a circle inscribed in a square
    /// face seen at an angle, the checker's cells fall where they fall, and
    /// shading darkens the faces turned away from the sun.
    const CHECKER_SHARE: f32 = 0.15;

    /// How much of a killed subject's disc may still carry one of its colours.
    ///
    /// Not zero: the disc is the subject's *bounding* circle, so its corners
    /// were never the subject and whatever is behind it is free to be any
    /// colour it likes.
    const KILLED_SHARE: f32 = 0.02;

    /// A sampled pixel with its alpha discarded.
    ///
    /// **The alpha of this viewer's composited frame is the glow mask, not
    /// opacity** — the same channel `screenshot.rs` drops before it writes a
    /// PNG. Two bands that carry the same colour and a different glow are not
    /// two different things to look at, so every comparison here is made on
    /// colour alone.
    fn colour(pixel: Vec4) -> Vec4 {
        Vec4::new(pixel.x, pixel.y, pixel.z, 0.0)
    }

    /// Log the skip a machine with no GPU adapter takes, in the one wording the
    /// whole tier uses.
    fn no_adapter(what: &str) {
        warn!("skipping {what}: no frame came back, so this machine has no usable GPU adapter");
    }

    /// Frame `subject` against the sky and return the disc its 1 m body covers.
    ///
    /// The disc is derived by projecting the subject's centre and a point on
    /// its body through the very camera that drew the frame, rather than
    /// computed from the field of view by hand — a hand-computed disc is how a
    /// pixel check ends up measuring the background.
    fn frame_subject(
        harness: &mut ViewerHarness,
        subject: &CatalogueEntry,
    ) -> Result<Option<(Frame, Silhouette)>, TestError> {
        let at = subject.position();
        harness.look_from(
            Vector {
                x: at.x,
                y: at.y - SUBJECT_DISTANCE,
                z: SUBJECT_EYE_Z,
            },
            Vector {
                x: at.x,
                y: at.y,
                z: at.z + SUBJECT_LOOK_UP,
            },
        );
        let Some(frame) = harness.capture()? else {
            return Ok(None);
        };
        let edge = Vector {
            x: at.x,
            y: at.y,
            z: at.z + 0.35,
        };
        let projected = harness.project(&[at, edge]);
        let disc = disc_from(&projected)?;
        Ok(Some((frame, disc)))
    }

    /// The disc a projected (centre, edge) pair describes.
    fn disc_from(projected: &Projected) -> Result<Silhouette, TestError> {
        let (centre, edge) = projected.get(0).zip(projected.get(1)).ok_or(
            "the subject did not project onto the frame — the camera is not looking at it",
        )?;
        // Component by component: the workspace's `arithmetic_side_effects`
        // lint bans the overloaded `Vec2` operator, as the readback rig's own
        // silhouette maths already works around.
        let radius = Vec2::new(edge.x - centre.x, edge.y - centre.y).length();
        if radius < 4.0 {
            return Err(format!(
                "the subject covers only {radius} px of the frame — too few to decide anything \
                 about"
            )
            .into());
        }
        Ok(Silhouette { centre, radius })
    }

    /// **A login renders the ground, the sea and the sky — each in its place.**
    ///
    /// The first question this tier exists to answer, and the one nothing else
    /// can: a login against a real grid produces a *picture*. From 60 m over the
    /// middle of a flat 25 m region, looking level north, the frame has three
    /// horizontal bands whose boundaries are geometry, not taste — sky above the
    /// horizon, the endless sea from the horizon down to where the region's own
    /// ground ends, and that ground below it.
    ///
    /// What is asserted is that the three are three *different* things and that
    /// none of them is black. No colour is named: the sea's colour is the
    /// region's water settings and the ground's is its detail textures, and both
    /// are content. A sea that failed to render would read as the sky and a
    /// ground that failed would read as the sea — which is the failure this
    /// catches.
    #[test]
    fn a_login_renders_the_ground_the_sea_and_the_sky() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start(stock_fixture())?;
        harness.login()?;
        // Level north over the middle of the region, high enough that the
        // ground's far edge is well inside the frame.
        harness.look_from(
            Vector {
                x: 128.0,
                y: 128.0,
                z: 60.0,
            },
            Vector {
                x: 128.0,
                y: 228.0,
                z: 60.0,
            },
        );
        let Some(frame) = harness.capture()? else {
            no_adapter("the login band classification");
            return Ok(());
        };
        // Only the black half of the health verdict. **This viewer's frames are
        // fully transparent by design**: the alpha channel of its composited
        // output is the glow mask, not opacity (`screenshot.rs` says the same
        // where it drops the channel before writing a PNG), so
        // `all_transparent` is the normal state of a correct frame here and
        // asserting against it would fail every capture in this tier.
        assert!(
            !health(&frame).all_black,
            "the viewer logged in and rendered a black frame"
        );

        // The horizon is the view axis, which a level camera puts on the middle
        // row; the far shore is where the region's own ground stops.
        let horizon = FRAME / 2;
        let shore = harness.project(&[Vector {
            x: 128.0,
            y: 256.0,
            z: 25.0,
        }]);
        let shore_row = shore
            .get(0)
            .ok_or("the region's far edge is off the frame")?
            .y;
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a projected row of a 256-row frame, already known to be on it"
        )]
        let shore_row = shore_row.max(0.0) as u32;
        assert!(
            shore_row > horizon + 16 && shore_row < FRAME - 32,
            "the region's far edge projects to row {shore_row}, which leaves no room for a sea \
             band between it and the horizon at {horizon} — the framing this test reasons about \
             is not the one it rendered"
        );

        let sky = colour(band_mean(&frame, 8, horizon.saturating_sub(8)).ok_or("the sky band")?);
        let sea = colour(
            band_mean(&frame, horizon + 4, shore_row.saturating_sub(4)).ok_or("the sea band")?,
        );
        let ground = colour(
            band_mean(&frame, shore_row + 8, FRAME.saturating_sub(6)).ok_or("the ground band")?,
        );
        for (name, band) in [("sky", sky), ("sea", sea), ("ground", ground)] {
            assert!(
                band.x.max(band.y).max(band.z) > NOT_BLACK,
                "the {name} band is black ({band}) — nothing was drawn where the {name} should be"
            );
        }
        assert!(
            pixels_differ(sky, sea),
            "the sky band ({sky}) and the sea band ({sea}) are the same colour, so the sea is not \
             being drawn between the horizon and the far shore"
        );
        assert!(
            pixels_differ(sea, ground),
            "the sea band ({sea}) and the ground band ({ground}) are the same colour, so the \
             region's own ground is not being drawn in front of the sea"
        );
        assert!(
            pixels_differ(sky, ground),
            "the sky band ({sky}) and the ground band ({ground}) are the same colour"
        );
        harness.logout()
    }

    /// **A textured prim shows the texture the grid served it.**
    ///
    /// The catalogue's checker is red-and-green marker cells, so the claim is
    /// decidable without naming a shade: the checkered box's own disc carries
    /// *both* colours. Everything between the object update and those pixels is
    /// under test at once — the `TextureEntry`, the `GetTexture` capability, the
    /// JPEG2000 decode, the upload, the face material and the camera.
    #[test]
    fn a_textured_prim_shows_its_checker_over_get_texture() -> Result<(), TestError> {
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;
        harness.wait_event("the checkered box", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == subject.local_id =>
            {
                Some(())
            }
            _ => None,
        })?;
        let Some((frame, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the checker check");
            return Ok(());
        };
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&frame, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "the checkered box paints only {share} of its own disc in {} — the texture the \
                 grid served over GetTexture did not reach the face",
                marker.name()
            );
        }
        harness.logout()
    }

    /// **A mesh prim is built from the asset the grid served over `GetMesh2`.**
    ///
    /// Two halves of one claim, because either alone is weak. The mesh store
    /// having decoded the asset says the fetch and the decode ran but not that
    /// anything was drawn; the checker on the disc says a face was drawn but not
    /// that it was the mesh's — a mesh prim whose asset never arrives still has
    /// its base shape. Together they say the mesh arrived *and* the object it
    /// belongs to is on screen.
    #[test]
    fn a_mesh_prim_is_built_from_its_get_mesh2_asset() -> Result<(), TestError> {
        let subject = entry("mesh-cube").ok_or("the catalogue has no mesh-cube")?;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;
        harness.run_until("the mesh asset to decode", |harness| {
            harness
                .world()
                .resource::<crate::meshes::MeshManager>()
                .decoded(MESH_ASSET)
                .is_some()
                .then_some(())
        })?;
        let Some((frame, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the mesh check");
            return Ok(());
        };
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&frame, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "the mesh cube paints only {share} of its own disc in {} — its geometry or its \
                 texture did not reach the frame",
                marker.name()
            );
        }
        harness.logout()
    }

    /// **A `KillObject` takes the object out of the picture.**
    ///
    /// The removal half of the world fold, which only a rendering tier can
    /// check: the ECS forgetting an object and the frame no longer showing it
    /// are different claims, and the second is the one a user makes.
    ///
    /// The synchronisation is the point of the marker. The kill goes out on the
    /// circuit, then a marker; UDP is ordered, so the client seeing the marker
    /// means it has already seen the kill, and the second capture is taken
    /// because something was observed rather than because a sleep expired.
    #[test]
    fn a_killed_object_leaves_its_disc_empty() -> Result<(), TestError> {
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;
        let Some((before, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the kill check");
            return Ok(());
        };
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&before, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "the box is not in the picture to begin with ({share} of its disc in {}), so its \
                 disappearing would prove nothing",
                marker.name()
            );
        }

        let agent = harness.agent()?;
        let now = agent.now();
        let local_id = subject.local_id;
        harness.grid(agent.with_sim(|sim| sim.send_kill_object(&[local_id], now)))?;
        harness.mark("killed")?;
        harness.wait_marker("killed")?;

        let after = harness
            .capture()?
            .ok_or("the adapter answered the first capture and not the second")?;
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&after, disc, marker);
            assert!(
                share < KILLED_SHARE,
                "the killed box still paints {share} of its disc in {} — the object was removed \
                 from the world state but not from the picture",
                marker.name()
            );
        }
        harness.logout()
    }

    /// **A teleport renders the region it landed in.**
    ///
    /// The one motion that replaces the whole scene: every store the world fold
    /// owns purges, the origin moves, and the destination's ground and objects
    /// stream in from nothing. Checking it in pixels is the only way to tell a
    /// scene that was rebuilt from one that was merely emptied — the catalogue's
    /// checker is in the *destination*, so it can only be on screen if the
    /// arrival built it.
    #[test]
    fn a_teleport_renders_the_destination_region() -> Result<(), TestError> {
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let destination = sl_fake_grid::catalogue().into_region(RegionConfig {
            name: "Fake Region East".to_owned(),
            grid_x: RegionConfig::default().grid_x.saturating_add(1),
            ..RegionConfig::default()
        });
        let mut harness = ViewerHarness::start_in(vec![
            stock_fixture().into_region(RegionConfig::default()),
            destination,
        ])?;
        harness.login()?;
        harness.teleport_to(
            "Fake Region East",
            sl_proto::RegionCoordinates::new(128.0, ROW_Y - SUBJECT_DISTANCE, SUBJECT_EYE_Z + 1.0),
        )?;
        let Some((frame, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the teleport check");
            return Ok(());
        };
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&frame, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "after the teleport the destination's checkered box paints only {share} of its \
                 disc in {} — the arrival did not rebuild the scene",
                marker.name()
            );
        }
        harness.logout()
    }
}
