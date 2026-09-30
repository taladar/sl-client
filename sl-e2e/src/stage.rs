//! [`StageBuilder`] and [`Stage`]: a fake grid, N viewers each logged in as
//! its own account, and the grid-control handle — set up, handed to a test
//! body, and taken down again whatever the body did.

use core::time::Duration;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bevy::math::UVec2;
use serde_json::json;
use sl_automation_proto::Probe;
use sl_client_bevy::{LoginParams, LoginRequest, StartLocation};
use sl_client_bevy_viewer::assembly::{
    Automation, MediaRuntime, Storage, ViewerApp, ViewerAppBuilder, ViewerAppOptions, ViewerPaths,
    WindowMode,
};
use sl_fake_grid::fixtures::scenarios;
use sl_fake_grid::{AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig};
use sl_proto::{AgentKey, RegionCoordinates};
use sl_viewer_automation::{BuildError, InProcessHost, ViewerHandle};
use sl_viewer_driver::{Viewer, ViewerOptions};
use sl_viewer_launch::{Ending, LOGOUT_GRACE, Launch, RunningViewer, ViewerDir};
use sl_viewer_world_avatar::avatar_overrides::AvatarOverrides;
use sl_viewer_world_scene::render_overrides::RenderOverrides;
use sl_viewer_world_view::session::TerminationFlag;

use crate::backend::Backend;
use crate::error::{BodyError, StageError};
use crate::logs::RouteGuard;

/// Every stage account's first name; a viewer's label is its last name.
pub const FIRST_NAME: &str = "Stage";

/// The password every stage account shares on the loopback grid.
const PASSWORD: &str = "password";

/// The `[avatars.<key>]` a process viewer's credentials file names.
const AVATAR_KEY: &str = "stage";

/// The off-screen window every stage viewer renders into, on both backends.
const WINDOW: UVec2 = UVec2::new(1280, 720);

/// How long a viewer may take to log in and settle: a cold shader cache in a
/// debug build, beside other viewers.
const LOGIN: Duration = Duration::from_secs(240);

/// How long a viewer process may take to open its automation socket.
const CONNECT: Duration = Duration::from_secs(120);

/// How long an in-process viewer may take to log out and exit once asked.
const LOGOUT: Duration = Duration::from_secs(60);

/// How often a starting viewer process is looked at.
const POLL: Duration = Duration::from_millis(100);

/// Numbers the process backend's socket directories within this process.
static NEXT_SOCKET_DIR: AtomicU64 = AtomicU64::new(0);

/// Reconfigures the grid a stage starts, beyond its regions and accounts.
type GridHook = Arc<dyn Fn(FakeGridBuilder) -> FakeGridBuilder + Send + Sync>;

/// What a stage is made of, and the entry point that runs a test body on it.
///
/// ```no_run
/// # fn main() -> Result<(), sl_e2e::StageError> {
/// sl_e2e::StageBuilder::new("two_viewers")
///     .viewer_binary("target/release/sl-client-bevy-viewer")
///     .viewer("Alpha")
///     .viewer("Beta")
///     .run(async |stage: &sl_e2e::Stage| {
///         let alpha = stage.viewer("Alpha")?;
///         alpha.open_floater("inventory").await?;
///         Ok(())
///     })
/// # }
/// ```
#[derive(Clone)]
pub struct StageBuilder {
    /// The test's name: the artifact directory's.
    name: String,
    /// The viewer binary, for the process backend.
    binary: Option<PathBuf>,
    /// The viewers' labels, in order.
    labels: Vec<String>,
    /// The regions, when not the stock scene's one.
    regions: Vec<RegionConfig>,
    /// Where each viewer starts in the first region.
    start: RegionCoordinates,
    /// Further grid configuration.
    grid: Option<GridHook>,
    /// The backends, when not `SL_E2E_BACKEND`'s.
    backends: Option<Vec<Backend>>,
    /// The artifact root, when not the target directory's `e2e/`.
    artifacts: Option<PathBuf>,
}

