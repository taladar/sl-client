//! [`StageBuilder`] and [`Stage`]: a fake grid, N viewers each logged in as
//! its own account, and the grid-control handle — set up, handed to a test
//! body, and taken down again whatever the body did.

use core::time::Duration;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use bevy::math::UVec2;
use serde_json::json;
use sl_automation_proto::Probe;
use sl_client_bevy::{LoginParams, LoginRequest, StartLocation};
use sl_client_bevy_viewer::assembly::{
    Automation, LoginOutcome, MediaRuntime, SkinSelection, Storage, ViewerApp, ViewerAppBuilder,
    ViewerAppOptions, ViewerPaths, WindowMode,
};
use sl_fake_grid::fixtures::scenarios;
use sl_fake_grid::{AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig};
use sl_proto::{AgentKey, RegionCoordinates};
use sl_repl::LoginCooldown;
use sl_viewer_automation::{BuildError, InProcessHost, ViewerHandle};
use sl_viewer_driver::{Viewer, ViewerOptions};
use sl_viewer_launch::{Ending, LOGOUT_GRACE, Launch, RunningViewer, ViewerDir};
use sl_viewer_world_avatar::avatar_overrides::AvatarOverrides;
use sl_viewer_world_scene::render_overrides::RenderOverrides;
use sl_viewer_world_view::session::TerminationFlag;

use crate::backend::Backend;
use crate::error::{BodyError, StageError};
use crate::grid::Grid;
use crate::live::{LiveAccount, LiveAccounts};
use crate::logs::RouteGuard;
use crate::need::{Need, unmet};

/// Every stage account's first name; a viewer's label is its last name.
pub const FIRST_NAME: &str = "Stage";

/// The password every stage account shares on the fake grid.
const PASSWORD: &str = "password";

/// The `[avatars.<key>]` the credentials file a process viewer on the fake
/// grid is handed names.
const AVATAR_KEY: &str = "stage";

/// The off-screen window every stage viewer renders into, on both backends.
const WINDOW: UVec2 = UVec2::new(1280, 720);

/// How long a viewer may take to log in and settle: a cold shader cache in a
/// debug build, beside other viewers, or a live region's content arriving.
const LOGIN: Duration = Duration::from_secs(240);

/// How long a viewer on a live grid may take to settle once it has arrived:
/// a live asset service answers some fetches 503 for minutes, and each such
/// texture walks its whole retry chain — six attempts, each retried inside the
/// store — before the scene is quiet (seen on aditi, 2026-09-30).
const LIVE_SETTLE: Duration = Duration::from_secs(600);

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
    /// What the test needs of the grid.
    needs: Vec<Need>,
    /// The grid, when not `SL_E2E_GRID`'s.
    on: Option<Grid>,
    /// The labels whose accounts hold estate powers on the fake grid.
    estate_managers: Vec<String>,
    /// How every viewer is set up beyond its login.
    setup: ViewerSetup,
}

/// How a stage viewer is set up beyond its login, on either backend — the
/// command-line switches a test may turn, and nothing it may not.
#[derive(Debug, Clone, Default)]
struct ViewerSetup {
    /// Whether the web (CEF) engine may start: off unless a test asks, since
    /// Chromium is a second process tree per viewer.
    web_media: bool,
    /// The skin to wear for the run, overriding the stored choice as
    /// `--skin` does.
    skin: Option<String>,
    /// Whether each viewer runs on a copy of the asset tree of its own and
    /// watches its skin sheets for edits, as `--watch-skins` does.
    watch_skins: bool,
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
            .field("needs", &self.needs)
            .field("on", &self.on)
            .field("estate_managers", &self.estate_managers)
            .field("setup", &self.setup)
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
            needs: Vec::new(),
            on: None,
            estate_managers: Vec::new(),
            setup: ViewerSetup::default(),
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
    /// viewers start in the first region. Only a fake grid can be told its
    /// regions, so the stage skips a live one.
    #[must_use]
    pub fn region(mut self, region: RegionConfig) -> Self {
        self.regions.push(region);
        self.dictates("it names its regions")
    }

    /// Where in the first region the viewers start. The stage skips a live
    /// grid.
    #[must_use]
    pub fn start_position(mut self, position: RegionCoordinates) -> Self {
        self.start = position;
        self.dictates("it sets where its viewers start")
    }

    /// Configure the grid further — a scenario, a timeline, a deterministic
    /// seed, login gates. Called on each backend's fresh grid; the stage skips
    /// a live one.
    #[must_use]
    pub fn configure_grid(
        mut self,
        hook: impl Fn(FakeGridBuilder) -> FakeGridBuilder + Send + Sync + 'static,
    ) -> Self {
        self.grid = Some(Arc::new(hook));
        self.dictates("it configures the grid")
    }

    /// Viewer `label`'s account holds estate powers over the grid's regions —
    /// what the Region / Estate window's write controls ask for. The stage
    /// skips a live grid.
    #[must_use]
    pub fn estate_manager(mut self, label: impl Into<String>) -> Self {
        self.estate_managers.push(label.into());
        self.dictates("it makes an account an estate manager")
    }

    /// Let every viewer start the web (CEF) engine, which a stage viewer
    /// otherwise runs without: the search window's web tab, the web browser
    /// window and media on a prim load nothing until a test asks for it.
    #[must_use]
    pub const fn web_media(mut self) -> Self {
        self.setup.web_media = true;
        self
    }

    /// Start every viewer wearing the skin `skin` (`graphite`, `azure`,
    /// `vintage`) for the run, as `--skin` does — over whatever the viewer has
    /// stored, and without storing it.
    #[must_use]
    pub fn skin(mut self, skin: impl Into<String>) -> Self {
        self.setup.skin = Some(skin.into());
        self
    }

