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
    Captured, FRAME, HOLD_FRAMES, PipelineStatusPlugin, Projected, STEP_DURATION, SettleError,
    frames_for, gpu_lock, settle,
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
/// would be comparing times of day.
///
/// `0.5` is **midday** on the synthesised preset track
/// ([`install_preset_day_cycle`]: `0.0` midnight, `0.25` sunrise, `0.5` noon,
/// `0.75` sunset). It was `0.25` — described as "the middle of the day track",
/// which sunrise is not: the scene came out in dawn light, dim enough that a
/// blue-baked avatar classified as no marker colour at all and the ground under
/// a grazing camera read as black.
///
/// [`install_preset_day_cycle`]: sl_viewer_kit::sky_presets::install_preset_day_cycle
const DAY_POSITION: f32 = 0.5;

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

/// What a harness may be built differently for.
///
/// One field so far, and it earns its struct: pinning the day is right for every
/// test *except* the one whose subject is the environment itself, and a bare
/// `Option<f32>` parameter on `start_in` would say nothing about which it was at
/// the call site.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HarnessOptions {
    /// Where the day is pinned, as a fraction of the day cycle, or `None` to
    /// render the region's **own** environment as the grid serves it.
    ///
    /// Pinning is the default because the sky is a function of a clock nothing
    /// in a test controls. It is also a *replacement*: a pinned position
    /// installs a synthesised cycle of legacy presets
    /// ([`EnvironmentState::apply`]), which is exactly what an environment test
    /// must not have — the grid's own sky would never reach the picture.
    ///
    /// [`EnvironmentState::apply`]: sl_viewer_world_scene::environment::EnvironmentState
    day_position: Option<f32>,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            day_position: Some(DAY_POSITION),
        }
    }
}

impl HarnessOptions {
    /// Render whatever environment the region serves, rather than a pinned day.
    fn following_the_region_environment() -> Self {
        Self { day_position: None }
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
        Self::start_in_with(regions, HarnessOptions::default())
    }

    /// [`start_in`](Self::start_in) with the render overrides `options` names.
    pub(crate) fn start_in_with(
        regions: Vec<RegionConfig>,
        options: HarnessOptions,
    ) -> Result<Self, TestError> {
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
        let (mut app, captured) = build_viewer_app(params, options);
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
        timeout_report(&self.app, &self.logs, what)
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

    /// Take the **scene clock** over: from here on `Time` advances only by what
    /// [`capture_after`](Self::capture_after) asks for.
    ///
    /// The default is real time, because everything else this tier waits on
    /// runs on other threads and a frozen clock buys nothing. A test whose
    /// subject *is* time — an animation, a scroll, a fade — needs the opposite:
    /// two captures whose gap is a number it chose rather than however long a
    /// settle happened to take. A settle takes tens of frames, so "a second
    /// apart" measured in wall time is somewhere between half a motion and two
    /// of them, and a looping motion sampled a whole period apart is the same
    /// pose twice.
    ///
    /// Everything the network does keeps working: the client's socket and the
    /// grid's runtime are their own threads, and this only changes what Bevy's
    /// `Time` reports.
    pub(crate) fn hold_clock(&mut self) {
        self.app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
    }

    /// Advance the held scene clock by `seconds` and capture the frame rendered
    /// at that time, with the clock held again for the settle.
    ///
    /// The full-stack twin of the readback rig's `frame_at`, and it must be
    /// preceded by [`hold_clock`](Self::hold_clock) — against a running clock
    /// the advance would be an offset on top of however much real time had
    /// already passed, which is the very thing this exists to remove.
    pub(crate) fn capture_after(&mut self, seconds: f32) -> Result<Option<Frame>, TestError> {
        self.app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                STEP_DURATION,
            ));
        for _frame in 0..frames_for(seconds) {
            self.app.update();
        }
        self.hold_clock();
        self.capture()
    }

    /// Write a viewer setting, as the preferences UI this tier leaves out would.
    ///
    /// The A/B half of a text or overlay check: a subject that is drawn *and*
    /// that stops being drawn when its setting is turned off is a subject the
    /// renderer is really putting there, where "some pixels above the head
    /// differ from the sky" alone is not.
    pub(crate) fn set_setting(&mut self, name: &str, value: sl_settings::SettingValue) {
        self.app
            .world_mut()
            .resource_mut::<crate::settings::ViewerSettings>()
            .set(sl_settings::Scope::Global, name, value);
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

    /// Where `points` land on the frame, given in **Bevy world space** — for
    /// asking where the viewer actually drew something, rather than where the
    /// grid said it was.
    ///
    /// [`project`](Self::project) is the same question asked in the grid's
    /// frame; this one takes a `GlobalTransform`'s translation straight from
    /// the entity that was rendered, which is the only way to see a body placed
    /// by something other than its own region-space position — a rider on a
    /// seat, most of all.
    pub(crate) fn project_world(&mut self, points: &[Vec3]) -> Projected {
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
        cameras
            .single(self.app.world())
            .map(|(camera, transform)| {
                Projected(
                    points
                        .iter()
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

    /// The viewer's world, mutably, for a query a read-only handle cannot run.
    pub(crate) fn app_world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// The viewer's world, for a claim about ECS state rather than pixels.
    ///
    /// Some of what this tier proves is not visible: that a mesh asset decoded,
    /// that a capability was announced. Those are read here, beside the picture
    /// they explain, rather than in a tier that could not also look at it.
    pub(crate) fn world(&self) -> &World {
        self.app.world()
    }

    /// The handle of the grid region called `name`, for an assertion about
    /// which region a streamed object or patch came from.
    pub(crate) fn region_handle(&self, name: &str) -> Option<sl_proto::RegionHandle> {
        self.grid.region_handle(name)
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

    /// Walk the avatar over the border into `region_name`, landing at
    /// `position`, and wait until the client is rooted there.
    ///
    /// The grid's path, not the client's: a crossing is something a simulator
    /// decides, and the fake grid — which simulates no movement — is told to
    /// decide it ([`sl_fake_grid::FakeGrid::cross_agent`]). Unlike
    /// [`teleport_to`](Self::teleport_to) nothing is waited for afterwards
    /// except the arrival itself: the destination's ground was streamed to the
    /// child circuit long before, which is the property this exists to test.
    ///
    /// The grid future is stepped *between viewer frames* rather than blocked
    /// on. The crossing waits for the client's `CompleteAgentMovement`, and the
    /// client only sends one when the app steps a frame, so
    /// [`grid`](Self::grid) — which blocks the whole thread — would deadlock
    /// the two halves against each other.
    pub(crate) fn cross_to(
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
        let agent = self.agent()?;
        let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
        // A block, so the future borrowing `self.grid` is dropped before the
        // wait below borrows `self` whole; destructured inside it, so the
        // borrow checker sees the future, the app and the log capture as the
        // disjoint fields they are.
        let destination = {
            let Self {
                runtime,
                grid,
                app,
                logs,
                ..
            } = self;
            let mut crossing = Box::pin(grid.cross_agent(
                &agent,
                region_name,
                position,
                // Placed, not walking: the fake grid runs no physics, so there
                // is no momentum to carry over the border.
                sl_proto::Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            ));
            loop {
                app.update();
                // The timeout is built *inside* `block_on`: `tokio::time::timeout`
                // arms a timer against the ambient runtime, and constructing it in
                // the argument position — outside the runtime — panics with "there
                // is no reactor running".
                let slice = runtime
                    .block_on(async { tokio::time::timeout(FRAME_PAUSE, &mut crossing).await });
                match slice {
                    Ok(result) => break result?,
                    Err(_still_going) => {}
                }
                if Instant::now() >= deadline {
                    let what = format!("the grid to hand the agent over to {region_name}");
                    return Err(timeout_report(app, logs, &what).into());
                }
            }
        };
        self.agent = Some(destination);
        // The client's own account of the handover: a `RegionChanged` naming
        // the destination that did **not** reset the world.
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
                        SlSessionEvent::RegionChanged { region_handle, world_reset: false, .. }
                            if *region_handle == handle
                    )
                })
                .then_some(())
        })
    }

    /// Step frames until the neighbouring region called `name` has finished
    /// streaming its scene to its child circuit
    /// ([`sl_fake_grid::neighbour_marker`]).
    pub(crate) fn wait_neighbour(&mut self, name: &str) -> Result<(), TestError> {
        let wanted = name.to_owned();
        self.wait_event(&format!("the neighbour {name}"), move |event| match event {
            SlSessionEvent::GenericMessage(generic) => {
                (sl_fake_grid::neighbour_marker_region(generic).as_ref() == Some(&wanted))
                    .then_some(())
            }
            _ => None,
        })
    }

    /// Log out cleanly and wait for the client to say so.
    pub(crate) fn logout(&mut self) -> Result<(), TestError> {
        self.command(Command::Logout);
        self.wait_event("LoggedOut", |event| {
            matches!(event, SlSessionEvent::LoggedOut).then_some(())
        })
    }
}

/// What a timeout has to say for itself, from the two fields that know: how
/// much the scene still owes and how far the session got ([`Recorded`],
/// [`SceneWork`]), and what it warned about on the way ([`LogCapture`]).
///
/// A free function rather than only a method because
/// [`ViewerHarness::cross_to`] holds a future borrowing one field of the
/// harness while it needs a report out of two others.
fn timeout_report(app: &App, logs: &LogCapture, what: &str) -> String {
    let outstanding = app.world().resource::<SceneWork>().outstanding;
    let events: Vec<String> = app
        .world()
        .resource::<Recorded>()
        .events
        .iter()
        .rev()
        .take(12)
        .map(|event| format!("{event:?}").chars().take(160).collect::<String>())
        .collect();
    let warnings: Vec<String> = logs.events().into_iter().rev().take(8).collect();
    format!(
        "timed out waiting for {what}\n  outstanding asset work: {outstanding}\n  last events: \
         {events:#?}\n  last warnings: {warnings:#?}"
    )
}