impl core::fmt::Debug for StageBuilder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StageBuilder")
            .field("name", &self.name)
            .field("binary", &self.binary)
            .field("labels", &self.labels)
            .field("regions", &self.regions.len())
            .field("backends", &self.backends)
            .field("artifacts", &self.artifacts)
            .finish_non_exhaustive()
    }
}

impl StageBuilder {
    /// A stage for the test called `name`, with no viewers yet, on the stock
    /// scene's region.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            binary: None,
            labels: Vec::new(),
            regions: Vec::new(),
            start: RegionCoordinates::new(
                128.0,
                128.0,
                f32::from(sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M)
                    + sl_fake_grid::AVATAR_CENTRE_ABOVE_GROUND_M,
            ),
            grid: None,
            backends: None,
            artifacts: None,
        }
    }

    /// The viewer binary the process backend launches — in a test of the
    /// viewer crate, `env!("CARGO_BIN_EXE_sl-client-bevy-viewer")`.
    #[must_use]
    pub fn viewer_binary(mut self, binary: impl Into<PathBuf>) -> Self {
        self.binary = Some(binary.into());
        self
    }

    /// One more viewer, logged in as the account `Stage <label>` and
    /// addressed by `label`: ASCII letters and digits.
    #[must_use]
    pub fn viewer(mut self, label: impl Into<String>) -> Self {
        self.labels.push(label.into());
        self
    }

    /// One more region; the first one named replaces the stock scene's. The
    /// viewers start in the first region.
    #[must_use]
    pub fn region(mut self, region: RegionConfig) -> Self {
        self.regions.push(region);
        self
    }

    /// Where in the first region the viewers start.
    #[must_use]
    pub const fn start_position(mut self, position: RegionCoordinates) -> Self {
        self.start = position;
        self
    }

    /// Configure the grid further — a scenario, a timeline, a deterministic
    /// seed, login gates. Called on each backend's fresh grid.
    #[must_use]
    pub fn configure_grid(
        mut self,
        hook: impl Fn(FakeGridBuilder) -> FakeGridBuilder + Send + Sync + 'static,
    ) -> Self {
        self.grid = Some(Arc::new(hook));
        self
    }

    /// Run on these backends, whatever `SL_E2E_BACKEND` says.
    #[must_use]
    pub fn backends(mut self, backends: impl Into<Vec<Backend>>) -> Self {
        self.backends = Some(backends.into());
        self
    }

    /// Keep artifacts under `root/<test>/<backend>/` instead of the target
    /// directory's `e2e/`.
    #[must_use]
    pub fn artifacts(mut self, root: impl Into<PathBuf>) -> Self {
        self.artifacts = Some(root.into());
        self
    }

    /// Run `body` on a fresh stage once per backend, one after the other, and
    /// take each stage down afterwards — also when the body fails or panics.
    ///
    /// A machine with no GPU adapter skips: it logs a warning, says so on
    /// standard error, and returns `Ok`.
    ///
    /// # Errors
    ///
    /// The first stage that failed to start, the first body that failed
    /// ([`StageError::Body`]), or a teardown that found a viewer that would
    /// not log out or a session left on the grid.
    ///
    /// # Panics
    ///
    /// Resumes the body's panic once the stage is down; if the teardown
    /// failed too, panics with both.
    pub fn run<F>(&self, body: F) -> Result<(), StageError>
    where
        F: AsyncFn(&Stage) -> Result<(), BodyError>,
    {
        self.check_labels()?;
        let backends = match &self.backends {
            Some(backends) => backends.clone(),
            None => Backend::from_env()?,
        };
        crate::logs::install();
        if !crate::gpu::adapter_available() {
            tracing::warn!("skipping {}: this machine has no GPU adapter", self.name);
            return Ok(());
        }
        let root = self.artifact_root()?.join(&self.name);
        for backend in backends {
            self.run_on(backend, &root.join(backend.name()), &body)?;
        }
        Ok(())
    }

    /// One backend's run: start, body, teardown.
    #[expect(
        clippy::panic,
        reason = "a test body that panicked is resumed as a panic; when the teardown failed too, \
                  the panic has to carry both"
    )]
    fn run_on<F>(&self, backend: Backend, dir: &Path, body: &F) -> Result<(), StageError>
    where
        F: AsyncFn(&Stage) -> Result<(), BodyError>,
    {
        fresh_dir(dir)?;
        let in_process_logs: Vec<(String, PathBuf)> = match backend {
            Backend::InProcess => self
                .labels
                .iter()
                .map(|label| (self.log_label(backend, label), viewer_dir(dir, label).log()))
                .collect(),
            Backend::Process => Vec::new(),
        };
        for label in &self.labels {
            viewer_dir(dir, label)
                .create()
                .map_err(|source| StageError::Artifacts {
                    path: dir.join(label),
                    source,
                })?;
        }
        let _route =
            RouteGuard::register(&dir.join("grid.log"), &in_process_logs).map_err(|source| {
                StageError::Artifacts {
                    path: dir.join("grid.log"),
                    source,
                }
            })?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(StageError::Runtime)?;
        tracing::info!(
            "stage {} on the {backend} backend in {}",
            self.name,
            dir.display()
        );
        let stage = runtime.block_on(Stage::start(self, backend, dir))?;
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| runtime.block_on(body(&stage))));
        let teardown = runtime.block_on(stage.shutdown());
        match outcome {
            Err(panic) => {
                if let Err(error) = teardown {
                    panic!(
                        "the test body panicked on the {backend} backend ({}), and the teardown \
                         failed too: {error}",
                        panic_message(panic.as_ref())
                    );
                }
                std::panic::resume_unwind(panic)
            }
            Ok(Err(source)) => {
                if let Err(error) = teardown {
                    tracing::error!("the teardown after the failed body failed too: {error}");
                }
                Err(StageError::Body { backend, source })
            }
            Ok(Ok(())) => teardown,
        }
    }

    /// Refuse a label an account cannot have, or one used twice.
    fn check_labels(&self) -> Result<(), StageError> {
        for (index, label) in self.labels.iter().enumerate() {
            let usable = label
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
                && label.chars().all(|c| c.is_ascii_alphanumeric())
                && !self.labels.iter().take(index).any(|other| other == label);
            if !usable {
                return Err(StageError::Label(label.clone()));
            }
        }
        Ok(())
    }

    /// The artifact root: the one given, `SL_E2E_ARTIFACTS`, or the target
    /// directory's `e2e/` — found from this test binary's own path,
    /// `<target>/<profile>/deps/<binary>`.
    fn artifact_root(&self) -> Result<PathBuf, StageError> {
        if let Some(root) = &self.artifacts {
            return Ok(root.clone());
        }
        if let Some(root) = std::env::var_os("SL_E2E_ARTIFACTS") {
            return Ok(PathBuf::from(root));
        }
        let exe = std::env::current_exe().map_err(|source| StageError::Artifacts {
            path: PathBuf::from("<current exe>"),
            source,
        })?;
        exe.ancestors()
            .nth(3)
            .map(|target| target.join("e2e"))
            .ok_or_else(|| StageError::Artifacts {
                path: exe.clone(),
                source: std::io::Error::other(
                    "the test binary is not under <target>/<profile>/deps",
                ),
            })
    }

    /// The name an in-process viewer's log span carries: unique to this test,
    /// backend and viewer, so two stages in one process never share a file.
    fn log_label(&self, backend: Backend, label: &str) -> String {
        format!("{}/{backend}/{label}", self.name)
    }

    /// The regions the grid serves: the ones named, else the stock scene's.
    fn regions(&self) -> Vec<RegionConfig> {
        if self.regions.is_empty() {
            scenarios::scenario(scenarios::DEFAULT).map_or_else(
                || vec![RegionConfig::default()],
                |scene| vec![scene.dress(RegionConfig::default())],
            )
        } else {
            self.regions.clone()
        }
    }
}