    /// Run every viewer on a copy of the viewer's asset tree of its own
    /// ([`Stage::assets`]) and watch its skin sheets, as `--watch-skins`
    /// does: a sheet the body edits in the copy re-dresses that viewer
    /// without a restart, and the workspace's own tree is never touched.
    #[must_use]
    pub const fn watch_skins(mut self) -> Self {
        self.setup.watch_skins = true;
        self
    }

    /// Record that the stage dictates the grid, `how` — once.
    fn dictates(self, how: &'static str) -> Self {
        if self
            .needs
            .iter()
            .any(|need| matches!(need, Need::DictatedGrid(_)))
        {
            self
        } else {
            self.needs(Need::DictatedGrid(how))
        }
    }

    /// The test needs `need`; on a grid that cannot provide it, the stage
    /// skips, saying why.
    #[must_use]
    pub fn needs(mut self, need: Need) -> Self {
        if !self.needs.contains(&need) {
            self.needs.push(need);
        }
        self
    }

    /// Run on `grid`, whatever `SL_E2E_GRID` says.
    #[must_use]
    pub const fn on_grid(mut self, grid: Grid) -> Self {
        self.on = Some(grid);
        self
    }

    /// The grid the stage runs on: the one given, else `SL_E2E_GRID`'s.
    fn chosen_grid(&self) -> Result<Grid, StageError> {
        self.on.map_or_else(Grid::from_env, Ok)
    }

    /// Why the stage would skip on its grid, or `None` when it would run: a
    /// need the grid cannot provide, or — on a live grid — fewer accounts
    /// than viewers. A need is judged before the credentials file is read,
    /// so a test the grid cannot serve skips without one.
    ///
    /// # Errors
    ///
    /// [`StageError::GridVariable`] for a bad `SL_E2E_GRID`, and on a live
    /// grid whose needs it meets, as reading its accounts
    /// ([`StageError::Credentials`]).
    pub fn skip_reason(&self) -> Result<Option<String>, StageError> {
        let grid = self.chosen_grid()?;
        Ok(self.plan(grid)?.err())
    }

    /// The accounts of `grid` (none on the fake grid), or why the stage
    /// skips there.
    fn plan(&self, grid: Grid) -> Result<Result<Option<LiveAccounts>, String>, StageError> {
        let viewers = self.labels.len();
        if let Some(reason) = unmet(&self.needs, viewers, grid, None) {
            return Ok(Err(reason));
        }
        if !grid.is_live() {
            return Ok(Ok(None));
        }
        let live = LiveAccounts::from_env(grid)?;
        Ok(
            match unmet(&self.needs, viewers, grid, Some(live.accounts.len())) {
                Some(reason) => Err(reason),
                None => Ok(Some(live)),
            },
        )
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
    /// The grid is `SL_E2E_GRID`'s (unset: a fresh fake grid per backend).
    /// A grid that cannot meet the test's needs ([`skip_reason`](Self::skip_reason))
    /// and a machine with no GPU adapter skip: the stage logs a warning, which
    /// the stage's subscriber also prints on standard error, and returns
    /// `Ok`.
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
        let grid = self.chosen_grid()?;
        let live = match self.plan(grid)? {
            Ok(live) => live,
            Err(reason) => {
                tracing::warn!("skipping {} on the {grid} grid: {reason}", self.name);
                return Ok(());
            }
        };
        if !crate::gpu::adapter_available() {
            tracing::warn!("skipping {}: this machine has no GPU adapter", self.name);
            return Ok(());
        }
        let root = self.artifact_root()?.join(&self.name).join(grid.name());
        for backend in backends {
            self.run_on(
                grid,
                live.as_ref(),
                backend,
                &root.join(backend.name()),
                &body,
            )?;
        }
        Ok(())
    }

    /// One backend's run: start, body, teardown.
    #[expect(
        clippy::panic,
        reason = "a test body that panicked is resumed as a panic; when the teardown failed too, \
                  the panic has to carry both"
    )]
    fn run_on<F>(
        &self,
        grid: Grid,
        live: Option<&LiveAccounts>,
        backend: Backend,
        dir: &Path,
        body: &F,
    ) -> Result<(), StageError>
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
            "stage {} on the {grid} grid, {backend} backend, in {}",
            self.name,
            dir.display()
        );
        let stage = runtime.block_on(Stage::start(self, grid, live, backend, dir))?;
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

/// The directory a viewer's own copy of the asset tree sits in — what
/// `BEVY_ASSET_ROOT` names — within its directory.
fn asset_base(viewer: &Path) -> PathBuf {
    viewer.join("asset-base")
}