/// Build the headless viewer: the readback base (no window, no winit, no log,
/// no render thread), the viewer's own world, input and render groups, the real
/// client plugin, and a camera rendering into the texture this reads back.
///
/// The UI, shell and edit groups are deliberately absent. They own no pixel of
/// the world this tier looks at, they drag in CEF and the whole floater
/// scaffold, and the UI tier already covers them under a synthetic pointer.
fn build_viewer_app(params: LoginParams, options: HarnessOptions) -> (App, Captured) {
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
        day_position: options.day_position,
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
    // Media-on-a-prim, which like the avatar library and the fonts is added by
    // `run()` and by none of the six groups. Both engines are **off**: with
    // `enabled: false` the plugin still registers `MediaEngine` /
    // `MediaSurfaces` and the `Pump` set that `MediaPrimPlugin` schedules
    // against, but never starts Chromium or GStreamer — and a test binary has
    // no `sl-cef-helper` beside it to start anyway.
    //
    // That is enough for the half of the media path this tier is for: the
    // object update's `MediaURL` version triggering a `RequestObjectMedia`,
    // the capability's reply, and the per-face `MediaEntry` set reaching
    // `MediaData`. The other half — a live surface's placeholder and its first
    // paint — needs a browser process, and belongs to a rig that has one.
    .add_plugins(crate::media_engine::MediaEnginePlugin {
        enabled: false,
        video_enabled: false,
    })
    .add_plugins(crate::media_prim::MediaPrimPlugin)
    .init_resource::<Recorded>()
    .init_resource::<SceneWork>()
    .init_resource::<Captured>()
    // The bundled font stack. The world's **text** — an object's floating text
    // and an avatar's name tag — is laid out through the same glyph atlas the
    // UI is, and the system that installs the faces belongs to the UI group
    // this harness deliberately leaves out. Without it the world-space text
    // billboards lay out against a font nothing registered and draw nothing,
    // which reads as "the tag renderer is broken" rather than as "the fixture
    // has no font".
    .add_systems(Startup, crate::ui_font::register_ui_fonts)
    // After the plugin's `(drive, maintain_world)` chain, so a frame's world
    // state and its events are observed together.
    .add_systems(PostUpdate, (record, drain_capabilities, note_scene_work));

    // The system-avatar `character/` assets: the skeleton, the body meshes and
    // the morph bindings a rigged avatar is built from. Loaded here for the same
    // reason the fonts above are — the loader belongs to `run_session` rather
    // than to any of the six plugin groups — and without them **every avatar in
    // the scene stays a placeholder sphere** (`avatars::spawn_sphere`), which is
    // not a picture of an avatar at all. Absent (a build with no vendored
    // `character/` beside it) the tier keeps the spheres, exactly as a session
    // with a bad `--viewer-assets` does.
    if let Some(library) = crate::load_avatar_library(crate::default_viewer_assets().as_deref()) {
        app.insert_resource(library);
    }

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
    use super::{FRAME, HarnessOptions, Recorded, SceneWork, ViewerHarness, stock_fixture};

    use bevy::prelude::*;
    use sl_fake_grid::RegionConfig;
    use sl_fake_grid::fixtures::border::BorderSide;
    use sl_fake_grid::fixtures::catalogue::{CatalogueEntry, MESH_ASSET, ROW_Y, entry};
    use sl_proto::Vector;

    use crate::pixel_oracle::{
        Frame, Marker, Silhouette, band_mean, centroid, coverage, differing_pixels, health,
        luminance, pixels_differ,
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
        frame_the_subject(harness, subject);
        let Some(frame) = harness.capture()? else {
            return Ok(None);
        };
        let disc = subject_disc(harness, subject)?;
        Ok(Some((frame, disc)))
    }

    /// The aiming half of [`frame_subject`], for a test that captures its own
    /// frames (a held clock, an A/B toggle) rather than taking the one
    /// [`frame_subject`] hands back.
    fn frame_the_subject(harness: &mut ViewerHarness, subject: &CatalogueEntry) {
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
    }

    /// The disc a catalogue subject's own body covers, projected through the
    /// camera that drew the current frame.
    fn subject_disc(
        harness: &mut ViewerHarness,
        subject: &CatalogueEntry,
    ) -> Result<Silhouette, TestError> {
        disc_at(harness, &subject.position(), SUBJECT_DISC)
    }

    /// The radius, in metres, of the disc taken on a one-metre catalogue prim:
    /// inscribed in its body rather than bounding it, so the disc is the prim
    /// and not the sky around its corners.
    const SUBJECT_DISC: f32 = 0.35;

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
    ///
    /// The destination is deliberately **ten regions away** rather than next
    /// door. An adjacent region is announced as a neighbour on login and
    /// streams its scene to a child circuit long before any teleport, so its
    /// checker would be on screen whether the arrival rebuilt anything or not
    /// — which is the claim this test makes, and would then no longer be
    /// making. (What a *neighbour* renders is
    /// [`a_neighbour_region_is_rendered_across_the_border`]'s question.)
    #[test]
    fn a_teleport_renders_the_destination_region() -> Result<(), TestError> {
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let destination = sl_fake_grid::catalogue().into_region(RegionConfig {
            name: "Fake Region East".to_owned(),
            grid_x: RegionConfig::default().grid_x.saturating_add(10),
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

    /// The two-region grid the border tests run on: a plain western region the
    /// account logs into, and the border scene to its east, whose marker pillar
    /// stands a few metres past the shared edge.
    ///
    /// The east region alone declares the pillar's checker, and the viewer
    /// fetches it over the **west** region's `GetTexture` — which works because
    /// the grid's asset store is grid-wide, as a real one's is. It was not
    /// always: this pair is what found that, with the pillar rendering
    /// untextured and the checker oracle reading it as "the neighbour was never
    /// streamed" when what had not arrived was one JPEG2000 blob.
    fn border_grid() -> Vec<RegionConfig> {
        let west = RegionConfig {
            name: WEST_REGION.to_owned(),
            ..RegionConfig::default()
        };
        let east = RegionConfig {
            name: EAST_REGION.to_owned(),
            grid_x: RegionConfig::default().grid_x.saturating_add(1),
            ..RegionConfig::default()
        };
        vec![
            stock_fixture().into_region(west),
            sl_fake_grid::fixtures::border::border().into_region(east),
        ]
    }

    /// Step frames until the eastern region's marker pillar has reached the
    /// client, streamed on the child circuit and stamped with the east
    /// region's own handle.
    ///
    /// Waited for separately from the picture so a failure says which half
    /// broke: the object never arriving is a grid or child-circuit fault, and
    /// its arriving but not being drawn is a rendering one.
    fn wait_for_the_marker(harness: &mut ViewerHarness) -> Result<(), TestError> {
        let east = harness
            .region_handle(EAST_REGION)
            .ok_or("the grid serves no eastern region")?;
        harness.wait_event("the eastern region's marker pillar", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == sl_fake_grid::fixtures::border::MARKER_LOCAL_ID
                    && object.region_handle == east =>
            {
                Some(())
            }
            _ => None,
        })
    }

    /// The region the border tests log into.
    const WEST_REGION: &str = "Fake Region West";

    /// The region the border tests look into, and then walk into.
    const EAST_REGION: &str = "Fake Region East";

    /// Where in the **western** region the camera stands to frame the eastern
    /// region's marker pillar: this far west of the shared border.
    ///
    /// Twelve metres, so a three metre pillar four metres the other side of the
    /// border covers a disc of a couple of dozen pixels — big enough to
    /// classify, small enough that the framing has room around it.
    const BORDER_CAMERA_WEST: f32 = 12.0;

    /// The eye height the border framing uses: the stock ground, aiming up at
    /// the floating pillar, so the horizon is below its disc.
    const BORDER_EYE_Z: f32 = 25.0;

    /// The offset from the marker's centre used to size its disc, in metres —
    /// half its body, so the disc is inscribed in the pillar rather than
    /// bounding it.
    const BORDER_DISC_EDGE: f32 = sl_fake_grid::fixtures::border::MARKER_SIZE / 2.0;

    /// How far, in pixels, the marker's projected centre may move across the
    /// crossing.
    ///
    /// Two, as the task states it. Not zero: the projection is float maths over
    /// a coordinate frame whose origin moved by 256 metres, and a pixel of
    /// rounding at that magnitude is not a viewer bug.
    const BORDER_DRIFT_PX: f32 = 2.0;

    /// The marker pillar's position in the frame of the region called `region`
    /// — its own region-local position, or that shifted a whole region east
    /// when read from the region to the west.
    fn marker_seen_from(region: &str) -> Vector {
        let at = sl_fake_grid::fixtures::border::marker_position();
        if region == EAST_REGION {
            at
        } else {
            Vector {
                x: at.x + 256.0,
                ..at
            }
        }
    }

    /// The disc the marker pillar covers, projected through the camera that
    /// drew the current frame, with the pillar's position stated in the frame
    /// of `region` — which is the region the session is rooted in, and
    /// therefore the scene origin.
    fn marker_disc(harness: &mut ViewerHarness, region: &str) -> Result<Silhouette, TestError> {
        let at = marker_seen_from(region);
        let edge = Vector {
            z: at.z + BORDER_DISC_EDGE,
            ..at
        };
        disc_from(&harness.project(&[at, edge]))
    }

    /// **The region across the border is drawn before you walk into it.**
    ///
    /// What a neighbour announcement is *for*: on arrival the grid tells the
    /// client about the region next door, the client opens a child circuit, and
    /// that circuit streams the neighbour's scene. Everything downstream of it
    /// is under test at once — the event-queue announcement, the child circuit,
    /// the neighbour's own object update, and the region offset that puts a
    /// neighbour's object on the neighbour's ground rather than on top of the
    /// root region's.
    ///
    /// The subject is the *eastern* region's pillar seen from the *western*
    /// region, so it can only be on screen if all of that ran: a viewer that
    /// ignored the neighbour draws nothing there, and one that placed it
    /// without the offset draws it 256 metres away.
    #[test]
    fn a_neighbour_region_is_rendered_across_the_border() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start_in(border_grid())?;
        harness.login()?;
        harness.wait_neighbour(EAST_REGION)?;
        wait_for_the_marker(&mut harness)?;
        frame_the_border(&mut harness);
        let Some(frame) = harness.capture()? else {
            no_adapter("the neighbour check");
            return Ok(());
        };
        let disc = marker_disc(&mut harness, WEST_REGION)?;
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&frame, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "the neighbour's marker paints only {share} of its own disc in {} — the region \
                 across the border was not streamed, or was not placed on its own ground",
                marker.name()
            );
        }
        harness.logout()
    }

    /// **Walking over a border does not move the world.**
    ///
    /// The claim a crossing exists to keep, and the one a teleport deliberately
    /// breaks: the scene is *kept* and re-based onto the new origin, so
    /// everything a camera was looking at is still where it was. The camera is
    /// framed once, before the crossing, and never touched again — what moves
    /// underneath it is the origin, by a whole region, and the viewer's
    /// recentering has to cancel that out exactly.
    ///
    /// Three things are asserted, because each alone would pass for the wrong
    /// reason: the marker's projected centre has not moved (the re-basing is
    /// right), its disc still carries its checker (it is still *drawn*, not
    /// merely still projected), and the session took no teleport and no
    /// disconnect on the way (it was a crossing, not the scene being rebuilt by
    /// something else).
    #[test]
    fn a_border_crossing_keeps_the_picture_still() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start_in(border_grid())?;
        harness.login()?;
        harness.wait_neighbour(EAST_REGION)?;
        wait_for_the_marker(&mut harness)?;
        frame_the_border(&mut harness);
        let Some(before) = harness.capture()? else {
            no_adapter("the crossing continuity check");
            return Ok(());
        };
        let disc_before = marker_disc(&mut harness, WEST_REGION)?;
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&before, disc_before, marker);
            assert!(
                share > CHECKER_SHARE,
                "the neighbour's marker is not in the picture to begin with ({share} of its disc \
                 in {}), so its staying put would prove nothing",
                marker.name()
            );
        }
        let crossed_at = harness
            .world()
            .resource::<Recorded>()
            .events
            .len()
            .saturating_sub(1);

        // Over the border, landing just past it — and the camera is left
        // exactly where it was.
        harness.cross_to(
            EAST_REGION,
            sl_proto::RegionCoordinates::new(
                2.0,
                sl_fake_grid::fixtures::border::MARKER_Y,
                BORDER_EYE_Z + 1.0,
            ),
        )?;
        let after = harness
            .capture()?
            .ok_or("the adapter answered the first capture and not the second")?;
        let disc_after = marker_disc(&mut harness, EAST_REGION)?;

        let drift = Vec2::new(
            disc_after.centre.x - disc_before.centre.x,
            disc_after.centre.y - disc_before.centre.y,
        )
        .length();
        assert!(
            drift <= BORDER_DRIFT_PX,
            "the marker moved {drift} px across the crossing ({:?} -> {:?}) — the scene was not \
             re-based onto the new origin, or the camera was not",
            disc_before.centre,
            disc_after.centre
        );
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&after, disc_after, marker);
            assert!(
                share > CHECKER_SHARE,
                "after the crossing the marker paints only {share} of its disc in {} — the object \
                 survived the handover in the world state but not in the picture",
                marker.name()
            );
        }

        // A crossing is not a teleport and not a reconnection: nothing between
        // the framing and here said otherwise.
        let interrupted: Vec<String> = harness
            .world()
            .resource::<Recorded>()
            .events
            .iter()
            .skip(crossed_at)
            .filter(|event| {
                matches!(
                    event,
                    sl_client_bevy::SlSessionEvent::TeleportStarted
                        | sl_client_bevy::SlSessionEvent::TeleportFinished { .. }
                        | sl_client_bevy::SlSessionEvent::TeleportFailed { .. }
                        | sl_client_bevy::SlSessionEvent::Disconnected { .. }
                        | sl_client_bevy::SlSessionEvent::LoggedOut
                )
            })
            .map(|event| format!("{event:?}").chars().take(120).collect())
            .collect();
        assert!(
            interrupted.is_empty(),
            "a border crossing must raise no teleport and no disconnect, but the session saw \
             {interrupted:#?}"
        );
        harness.logout()
    }

    /// The border grid with a rideable vehicle either side, numbered
    /// differently in each — which is what makes a ridden crossing a handover
    /// rather than two copies of one prim.
    fn ridden_border_grid() -> Vec<RegionConfig> {
        use sl_fake_grid::fixtures::border::border_with_vehicle;
        vec![
            border_with_vehicle(BorderSide::Leaving, false).into_region(RegionConfig {
                name: WEST_REGION.to_owned(),
                ..RegionConfig::default()
            }),
            border_with_vehicle(BorderSide::Arriving, false).into_region(RegionConfig {
                name: EAST_REGION.to_owned(),
                grid_x: RegionConfig::default().grid_x.saturating_add(1),
                ..RegionConfig::default()
            }),
        ]
    }

    /// **A rider stays on its vehicle across a border.**
    ///
    /// The claim `Session::seat()` cannot make. The seat survives the crossing
    /// as a *value* whatever the viewer does with it; what this asks is whether
    /// the body is still drawn **on the deck** afterwards — which it only is if
    /// the seat was re-found under the destination's own region-local id and
    /// the avatar re-placed against it.
    ///
    /// Measured as the avatar's offset from the vehicle in pixels, before and
    /// after. An absolute position would only restate what
    /// [`a_border_crossing_keeps_the_picture_still`] already proves about the
    /// origin; the *relative* one is the seat.
    #[test]
    fn a_rider_stays_on_its_vehicle_across_a_border() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::border;
        let mut harness = ViewerHarness::start_in(ridden_border_grid())?;
        harness.login()?;
        harness.wait_neighbour(EAST_REGION)?;

        // Aboard the western vehicle.
        harness.command(sl_client_bevy::Command::Sit {
            target: border::VEHICLE_OBJECT,
            offset: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        });
        let own = harness
            .world()
            .resource::<sl_client_bevy::SlIdentity>()
            .agent_id
            .ok_or("the session has no agent id")?;
        harness.wait_event("the agent aboard the vehicle", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.full_id.uuid() == own.uuid()
                    && object.parent_id == BorderSide::Leaving.vehicle_local_id() =>
            {
                Some(())
            }
            _ => None,
        })?;

        // Frame the vehicle the agent is on, from a fixed pose *relative to
        // it*. Unlike the continuity test the camera is re-aimed after the
        // crossing, and must be: the subject rides over the border, so a fixed
        // world pose would be looking at where it used to be. What is compared
        // is the rider's offset **from its vehicle**, which that re-aiming
        // leaves alone.
        frame_the_vehicle(&mut harness, BorderSide::Leaving);
        let Some(_before) = harness.capture()? else {
            no_adapter("the ridden crossing check");
            return Ok(());
        };
        let ride_before = rider_offset_px(&mut harness, BorderSide::Leaving)?;

        // Over the border, and the vehicle handed over with its rider.
        let source = harness.agent()?;
        let landing = BorderSide::Arriving.vehicle_position();
        harness.cross_to(
            EAST_REGION,
            sl_proto::RegionCoordinates::new(landing.x, landing.y, landing.z),
        )?;
        let destination = harness.agent()?;
        harness.grid(async {
            source
                .with_world(|world, sim| {
                    world
                        .objects
                        .retain(|object| object.full_id != border::VEHICLE_OBJECT);
                    sim.send_kill_object(&[BorderSide::Leaving.vehicle_local_id()], source.now())
                })
                .await
        })?;
        harness.grid(destination.seat_on(
            BorderSide::Arriving.vehicle_local_id(),
            sl_fake_grid::world::SIT_TARGET_OFFSET,
        ));
        harness.wait_event(
            "the agent aboard the destination's vehicle",
            |event| match event {
                sl_client_bevy::SlSessionEvent::ObjectAdded(object)
                | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                    if object.full_id.uuid() == own.uuid()
                        && object.parent_id == BorderSide::Arriving.vehicle_local_id() =>
                {
                    Some(())
                }
                _ => None,
            },
        )?;
        frame_the_vehicle(&mut harness, BorderSide::Arriving);
        let _after = harness
            .capture()?
            .ok_or("the adapter answered the first capture and not the second")?;
        let ride_after = rider_offset_px(&mut harness, BorderSide::Arriving)?;

        let drift = Vec2::new(ride_after.x - ride_before.x, ride_after.y - ride_before.y).length();
        assert!(
            drift <= BORDER_DRIFT_PX,
            "the rider moved {drift} px relative to its vehicle across the crossing ({ride_before:?}              -> {ride_after:?}) — the seat was not re-found under the destination's own local id",
        );
        harness.logout()
    }

    /// How far west of the vehicle its camera stands, in metres — looking east
    /// at it, so the floating deck has sky behind it.
    const VEHICLE_CAMERA_WEST: f32 = 12.0;

    /// Aim the camera at the vehicle the agent is riding, in the current
    /// origin's frame.
    ///
    /// Both regions place their vehicle at the same *region-local* spot, so
    /// this one framing serves either side of the border — which is what makes
    /// the before/after comparison a comparison of the **rider on the deck**
    /// rather than of where the deck is.
    fn frame_the_vehicle(harness: &mut ViewerHarness, side: BorderSide) {
        let deck = side.vehicle_position();
        harness.look_from(
            Vector {
                x: deck.x - VEHICLE_CAMERA_WEST,
                y: deck.y,
                z: BORDER_EYE_Z,
            },
            deck,
        );
    }

    /// Where the own avatar sits on screen **relative to its vehicle**, in
    /// pixels.
    ///
    /// Both are read in the current origin's frame, so the measurement says
    /// nothing about which region either is in — only whether the body is on
    /// the deck. Read from the projection rather than the wire: the question is
    /// where the viewer *drew* the body, which is the only place a lost seat
    /// shows.
    fn rider_offset_px(harness: &mut ViewerHarness, side: BorderSide) -> Result<Vec2, TestError> {
        let deck = side.vehicle_position();
        let vehicle_px = harness
            .project(&[deck])
            .get(0)
            .ok_or("the vehicle is not on the frame — the camera is not looking at it")?;
        let body = own_avatar_px(harness)?;
        Ok(Vec2::new(body.x - vehicle_px.x, body.y - vehicle_px.y))
    }

    /// Where the own avatar's body root projects on the current frame.
    ///
    /// Taken from the entity's transform — where the viewer *put* the body —
    /// and not from any region-space position it was told, because a seated
    /// avatar's position is its offset from a seat and says nothing about
    /// where the seat is.
    ///
    /// Returns the reason rather than `None`: every step here has failed at
    /// least once during this test's development, and "the own avatar has no
    /// world position" says nothing about which.
    fn own_avatar_px(harness: &mut ViewerHarness) -> Result<Vec2, String> {
        let own = harness
            .world()
            .resource::<sl_client_bevy::SlIdentity>()
            .agent_id
            .ok_or_else(|| "the session has no agent id".to_owned())?;
        let anchor = harness
            .world()
            .resource::<crate::world_api::AvatarState>()
            .body_root_of(own)
            .ok_or_else(|| format!("no body root is tracked for {own}"))?;
        let live = harness.world().entities().contains(anchor);
        let anchors = harness
            .app_world_mut()
            .query_filtered::<Entity, With<crate::world_api::AvatarAnchor>>()
            .iter(harness.world())
            .count();
        let at = harness
            .world()
            .get::<Transform>(anchor)
            .map(|transform| transform.translation)
            .ok_or_else(|| {
                format!(
                    "the tracked body root {anchor:?} has no Transform (entity live: {live}, \
                     avatar anchors in the world: {anchors})"
                )
            })?;
        harness
            .project_world(&[at])
            .get(0)
            .ok_or_else(|| format!("the body root at {at:?} does not project onto the frame"))
    }

    // ---------------------------------------------------------------------
    // The catalogue subjects: an NPC, its attachment, its name tag, floating
    // text, a media face, a parcel line and an environment change.
    // ---------------------------------------------------------------------

    /// The disc `radius` metres across, centred on the region point `at`,
    /// projected through the camera that drew the current frame.
    ///
    /// The general form of the framing the subject tests do by hand: a subject
    /// whose own body says how big it is gets its disc from its body, and this
    /// is what a caller uses when it is asking about a patch of *space* — above
    /// a head, over a prim — rather than about an object.
    fn disc_at(
        harness: &mut ViewerHarness,
        at: &Vector,
        radius: f32,
    ) -> Result<Silhouette, TestError> {
        let edge = Vector {
            z: at.z + radius,
            ..at.clone()
        };
        disc_from(&harness.project(&[at.clone(), edge]))
    }

    /// The disc of a fixed **pixel** radius around the region point `at`.
    ///
    /// For a patch of ground, which has no silhouette to derive a radius from
    /// and no size a metre figure would mean anything about: what is wanted is
    /// "a few pixels of the terrain there", and the answer is the same whatever
    /// angle it is seen at.
    fn patch_at(
        harness: &mut ViewerHarness,
        at: &Vector,
        radius_px: f32,
    ) -> Result<Silhouette, TestError> {
        let centre = harness
            .project(std::slice::from_ref(at))
            .get(0)
            .ok_or("the sampled point is not on the frame — the camera is not looking at it")?;
        Ok(Silhouette {
            centre,
            radius: radius_px,
        })
    }

    /// The fraction of `disc` whose pixels changed between the two frames.
    ///
    /// A fraction rather than a count, so a threshold means the same thing for
    /// a disc of any size — the discs here range from a few dozen pixels on an
    /// attachment to a few thousand on an avatar.
    fn changed_share(before: &Frame, after: &Frame, disc: Silhouette) -> f32 {
        let area = core::f32::consts::PI * disc.radius * disc.radius;
        if area <= 0.0 {
            return 0.0;
        }
        crate::pixel_oracle::f32_from_u32(differing_pixels(before, after, Some(disc))) / area
    }

    /// The catalogue NPC's own region position.
    fn npc_position() -> Vector {
        Vector {
            x: sl_fake_grid::fixtures::catalogue::NPC_X,
            y: ROW_Y,
            z: sl_fake_grid::fixtures::catalogue::NPC_Z,
        }
    }

    /// Aim the camera at the catalogue NPC from `distance` metres south of it,
    /// standing on the ground and looking up past it — so everything behind the
    /// body, its attachment and its name tag is sky.
    fn frame_the_npc(harness: &mut ViewerHarness, distance: f32, look_at_z: f32) {
        let at = npc_position();
        harness.look_from(
            Vector {
                x: at.x,
                y: at.y - distance,
                z: SUBJECT_EYE_Z,
            },
            Vector {
                x: at.x,
                y: at.y,
                z: look_at_z,
            },
        );
    }

    /// Aim the camera **down** at the catalogue NPC from close in and above, so
    /// what is behind the body is the *ground*.
    ///
    /// The framing an avatar's own colour has to be read against. The NPC's bake
    /// is [`NPC_BAKE_COLOR`] and a midday sky is blue too, so a body against the
    /// sky is a blue disc whether the bakes arrived or not — where the catalogue's
    /// ground is the brown-and-green Linden detail set, which no marker class
    /// claims.
    ///
    /// [`NPC_BAKE_COLOR`]: sl_fake_grid::fixtures::catalogue::NPC_BAKE_COLOR
    fn frame_the_npc_against_the_ground(harness: &mut ViewerHarness) {
        let at = npc_position();
        harness.look_from(
            Vector {
                x: at.x,
                y: at.y - NPC_GROUND_SOUTH,
                z: NPC_GROUND_EYE_Z,
            },
            Vector {
                x: at.x,
                y: at.y,
                z: at.z + NPC_CHEST_ABOVE_CENTRE,
            },
        );
    }

    /// How far south of the NPC the ground-backed camera stands, in metres.
    const NPC_GROUND_SOUTH: f32 = 3.0;

    /// How high that camera stands, in metres: above the avatar, looking down,
    /// so its whole frame is ground rather than horizon.
    const NPC_GROUND_EYE_Z: f32 = 28.5;

    /// How far the NPC's camera stands from it for a whole-body framing.
    const NPC_DISTANCE: f32 = 6.0;

    /// The radius, in metres, of the disc taken on the NPC's **chest** — small
    /// enough to be inside the torso, so what it measures is the bake and not
    /// the ground beside it.
    const NPC_BAKE_DISC: f32 = 0.15;

    /// How far above the avatar object's centre that chest disc sits, in metres.
    const NPC_CHEST_ABOVE_CENTRE: f32 = 0.4;

    /// The radius, in metres, of the disc the NPC's **motion** is measured in:
    /// most of a 1.9 m avatar, so the arms the twist swings out are inside it.
    const NPC_MOTION_DISC: f32 = 0.9;

    /// How far east of the NPC the ground control patch is sampled, in metres —
    /// clear of the body and of the arms it swings, and of the prim row a
    /// further two metres on.
    const NPC_CONTROL_EAST: f32 = 2.0;

    /// How much of its own chest disc an NPC's bake colour must paint.
    ///
    /// Half: the disc is inscribed in the torso but a torso is not a disc, and
    /// the lighting darkens the side turned away from the sun.
    const BAKE_SHARE: f32 = 0.5;

    /// How much of a disc must change between two captures for the change to be
    /// the subject moving rather than the frame breathing.
    const MOVED_SHARE: f32 = 0.05;

    /// How much of a disc may change between two captures and still count as
    /// **unchanged** — the control every difference check is read against.
    const STILL_SHARE: f32 = 0.01;

    /// Half the catalogue animation's period, in seconds: the gap between the
    /// chest twist's two extremes
    /// ([`sl_test_assets::anim::chest_twist_animation_asset`]).
    const TWIST_HALF_PERIOD: f32 = 1.0;

    /// **An NPC arrives wearing its bakes, and plays its animation.**
    ///
    /// The other-avatar path end to end, which no tier below this one runs: the
    /// `AvatarAppearance` naming five baked textures, the `GetTexture` fetches
    /// behind them, the composited skin on a system body — and, separately, an
    /// `AvatarAnimation` naming a motion the viewer has to fetch over
    /// `ViewerAsset`, decode as a keyframe motion, and drive a skeleton with.
    ///
    /// Two claims, because either alone would pass for the wrong reason. A body
    /// painted [`NPC_BAKE_COLOR`] says the bakes arrived but not that anything
    /// animates; two captures differing says something moved but not that it
    /// was an avatar. The second is measured against a **ground control patch**
    /// of the same size beside it: a frame where everything changed is a frame
    /// where the clock moved, not one where the NPC did.
    ///
    /// The clock is held for exactly this reason. The motion loops every two
    /// seconds with its extremes a second apart, so the gap between the two
    /// captures has to be a number this test chose — measured in wall time it
    /// would be however long a settle took, and a loop sampled a whole period
    /// apart is the same pose twice.
    ///
    /// [`NPC_BAKE_COLOR`]: sl_fake_grid::fixtures::catalogue::NPC_BAKE_COLOR
    #[test]
    fn an_npc_arrives_with_its_bakes_and_plays_its_animation() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::NPC_LOCAL_ID;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        // The tag over the NPC's head would sit in the sky control disc, and it
        // is another test's subject.
        harness.set_setting(
            crate::name_tag_billboard::SETTING_SHOW_NAME_TAGS,
            sl_settings::SettingValue::Bool(false),
        );
        harness.login()?;
        harness.wait_event("the catalogue NPC's body", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == NPC_LOCAL_ID =>
            {
                Some(())
            }
            _ => None,
        })?;
        frame_the_npc_against_the_ground(&mut harness);
        harness.hold_clock();
        let Some(first) = harness.capture_after(0.0)? else {
            no_adapter("the NPC appearance and animation check");
            return Ok(());
        };

        // Read off the **rendered** body rather than the position the grid
        // named: the viewer sets an avatar's feet on the terrain, so a height
        // measured from the object's own centre would be a metre out.
        let root = npc_body_root(&harness)?;
        let bake = world_disc(
            &mut harness,
            Vec3::new(root.x, root.y + NPC_CHEST_ABOVE_ROOT, root.z),
            NPC_BAKE_DISC,
        )?;
        let share = coverage(&first, bake, Marker::Blue);
        assert!(
            share > BAKE_SHARE,
            "the NPC paints only {share} of its own chest in blue — its baked textures never \
             reached the body"
        );

        let body = world_disc(
            &mut harness,
            Vec3::new(root.x, root.y + NPC_CHEST_ABOVE_ROOT, root.z),
            NPC_MOTION_DISC,
        )?;
        let ground = world_disc(
            &mut harness,
            Vec3::new(root.x + NPC_CONTROL_EAST, root.y, root.z),
            NPC_MOTION_DISC,
        )?;
        let second = harness
            .capture_after(TWIST_HALF_PERIOD)?
            .ok_or("the adapter answered the first capture and not the second")?;
        let moved = changed_share(&first, &second, body);
        let still = changed_share(&first, &second, ground);
        assert!(
            moved > MOVED_SHARE,
            "only {moved} of the NPC's own disc changed over half the twist's period — the \
             animation the grid named is not driving the skeleton"
        );
        assert!(
            still < STILL_SHARE,
            "the ground beside the NPC changed by {still} over the same second, so {moved} of \
             the body changing says nothing about the avatar"
        );
        harness.logout()
    }

    /// How far above the **rendered** body root the NPC's chest sits, in metres:
    /// a little over two thirds of a 1.9 m avatar.
    const NPC_CHEST_ABOVE_ROOT: f32 = 1.35;

    /// Where the viewer has actually put the catalogue NPC's body, in Bevy world
    /// space — the anchor the avatar's parts hang off.
    fn npc_body_root(harness: &ViewerHarness) -> Result<Vec3, TestError> {
        let agent = sl_client_bevy::AgentKey::from(sl_fake_grid::fixtures::catalogue::NPC_AGENT);
        let anchor = harness
            .world()
            .resource::<crate::world_api::AvatarState>()
            .body_root_of(agent)
            .ok_or("no body root is tracked for the catalogue NPC")?;
        harness
            .world()
            .get::<GlobalTransform>(anchor)
            .map(|transform| transform.translation())
            .ok_or_else(|| {
                "the catalogue NPC's body root has no GlobalTransform — it is still a \
                 placeholder sphere, so the avatar assets never loaded"
                    .into()
            })
    }

    /// How far the NPC's camera stands from it while the attachment is watched,
    /// in metres: close, so a quarter-metre box still covers a disc worth
    /// classifying.
    const ATTACHMENT_DISTANCE: f32 = 3.0;

    /// The radius, in metres, of the disc taken on the NPC's worn box — half
    /// its body, so the disc is inscribed in it.
    const ATTACHMENT_DISC: f32 = 0.12;

    /// How far east the NPC is moved, in metres.
    ///
    /// A metre: far enough that the box's old and new discs do not overlap
    /// (they are a quarter of a metre across), near enough that both are on a
    /// frame taken from three metres away.
    const ATTACHMENT_MOVE: f32 = 1.0;

    /// How much a projected world position may disagree with the metre figure
    /// the grid moved the avatar by.
    const MOVE_TOLERANCE_M: f32 = 0.05;

    /// **An attachment follows its avatar when the avatar is moved.**
    ///
    /// The claim an attachment *is*: the grid moves the wearer's body and says
    /// nothing at all about the worn prim, whose own update never comes again —
    /// so the box is over the new head only if the viewer is composing it onto
    /// the skeleton rather than onto the region.
    ///
    /// Both halves are asserted. In the ECS, the entity the viewer drew the box
    /// as moved by the distance the body did; in the picture, the checker is on
    /// the box's new disc and gone from where it used to be. The second is what
    /// catches a viewer that moved the *transform* while the rendered face
    /// stayed behind.
    ///
    /// The clock is held throughout: the NPC is playing a chest twist that
    /// swings its head, and an attachment measured across a running animation
    /// would move for two reasons at once.
    #[test]
    fn an_attachment_follows_its_avatar_across_a_scripted_move() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::{NPC_AGENT, NPC_ATTACHMENT_LOCAL_ID};
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        // The tag hangs over the head, which is exactly where the worn box is.
        harness.set_setting(
            crate::name_tag_billboard::SETTING_SHOW_NAME_TAGS,
            sl_settings::SettingValue::Bool(false),
        );
        harness.login()?;
        harness.wait_event("the NPC's attachment", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == NPC_ATTACHMENT_LOCAL_ID =>
            {
                Some(())
            }
            _ => None,
        })?;
        // Aimed at the head rather than the middle of the body, so the worn box
        // is in the centre of the frame both before and after the move.
        let head_z = npc_position().z + 1.2;
        frame_the_npc(&mut harness, ATTACHMENT_DISTANCE, head_z);
        harness.hold_clock();
        let Some(before) = harness.capture_after(0.0)? else {
            no_adapter("the attachment check");
            return Ok(());
        };
        let worn_before = attachment_world_position(&mut harness)?;
        let disc_before = world_disc(&mut harness, worn_before, ATTACHMENT_DISC)?;
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&before, disc_before, marker);
            assert!(
                share > CHECKER_SHARE,
                "the NPC's worn box paints only {share} of its own disc in {} to begin with, so \
                 its following anything would prove nothing",
                marker.name()
            );
        }

        // The grid moves the **body**, and only the body: the attachment's own
        // update is never sent again.
        let agent = harness.agent()?;
        let now = agent.now();
        harness.grid(agent.with_world(|world, sim| {
            let Some(npc) = world
                .npcs
                .iter_mut()
                .find(|npc| npc.agent_id().uuid() == NPC_AGENT)
            else {
                return Ok(());
            };
            npc.position.x += ATTACHMENT_MOVE;
            let body = npc.avatar_prim();
            sim.send_object_update(&[body], REAL_TIME_DILATION, now)
        }))?;
        harness.mark("moved")?;
        harness.wait_marker("moved")?;

        // The same framing, carried east with the avatar — so the box is in the
        // middle of the frame again and the place it *used* to be is still on
        // it, a metre to the west.
        let at = npc_position();
        harness.look_from(
            Vector {
                x: at.x + ATTACHMENT_MOVE,
                y: at.y - ATTACHMENT_DISTANCE,
                z: SUBJECT_EYE_Z,
            },
            Vector {
                x: at.x + ATTACHMENT_MOVE,
                y: at.y,
                z: head_z,
            },
        );
        let after = harness
            .capture_after(0.0)?
            .ok_or("the adapter answered the first capture and not the second")?;
        let worn_after = attachment_world_position(&mut harness)?;

        let moved = Vec3::new(
            worn_after.x - worn_before.x,
            worn_after.y - worn_before.y,
            worn_after.z - worn_before.z,
        );
        let expected = sl_viewer_kit::coords::sl_to_bevy_vec(&Vector {
            x: ATTACHMENT_MOVE,
            y: 0.0,
            z: 0.0,
        });
        assert!(
            moved.abs_diff_eq(expected, MOVE_TOLERANCE_M),
            "the worn box moved {moved:?} while its wearer moved {expected:?} — the attachment is \
             not riding the body"
        );

        let disc_after = world_disc(&mut harness, worn_after, ATTACHMENT_DISC)?;
        let stale = world_disc(&mut harness, worn_before, ATTACHMENT_DISC)?;
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&after, disc_after, marker);
            assert!(
                share > CHECKER_SHARE,
                "after the move the worn box paints only {share} of its disc in {} — the viewer \
                 moved the attachment's transform but not the face it draws",
                marker.name()
            );
            let left = coverage(&after, stale, marker);
            assert!(
                left < KILLED_SHARE,
                "the worn box still paints {left} of its *old* disc in {} — it was copied rather \
                 than carried",
                marker.name()
            );
        }
        harness.logout()
    }

    /// The physics time dilation a fixture update reports: real time, the same
    /// figure the fake grid's own arrival burst sends.
    const REAL_TIME_DILATION: u16 = 0xFFFF;

    /// Where the viewer has actually **drawn** the catalogue NPC's worn box, in
    /// Bevy world space.
    ///
    /// Read off the rendered entity rather than computed from the wire, because
    /// an attachment has no region position of its own: what the grid sent is
    /// an offset from a joint, and where that lands is the whole question.
    fn attachment_world_position(harness: &mut ViewerHarness) -> Result<Vec3, TestError> {
        use sl_fake_grid::fixtures::catalogue::NPC_ATTACHMENT_LOCAL_ID;
        let mut objects = harness
            .app_world_mut()
            .query::<(&crate::world_api::SceneObject, &GlobalTransform)>();
        objects
            .iter(harness.world())
            .find_map(|(object, transform)| {
                (object.scoped_id.id == NPC_ATTACHMENT_LOCAL_ID).then(|| transform.translation())
            })
            .ok_or_else(|| "the NPC's worn box has no entity in the world".into())
    }

    /// The disc `radius` metres across around a **Bevy world** position — the
    /// [`disc_at`] of something the viewer placed rather than something the grid
    /// named.
    fn world_disc(
        harness: &mut ViewerHarness,
        at: Vec3,
        radius: f32,
    ) -> Result<Silhouette, TestError> {
        disc_from(&harness.project_world(&[at, Vec3::new(at.x, at.y + radius, at.z)]))
    }

    /// How far, in pixels, a subject's centroid may move across a teleport.
    ///
    /// Two, as the task states it — the same tolerance the border crossing
    /// keeps, and for the same reason: the projection is float maths over an
    /// origin that moved by ten regions.
    const TELEPORT_DRIFT_PX: f32 = 2.0;

    /// **A teleport puts the destination's scene exactly where it belongs.**
    ///
    /// A crossing re-bases the scene it already has; a teleport throws it away
    /// and builds the next one from nothing, on a fresh circuit, at a new
    /// origin. Both regions here are the catalogue, so the same framing — stated
    /// in region metres, which is the frame that survives — should photograph
    /// the same picture twice. Its subject's centroid landing within two pixels
    /// of where it was is the arrival origin being right; nothing coarser would
    /// notice a scene built a metre out.
    ///
    /// The purge is asserted alongside, because without it the claim is empty:
    /// a viewer that kept the old region's objects would have the *old*
    /// checker at those pixels and pass a centroid check by never having moved.
    #[test]
    fn a_teleport_keeps_the_subject_where_it_is() -> Result<(), TestError> {
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let mut harness = ViewerHarness::start_in(vec![
            sl_fake_grid::catalogue().into_region(RegionConfig::default()),
            sl_fake_grid::catalogue().into_region(RegionConfig {
                name: "Fake Region East".to_owned(),
                grid_x: RegionConfig::default().grid_x.saturating_add(10),
                ..RegionConfig::default()
            }),
        ])?;
        harness.login()?;
        let Some((before, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the teleport continuity check");
            return Ok(());
        };
        let was = centroid(&before, disc, Marker::Red)
            .ok_or("the checker is not in the picture to begin with")?;
        let source_circuits = tracked_circuits(&harness);
        assert!(
            !source_circuits.is_empty(),
            "nothing was streamed before the teleport, so nothing could be left behind"
        );

        harness.teleport_to(
            "Fake Region East",
            sl_proto::RegionCoordinates::new(128.0, ROW_Y - SUBJECT_DISTANCE, SUBJECT_EYE_Z + 1.0),
        )?;
        let Some((after, disc_after)) = frame_subject(&mut harness, &subject)? else {
            return Err("the adapter answered the first capture and not the second".into());
        };
        let now = centroid(&after, disc_after, Marker::Red)
            .ok_or("the destination's checker is not in the picture")?;

        let left: Vec<sl_proto::CircuitId> = tracked_circuits(&harness)
            .into_iter()
            .filter(|circuit| source_circuits.contains(circuit))
            .collect();
        assert!(
            left.is_empty(),
            "the region the teleport left still has objects in the world ({left:?}) — the scene \
             was emptied around them rather than purged"
        );

        let drift = Vec2::new(now.x - was.x, now.y - was.y).length();
        assert!(
            drift <= TELEPORT_DRIFT_PX,
            "the same subject landed {drift} px away after the teleport ({was:?} -> {now:?}) — the \
             destination's scene was not built on the destination's origin"
        );
        harness.logout()
    }

    /// Every circuit the viewer currently has tracked objects from.
    ///
    /// A region's identity as the object store keeps it: a teleport opens a
    /// fresh circuit, so "an object from a circuit that was live before the
    /// teleport" is exactly an object the purge should have taken.
    fn tracked_circuits(harness: &ViewerHarness) -> Vec<sl_proto::CircuitId> {
        let mut circuits: Vec<sl_proto::CircuitId> = harness
            .world()
            .resource::<crate::world_api::ObjectState>()
            .objects
            .keys()
            .map(|scoped| scoped.circuit)
            .collect();
        circuits.sort_unstable();
        circuits.dedup();
        circuits
    }

    /// How much of a solid-textured prim's own disc its colour must paint.
    ///
    /// Higher than the checker's share, because a solid has only one colour to
    /// find where a checker splits its disc between two.
    const SOLID_SHARE: f32 = 0.4;

    /// The radius, in metres, of the disc taken on the arrival scene's prim —
    /// inscribed in its one-metre body, as the catalogue subjects' discs are.
    const ARRIVAL_DISC: f32 = 0.35;

    /// **A teleport leaves nothing of the region it left behind.**
    ///
    /// Three leaks in one check, because they are three ways for the same
    /// mistake to show. The destination is ten regions away — not a neighbour,
    /// so nothing of the source has any business being connected — and the
    /// teleport is taken while the source's assets are *still arriving*, which
    /// is when a purge has the most to get wrong.
    ///
    /// What is asserted: no object from a circuit that was live before the
    /// teleport is still tracked; the scene owes no asset work (a fetch left in
    /// flight against a dead region never completes, so the arrival's own
    /// quiescence would never be reached — reaching this line at all is half the
    /// proof, and the count is stated anyway); and the destination's prim,
    /// standing in the very slot the source's checkered box stood in, is
    /// painted its own colour and neither of the checker's.
    #[test]
    fn a_teleport_leaks_nothing_between_regions() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::arrival::{arrival, arrival_position};
        let subject = entry("checker-box").ok_or("the catalogue has no checker-box")?;
        let mut harness = ViewerHarness::start_in(vec![
            sl_fake_grid::catalogue().into_region(RegionConfig::default()),
            arrival().into_region(RegionConfig {
                name: "Fake Region East".to_owned(),
                grid_x: RegionConfig::default().grid_x.saturating_add(10),
                ..RegionConfig::default()
            }),
        ])?;
        harness.login()?;
        // The source's content has been *built* — an event alone would be one
        // frame early, and the object store is what the purge is asserted
        // against. Its textures have not arrived and are not waited for: a
        // teleport taken mid-fetch is the case under test.
        harness.wait_event("the source's checkered box", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == subject.local_id =>
            {
                Some(())
            }
            _ => None,
        })?;
        let source_circuits =
            harness.run_until("the source's objects to be tracked", |harness| {
                let circuits = tracked_circuits(harness);
                (!circuits.is_empty()).then_some(circuits)
            })?;

        harness.teleport_to(
            "Fake Region East",
            sl_proto::RegionCoordinates::new(128.0, ROW_Y - SUBJECT_DISTANCE, SUBJECT_EYE_Z + 1.0),
        )?;
        let at = arrival_position();
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
            no_adapter("the teleport leak check");
            return Ok(());
        };

        let left: Vec<sl_proto::CircuitId> = tracked_circuits(&harness)
            .into_iter()
            .filter(|circuit| source_circuits.contains(circuit))
            .collect();
        assert!(
            left.is_empty(),
            "objects from the region the teleport left are still in the world ({left:?})"
        );
        let outstanding = harness.world().resource::<SceneWork>().outstanding;
        assert!(
            outstanding == 0,
            "the arrival settled with {outstanding} pieces of asset work still owed"
        );

        let disc = disc_at(&mut harness, &at, ARRIVAL_DISC)?;
        let own = coverage(&frame, disc, Marker::Blue);
        assert!(
            own > SOLID_SHARE,
            "the destination's prim paints only {own} of its own disc in blue — the arrival did \
             not dress it in the texture its own region serves"
        );
        for marker in [Marker::Red, Marker::Green] {
            let leaked = coverage(&frame, disc, marker);
            assert!(
                leaked < KILLED_SHARE,
                "the destination's prim paints {leaked} of its disc in {} — the region the \
                 teleport left is still on its face",
                marker.name()
            );
        }
        harness.logout()
    }

    /// The border grid with each region's ground painted its own marker colour.
    fn painted_border_grid() -> Vec<RegionConfig> {
        use sl_fake_grid::fixtures::border::border_on_painted_ground;
        vec![
            border_on_painted_ground(BorderSide::Leaving).into_region(RegionConfig {
                name: WEST_REGION.to_owned(),
                ..RegionConfig::default()
            }),
            border_on_painted_ground(BorderSide::Arriving).into_region(RegionConfig {
                name: EAST_REGION.to_owned(),
                grid_x: RegionConfig::default().grid_x.saturating_add(1),
                ..RegionConfig::default()
            }),
        ]
    }

    /// The `x` of the shared border in the **western** region's frame: its east
    /// edge, which is also the eastern region's origin.
    const BORDER_X: f32 = 256.0;

    /// How far either side of that border the two ground patches are sampled, in
    /// metres — clear of the white region-rim property line drawn along it.
    const GROUND_SAMPLE_FROM_BORDER: f32 = 12.0;

    /// The `y` the both-terrains framing looks along.
    const GROUND_SAMPLE_Y: f32 = 148.0;

    /// The radius, in pixels, of a sampled ground patch.
    const GROUND_PATCH_PX: f32 = 5.0;

    /// How much of a ground patch must carry its region's colour.
    ///
    /// Most of it: a patch of flat, evenly-lit, solid-textured ground has
    /// nothing else in it, and a share much below this means the patch is
    /// straddling something.
    const GROUND_SHARE: f32 = 0.8;

    /// **One frame holds the ground of both regions, each its own.**
    ///
    /// The half of a border crossing the crossing tests do not reach: they
    /// assert an *object* across the line, and an object streamed on a child
    /// circuit would be drawn at the right place even if the neighbour's ground
    /// never arrived at all. This looks at the ground itself, either side of the
    /// same line, in one capture — so the neighbour's `LayerData`, its terrain
    /// mesh, its detail textures and the region offset that puts it on its own
    /// side of the border are all under test at once.
    ///
    /// Decidable because the two regions' grounds are *painted*: every detail
    /// slot of each carries one solid, and the two solids are two marker classes
    /// ([`BorderSide::ground_color`]). A viewer that drew the neighbour's ground
    /// on the root region's origin paints the frame one colour; one that drew no
    /// neighbour ground at all leaves sea or sky where the second colour should
    /// be.
    #[test]
    fn a_border_framing_shows_both_regions_ground() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start_in(painted_border_grid())?;
        harness.login()?;
        harness.wait_neighbour(EAST_REGION)?;
        // Standing on the border looking north along it from above, so the
        // frame's left half is the western region and its right half the
        // eastern one.
        harness.look_from(
            Vector {
                x: BORDER_X,
                y: GROUND_SAMPLE_Y - 40.0,
                z: 40.0,
            },
            Vector {
                x: BORDER_X,
                y: GROUND_SAMPLE_Y,
                z: 25.0,
            },
        );
        let Some(frame) = harness.capture()? else {
            no_adapter("the both-terrains check");
            return Ok(());
        };
        for (side, offset) in [
            (BorderSide::Leaving, -GROUND_SAMPLE_FROM_BORDER),
            (BorderSide::Arriving, GROUND_SAMPLE_FROM_BORDER),
        ] {
            let patch = patch_at(
                &mut harness,
                &Vector {
                    x: BORDER_X + offset,
                    y: GROUND_SAMPLE_Y,
                    z: 25.0,
                },
                GROUND_PATCH_PX,
            )?;
            let marker = ground_marker(side)?;
            let share = coverage(&frame, patch, marker);
            assert!(
                share > GROUND_SHARE,
                "the ground {offset} m from the border paints only {share} of its patch in {} — \
                 the {side:?} region's own terrain is not there",
                marker.name()
            );
        }
        harness.logout()
    }

    /// The marker class a border region's painted ground classifies as.
    ///
    /// Derived from the fixture's own colour rather than restated, so a fixture
    /// that repaints its ground cannot leave this test asserting the old shade.
    fn ground_marker(side: BorderSide) -> Result<Marker, TestError> {
        let [red, green, blue, _alpha] = side.ground_color();
        crate::pixel_oracle::dominant(Vec4::new(
            f32::from(red) / 255.0,
            f32::from(green) / 255.0,
            f32::from(blue) / 255.0,
            0.0,
        ))
        .ok_or_else(|| format!("the {side:?} region's ground colour is not a marker").into())
    }

    /// Where the parcel boundary this test draws and removes runs, in region
    /// metres — clear of the arrival point and of the stock scripted box.
    const PARCEL_BOUNDARY_X: f32 = 96.0;

    /// The `y` the parcel-line framing looks at.
    const PARCEL_BOUNDARY_Y: f32 = 160.0;

    /// How far east of the boundary its camera stands, in metres: close, so the
    /// one-metre band is tens of pixels tall rather than a few.
    const PARCEL_CAMERA_EAST: f32 = 8.0;

    /// The radius, in pixels, of the disc taken on the property line.
    const PARCEL_DISC_PX: f32 = 10.0;

    /// How far west of the boundary the control patch is sampled, in metres —
    /// ground that no parcel change touches.
    const PARCEL_CONTROL_WEST: f32 = 24.0;

    /// **Splitting a parcel draws its property line, and joining removes it.**
    ///
    /// The in-world half of the parcel overlay: a region-wide parcel has no
    /// interior boundary, two parcels have one, and the viewer drapes a band
    /// along it. Nothing about the band's colour is asserted — it is the
    /// ownership palette, which is content — only that it *appears* where the
    /// boundary is and *disappears* when the parcels are joined again. Two
    /// directions rather than one: a viewer that drew the band and never
    /// rebuilt would pass the first half alone.
    ///
    /// A patch of ground the boundary does not touch is read alongside every
    /// capture, so "the band appeared" cannot be the whole frame having shifted.
    #[test]
    fn a_parcel_split_draws_its_property_line_and_a_join_removes_it() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start(stock_fixture())?;
        harness.login()?;
        harness.look_from(
            Vector {
                x: PARCEL_BOUNDARY_X + PARCEL_CAMERA_EAST,
                y: PARCEL_BOUNDARY_Y,
                z: 27.0,
            },
            Vector {
                x: PARCEL_BOUNDARY_X,
                y: PARCEL_BOUNDARY_Y,
                z: 25.5,
            },
        );
        // Held, so the only reason two of these captures differ is the parcel
        // change between them: a running clock re-captures reflection probes and
        // drifts the sky, and a tenth of the control patch changed on its own
        // between two live captures.
        harness.hold_clock();
        let Some(whole) = harness.capture_after(0.0)? else {
            no_adapter("the parcel property-line check");
            return Ok(());
        };
        let line = patch_at(
            &mut harness,
            &Vector {
                x: PARCEL_BOUNDARY_X,
                y: PARCEL_BOUNDARY_Y,
                z: 25.5,
            },
            PARCEL_DISC_PX,
        )?;
        let control = patch_at(
            &mut harness,
            &Vector {
                x: PARCEL_BOUNDARY_X - PARCEL_CONTROL_WEST,
                y: PARCEL_BOUNDARY_Y,
                z: 25.5,
            },
            PARCEL_DISC_PX,
        )?;

        // The region split in two down `PARCEL_BOUNDARY_X`, keeping the same
        // owner: what changes is the boundary and nothing else.
        change_parcels(&mut harness, |owner| {
            vec![
                sl_fake_grid::world::rect_parcel(
                    sl_proto::RegionLocalParcelId(1),
                    owner,
                    "West Half",
                    0.0,
                    0.0,
                    PARCEL_BOUNDARY_X,
                    256.0,
                ),
                sl_fake_grid::world::rect_parcel(
                    sl_proto::RegionLocalParcelId(2),
                    owner,
                    "East Half",
                    PARCEL_BOUNDARY_X,
                    0.0,
                    256.0,
                    256.0,
                ),
            ]
        })?;
        let split = harness
            .capture_after(0.0)?
            .ok_or("the adapter answered the first capture and not the second")?;
        let drawn = changed_share(&whole, &split, line);
        let elsewhere = changed_share(&whole, &split, control);
        assert!(
            drawn > MOVED_SHARE,
            "only {drawn} of the boundary's own patch changed when the region was split in two — \
             no property line was drawn along it"
        );
        assert!(
            elsewhere < STILL_SHARE,
            "ground the split does not touch changed by {elsewhere}, so {drawn} at the boundary \
             says nothing about a property line"
        );

        // ... and joined again.
        change_parcels(&mut harness, |owner| {
            vec![sl_fake_grid::world::region_wide_parcel(
                sl_proto::RegionLocalParcelId(1),
                owner,
                "Whole Again",
            )]
        })?;
        let joined = harness
            .capture_after(0.0)?
            .ok_or("the adapter answered the earlier captures and not this one")?;
        let remaining = changed_share(&whole, &joined, line);
        assert!(
            remaining < STILL_SHARE,
            "{remaining} of the boundary's patch still differs from the picture before the split \
             — the property line was drawn once and never taken down"
        );
        harness.logout()
    }

    /// Rewrite the region's parcels grid-side and push the overlay the viewer
    /// draws its property lines from, then wait for the client to have seen it.
    ///
    /// The two halves go together for the reason
    /// [`FakeAgent::with_world`](sl_fake_grid::FakeAgent::with_world) exists: a
    /// change the session's fixtures do not carry is undone by the next refetch,
    /// and one that is never sent is invisible.
    /// `parcels` is handed the region's **current** owner and answers with the
    /// parcels to replace its own with, so a split keeps the land in the same
    /// hands — an owner change would also change the band's colour class, and
    /// this test is about where the band is.
    fn change_parcels(
        harness: &mut ViewerHarness,
        parcels: impl FnOnce(sl_proto::OwnerKey) -> Vec<sl_proto::ParcelInfo>,
    ) -> Result<(), TestError> {
        let agent = harness.agent()?;
        let owner = harness
            .grid(agent.with_world(|world, _sim| world.parcels.first().map(|parcel| parcel.owner)))
            .ok_or("the region has no parcel to change")?;
        let replacement = parcels(owner);
        let now = agent.now();
        let viewer = agent.agent_id();
        harness.grid(agent.with_world(move |world, sim| {
            world.parcels = replacement;
            let overlay = world.overlay_for(viewer);
            sim.send_parcel_overlay(&overlay, now)
        }))?;
        harness.mark("parcels")?;
        harness.wait_marker("parcels")
    }

    /// How much darker than the day sky the night sky has to come out.
    ///
    /// Half, as the task states it. The two fixture skies are a factor of nine
    /// apart in their authored sunlight, so this is a wide margin over what the
    /// change actually does — deliberately, because what reaches a pixel is that
    /// light through the whole atmosphere model and not the number itself.
    const NIGHT_LUMINANCE_SHARE: f32 = 0.5;

    /// **Changing the region's environment to night darkens the sky.**
    ///
    /// The EEP path end to end and the one thing a capture can honestly say
    /// about a sky: how bright it came out. The grid publishes a new
    /// single-frame environment over `ExtEnvironment`, the client re-requests
    /// it, and the sky band the login test already reasons about is measured
    /// before and after.
    ///
    /// This is the one harness in the tier that does **not** pin the day. A
    /// pinned day position replaces the region's cycle with a synthesised one
    /// built from the legacy presets, so the grid's own sky would never reach
    /// the picture and the test would compare noon with noon.
    #[test]
    fn an_environment_change_to_night_darkens_the_sky() -> Result<(), TestError> {
        let region = sl_fake_grid::RegionFixture {
            environment: Some(sl_test_assets::environment::noon_environment()),
            ..stock_fixture()
        };
        let mut harness = ViewerHarness::start_in_with(
            vec![region.into_region(RegionConfig::default())],
            HarnessOptions::following_the_region_environment(),
        )?;
        harness.login()?;
        harness.wait_event("the region's environment", |event| {
            matches!(event, sl_client_bevy::SlSessionEvent::Environment(_)).then_some(())
        })?;
        // Level north over the middle of the region, as the login test frames
        // it, so the top of the frame is sky and nothing else.
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
        let Some(day) = harness.capture()? else {
            no_adapter("the environment check");
            return Ok(());
        };
        let sky_band = |frame: &Frame| {
            band_mean(frame, 8, FRAME / 2 - 8)
                .map(luminance)
                .ok_or_else(|| TestError::from("the sky band"))
        };
        let bright = sky_band(&day)?;
        assert!(
            bright > NOT_BLACK,
            "the daylit sky is already black ({bright}), so darkening it would prove nothing"
        );

        let seen = harness.world().resource::<Recorded>().events.len();
        let agent = harness.agent()?;
        harness.grid(agent.with_sim(|sim| {
            sim.set_environment(sl_test_assets::environment::night_environment());
        }));
        harness.command(sl_client_bevy::Command::RequestEnvironment { parcel_id: None });
        harness.run_until("the night environment", |harness| {
            harness
                .app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .skip(seen)
                .any(|event| matches!(event, sl_client_bevy::SlSessionEvent::Environment(_)))
                .then_some(())
        })?;

        let night = harness
            .capture()?
            .ok_or("the adapter answered the first capture and not the second")?;
        let dark = sky_band(&night)?;
        assert!(
            dark < bright * NIGHT_LUMINANCE_SHARE,
            "the sky went from {bright} to {dark} when the region's environment changed to night \
             — the grid's own sky is not what is being rendered"
        );
        harness.logout()
    }

    /// The fewest pixels a world-space text billboard has to be made of.
    ///
    /// A short word at the size these are drawn covers a few hundred; this is
    /// low enough not to be a claim about the font's metrics and high enough
    /// that a stray anti-aliased edge is not "the text is there".
    const TEXT_PIXELS: u32 = 60;

    /// How far above its subject, in pixels, the change a world-space text
    /// billboard makes has to sit.
    ///
    /// Both billboards hang **over** what they label — an object's floating
    /// text off the prim's own centre, an avatar's tag off the top of its head —
    /// and both grow upward from there, so their centre of change is above the
    /// subject's projected centre. Twelve pixels rather than one: a rounding
    /// error is not a lift, and a renderer that drew a tag *through* its subject
    /// would have its change centred on it.
    const TEXT_LIFT_PX: f32 = 12.0;

    /// **A prim's floating text is drawn above it.**
    ///
    /// `llSetText` end to end: the object update's text field, the world-space
    /// billboard the name tags share, the glyph atlas and the camera-facing
    /// mesh. Asserted as a **toggle**, because "some pixels above the prim are
    /// not the sky" is also true of a lens flare: the text's own setting is
    /// turned off and the frame taken again, so what is measured is the
    /// difference the text makes and nothing else.
    ///
    /// *Where* that difference is, rather than how much of some disc it fills,
    /// because a camera-facing billboard whose on-screen size is constant at
    /// every distance is not a thing a metre figure can bound: what is asserted
    /// is that the change is real, that its centre is above the prim, and that
    /// the prim's own disc did not change at all.
    #[test]
    fn a_prims_floating_text_is_drawn_above_it() -> Result<(), TestError> {
        let subject = entry("hover-text-box").ok_or("the catalogue has no hover-text-box")?;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;
        frame_the_subject(&mut harness, &subject);
        // Held, so the drifting cloud layer behind the text is not a second
        // reason for the two frames to differ: the space over the prim is
        // against the sky, and the sky is never still.
        harness.hold_clock();
        let Some(shown) = harness.capture_after(0.0)? else {
            no_adapter("the floating-text check");
            return Ok(());
        };
        let prim = subject_disc(&mut harness, &subject)?;

        harness.set_setting(
            crate::hover_text::SETTING_SHOW_HOVER_TEXT,
            sl_settings::SettingValue::Bool(false),
        );
        let hidden = harness
            .capture_after(0.0)?
            .ok_or("the adapter answered the first capture and not the second")?;
        assert_text_over(&shown, &hidden, prim.centre, "the prim's floating text")?;
        let on_the_prim = changed_share(&shown, &hidden, prim);
        assert!(
            on_the_prim < STILL_SHARE,
            "turning the floating text off changed {on_the_prim} of the prim itself, so what \
             changed above it is not the text"
        );
        harness.logout()
    }

    /// Assert that turning a world-space text billboard off changed the picture,
    /// and that what changed sits **above** `subject` on the frame.
    ///
    /// Shared by the two billboard subjects, which are the same renderer read
    /// from either end — an object's floating text and an avatar's name tag.
    fn assert_text_over(
        shown: &Frame,
        hidden: &Frame,
        subject: Vec2,
        what: &str,
    ) -> Result<(), TestError> {
        let pixels = differing_pixels(shown, hidden, None);
        assert!(
            pixels >= TEXT_PIXELS,
            "turning {what} off changed only {pixels} pixels of the whole frame — nothing was \
             being drawn"
        );
        let at = crate::pixel_oracle::changed_centroid(shown, hidden, None)
            .ok_or_else(|| format!("turning {what} off changed nothing to point at"))?;
        assert!(
            at.y < subject.y - TEXT_LIFT_PX,
            "what changed when {what} was turned off is centred at {at:?}, which is not above \
             its subject at {subject:?}"
        );
        Ok(())
    }

    /// **A name tag is drawn over an avatar's head.**
    ///
    /// The same billboard renderer as the floating text, driven from the other
    /// side: an avatar's identity rather than an object's text field, which
    /// means the `GetDisplayNames` reply is under test too — an avatar whose
    /// name never resolved still gets a tag, but it reads `(???) (???)` and this
    /// one does not.
    ///
    /// A toggle again ([`SETTING_SHOW_NAME_TAGS`]) read the same way, and the
    /// NPC's own chest is the control: turning tags off must change the picture
    /// above the head and leave the avatar under it alone.
    ///
    /// [`SETTING_SHOW_NAME_TAGS`]: crate::name_tag_billboard::SETTING_SHOW_NAME_TAGS
    #[test]
    fn a_name_tag_is_drawn_over_an_npcs_head() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::NPC_LOCAL_ID;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;
        harness.wait_event("the catalogue NPC's body", |event| match event {
            sl_client_bevy::SlSessionEvent::ObjectAdded(object)
            | sl_client_bevy::SlSessionEvent::ObjectUpdated(object)
                if object.local_id == NPC_LOCAL_ID =>
            {
                Some(())
            }
            _ => None,
        })?;
        frame_the_npc(
            &mut harness,
            NPC_DISTANCE,
            npc_position().z + SUBJECT_LOOK_UP,
        );
        // Held, so the twist the NPC is playing is not a second reason for the
        // two frames to differ.
        harness.hold_clock();
        let Some(shown) = harness.capture_after(0.0)? else {
            no_adapter("the name-tag check");
            return Ok(());
        };
        let root = npc_body_root(&harness)?;
        let chest = Vec3::new(root.x, root.y + NPC_CHEST_ABOVE_ROOT, root.z);
        let body = world_disc(&mut harness, chest, NPC_BAKE_DISC)?;

        harness.set_setting(
            crate::name_tag_billboard::SETTING_SHOW_NAME_TAGS,
            sl_settings::SettingValue::Bool(false),
        );
        let hidden = harness
            .capture_after(0.0)?
            .ok_or("the adapter answered the first capture and not the second")?;
        assert_text_over(&shown, &hidden, body.centre, "the NPC's name tag")?;
        let on_the_body = changed_share(&shown, &hidden, body);
        assert!(
            on_the_body < STILL_SHARE,
            "turning name tags off changed {on_the_body} of the NPC's own chest, so what changed \
             above it is not the tag"
        );
        harness.logout()
    }

    /// **A media face fetches its `ObjectMedia`, and still draws the texture
    /// under it.**
    ///
    /// The whole MOAP hand-shake but the browser: the object update carries a
    /// `MediaURL` version, the viewer answers it with a `RequestObjectMedia`,
    /// the capability replies with the per-face entry set, and the entry for
    /// face 0 lands in [`MediaData`] with the URL the region published. Nothing
    /// below this tier runs that chain — the version is on the wire, the reply
    /// is over CAPS, and what ties them together is a command the viewer sends
    /// itself.
    ///
    /// What is **not** here is the surface: this harness gives
    /// `MediaEnginePlugin` `enabled: false`, and a test binary has no
    /// `sl-cef-helper` beside it to start a browser with anyway. So the
    /// placeholder a live surface shows before its first paint is a rig with a
    /// browser process's question, and this asserts the pixel claim that
    /// *does* belong here: a face whose `TextureEntry` carries the media flag
    /// is an ordinary textured face until something claims it. A viewer that
    /// blanked it on the flag alone would leave a hole in the world for every
    /// media prim on a grid — most vendors, most rental boxes, every video
    /// screen.
    ///
    /// [`MediaData`]: crate::media_prim::MediaData
    #[test]
    fn a_media_face_fetches_its_object_media_and_keeps_its_texture() -> Result<(), TestError> {
        let subject = entry("media-face-box").ok_or("the catalogue has no media-face-box")?;
        let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
        harness.login()?;

        // The capability's reply, folded into the store the media driver reads.
        // Waited for rather than read once: the fetch is a round trip the
        // viewer starts on its own when the object's media version arrives.
        let target = crate::world_api::MediaTarget {
            object: subject.full_id,
            face: sl_client_bevy::PrimFaceId::new(0),
        };
        let served = harness.run_until("the ObjectMedia reply", move |harness| {
            harness
                .app
                .world()
                .resource::<crate::media_prim::MediaData>()
                .entry(target)
                .and_then(|media| media.home_url.clone())
        })?;
        assert!(
            served.as_str() == sl_fake_grid::fixtures::catalogue::MEDIA_URL,
            "the face's media entry names {served}, a URL the region never published"
        );
        // And only face 0 carries one, as the region's record says.
        let other = crate::world_api::MediaTarget {
            face: sl_client_bevy::PrimFaceId::new(1),
            ..target
        };
        assert!(
            harness
                .world()
                .resource::<crate::media_prim::MediaData>()
                .entry(other)
                .is_none(),
            "a face the region put no media on came back carrying some"
        );

        let Some((frame, disc)) = frame_subject(&mut harness, &subject)? else {
            no_adapter("the media-face check");
            return Ok(());
        };
        for marker in [Marker::Red, Marker::Green] {
            let share = coverage(&frame, disc, marker);
            assert!(
                share > CHECKER_SHARE,
                "the media-flagged box paints only {share} of its own disc in {} — the media flag \
                 blanked a face nothing is painting yet",
                marker.name()
            );
        }
        harness.logout()
    }

    /// Aim the camera from inside the western region at the eastern region's
    /// marker pillar, across the shared border.
    ///
    /// Stated once and used by both border tests, because the whole point of
    /// the continuity one is that this framing is never repeated.
    fn frame_the_border(harness: &mut ViewerHarness) {
        let at = marker_seen_from(WEST_REGION);
        harness.look_from(
            Vector {
                x: at.x - BORDER_CAMERA_WEST,
                y: at.y,
                z: BORDER_EYE_Z,
            },
            at,
        );
    }
}