/// `dir`, emptied if it was there.
fn fresh_dir(dir: &Path) -> Result<(), StageError> {
    let fail = |source| StageError::Artifacts {
        path: dir.to_path_buf(),
        source,
    };
    if fs_err::metadata(dir).is_ok() {
        fs_err::remove_dir_all(dir).map_err(fail)?;
    }
    fs_err::create_dir_all(dir).map_err(fail)
}

/// A viewer's directory within a stage's.
fn viewer_dir(stage: &Path, label: &str) -> ViewerDir {
    ViewerDir {
        root: stage.join(label),
    }
}

/// A panic's message, when it carried one.
fn panic_message(panic: &(dyn core::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned())
}

/// How one stage viewer runs.
#[derive(Debug)]
enum ViewerRun {
    /// An App on the stage's in-process host.
    InProcess(ViewerHandle),
    /// A process; `None` once it has been stopped.
    Process(Option<RunningViewer>),
}

/// One stage viewer.
#[derive(Debug)]
struct StageViewer {
    /// Its label.
    label: String,
    /// Its account's agent.
    agent: AgentKey,
    /// Its artifact directory.
    dir: PathBuf,
    /// The driver's handle, once connected.
    driver: Option<Viewer>,
    /// How it runs.
    run: ViewerRun,
}

