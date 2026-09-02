//! What each viewer is launched with: its program, its arguments, its
//! environment, and the directories it is confined to.
//!
//! The two launches differ more than they look. Firestorm's harness is
//! configured almost entirely by environment and reads the grid through its own
//! grid manager; this workspace's viewer takes flags. What they share is the
//! capture block ([`crate::plan::CaptureSpec::env`]) and the camera flags, which
//! is precisely the part that has to agree for the frames to be comparable.
//!
//! # Confining a run
//!
//! Both viewers keep settings, caches, logs and credential stores in a
//! per-user directory, and a harness run that shares those with the operator's
//! real session is a bad neighbour three ways over: it rewrites settings a
//! person tuned by hand (this viewer saves its settings on the way out), it
//! serves textures from a cache filled by an earlier run — which is how a
//! fixture whose pixels changed under a stable id goes unnoticed — and two runs
//! at once fight over the same files.
//!
//! So each viewer is pointed inside the run directory: `FIRESTORM_X64_USER_DIR`
//! for Firestorm, and the `XDG_*` roots for this viewer, which resolves all
//! three of its config / state / cache trees through them.

use std::path::{Path, PathBuf};

use crate::files::{AVATAR_KEY, Paths};
use crate::plan::RunPlan;

/// One of the two viewers a cross-check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    /// This workspace's Bevy viewer.
    SlClient,
    /// The patched Firestorm — its `test-harness` branch, built wherever the
    /// machine keeps it; the runner is told the launcher's path and assumes
    /// nothing about where a build tree lives.
    Firestorm,
}

impl Viewer {
    /// The name this viewer's artefacts are collected under, and the one it
    /// writes into its own status file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SlClient => "sl-client",
            Self::Firestorm => "firestorm",
        }
    }

    /// Both viewers, in the order a run drives them.
    #[must_use]
    pub const fn both() -> [Self; 2] {
        [Self::SlClient, Self::Firestorm]
    }
}

/// The directories one run owns.
///
/// Everything a run writes lives under [`root`](Self::root), so a run is kept,
/// copied or deleted as one thing, and nothing it did survives outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirs {
    /// The run directory itself.
    pub root: PathBuf,
}

impl RunDirs {
    /// A run rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where the credentials and grid files go.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// Where a viewer's frames, scene dump and status file go — the directory
    /// handed to its `--screenshot-dir`.
    #[must_use]
    pub fn artefacts(&self, viewer: Viewer) -> PathBuf {
        self.root.join(viewer.name())
    }

    /// A viewer's own log (its standard output and error), kept beside its
    /// frames: when a run fails, this is the file that says why, and it must not
    /// be the terminal's scrollback.
    #[must_use]
    pub fn log(&self, viewer: Viewer) -> PathBuf {
        self.artefacts(viewer).join("viewer.log")
    }

    /// The per-run private state directory for a viewer — its settings, caches
    /// and logs, kept out of the operator's own.
    #[must_use]
    pub fn state(&self, viewer: Viewer) -> PathBuf {
        self.root.join(format!("{}-state", viewer.name()))
    }

    /// Create every directory a run writes into.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error, named after the directory.
    pub fn create(&self) -> Result<(), std::io::Error> {
        fs_err::create_dir_all(self.config())?;
        for viewer in Viewer::both() {
            fs_err::create_dir_all(self.artefacts(viewer))?;
            fs_err::create_dir_all(self.state(viewer))?;
        }
        Ok(())
    }
}

/// A viewer, ready to be spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Which viewer this is.
    pub viewer: Viewer,
    /// The executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment entries **added** to the inherited environment.
    pub env: Vec<(String, String)>,
    /// Where its frames, dump and status file land.
    pub artefacts: PathBuf,
    /// Where its own output is written.
    pub log: PathBuf,
}

/// The launch for this workspace's viewer.
///
/// `--login-uri` rather than a grid nickname, because the fake grid has no
/// nickname; the credentials file carries the same URI, and passing it
/// explicitly means a stale file cannot silently send the run elsewhere.
///
/// `asset_root` is the crate directory holding the viewer's `assets/` (its skins
/// and fonts). Bevy resolves its asset root relative to the *executable*, so a
/// viewer run straight out of `target/release` finds no skin at all and draws an
/// unskinned interface — which matters the moment a run captures the UI layer,
/// and is invisible until then.
#[must_use]
pub fn sl_client(
    program: impl Into<PathBuf>,
    dirs: &RunDirs,
    plan: &RunPlan,
    files: &Paths,
    asset_root: Option<&Path>,
) -> Launch {
    let artefacts = dirs.artefacts(Viewer::SlClient);
    let state = dirs.state(Viewer::SlClient);
    let mut args = vec![
        "--credentials".to_owned(),
        files.credentials.display().to_string(),
        "--avatar".to_owned(),
        AVATAR_KEY.to_owned(),
        "--login-uri".to_owned(),
        plan.login_uri.to_string(),
        "--screenshot-dir".to_owned(),
        artefacts.display().to_string(),
    ];
    args.extend(camera_args(plan));
    let mut env = plan.capture.env();
    if let Some(root) = asset_root {
        env.push(("BEVY_ASSET_ROOT".to_owned(), root.display().to_string()));
    }
    // All four roots, not just the cache: this viewer writes its settings back
    // on the way out, and a harness run must not be able to edit the operator's.
    for (key, leaf) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        env.push((key.to_owned(), state.join(leaf).display().to_string()));
    }
    Launch {
        viewer: Viewer::SlClient,
        program: program.into(),
        args,
        env,
        log: dirs.log(Viewer::SlClient),
        artefacts,
    }
}

