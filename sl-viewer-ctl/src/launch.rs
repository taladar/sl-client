//! Bringing viewer processes up for `launch` and `stage` — headless, behind
//! an automation socket, each confined to its own directory — holding them
//! while a person or a script drives them, and logging them out again.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sl_automation_proto::{Probe, ViewerIdentity};
use sl_repl::{Credentials, LoginCooldown};
use sl_viewer_driver::{Viewer, ViewerOptions};
use sl_viewer_launch::{LOGOUT_GRACE, Launch, RunningViewer, ViewerDir, is_executable, stop_all};
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::cli::{Global, LaunchArgs};
use crate::error::CtlError;
use crate::output::{Outcome, Printer};
use crate::target::socket_dir;

/// How long a started viewer has to open its automation socket.
const CONNECT: Duration = Duration::from_secs(60);

/// How long a viewer has to log in and arrive in a region.
const LOGIN: Duration = Duration::from_secs(180);

/// How long a viewer that arrived has to settle. Long: a live grid's asset
/// service answers some textures 503 for minutes, each walking its retry
/// chain.
const SETTLE: Duration = Duration::from_secs(600);

/// How often a starting or held viewer is looked at.
const POLL: Duration = Duration::from_millis(100);

/// The viewer binary this crate launches unless told otherwise, looked for
/// beside the running executable (cargo builds both into one directory).
const VIEWER_BINARY: &str = "sl-client-bevy-viewer";

/// The Ctrl-C and `SIGTERM` a held viewer is logged out on.
#[derive(Debug)]
pub(crate) struct Shutdown {
    /// `SIGINT`.
    interrupt: Signal,
    /// `SIGTERM`.
    terminate: Signal,
}