/// A running stage: the grid and its viewers, each logged in.
#[derive(Debug)]
pub struct Stage {
    /// The test's name.
    name: String,
    /// The backend.
    backend: Backend,
    /// The stage's artifact directory.
    dir: PathBuf,
    /// The grid.
    grid: FakeGrid,
    /// The region the viewers start in.
    home: String,
    /// The viewers, in the order they were named.
    viewers: Vec<StageViewer>,
    /// The in-process host, on that backend.
    host: Option<InProcessHost<ViewerApp>>,
    /// Where the process backend's automation sockets are.
    sockets: Option<PathBuf>,
}

impl Stage {
    /// Start the grid and every viewer, and wait until each has logged in
    /// and settled. What was started is taken down again when a step fails.
    async fn start(
        builder: &StageBuilder,
        backend: Backend,
        dir: &Path,
    ) -> Result<Self, StageError> {
        let regions = builder.regions();
        let home = regions
            .first()
            .map(|region| region.name.clone())
            .unwrap_or_default();
        let mut grid = FakeGridBuilder::new()
            // A long hold, so the CAPS long-poll does not compete with the
            // renders for the cores.
            .event_queue_hold(Duration::from_secs(2));
        for region in regions {
            grid = grid.region(region);
        }
        for label in &builder.labels {
            grid = grid.account(AccountConfig::new(FIRST_NAME, label, PASSWORD));
        }
        if let Some(hook) = &builder.grid {
            grid = hook(grid);
        }
        let grid = grid.start().await?;
        tracing::info!("stage grid at {}", grid.login_uri());
        let mut stage = Self {
            name: builder.name.clone(),
            backend,
            dir: dir.to_path_buf(),
            grid,
            home,
            viewers: Vec::new(),
            host: None,
            sockets: None,
        };
        match stage.launch(builder).await {
            Ok(()) => Ok(stage),
            Err(error) => {
                if let Err(teardown) = stage.shutdown().await {
                    tracing::error!("taking down the stage that failed to start: {teardown}");
                }
                Err(error)
            }
        }
    }

    /// Launch every viewer on the stage's backend, connect the driver to
    /// each, and wait until all have logged in and settled.
    async fn launch(&mut self, builder: &StageBuilder) -> Result<(), StageError> {
        let start = StartLocation::region(self.home.clone(), builder.start);
        match self.backend {
            Backend::InProcess => self.launch_in_process(builder, &start).await?,
            Backend::Process => self.launch_processes(builder, &start).await?,
        }
        let mut arrivals = tokio::task::JoinSet::new();
        for viewer in &self.viewers {
            let driver = viewer
                .driver
                .clone()
                .ok_or_else(|| StageError::UnknownViewer(viewer.label.clone()))?;
            let home = self.home.clone();
            let _task = arrivals.spawn(async move {
                let arrived = async {
                    let _held = driver
                        .expect_state(Probe::Agent)
                        .at("/region/name")
                        .timeout(LOGIN)
                        .to_equal(json!(home))
                        .await?;
                    driver.wait_until_quiet(LOGIN).await
                };
                arrived.await.map_err(|source| StageError::Driver {
                    viewer: driver.label().to_owned(),
                    source,
                })
            });
        }
        while let Some(joined) = arrivals.join_next().await {
            match joined {
                Ok(arrived) => arrived?,
                Err(join) => std::panic::resume_unwind(join.into_panic()),
            }
        }
        Ok(())
    }