/// Copy the directory tree `from` to `to`, which must not exist yet.
fn copy_tree(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    fs_err::create_dir(to)?;
    for entry in fs_err::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            let _bytes = fs_err::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Give the viewer whose directory is `viewer` a copy of the asset tree the
/// viewer would otherwise read, under [`asset_base`].
fn copy_assets(viewer: &Path) -> Result<(), StageError> {
    let base = asset_base(viewer);
    let fail = |source| StageError::Artifacts {
        path: base.clone(),
        source,
    };
    fs_err::create_dir_all(&base).map_err(fail)?;
    copy_tree(
        &sl_client_bevy_viewer::asset_root::resolved_assets_dir(),
        &base.join("assets"),
    )
    .map_err(fail)
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

/// Start a fresh fake grid for `builder`: its regions, an account per
/// viewer, and its further configuration. Answers the grid and the region
/// the viewers start in.
async fn start_fake_grid(builder: &StageBuilder) -> Result<(FakeGrid, String), StageError> {
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
        let account = AccountConfig::new(FIRST_NAME, label, PASSWORD);
        grid = grid.account(if builder.estate_managers.contains(label) {
            account.estate_manager()
        } else {
            account
        });
    }
    if let Some(hook) = &builder.grid {
        grid = hook(grid);
    }
    let grid = grid.start().await?;
    tracing::info!("stage grid at {}", grid.login_uri());
    Ok((grid, home))
}

/// Write the credentials file a process viewer on the fake grid logs in
/// with, into its directory, and answer its path.
fn write_stage_credentials(
    dir: &ViewerDir,
    label: &str,
    login_uri: &str,
) -> Result<PathBuf, StageError> {
    let credentials = dir.root.join("credentials.toml");
    fs_err::write(
        &credentials,
        format!(
            "# Written by sl-e2e for one stage against a fake grid.\n\
             default_avatar = \"{AVATAR_KEY}\"\n\n[avatars.{AVATAR_KEY}]\nfirst = \
             \"{FIRST_NAME}\"\nlast = \"{label}\"\npassword = \"{PASSWORD}\"\nlogin_uri = \
             \"{login_uri}\"\n"
        ),
    )
    .map_err(|source| StageError::Artifacts {
        path: credentials.clone(),
        source,
    })?;
    Ok(credentials)
}

/// Who a stage viewer logs in as.
#[derive(Debug, Clone)]
enum Login {
    /// The fake grid's account `Stage <label>`.
    Stage {
        /// Its agent.
        agent: AgentKey,
        /// The grid's login URI.
        login_uri: String,
    },
    /// A live grid's account.
    Live(LiveAccount),
}

impl Login {
    /// The account's `First Last` name, for the viewer `label`.
    fn account_name(&self, label: &str) -> String {
        match self {
            Self::Stage { .. } => format!("{FIRST_NAME} {label}"),
            Self::Live(account) => account.name(),
        }
    }

    /// The account's agent, when it is known before the login.
    const fn agent(&self) -> Option<AgentKey> {
        match self {
            Self::Stage { agent, .. } => Some(*agent),
            Self::Live(_) => None,
        }
    }
}

/// How one stage viewer runs.
#[derive(Debug)]
enum ViewerRun {
    /// An App on the stage's in-process host; `None` before it is hosted and
    /// once it has exited.
    InProcess(Option<ViewerHandle>),
    /// A process; `None` before it is spawned and once it has been stopped.
    Process(Option<RunningViewer>),
}

impl ViewerRun {
    /// Nothing running yet, on `backend`.
    const fn idle(backend: Backend) -> Self {
        match backend {
            Backend::InProcess => Self::InProcess(None),
            Backend::Process => Self::Process(None),
        }
    }
}

/// One stage viewer.
///
/// What a relog replaces — the driver's handle and what runs — sits behind a
/// lock, since a relog happens while the body holds the stage shared.
#[derive(Debug)]
struct StageViewer {
    /// Its label.
    label: String,
    /// Its place among the stage's viewers: which live account it takes, and
    /// its socket's number.
    index: usize,
    /// Its account's `First Last` name.
    account: String,
    /// Its account's agent: on the fake grid from the start, on a live grid
    /// once it has logged in.
    agent: Option<AgentKey>,
    /// Its artifact directory.
    dir: PathBuf,
    /// The driver's handle on its current session, once connected.
    driver: Mutex<Option<Viewer>>,
    /// How its current session runs.
    run: Mutex<ViewerRun>,
    /// How many sessions it has started: the first is 0, each relog the
    /// next.
    sessions: AtomicU32,
    /// Whether the body made it quit by itself ([`Stage::expect_quit`]), so
    /// the teardown takes its own exit as its logout rather than asking for
    /// one.
    quits: AtomicBool,
}

impl StageViewer {
    /// The driver's handle on its current session.
    fn driver(&self) -> Result<Viewer, StageError> {
        lock(&self.driver)
            .clone()
            .ok_or_else(|| StageError::UnknownViewer(self.label.clone()))
    }

    /// Make `driver` the handle on its current session.
    fn connected(&self, driver: Viewer) {
        *lock(&self.driver) = Some(driver);
    }

    /// Make `run` its current session.
    fn running(&self, run: ViewerRun) {
        *lock(&self.run) = run;
    }
}

/// `mutex`'s contents, also after a panic while it was held: what it guards
/// is replaced whole, never left half-written.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running stage: the grid and its viewers, each logged in.
#[derive(Debug)]
pub struct Stage {
    /// The test's name.
    name: String,
    /// The backend.
    backend: Backend,
    /// The grid the viewers are on.
    on: Grid,
    /// The stage's artifact directory.
    dir: PathBuf,
    /// The fake grid, when the stage started one.
    grid: Option<FakeGrid>,
    /// A live grid's accounts, in the order the viewers take them.
    live: Option<LiveAccounts>,
    /// The region the viewers start in.
    home: String,
    /// Where every viewer logs in to, a relog included.
    start: StartLocation,
    /// The viewer binary, for the process backend.
    binary: Option<PathBuf>,
    /// How every viewer is set up beyond its login.
    setup: ViewerSetup,
    /// The viewers, in the order they were named.
    viewers: Vec<StageViewer>,
    /// The in-process host, on that backend.
    host: Option<InProcessHost<ViewerApp>>,
    /// Where the process backend's automation sockets are, once one is.
    sockets: OnceLock<PathBuf>,
}

impl Stage {
    /// Start the grid (unless it is live) and every viewer, and wait until
    /// each has logged in and settled. What was started is taken down again
    /// when a step fails.
    async fn start(
        builder: &StageBuilder,
        on: Grid,
        live: Option<&LiveAccounts>,
        backend: Backend,
        dir: &Path,
    ) -> Result<Self, StageError> {
        let (grid, home) = match live {
            Some(_) => (None, String::new()),
            None => {
                let (grid, home) = start_fake_grid(builder).await?;
                (Some(grid), home)
            }
        };
        let start = match live {
            Some(live) => live.start.clone(),
            None => StartLocation::region(home.clone(), builder.start),
        };
        let mut stage = Self {
            name: builder.name.clone(),
            backend,
            on,
            dir: dir.to_path_buf(),
            grid,
            live: live.cloned(),
            home,
            start,
            binary: builder.binary.clone(),
            setup: builder.setup.clone(),
            viewers: Vec::new(),
            host: None,
            sockets: OnceLock::new(),
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
    /// each, and wait until all have logged in and settled — on the fake
    /// grid in its home region, on a live grid wherever the grid put them.
    async fn launch(&mut self, builder: &StageBuilder) -> Result<(), StageError> {
        if self.backend == Backend::InProcess {
            self.host = Some(InProcessHost::<ViewerApp>::start()?);
        }
        for (index, label) in builder.labels.iter().enumerate() {
            let login = self.login_of(index, label)?;
            self.viewers.push(StageViewer {
                label: label.clone(),
                index,
                account: login.account_name(label),
                agent: login.agent(),
                dir: self.dir.join(label),
                driver: Mutex::new(None),
                run: Mutex::new(ViewerRun::idle(self.backend)),
                sessions: AtomicU32::new(0),
                quits: AtomicBool::new(false),
            });
            if let Some(viewer) = self.viewers.last() {
                if self.setup.watch_skins {
                    copy_assets(&viewer.dir)?;
                }
                self.start_session(viewer).await?;
            }
        }
        let mut arrivals = tokio::task::JoinSet::new();
        for viewer in &self.viewers {
            let _task = arrivals.spawn(arrive(viewer.driver()?, self.arrival(), self.settle()));
        }
        while let Some(joined) = arrivals.join_next().await {
            match joined {
                Ok(arrived) => arrived?,
                Err(join) => std::panic::resume_unwind(join.into_panic()),
            }
        }
        // A live grid's agents and region are known only now.
        for viewer in &mut self.viewers {
            if viewer.agent.is_some() && !self.home.is_empty() {
                continue;
            }
            let driver = viewer.driver()?;
            let readout = driver.agent().await.map_err(|source| StageError::Driver {
                viewer: viewer.label.clone(),
                source,
            })?;
            if viewer.agent.is_none() {
                viewer.agent = readout.agent_id.map(AgentKey::from);
            }
            if self.home.is_empty()
                && let Some(region) = readout.region.and_then(|region| region.name)
            {
                self.home = region;
            }
        }
        Ok(())
    }

    /// The region a viewer that logs in waits to arrive in: the fake grid's
    /// home region, and on a live grid whichever the grid puts it in.
    fn arrival(&self) -> Option<String> {
        (!self.on.is_live() && !self.home.is_empty()).then(|| self.home.clone())
    }

    /// How long a viewer that has arrived may take to settle.
    const fn settle(&self) -> Duration {
        if self.on.is_live() {
            LIVE_SETTLE
        } else {
            LOGIN
        }
    }

    /// Start a session of `viewer` on the stage's backend, logged in as its
    /// account, and connect the driver to it. Each step's result is kept on
    /// `viewer` as it is made, so a teardown after a failed step stops what
    /// was started.
    async fn start_session(&self, viewer: &StageViewer) -> Result<(), StageError> {
        let login = self.login_of(viewer.index, &viewer.label)?;
        let session = viewer.sessions.fetch_add(1, Ordering::Relaxed);
        match self.backend {
            Backend::InProcess => self.start_in_process(viewer, &login).await,
            Backend::Process => self.start_process(viewer, &login, session).await,
        }
    }

    /// Who viewer `index` (labelled `label`) logs in as: its account on a
    /// live grid, else the stage account `Stage <label>`.
    fn login_of(&self, index: usize, label: &str) -> Result<Login, StageError> {
        match &self.live {
            Some(live) => {
                let account = live
                    .accounts
                    .get(index)
                    .ok_or_else(|| StageError::NoAccount(label.to_owned()))?;
                Ok(Login::Live(account.clone()))
            }
            None => Ok(Login::Stage {
                agent: self.agent_of(label)?,
                login_uri: self.fake()?.login_uri().to_string(),
            }),
        }
    }

    /// Wait out, then take, `login`'s turn under the shared login cooldown,
    /// on a grid that has one.
    async fn take_login_turn(&self, login: &Login) -> Result<(), StageError> {
        let Login::Live(account) = login else {
            return Ok(());
        };
        if !self.on.needs_cooldown() {
            return Ok(());
        }
        let cooldown = LoginCooldown::shared()?;
        let name = account.name();
        let wait = cooldown.wait_time(&name);
        if !wait.is_zero() {
            tracing::info!(
                "waiting {} s out the {} login cooldown of avatar {}",
                wait.as_secs(),
                self.on,
                account.key
            );
            tokio::time::sleep(wait).await;
        }
        cooldown.stamp(&name)?;
        Ok(())
    }

    /// The name an in-process viewer's log span carries
    /// ([`StageBuilder::log_label`]).
    fn log_label(&self, label: &str) -> String {
        format!("{}/{}/{label}", self.name, self.backend)
    }

    /// The in-process backend: `viewer` an App on the stage's host, storing
    /// under its directory's `state/` — the same tree every session of it
    /// reads back.
    async fn start_in_process(
        &self,
        viewer: &StageViewer,
        login: &Login,
    ) -> Result<(), StageError> {
        let label = &viewer.label;
        let dir = &viewer.dir;
        self.take_login_turn(login).await?;
        let (first, last, password, login_uri) = match login {
            Login::Live(account) => (
                account.avatar.first().to_owned(),
                account.avatar.last().to_owned(),
                account.avatar.password().expose().to_owned(),
                account.login_uri.clone(),
            ),
            Login::Stage { login_uri, .. } => (
                FIRST_NAME.to_owned(),
                label.clone(),
                PASSWORD.to_owned(),
                login_uri.clone(),
            ),
        };
        let mut request =
            LoginRequest::new(first, last, password, self.start.clone(), "sl-e2e", "0.0");
        // A grid that asks for a second factor ends the App with the
        // challenge: answer it, and log in again with a new App.
        loop {
            let params = LoginParams {
                login_uri: login_uri.parse().map_err(|error| StageError::Login {
                    viewer: label.clone(),
                    reason: format!("the login URI {login_uri}: {error}"),
                })?,
                request: request.clone(),
            };
            let log_label = self.log_label(label);
            let state = dir.join("state");
            let assets = self.setup.watch_skins.then(|| asset_base(dir));
            let setup = self.setup.clone();
            let host = self
                .host
                .as_ref()
                .ok_or(sl_viewer_automation::HostError::Stopped)?;
            let (handle, link) = host
                .host(label.clone(), move || {
                    in_process_viewer(params, log_label, &state, assets, &setup)
                })
                .await?;
            viewer.running(ViewerRun::InProcess(Some(handle)));
            let driver =
                Viewer::over_link(link.requests, link.messages, driver_options(label, dir))
                    .await
                    .map_err(|source| StageError::Driver {
                        viewer: label.clone(),
                        source,
                    })?;
            viewer.connected(driver.clone());
            let logged_in = driver
                .expect_state(Probe::Agent)
                .at("/agent_id")
                .timeout(LOGIN)
                .to_be_present();
            tokio::select! {
                logged_in = logged_in => {
                    let _held = logged_in.map_err(|source| StageError::Driver {
                        viewer: label.clone(),
                        source,
                    })?;
                    return Ok(());
                }
                exited = host.exited(handle) => exited?,
            }
            viewer.running(ViewerRun::InProcess(None));
            let outcome = host
                .with_app(handle, |viewer| {
                    viewer
                        .app_mut()
                        .world_mut()
                        .remove_resource::<LoginOutcome>()
                })
                .await?
                .unwrap_or_default();
            let challenge = match (outcome.challenge, outcome.rejected) {
                (Some(challenge), _) => challenge,
                (None, Some(rejected)) => {
                    return Err(StageError::Login {
                        viewer: label.clone(),
                        reason: format!("{} ({})", rejected.reason, rejected.message),
                    });
                }
                (None, None) => {
                    return Err(StageError::Login {
                        viewer: label.clone(),
                        reason: "it exited before it logged in".to_owned(),
                    });
                }
            };
            let Login::Live(account) = login else {
                return Err(StageError::Login {
                    viewer: label.clone(),
                    reason: "the fake grid asked for a second factor".to_owned(),
                });
            };
            tracing::info!("viewer {label}: the grid asks for a second factor");
            let avatar = account.avatar.clone();
            let token = tokio::task::spawn_blocking(move || avatar.acquire_mfa())
                .await
                .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()))
                .map_err(|error| StageError::Login {
                    viewer: label.clone(),
                    reason: error.to_string(),
                })?
                .ok_or_else(|| StageError::Login {
                    viewer: label.clone(),
                    reason: format!(
                        "the grid asks for a second factor and avatar {} has no mfa_command",
                        account.key
                    ),
                })?;
            request = request.with_mfa(token.expose(), challenge.mfa_hash);
        }
    }

    /// The process backend: `viewer` the real binary, headless, behind an
    /// automation socket, confined to its directory — the same tree every
    /// session of it reads back. Session `session`'s output goes to
    /// `viewer.log` for the first and `viewer.<session>.log` after a relog.
    async fn start_process(
        &self,
        viewer: &StageViewer,
        login: &Login,
        session: u32,
    ) -> Result<(), StageError> {
        let label = &viewer.label;
        let binary = self.binary.clone().ok_or(StageError::NoBinary)?;
        let sockets = self.socket_dir()?;
        let dir = ViewerDir {
            root: viewer.dir.clone(),
        };
        // A live account's own file and key; a stage account's written
        // here. The viewer answers a second-factor challenge itself.
        let (credentials, avatar_key, login_uri) = match login {
            Login::Live(account) => (
                self.live
                    .as_ref()
                    .map(|live| live.file.clone())
                    .ok_or_else(|| StageError::NoAccount(label.clone()))?,
                account.key.clone(),
                account.login_uri.clone(),
            ),
            Login::Stage { login_uri, .. } => (
                write_stage_credentials(&dir, label, login_uri)?,
                AVATAR_KEY.to_owned(),
                login_uri.clone(),
            ),
        };
        self.take_login_turn(login).await?;
        let socket = sockets.join(format!("{}.{session}.sock", viewer.index));
        let mut launch = Launch::in_dir(label.clone(), &binary, &dir).args([
            "--credentials".to_owned(),
            credentials.display().to_string(),
            "--avatar".to_owned(),
            avatar_key,
            "--login-uri".to_owned(),
            login_uri,
            "--start".to_owned(),
            self.start.to_wire_string(),
            "--headless".to_owned(),
            "--capture-size".to_owned(),
            format!("{}x{}", WINDOW.x, WINDOW.y),
            "--automation-socket".to_owned(),
            socket.display().to_string(),
        ]);
        if !self.setup.web_media {
            launch = launch.args(["--disable-web-media".to_owned()]);
        }
        if let Some(skin) = &self.setup.skin {
            launch = launch.args(["--skin".to_owned(), skin.clone()]);
        }
        if self.setup.watch_skins {
            launch = launch
                .env(
                    "BEVY_ASSET_ROOT",
                    asset_base(&viewer.dir).display().to_string(),
                )
                .args(["--watch-skins".to_owned()]);
        }
        if session > 0 {
            launch.log = dir.root.join(format!("viewer.{session}.log"));
        }
        let running = RunningViewer::spawn(&launch).map_err(|source| StageError::Launch {
            viewer: label.clone(),
            source,
        })?;
        viewer.running(ViewerRun::Process(Some(running)));
        let driver = connect(viewer, &socket, &launch.log).await?;
        viewer.connected(driver);
        Ok(())
    }

    /// The process backend's socket directory, made on first use. Short: a
    /// socket path must stay under about a hundred bytes, and the artifact
    /// directory is deep.
    fn socket_dir(&self) -> Result<PathBuf, StageError> {
        let sockets = self.sockets.get_or_init(|| {
            std::env::temp_dir().join(format!(
                "sl-e2e-{}-{}",
                std::process::id(),
                NEXT_SOCKET_DIR.fetch_add(1, Ordering::Relaxed)
            ))
        });
        fs_err::create_dir_all(sockets).map_err(|source| StageError::Artifacts {
            path: sockets.clone(),
            source,
        })?;
        Ok(sockets.clone())
    }

    /// Log viewer `label` out and back in, keeping its directories — its
    /// settings, its per-account files, its caches — as a person quitting
    /// and starting the viewer again would; answers the driver's handle on
    /// the new session, which [`viewer`](Self::viewer) answers from now on.
    ///
    /// The viewer is asked to log out as the teardown asks it (its
    /// termination flag in process, `SIGTERM` to a process) and must exit
    /// having done so; on the fake grid, its session must then be gone from
    /// the grid. The new session logs in where the stage's viewers start —
    /// on a live grid after its turn under the login cooldown — and is waited
    /// for as the stage's start waits: arrived, and quiet.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`]; [`StageError::NoLogout`] when it does
    /// not log out, [`StageError::Stranded`] when the grid keeps its session,
    /// and as the stage's start when the new session does not come up.
    pub async fn relog(&self, label: &str) -> Result<Viewer, StageError> {
        let viewer = self.stage_viewer(label)?;
        tracing::info!("stage {}: relogging viewer {label}", self.name);
        self.log_out(viewer).await?;
        self.start_session(viewer).await?;
        let driver = viewer.driver()?;
        arrive(driver.clone(), self.arrival(), self.settle()).await?;
        tracing::info!("stage {}: viewer {label} is back", self.name);
        Ok(driver)
    }

    /// Ask `viewer` to log out, wait until it has exited, and — on the fake
    /// grid — until the grid no longer holds its session.
    async fn log_out(&self, viewer: &StageViewer) -> Result<(), StageError> {
        let label = &viewer.label;
        drop(lock(&viewer.driver).take());
        let run = core::mem::replace(&mut *lock(&viewer.run), ViewerRun::idle(self.backend));
        match run {
            ViewerRun::InProcess(Some(handle)) => {
                let host = self
                    .host
                    .as_ref()
                    .ok_or(sl_viewer_automation::HostError::Stopped)?;
                ask_to_log_out(host, label, handle).await?;
                match tokio::time::timeout(LOGOUT, host.exited(handle)).await {
                    Ok(exited) => exited?,
                    Err(_elapsed) => {
                        return Err(StageError::NoLogout {
                            viewer: label.clone(),
                            reason: format!(
                                "it had not exited {} s after being asked",
                                LOGOUT.as_secs()
                            ),
                        });
                    }
                }
            }
            ViewerRun::Process(Some(running)) => {
                let ran = tokio::task::spawn_blocking(move || running.stop(LOGOUT_GRACE))
                    .await
                    .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()))
                    .map_err(|source| StageError::Launch {
                        viewer: label.clone(),
                        source,
                    })?;
                if ran.ending != Ending::AskedToQuit {
                    return Err(StageError::NoLogout {
                        viewer: label.clone(),
                        reason: format!("it ended without logging out ({:?})", ran.ending),
                    });
                }
            }
            ViewerRun::InProcess(None) | ViewerRun::Process(None) => {
                return Err(StageError::NoLogout {
                    viewer: label.clone(),
                    reason: "it is not running".to_owned(),
                });
            }
        }
        let (Some(grid), Some(agent)) = (&self.grid, viewer.agent) else {
            return Ok(());
        };
        let left = self.sessions_left(grid, Some(agent)).await;
        if left.is_empty() {
            Ok(())
        } else {
            Err(StageError::Stranded(left))
        }
    }

    /// The sessions `grid` still holds open — `agent`'s, or everybody's — as
    /// `<viewer> in <region>`. A session the logout closed is not one of them,
    /// though it stays in the grid's table a moment longer, until its tasks
    /// have wound down; one that is open has had no logout. Looked at once,
    /// never waited on: a killed viewer's session closes by itself when the
    /// grid's inactivity timer runs out, and a wait would let it.
    async fn sessions_left(&self, grid: &FakeGrid, agent: Option<AgentKey>) -> Vec<String> {
        let mut left = Vec::new();
        for region in grid.region_names() {
            for session in grid.sessions_in(&region).await {
                let held = session.agent_id();
                if session.is_closed() || agent.is_some_and(|agent| agent != held) {
                    continue;
                }
                let who = self
                    .viewers
                    .iter()
                    .find(|viewer| viewer.agent == Some(held))
                    .map_or_else(|| format!("{held:?}"), |viewer| viewer.label.clone());
                left.push(format!("{who} in {region}"));
            }
        }
        left
    }

    /// The agent of the fake grid's account `Stage <label>`.
    fn agent_of(&self, label: &str) -> Result<AgentKey, StageError> {
        self.fake()?
            .account_agent_id(FIRST_NAME, label)
            .ok_or_else(|| StageError::NoAccount(label.to_owned()))
    }

    /// The fake grid, or [`StageError::NoGridControl`] on a live one.
    fn fake(&self) -> Result<&FakeGrid, StageError> {
        self.grid.as_ref().ok_or(StageError::NoGridControl(self.on))
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

    /// The grid the viewers are on.
    #[must_use]
    pub const fn on_grid(&self) -> Grid {
        self.on
    }

    /// The grid, to drive from the grid side: teleports, crossings, the
    /// sessions in a region — grid control, which a test that uses it
    /// declares ([`Need::GridControl`]).
    ///
    /// # Errors
    ///
    /// [`StageError::NoGridControl`] on a live grid.
    pub fn grid(&self) -> Result<&FakeGrid, StageError> {
        self.fake()
    }

    /// The region the viewers started in: on a live grid, the one the first
    /// viewer arrived in.
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

    /// The driver's handle on viewer `label`'s current session — after a
    /// [`relog`](Self::relog), the new one's.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn viewer(&self, label: &str) -> Result<Viewer, StageError> {
        self.stage_viewer(label)?.driver()
    }

    /// The agent viewer `label` is logged in as.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given, and
    /// [`StageError::NoAccount`] when its agent is not known.
    pub fn agent_id(&self, label: &str) -> Result<AgentKey, StageError> {
        self.stage_viewer(label)?
            .agent
            .ok_or_else(|| StageError::NoAccount(label.to_owned()))
    }

    /// The `First Last` name of the account viewer `label` is logged in as:
    /// `Stage <label>` on the fake grid, the credentials file's avatar on a
    /// live one — who other viewers hear it as.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn account_name(&self, label: &str) -> Result<&str, StageError> {
        Ok(&self.stage_viewer(label)?.account)
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

    /// The `assets/` directory viewer `label` runs on — its own copy, on a
    /// stage that [watches skins](StageBuilder::watch_skins): a sheet edited
    /// under `skins/` there re-dresses that viewer alone.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given, and
    /// [`StageError::NoOwnAssets`] on a stage whose viewers run on the
    /// workspace's tree.
    pub fn assets(&self, label: &str) -> Result<PathBuf, StageError> {
        let viewer = self.stage_viewer(label)?;
        if self.setup.watch_skins {
            Ok(asset_base(&viewer.dir).join("assets"))
        } else {
            Err(StageError::NoOwnAssets(label.to_owned()))
        }
    }

    /// Viewer `label`'s process id, on the process backend.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`] for a label the stage was not given.
    pub fn pid(&self, label: &str) -> Result<Option<u32>, StageError> {
        Ok(match &*lock(&self.stage_viewer(label)?.run) {
            ViewerRun::Process(Some(running)) => Some(running.pid()),
            ViewerRun::Process(None) | ViewerRun::InProcess(_) => None,
        })
    }

    /// The grid's session of viewer `label` in the region it is in now: the
    /// grid side of its conversation.
    ///
    /// # Errors
    ///
    /// [`StageError::NoGridControl`] on a live grid,
    /// [`StageError::UnknownViewer`] for a label the stage was not given,
    /// [`StageError::Driver`] when its region cannot be read, and
    /// [`StageError::NoAccount`] when the grid holds no session of it there.
    pub async fn agent(&self, label: &str) -> Result<FakeAgent, StageError> {
        let grid = self.fake()?;
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
        grid.sessions_in(&region)
            .await
            .into_iter()
            .find(|session| session.agent_id() == agent)
            .ok_or_else(|| StageError::NoAccount(label.to_owned()))
    }

    /// Say that the body is about to make viewer `label` quit by itself — a
    /// Quit chord, a menu's Quit — so the teardown takes its exit as its
    /// logout instead of asking for one, and holds it to having exited
    /// cleanly: a process with status 0, and on the fake grid no session left
    /// behind. Call it before the quit, since a viewer that exits unannounced
    /// is a viewer that died.
    ///
    /// # Errors
    ///
    /// [`StageError::UnknownViewer`].
    pub fn expect_quit(&self, label: &str) -> Result<(), StageError> {
        self.viewers
            .iter()
            .find(|viewer| viewer.label == label)
            .map(|viewer| viewer.quits.store(true, Ordering::Relaxed))
            .ok_or_else(|| StageError::UnknownViewer(label.to_owned()))
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
    /// [`StageError::NoGridControl`] on a live grid, which sends no markers;
    /// [`StageError::UnknownViewer`], or [`StageError::Driver`] when it does
    /// not arrive.
    pub async fn wait_marker(
        &self,
        label: &str,
        name: &str,
        timeout: Duration,
    ) -> Result<(), StageError> {
        let _grid = self.fake()?;
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
    /// has exited, then — on the fake grid — check that the grid holds no
    /// session, and stop it. Each step runs even when an earlier one failed; the first
    /// failure is returned and the others logged.
    async fn shutdown(mut self) -> Result<(), StageError> {
        let mut problems = Vec::new();
        for viewer in &self.viewers {
            drop(lock(&viewer.driver).take());
        }
        if let Some(host) = self.host.take() {
            let handles: Vec<(String, ViewerHandle, bool)> = self
                .viewers
                .iter()
                .filter_map(|viewer| match *lock(&viewer.run) {
                    ViewerRun::InProcess(Some(handle)) => Some((
                        viewer.label.clone(),
                        handle,
                        viewer.quits.load(Ordering::Relaxed),
                    )),
                    ViewerRun::InProcess(None) | ViewerRun::Process(_) => None,
                })
                .collect();
            for (label, handle, _quits) in handles.iter().filter(|(_, _, quits)| !quits) {
                if let Err(error) = ask_to_log_out(&host, label, *handle).await {
                    problems.push(error);
                }
            }
            for (label, handle, _quits) in &handles {
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
        let processes: Vec<((String, bool), RunningViewer)> = self
            .viewers
            .iter_mut()
            .filter_map(|viewer| {
                match viewer.run.get_mut().unwrap_or_else(PoisonError::into_inner) {
                    ViewerRun::Process(running) => running.take().map(|running| {
                        (
                            (viewer.label.clone(), viewer.quits.load(Ordering::Relaxed)),
                            running,
                        )
                    }),
                    ViewerRun::InProcess(_) => None,
                }
            })
            .collect();
        if !processes.is_empty() {
            let (labels, running): (Vec<(String, bool)>, Vec<RunningViewer>) =
                processes.into_iter().unzip();
            let stopped = tokio::task::spawn_blocking(move || {
                sl_viewer_launch::stop_all(running, LOGOUT_GRACE)
            })
            .await
            .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()));
            for ((label, quits), result) in labels.into_iter().zip(stopped) {
                match result {
                    Ok(ran) if ran.ending == Ending::AskedToQuit => {}
                    // Quit by itself as the body said it would, and cleanly.
                    Ok(ran) if quits && ran.ending == Ending::Exited(0) => {}
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
        if let Some(grid) = &self.grid {
            let stranded = self.sessions_left(grid, None).await;
            if !stranded.is_empty() {
                problems.push(StageError::Stranded(stranded));
            }
            grid.shutdown();
        }
        if let Some(sockets) = self.sockets.get()
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

/// Ask the in-process viewer `handle` (labelled `label`) on `host` to log out
/// and exit, by raising its own termination flag.
async fn ask_to_log_out(
    host: &InProcessHost<ViewerApp>,
    label: &str,
    handle: ViewerHandle,
) -> Result<(), StageError> {
    let raised = host
        .with_app(handle, |viewer| {
            viewer
                .app()
                .world()
                .get_resource::<TerminationFlag>()
                .map(TerminationFlag::raise)
                .is_some()
        })
        .await?;
    if raised {
        Ok(())
    } else {
        Err(StageError::NoLogout {
            viewer: label.to_owned(),
            reason: "it has no termination flag".to_owned(),
        })
    }
}

/// Wait until the viewer `driver` drives has arrived — in `region` when
/// given, else anywhere — and its scene is quiet, giving it `settle` for
/// that.
async fn arrive(
    driver: Viewer,
    region: Option<String>,
    settle: Duration,
) -> Result<(), StageError> {
    let arrived = async {
        let at = driver
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(LOGIN);
        let _held = match region {
            Some(region) => at.to_equal(json!(region)).await?,
            None => at.to_be_present().await?,
        };
        driver.wait_until_quiet(settle).await
    };
    arrived.await.map_err(|source| StageError::Driver {
        viewer: driver.label().to_owned(),
        source,
    })
}

/// Connect the driver to `viewer`'s process once its `socket` answers —
/// failing early, with its `log`, if the process ends first.
async fn connect(viewer: &StageViewer, socket: &Path, log: &Path) -> Result<Viewer, StageError> {
    let since = Instant::now();
    let mut last = "the socket does not exist yet".to_owned();
    loop {
        if fs_err::metadata(socket).is_ok() {
            match Viewer::connect(socket, driver_options(&viewer.label, &viewer.dir)).await {
                Ok(driver) => return Ok(driver),
                Err(error) => last = error.to_string(),
            }
        }
        if let ViewerRun::Process(Some(running)) = &mut *lock(&viewer.run)
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

/// The driver's options for viewer `label`, its failure artifacts in its
/// directory.
fn driver_options(label: &str, dir: &Path) -> ViewerOptions {
    ViewerOptions::new(label).with_artifacts(dir.join("failures"))
}

/// An in-process stage viewer: the viewer's own builder's App, headless,
/// storing under `state`, reading the asset tree in `assets` when given, set
/// up as `setup` says, reached through the in-process transport and logging
/// inside a span named `log_label`.
fn in_process_viewer(
    params: LoginParams,
    log_label: String,
    state: &Path,
    assets: Option<PathBuf>,
    setup: &ViewerSetup,
) -> Result<ViewerApp, BuildError> {
    let mut options = ViewerAppOptions::new(params);
    options.window = WindowMode::Headless {
        size: WINDOW,
        watch: false,
    };
    options.storage = Storage::Directories(ViewerPaths::under(state));
    options.audio_device = false;
    options.media = if setup.web_media {
        MediaRuntime {
            web: true,
            ..MediaRuntime::OFF
        }
    } else {
        MediaRuntime::OFF
    };
    if let Some(skin) = &setup.skin {
        options.skin.selection = SkinSelection {
            skin: skin.clone(),
            theme: None,
        };
    }
    options.assets = assets;
    options.skin.watch = setup.watch_skins;
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_fake_grid::RegionConfig;

    use super::StageBuilder;
    use crate::error::{BodyError, StageError};
    use crate::grid::Grid;
    use crate::need::Need;

    /// A test that needs grid control skips on a live grid, saying why,
    /// before it reads a credentials file or logs anything in: its body never
    /// runs. On the fake grid it would run.
    #[test]
    fn a_test_needing_grid_control_skips_on_a_live_grid() -> Result<(), StageError> {
        let builder = StageBuilder::new("needs_grid_control")
            .viewer("Alpha")
            .needs(Need::GridControl);
        for live in [Grid::OpenSim, Grid::Aditi] {
            let on_live = builder.clone().on_grid(live);
            assert_eq!(
                on_live.skip_reason()?.as_deref(),
                Some("it needs grid control (the fake grid's handle)")
            );
            on_live.run(async |_stage: &super::Stage| -> Result<(), BodyError> {
                Err("the body ran on a live grid".into())
            })?;
        }
        assert_eq!(builder.on_grid(Grid::Fake).skip_reason()?, None);
        Ok(())
    }

    /// Naming a region dictates the grid, which only the fake grid obeys; the
    /// skip names it once however many regions are named.
    #[test]
    fn naming_regions_dictates_the_grid() -> Result<(), StageError> {
        let builder = StageBuilder::new("names_regions")
            .viewer("Alpha")
            .region(RegionConfig::default())
            .region(RegionConfig::default())
            .configure_grid(|grid| grid);
        assert_eq!(
            builder
                .clone()
                .on_grid(Grid::OpenSim)
                .skip_reason()?
                .as_deref(),
            Some("it needs a grid it configures itself (it names its regions)")
        );
        assert_eq!(builder.on_grid(Grid::Fake).skip_reason()?, None);
        Ok(())
    }
}