/// The launch for the patched Firestorm.
///
/// Three things about driving it are not guessable, and all three are here:
///
/// - **`--grid <host:port>`, never `--loginuri`.** `CmdLineLoginURI` is dead
///   code in the OpenSim build — declared, mapped, and read by nothing — so a
///   run configured with it logs into whichever grid the viewer used last.
///   `--grid` with an unknown name is treated as a host and resolved through
///   `GET /get_grid_info`, which the fake grid serves.
/// - **`--multiple`**, or a second instance refuses to start at all.
/// - **`FIRESTORM_X64_USER_DIR`**, or the run shares settings, cache, logs,
///   `grids.user.xml` and the credential store with the operator's real session.
///
/// # Errors
///
/// Returns a message when the plan's login URI has no host to hand `--grid`.
pub fn firestorm(
    program: impl Into<PathBuf>,
    dirs: &RunDirs,
    plan: &RunPlan,
    files: &Paths,
) -> Result<Launch, String> {
    let artefacts = dirs.artefacts(Viewer::Firestorm);
    let mut args = vec![
        "--grid".to_owned(),
        plan.firestorm_grid_name()?,
        "--multiple".to_owned(),
        "--credentials".to_owned(),
        files.credentials.display().to_string(),
        "--avatar".to_owned(),
        AVATAR_KEY.to_owned(),
        "--gridfile".to_owned(),
        files.grid.display().to_string(),
        "--screenshot-dir".to_owned(),
        artefacts.display().to_string(),
    ];
    args.extend(camera_args(plan));
    let mut env = plan.capture.env();
    env.push((
        "FIRESTORM_X64_USER_DIR".to_owned(),
        dirs.state(Viewer::Firestorm).display().to_string(),
    ));
    Ok(Launch {
        viewer: Viewer::Firestorm,
        program: program.into(),
        args,
        env,
        log: dirs.log(Viewer::Firestorm),
        artefacts,
    })
}