    /// The in-process backend: each viewer an App on one host.
    async fn launch_in_process(
        &mut self,
        builder: &StageBuilder,
        start: &StartLocation,
    ) -> Result<(), StageError> {
        self.host = Some(InProcessHost::<ViewerApp>::start()?);
        for label in &builder.labels {
            let agent = self.agent_of(label)?;
            let dir = self.dir.join(label);
            let params = LoginParams {
                login_uri: self.grid.login_uri(),
                request: LoginRequest::new(
                    FIRST_NAME,
                    label,
                    PASSWORD,
                    start.clone(),
                    "sl-e2e",
                    "0.0",
                ),
            };
            let log_label = builder.log_label(self.backend, label);
            let state = dir.join("state");
            let host = self
                .host
                .as_ref()
                .ok_or(sl_viewer_automation::HostError::Stopped)?;
            let (handle, link) = host
                .host(label.clone(), move || {
                    in_process_viewer(params, log_label, &state)
                })
                .await?;
            self.viewers.push(StageViewer {
                label: label.clone(),
                agent,
                dir: dir.clone(),
                driver: None,
                run: ViewerRun::InProcess(handle),
            });
            let driver =
                Viewer::over_link(link.requests, link.messages, driver_options(label, &dir))
                    .await
                    .map_err(|source| StageError::Driver {
                        viewer: label.clone(),
                        source,
                    })?;
            if let Some(viewer) = self.viewers.last_mut() {
                viewer.driver = Some(driver);
            }
        }
        Ok(())
    }