impl Shutdown {
    /// Take over `SIGINT` and `SIGTERM` from now on, so neither ends this
    /// process before its viewers are logged out.
    ///
    /// # Errors
    ///
    /// The signal registration's.
    pub(crate) fn install() -> Result<Self, CtlError> {
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    /// Resolve on the next `SIGINT` or `SIGTERM`.
    pub(crate) async fn requested(&mut self) {
        tokio::select! {
            _signal = self.interrupt.recv() => {}
            _signal = self.terminate.recv() => {}
        }
    }
}

/// How one viewer process is started.
#[derive(Debug, Clone)]
pub(crate) struct ViewerStart {
    /// Its label, in output and errors.
    pub(crate) label: String,
    /// The executable.
    pub(crate) binary: PathBuf,
    /// Its run directory.
    pub(crate) dir: ViewerDir,
    /// Where it opens its automation socket.
    pub(crate) socket: PathBuf,
    /// Its login arguments: credentials, avatar, grid, start.
    pub(crate) login: Vec<String>,
    /// Whether it also opens a watch window.
    pub(crate) watch: bool,
    /// Whether it runs its web-media engine.
    pub(crate) web_media: bool,
    /// Its frame's size, `WIDTHxHEIGHT`.
    pub(crate) capture_size: String,
    /// More arguments.
    pub(crate) extra: Vec<String>,
    /// Whether to wait for it to log in and settle.
    pub(crate) wait: bool,
}

impl ViewerStart {
    /// The launch: headless, behind its socket, confined to its directory.
    fn launch(&self) -> Launch {
        let mut launch = Launch::in_dir(self.label.clone(), &self.binary, &self.dir)
            .args(self.login.iter().cloned())
            .args([
                "--headless".to_owned(),
                "--capture-size".to_owned(),
                self.capture_size.clone(),
                "--automation-socket".to_owned(),
                self.socket.display().to_string(),
            ]);
        if self.watch {
            launch = launch.arg("--watch");
        }
        if !self.web_media {
            launch = launch.arg("--disable-web-media");
        }
        launch.args(self.extra.iter().cloned())
    }
}

/// A viewer process that answers on its socket.
#[derive(Debug)]
pub(crate) struct ReadyViewer {
    /// Its label.
    pub(crate) label: String,
    /// The process.
    pub(crate) running: RunningViewer,
    /// Who it said it is once it was ready — logged in, unless not waited
    /// for.
    pub(crate) identity: ViewerIdentity,
    /// Its automation socket.
    pub(crate) socket: PathBuf,
    /// Its log.
    pub(crate) log: PathBuf,
}

impl ReadyViewer {
    /// What `launch` and `stage` print for it.
    #[must_use]
    pub(crate) fn outcome(&self) -> Outcome {
        Outcome::Ready {
            label: self.label.clone(),
            socket: self.socket.display().to_string(),
            log: self.log.display().to_string(),
            pid: self.running.pid(),
            identity: self.identity.clone(),
        }
    }
}

/// The error for a viewer that failed to come up, pointing at its log.
fn failed(label: &str, log: &Path, reason: impl core::fmt::Display) -> CtlError {
    CtlError::Launch {
        viewer: label.to_owned(),
        reason: format!("{reason} (its log is {})", log.display()),
    }
}

/// Start the viewer `start` describes, connect to its socket and — unless
/// told not to — wait until it has logged in and settled.
///
/// A viewer that fails on the way is logged out again (dropping its process
/// asks it to quit).
///
/// # Errors
///
/// [`CtlError::Launch`] when it cannot be started, ends early, never opens
/// its socket, or never logs in or settles.
pub(crate) async fn start(
    start: &ViewerStart,
    options: ViewerOptions,
) -> Result<ReadyViewer, CtlError> {
    let log = start.dir.log();
    start.dir.create().map_err(|source| CtlError::File {
        path: start.dir.root.clone(),
        source,
    })?;
    if let Some(parent) = start.socket.parent() {
        fs_err::create_dir_all(parent).map_err(|source| CtlError::File {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut running =
        RunningViewer::spawn(&start.launch()).map_err(|error| failed(&start.label, &log, error))?;
    let since = Instant::now();
    let viewer = loop {
        if fs_err::metadata(&start.socket).is_ok()
            && let Ok(viewer) = Viewer::connect(&start.socket, options.clone()).await
        {
            break viewer;
        }
        if let Some(ending) = running
            .try_ending()
            .map_err(|error| failed(&start.label, &log, error))?
        {
            return Err(failed(
                &start.label,
                &log,
                format!("it ended ({ending:?}) before it opened its socket"),
            ));
        }
        if since.elapsed() > CONNECT {
            return Err(failed(
                &start.label,
                &log,
                format!(
                    "no automation socket at {} within {} s",
                    start.socket.display(),
                    CONNECT.as_secs()
                ),
            ));
        }
        tokio::time::sleep(POLL).await;
    };
    if start.wait {
        let arrived = async {
            let _region = viewer
                .expect_state(Probe::Agent)
                .at("/region/name")
                .timeout(LOGIN)
                .to_be_present()
                .await?;
            viewer.wait_until_quiet(SETTLE).await
        };
        arrived
            .await
            .map_err(|error| failed(&start.label, &log, error))?;
    }
    // The hello answered on connecting came before the login; ask again.
    let identity = viewer
        .hello()
        .await
        .map_err(|error| failed(&start.label, &log, error))?;
    Ok(ReadyViewer {
        label: start.label.clone(),
        running,
        identity,
        socket: start.socket.clone(),
        log,
    })
}

/// Hold `viewers` until Ctrl-C or `SIGTERM`, or until one of them ends on
/// its own; then log every one out, printing how each ended.
///
/// # Errors
///
/// [`CtlError::Output`] when printing fails. A viewer that would not log out
/// is reported in its `stopped` line, not as an error.
pub(crate) async fn hold_then_stop<W: Write>(
    viewers: Vec<ReadyViewer>,
    shutdown: &mut Shutdown,
    printer: &mut Printer<W>,
) -> Result<(), CtlError> {
    let mut viewers = viewers;
    let ended = async {
        loop {
            for viewer in &mut viewers {
                if let Ok(Some(ending)) = viewer.running.try_ending() {
                    return (viewer.label.clone(), ending);
                }
            }
            tokio::time::sleep(POLL.saturating_mul(5)).await;
        }
    };
    tokio::select! {
        () = shutdown.requested() => {}
        (label, ending) = ended => {
            tracing::warn!("{label} ended on its own ({ending:?}); stopping the rest");
        }
    }
    stop(viewers, printer).await
}

/// Log every viewer out at once, printing how each ended.
///
/// # Errors
///
/// [`CtlError::Output`] when printing fails.
pub(crate) async fn stop<W: Write>(
    viewers: Vec<ReadyViewer>,
    printer: &mut Printer<W>,
) -> Result<(), CtlError> {
    let (labels, running): (Vec<String>, Vec<RunningViewer>) = viewers
        .into_iter()
        .map(|viewer| (viewer.label, viewer.running))
        .unzip();
    let ran = tokio::task::spawn_blocking(move || stop_all(running, LOGOUT_GRACE))
        .await
        .unwrap_or_else(|join| std::panic::resume_unwind(join.into_panic()));
    for (label, ran) in labels.into_iter().zip(ran) {
        let ending = match ran {
            Ok(ran) => format!("{:?} after {} s", ran.ending, ran.duration.as_secs()),
            Err(error) => format!("could not be stopped: {error}"),
        };
        printer.print(&Outcome::Stopped { label, ending })?;
    }
    Ok(())
}

/// The viewer binary: `explicit`, else the one beside this executable.
///
/// # Errors
///
/// [`CtlError::Launch`] when it is not an executable file.
pub(crate) fn viewer_binary(explicit: Option<&Path>) -> Result<PathBuf, CtlError> {
    let binary = match explicit {
        Some(binary) => binary.to_path_buf(),
        None => std::env::current_exe()
            .map_err(|source| CtlError::File {
                path: PathBuf::from("/proc/self/exe"),
                source,
            })?
            .with_file_name(VIEWER_BINARY),
    };
    if is_executable(&binary) {
        Ok(binary)
    } else {
        Err(CtlError::Launch {
            viewer: binary.display().to_string(),
            reason: "no such executable; build it (`cargo build --release -p \
                     sl-client-bevy-viewer`) or pass --viewer"
                .to_owned(),
        })
    }
}

/// A fresh run directory under `$XDG_STATE_HOME/sl-viewer-ctl/runs` (not a
/// temporary directory: a long run's log and cache outgrow a `tmpfs`).
///
/// # Errors
///
/// [`CtlError::Launch`] when neither `XDG_STATE_HOME` nor `HOME` is set.
pub(crate) fn fresh_run_dir(name: &str) -> Result<PathBuf, CtlError> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| CtlError::Launch {
            viewer: name.to_owned(),
            reason: "neither XDG_STATE_HOME nor HOME is set; pass --dir".to_owned(),
        })?;
    Ok(state
        .join("sl-viewer-ctl")
        .join("runs")
        .join(format!("{name}-{}", std::process::id())))
}

/// Where the socket of viewer `label` of this process goes: the default
/// socket directory, where commands without `--socket` look.
///
/// # Errors
///
/// [`CtlError::NoSocket`] without `XDG_RUNTIME_DIR`.
pub(crate) fn socket_path(label: &str) -> Result<PathBuf, CtlError> {
    let dir = socket_dir().ok_or_else(|| {
        CtlError::NoSocket("XDG_RUNTIME_DIR is not set to put the automation socket in".to_owned())
    })?;
    Ok(dir.join(format!("ctl-{}-{label}.sock", std::process::id())))
}

/// The driver options of a viewer the command line starts.
#[must_use]
pub(crate) fn driver_options(label: &str, global: &Global) -> ViewerOptions {
    let options = ViewerOptions::new(label).with_timeout(global.timeout);
    match &global.artifacts {
        Some(dir) => options.with_artifacts(dir.join(label)),
        None => options,
    }
}

/// Whether a login with `args` goes to aditi, whose logins share a cooldown:
/// the grid named, else the avatar's own grid or login URI.
fn logs_into_aditi(args: &LaunchArgs, credentials: &Credentials) -> bool {
    let aditi = |text: &str| text.to_ascii_lowercase().contains("aditi");
    if let Some(uri) = &args.login_uri {
        return aditi(uri);
    }
    if let Some(grid) = &args.grid {
        return aditi(grid);
    }
    credentials
        .select(args.avatar.as_deref())
        .is_ok_and(|avatar| {
            avatar.grid().is_some_and(aditi) || avatar.login_uri().is_some_and(aditi)
        })
}

/// Wait out, then take, the avatar's aditi login cooldown, which every
/// unattended harness shares.
async fn take_aditi_turn(args: &LaunchArgs, credentials: &Credentials) -> Result<(), CtlError> {
    let avatar = credentials
        .select(args.avatar.as_deref())
        .map_err(|error| CtlError::Launch {
            viewer: "launch".to_owned(),
            reason: format!("{}: {error}", args.credentials.display()),
        })?;
    let name = format!("{} {}", avatar.first(), avatar.last());
    let cooldown = LoginCooldown::shared().map_err(|error| CtlError::Launch {
        viewer: "launch".to_owned(),
        reason: error.to_string(),
    })?;
    let wait = cooldown.wait_time(&name);
    if !wait.is_zero() {
        tracing::warn!(
            "waiting {} s out the aditi login cooldown of this avatar",
            wait.as_secs()
        );
        tokio::time::sleep(wait).await;
    }
    cooldown.stamp(&name).map_err(|error| CtlError::Launch {
        viewer: "launch".to_owned(),
        reason: error.to_string(),
    })
}

/// `sl-viewer-ctl launch`: one viewer, held until Ctrl-C.
///
/// # Errors
///
/// As [`start`], and [`CtlError::Output`].
pub(crate) async fn launch<W: Write>(
    args: &LaunchArgs,
    global: &Global,
    printer: &mut Printer<W>,
) -> Result<(), CtlError> {
    let mut shutdown = Shutdown::install()?;
    let binary = viewer_binary(args.viewer.as_deref())?;
    let credentials = Credentials::load(&args.credentials).map_err(|error| CtlError::Launch {
        viewer: "launch".to_owned(),
        reason: format!("{}: {error}", args.credentials.display()),
    })?;
    let credentials_path =
        std::path::absolute(&args.credentials).map_err(|source| CtlError::File {
            path: args.credentials.clone(),
            source,
        })?;
    let label = "viewer".to_owned();
    let dir = match &args.dir {
        Some(dir) => dir.clone(),
        None => fresh_run_dir(&label)?,
    };
    let dir = ViewerDir::new(dir).map_err(|source| CtlError::File {
        path: PathBuf::from("."),
        source,
    })?;
    let mut login = vec![
        "--credentials".to_owned(),
        credentials_path.display().to_string(),
    ];
    for (flag, value) in [
        ("--avatar", &args.avatar),
        ("--grid", &args.grid),
        ("--login-uri", &args.login_uri),
        ("--start", &args.start),
    ] {
        if let Some(value) = value {
            login.extend([flag.to_owned(), value.clone()]);
        }
    }
    let start_spec = ViewerStart {
        socket: match &global.socket {
            Some(socket) => socket.clone(),
            None => socket_path(&label)?,
        },
        label,
        binary,
        dir,
        login,
        watch: args.watch,
        web_media: args.web_media,
        capture_size: args.capture_size.clone(),
        extra: args.viewer_args.clone(),
        wait: !args.no_wait,
    };
    if logs_into_aditi(args, &credentials) {
        take_aditi_turn(args, &credentials).await?;
    }
    let options = driver_options(&start_spec.label, global);
    let ready = tokio::select! {
        ready = start(&start_spec, options) => ready?,
        () = shutdown.requested() => {
            // Dropping the starting viewer asks it to log out and quit.
            printer.print(&Outcome::Stopped {
                label: start_spec.label.clone(),
                ending: "interrupted while starting".to_owned(),
            })?;
            return Ok(());
        }
    };
    printer.print(&ready.outcome())?;
    tracing::info!("holding the viewer; Ctrl-C logs it out");
    hold_then_stop(vec![ready], &mut shutdown, printer).await
}