/// The camera flags, which both viewers spell the same way and parse in the same
/// region-local frame.
///
/// By flag rather than by environment because this workspace's viewer takes a
/// camera only that way; keeping both sides on flags means the two are set from
/// one place in this crate rather than two.
fn camera_args(plan: &RunPlan) -> Vec<String> {
    let Some(camera) = plan.camera else {
        return Vec::new();
    };
    let mut args = vec!["--camera-position".to_owned(), camera.position.to_string()];
    if let Some(look_at) = camera.look_at {
        args.push("--camera-look-at".to_owned());
        args.push(look_at.to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;

    use super::{Launch, RunDirs, Viewer, firestorm, sl_client};
    use crate::files::Paths;
    use crate::plan::{CameraSpec, CaptureSpec, RegionPoint, RunPlan};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A plan with a camera, since a comparison always has one.
    fn plan() -> Result<RunPlan, TestError> {
        Ok(RunPlan {
            scenario: "catalogue".to_owned(),
            login_uri: "http://127.0.0.1:9100/".parse()?,
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
            password: "password".to_owned(),
            capture: CaptureSpec::default(),
            camera: Some(CameraSpec::facing(
                RegionPoint::new(128.0, 140.0, 25.0),
                8.0,
                2.0,
            )),
        })
    }

    /// The configuration files, wherever a run put them.
    fn files(dirs: &RunDirs) -> Paths {
        Paths {
            credentials: dirs.config().join("credentials.toml"),
            grid: dirs.config().join("grid.toml"),
        }
    }

    /// The value following `flag`, for asserting on an argument list.
    fn value_of<'args>(launch: &'args Launch, flag: &str) -> Option<&'args str> {
        let index = launch.args.iter().position(|arg| arg == flag)?;
        launch.args.get(index.checked_add(1)?).map(String::as_str)
    }

    /// Both viewers are aimed at the same point from the same point, in the same
    /// units, with the same spelling. This is the whole comparison: two frames
    /// from two cameras are two pictures, not a cross-check.
    #[test]
    fn both_viewers_get_the_same_camera() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        let plan = plan()?;
        let files = files(&dirs);
        let ours = sl_client("viewer", &dirs, &plan, &files, None);
        let theirs = firestorm("firestorm", &dirs, &plan, &files)?;
        assert_eq!(
            value_of(&ours, "--camera-position"),
            value_of(&theirs, "--camera-position")
        );
        assert_eq!(
            value_of(&ours, "--camera-look-at"),
            value_of(&theirs, "--camera-look-at")
        );
        assert_eq!(value_of(&ours, "--camera-position"), Some("128,132,27"));
        Ok(())
    }

    /// The capture block reaches both viewers identically: it is the only thing
    /// that decides what a frame *is*, and one of the two reading a different
    /// size produces a pair that cannot be diffed at all.
    #[test]
    fn both_viewers_get_the_same_capture_block() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        let plan = plan()?;
        let files = files(&dirs);
        let ours = sl_client("viewer", &dirs, &plan, &files, None);
        let theirs = firestorm("firestorm", &dirs, &plan, &files)?;
        for (key, value) in plan.capture.env() {
            assert!(
                ours.env.contains(&(key.clone(), value.clone())),
                "sl-client is missing {key}={value}"
            );
            assert!(
                theirs.env.contains(&(key.clone(), value.clone())),
                "firestorm is missing {key}={value}"
            );
        }
        Ok(())
    }

    /// Firestorm is told its grid the one way it actually reads:
    /// `CmdLineLoginURI` is dead code in the OpenSim build, so a `--loginuri`
    /// here would silently log the run into whichever grid it used last.
    #[test]
    fn firestorm_is_pointed_at_the_grid_by_host_and_port() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        let launch = firestorm("firestorm", &dirs, &plan()?, &files(&dirs))?;
        assert_eq!(value_of(&launch, "--grid"), Some("127.0.0.1:9100"));
        assert!(!launch.args.iter().any(|arg| arg == "--loginuri"));
        // Without this a second instance refuses to start.
        assert!(launch.args.iter().any(|arg| arg == "--multiple"));
        Ok(())
    }

    /// Neither viewer may touch the operator's own settings, caches or logs: a
    /// harness run that rewrites a hand-tuned settings file, or serves last
    /// run's textures, costs more than the run is worth.
    #[test]
    fn each_viewer_is_confined_to_the_run_directory() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        let plan = plan()?;
        let files = files(&dirs);
        let ours = sl_client("viewer", &dirs, &plan, &files, None);
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
        ] {
            let value = ours
                .env
                .iter()
                .find(|(name, _value)| name == key)
                .map(|(_name, value)| value.as_str())
                .ok_or_else(|| format!("sl-client should set {key}"))?;
            assert!(
                Path::new(value).starts_with(&dirs.root),
                "{key} points outside the run at {value}"
            );
        }
        let theirs = firestorm("firestorm", &dirs, &plan, &files)?;
        let user_dir = theirs
            .env
            .iter()
            .find(|(name, _value)| name == "FIRESTORM_X64_USER_DIR")
            .map(|(_name, value)| PathBuf::from(value))
            .ok_or("firestorm should get its own user directory")?;
        assert!(user_dir.starts_with(&dirs.root));
        Ok(())
    }

    /// A run whose viewer was built into `target/release` has no `assets/` beside
    /// it, so the skin 404s and the interface draws unstyled — invisible until a
    /// run captures the UI layer, and then baffling.
    #[test]
    fn the_bevy_asset_root_is_passed_when_known() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        let plan = plan()?;
        let launch = sl_client(
            "viewer",
            &dirs,
            &plan,
            &files(&dirs),
            Some(Path::new("/repo/sl-client-bevy-viewer")),
        );
        assert!(launch.env.contains(&(
            "BEVY_ASSET_ROOT".to_owned(),
            "/repo/sl-client-bevy-viewer".to_owned()
        )));
        Ok(())
    }

    /// Each viewer's artefacts land in its own directory, under the name it also
    /// writes into its status file — so a collected run says what it holds even
    /// after being copied somewhere else.
    #[test]
    fn each_viewer_writes_into_its_own_named_directory() -> Result<(), TestError> {
        let dirs = RunDirs::new("/tmp/run");
        assert_eq!(
            dirs.artefacts(Viewer::SlClient),
            Path::new("/tmp/run/sl-client")
        );
        assert_eq!(
            dirs.artefacts(Viewer::Firestorm),
            Path::new("/tmp/run/firestorm")
        );
        assert_eq!(
            dirs.log(Viewer::Firestorm),
            Path::new("/tmp/run/firestorm/viewer.log")
        );
        Ok(())
    }
}