    /// The process backend: each viewer the real binary, headless, behind an
    /// automation socket.
    async fn launch_processes(
        &mut self,
        builder: &StageBuilder,
        start: &StartLocation,
    ) -> Result<(), StageError> {
        let binary = builder.binary.clone().ok_or(StageError::NoBinary)?;
        // Short: a socket path must stay under about a hundred bytes, and the
        // artifact directory is deep.
        let sockets = std::env::temp_dir().join(format!(
            "sl-e2e-{}-{}",
            std::process::id(),
            NEXT_SOCKET_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs_err::create_dir_all(&sockets).map_err(|source| StageError::Artifacts {
            path: sockets.clone(),
            source,
        })?;
        let sockets = self.sockets.insert(sockets).clone();
        for (index, label) in builder.labels.iter().enumerate() {
            let agent = self.agent_of(label)?;
            let dir = viewer_dir(&self.dir, label);
            let credentials = dir.root.join("credentials.toml");
            fs_err::write(
                &credentials,
                format!(
                    "# Written by sl-e2e for one stage against a fake grid.\n\
                     default_avatar = \"{AVATAR_KEY}\"\n\n[avatars.{AVATAR_KEY}]\nfirst = \
                     \"{FIRST_NAME}\"\nlast = \"{label}\"\npassword = \"{PASSWORD}\"\nlogin_uri = \
                     \"{uri}\"\n",
                    uri = self.grid.login_uri()
                ),
            )
            .map_err(|source| StageError::Artifacts {
                path: credentials.clone(),
                source,
            })?;
            let socket = sockets.join(format!("{index}.sock"));
            let launch = Launch::in_dir(label.clone(), &binary, &dir).args([
                "--credentials".to_owned(),
                credentials.display().to_string(),
                "--avatar".to_owned(),
                AVATAR_KEY.to_owned(),
                "--login-uri".to_owned(),
                self.grid.login_uri().to_string(),
                "--start".to_owned(),
                start.to_wire_string(),
                "--headless".to_owned(),
                "--capture-size".to_owned(),
                format!("{}x{}", WINDOW.x, WINDOW.y),
                "--disable-web-media".to_owned(),
                "--automation-socket".to_owned(),
                socket.display().to_string(),
            ]);
            let running = RunningViewer::spawn(&launch).map_err(|source| StageError::Launch {
                viewer: label.clone(),
                source,
            })?;
            self.viewers.push(StageViewer {
                label: label.clone(),
                agent,
                dir: dir.root.clone(),
                driver: None,
                run: ViewerRun::Process(Some(running)),
            });
            let driver = self.connect_last(&socket, &dir.log()).await?;
            if let Some(viewer) = self.viewers.last_mut() {
                viewer.driver = Some(driver);
            }
        }
        Ok(())
    }

    /// Connect the driver to the viewer process launched last, once its
    /// socket answers — failing early, with its log, if it ends first.
    async fn connect_last(&mut self, socket: &Path, log: &Path) -> Result<Viewer, StageError> {
        let since = Instant::now();
        let mut last = "the socket does not exist yet".to_owned();
        loop {
            let Some(viewer) = self.viewers.last_mut() else {
                return Err(StageError::UnknownViewer(String::new()));
            };
            if fs_err::metadata(socket).is_ok() {
                match Viewer::connect(socket, driver_options(&viewer.label, &viewer.dir)).await {
                    Ok(driver) => return Ok(driver),
                    Err(error) => last = error.to_string(),
                }
            }
            if let ViewerRun::Process(Some(running)) = &mut viewer.run
                && let Some(ending) = running.try_ending().map_err(|source| StageError::Launch {
                    viewer: viewer.label.clone(),
                    source,
                })?
            {
                return Err(StageError::EndedEarly {
                    viewer: viewer.label.clone(),
                    ending: format!("{ending:?}"),
                    log: log.to_path_buf(),
                });
            }
            if since.elapsed() > CONNECT {
                return Err(StageError::NoSocket {
                    viewer: viewer.label.clone(),
                    seconds: CONNECT.as_secs(),
                    last,
                });
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// The agent of the account `Stage <label>`.
    fn agent_of(&self, label: &str) -> Result<AgentKey, StageError> {
        self.grid
            .account_agent_id(FIRST_NAME, label)
            .ok_or_else(|| StageError::NoAccount(label.to_owned()))
    }

    /// The test's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The backend this stage runs its viewers on.
    #[must_use]
    pub const fn backend(&self) -> Backend {
        self.backend
    }

    /// The grid, to drive from the grid side: teleports, crossings, the
    /// sessions in a region.
    #[must_use]
    pub const fn grid(&self) -> &FakeGrid {
        &self.grid
    }

    /// The region the viewers started in.
    #[must_use]
    pub fn home_region(&self) -> &str {
        &self.home
    }

    /// The viewers' labels, in the order they were named.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.viewers.iter().map(|viewer| viewer.label.as_str())
    }

    /// The stage viewer `label`.
    fn stage_viewer(&self, label: &str) -> Result<&StageViewer, StageError> {
        self.viewers
            .iter()
            .find(|viewer| viewer.label == label)
            .ok_or_else(|| StageError::UnknownViewer(label.to_owned()))
    }

    /// The driver's handle on viewer `label`.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn viewer(&self, label: &str) -> Result<&Viewer, StageError> {
        self.stage_viewer(label)?
            .driver
            .as_ref()
            .ok_or_else(|| StageError::UnknownViewer(label.to_owned()))
    }

    /// The agent viewer `label` is logged in as.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn agent_id(&self, label: &str) -> Result<AgentKey, StageError> {
        Ok(self.stage_viewer(label)?.agent)
    }

    /// Viewer `label`'s artifact directory: its log, its failure artifacts,
    /// its state.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn artifacts(&self, label: &str) -> Result<&Path, StageError> {
        Ok(&self.stage_viewer(label)?.dir)
    }

    /// Viewer `label`'s process id, on the process backend.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn pid(&self, label: &str) -> Result<Option<u32>, StageError> {
        Ok(match &self.stage_viewer(label)?.run {
            ViewerRun::Process(Some(running)) => Some(running.pid()),
            ViewerRun::Process(None) | ViewerRun::InProcess(_) => None,
        })
    }

    /// The grid's session of viewer `label` in the region it is in now: the
    /// grid side of its conversation.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given,
    /// [`StageError::Driver`] when its region cannot be read, and
    /// [`StageError::NoAccount`] when the grid holds no session of it there.
    pub async fn agent(&self, label: &str) -> Result<FakeAgent, StageError> {
        let viewer = self.viewer(label)?;
        let agent = self.agent_id(label)?;
        let readout = viewer.agent().await.map_err(|source| StageError::Driver {
            viewer: label.to_owned(),
            source,
        })?;
        let region = readout
            .region
            .and_then(|region| region.name)
            .unwrap_or_else(|| self.home.clone());
        self.grid
            .sessions_in(&region)
            .await
            .into_iter()
            .find(|session| session.agent_id() == agent)
            .ok_or_else(|| StageError::NoAccount(label.to_owned()))
    }

    /// Send viewer `label` the marker `name` from the grid: it arrives after
    /// everything the grid sent that viewer before it.
    ///
    /// # Errors
    ///
    /// As [`agent`](Self::agent); [`StageError::Mark`] when the send fails.
    pub async fn mark(&self, label: &str, name: &str) -> Result<(), StageError> {
        let agent = self.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_generic_message(&sl_fake_grid::marker(name), now))
            .await
            .map_err(|error| StageError::Mark {
                viewer: label.to_owned(),
                marker: name.to_owned(),
                reason: error.to_string(),
            })
    }

    /// Wait up to `timeout` until viewer `label` has received the marker
    /// `name` — whether it arrived before this wait began or after.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`], or [`StageError::Driver`] when it does
    /// not arrive.
    pub async fn wait_marker(
        &self,
        label: &str,
        name: &str,
        timeout: Duration,
    ) -> Result<(), StageError> {
        let viewer = self.viewer(label)?;
        let part = format!(
            "method: {:?}, params: [{name:?}]",
            sl_fake_grid::marker::MARKER_METHOD
        );
        viewer
            .events_from_start()
            .wait_for_containing("GenericMessage", &part, timeout)
            .await
            .map(drop)
            .map_err(|source| StageError::Driver {
                viewer: label.to_owned(),
                source,
            })
    }

    /// Take the stage down: ask every viewer to log out and wait until each
    /// has exited, then check that the grid holds no session, then stop the
    /// grid. Each step runs even when an earlier one failed; the first
    /// failure is returned and the others logged.
    async fn shutdown(mut self) -> Result<(), StageError> {
        let mut problems = Vec::new();
        for viewer in &mut self.viewers {
            viewer.driver = None;
        }
        if let Some(host) = self.host.take() {
            let handles: Vec<(String, ViewerHandle)> = self
                .viewers
                .iter()
                .filter_map(|viewer| match viewer.run {
                    ViewerRun::InProcess(handle) => Some((viewer.label.clone(), handle)),
                    ViewerRun::Process(_) => None,
                })
                .collect();
            for (label, handle) in &handles {
                let raised = host
                    .with_app(*handle, |viewer| {
                        viewer
                            .app()
                            .world()
                            .get_resource::<TerminationFlag>()
                            .map(TerminationFlag::raise)
                            .is_some()
                    })
                    .await;
                match raised {
                    Ok(true) => {}
                    Ok(false) => problems.push(StageError::NoLogout {
                        viewer: label.clone(),
                        reason: "it has no termination flag".to_owned(),
                    }),
                    Err(error) => problems.push(StageError::Host(error)),
                }
            }
            for (label, handle) in &handles {
                match tokio::time::timeout(LOGOUT, host.exited(*handle)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => problems.push(StageError::Host(error)),
                    Err(_elapsed) => problems.push(StageError::NoLogout {
                        viewer: label.clone(),
                        reason: format!(
                            "it had not exited {} s after being asked",
                            LOGOUT.as_secs()
                        ),
                    }),
                }
            }
            if let Err(error) = tokio::task::spawn_blocking(move || host.stop())
                .await
                .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()))
            {
                problems.push(StageError::Host(error));
            }
        }
        let processes: Vec<(String, RunningViewer)> = self
            .viewers
            .iter_mut()
            .filter_map(|viewer| match &mut viewer.run {
                ViewerRun::Process(running) => running
                    .take()
                    .map(|running| (viewer.label.clone(), running)),
                ViewerRun::InProcess(_) => None,
            })
            .collect();
        if !processes.is_empty() {
            let (labels, running): (Vec<String>, Vec<RunningViewer>) =
                processes.into_iter().unzip();
            let stopped = tokio::task::spawn_blocking(move || {
                sl_viewer_launch::stop_all(running, LOGOUT_GRACE)
            })
            .await
            .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()));
            for (label, result) in labels.into_iter().zip(stopped) {
                match result {
                    Ok(ran) if ran.ending == Ending::AskedToQuit => {}
                    Ok(ran) => problems.push(StageError::NoLogout {
                        viewer: label,
                        reason: match ran.ending {
                            Ending::Killed => "it was killed after the logout grace".to_owned(),
                            other => format!("it ended without logging out ({other:?})"),
                        },
                    }),
                    Err(source) => problems.push(StageError::Launch {
                        viewer: label,
                        source,
                    }),
                }
            }
        }
        let mut stranded = Vec::new();
        for region in self.grid.region_names() {
            for session in self.grid.sessions_in(&region).await {
                let agent = session.agent_id();
                let who = self
                    .viewers
                    .iter()
                    .find(|viewer| viewer.agent == agent)
                    .map_or_else(|| format!("{agent:?}"), |viewer| viewer.label.clone());
                stranded.push(format!("{who} in {region}"));
            }
        }
        if !stranded.is_empty() {
            problems.push(StageError::Stranded(stranded));
        }
        self.grid.shutdown();
        if let Some(sockets) = &self.sockets
            && let Err(error) = fs_err::remove_dir_all(sockets)
        {
            tracing::warn!("removing the stage's socket directory: {error}");
        }
        let mut problems = problems.into_iter();
        let first = problems.next();
        for other in problems {
            tracing::error!("stage {} teardown: {other}", self.name);
        }
        first.map_or(Ok(()), Err)
    }
}

/// The driver's options for viewer `label`, its failure artifacts in its
/// directory.
fn driver_options(label: &str, dir: &Path) -> ViewerOptions {
    ViewerOptions::new(label).with_artifacts(dir.join("failures"))
}

/// An in-process stage viewer: the viewer's own builder's App, headless,
/// storing under `state`, reached through the in-process transport and
/// logging inside a span named `log_label`.
fn in_process_viewer(
    params: LoginParams,
    log_label: String,
    state: &Path,
) -> Result<ViewerApp, BuildError> {
    let mut options = ViewerAppOptions::new(params);
    options.window = WindowMode::Headless {
        size: WINDOW,
        watch: false,
    };
    options.storage = Storage::Directories(ViewerPaths::under(state));
    options.audio_device = false;
    options.media = MediaRuntime::OFF;
    options.render_overrides = Some(RenderOverrides::default());
    options.avatar_overrides = Some(AvatarOverrides::default());
    options.content.fetch_server_chat_history = false;
    options.automation = Automation::InProcess;
    options.log_label = Some(log_label);
    let mut viewer = ViewerAppBuilder::from_options(options)
        .build()
        .map_err(|error| error.to_string())?;
    // Its own flag: raising the process's would ask every viewer to quit.
    viewer.app_mut().insert_resource(TerminationFlag::own());
    viewer.finish();
    Ok(viewer)
}
