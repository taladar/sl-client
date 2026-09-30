//! `sl-viewer-ctl stage`: a fresh fake grid and the viewers a small TOML file
//! names, each logged in as its own account, held until Ctrl-C.
//!
//! ```toml
//! scenario = "catalogue"   # the fake grid's scene; "stock" when absent
//! port = 9100              # its HTTP port; any free one when absent
//! capture_size = "1280x720"
//!
//! [[viewer]]
//! label = "alice"          # logs in as "Stage alice"
//! watch = true             # also opens a window showing the run
//!
//! [[viewer]]
//! label = "bob"
//! ```
//!
//! A viewer may also set `web_media = true` and `args = [...]` (more viewer
//! arguments); the file may set `viewer_binary` and `dir` (the run
//! directory), relative to the file.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sl_fake_grid::fixtures::scenarios;
use sl_fake_grid::{AccountConfig, FakeGridBuilder, RegionConfig};
use sl_viewer_launch::ViewerDir;

use crate::cli::Global;
use crate::error::CtlError;
use crate::launch::{
    ReadyViewer, Shutdown, ViewerStart, driver_options, fresh_run_dir, hold_then_stop, start, stop,
    viewer_binary,
};
use crate::output::{Outcome, Printer};
use crate::target::socket_dir;

/// Every stage account's first name: viewer `alice` is `Stage alice`.
pub const FIRST_NAME: &str = "Stage";

/// Every stage account's password.
const PASSWORD: &str = "password";

/// The key of the one avatar in a stage viewer's credentials file.
const AVATAR_KEY: &str = "stage";

/// A stage file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// The fake grid's scene.
    #[serde(default = "default_scenario")]
    pub scenario: String,
    /// The fake grid's HTTP port; `0` for any free one.
    #[serde(default)]
    pub port: u16,
    /// The viewer executable.
    #[serde(default)]
    pub viewer_binary: Option<PathBuf>,
    /// The run directory.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Every viewer's frame size, `WIDTHxHEIGHT`.
    #[serde(default = "default_capture_size")]
    pub capture_size: String,
    /// The viewers, in the order they are started.
    #[serde(default, rename = "viewer")]
    pub viewers: Vec<ViewerSpec>,
}

/// One viewer of a stage file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewerSpec {
    /// Its label: its directory's name and its account's last name.
    pub label: String,
    /// Whether it also opens a window showing the run.
    #[serde(default)]
    pub watch: bool,
    /// Whether it runs its web-media engine.
    #[serde(default)]
    pub web_media: bool,
    /// More viewer arguments.
    #[serde(default)]
    pub args: Vec<String>,
}

/// The scene a stage file names when it names none.
fn default_scenario() -> String {
    scenarios::DEFAULT.to_owned()
}

/// The frame size a stage file names when it names none.
fn default_capture_size() -> String {
    "1280x720".to_owned()
}

impl Spec {
    /// Parse and check a stage file's `text`; `path` names it in errors.
    ///
    /// # Errors
    ///
    /// [`CtlError::StageFile`] when it is not TOML of this shape, names an
    /// unknown scene, no viewer, a label twice, or a label that is not a
    /// plain word (it is an account's last name and a directory's name).
    pub fn parse(path: &Path, text: &str) -> Result<Self, CtlError> {
        let invalid = |reason: String| CtlError::StageFile {
            path: path.to_path_buf(),
            reason,
        };
        let file: Self = toml::from_str(text).map_err(|error| invalid(error.to_string()))?;
        if scenarios::scenario(&file.scenario).is_none() {
            return Err(invalid(format!(
                "unknown scenario {:?} (known: {})",
                file.scenario,
                scenarios::names().join(", ")
            )));
        }
        if file.viewers.is_empty() {
            return Err(invalid("it names no [[viewer]]".to_owned()));
        }
        let mut labels = BTreeSet::new();
        for viewer in &file.viewers {
            let plain = !viewer.label.is_empty()
                && viewer
                    .label
                    .chars()
                    .all(|next| next.is_ascii_alphanumeric() || next == '-' || next == '_');
            if !plain {
                return Err(invalid(format!(
                    "viewer label {:?} is not a plain word (letters, digits, - and _)",
                    viewer.label
                )));
            }
            if !labels.insert(viewer.label.as_str()) {
                return Err(invalid(format!(
                    "viewer label {:?} is given twice",
                    viewer.label
                )));
            }
        }
        Ok(file)
    }
}

/// Write the credentials file viewer `label` logs into the grid at
/// `login_uri` with, into its directory, and answer its path.
fn write_credentials(dir: &ViewerDir, label: &str, login_uri: &str) -> Result<PathBuf, CtlError> {
    let path = dir.root.join("credentials.toml");
    fs_err::create_dir_all(&dir.root).map_err(|source| CtlError::File {
        path: dir.root.clone(),
        source,
    })?;
    let text = format!(
        "# Written by sl-viewer-ctl for one stage against a fake grid.\n\
         default_avatar = \"{AVATAR_KEY}\"\n\n[avatars.{AVATAR_KEY}]\nfirst = \"{FIRST_NAME}\"\n\
         last = \"{label}\"\npassword = \"{PASSWORD}\"\nlogin_uri = \"{login_uri}\"\n"
    );
    fs_err::write(&path, text).map_err(|source| CtlError::File {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// `path` against the stage file's directory, unless absolute.
fn beside(file: &Path, path: &Path) -> PathBuf {
    file.parent()
        .map_or_else(|| path.to_path_buf(), |dir| dir.join(path))
}

/// `sl-viewer-ctl stage <file>`: the grid and every viewer, held until
/// Ctrl-C, then logged out and taken down.
///
/// # Errors
///
/// [`CtlError::StageFile`] for a bad file, [`CtlError::Grid`] when the grid
/// does not start, [`CtlError::Launch`] for a viewer that does not come up (the
/// others are logged out first), and [`CtlError::Output`].
pub async fn stage<W: Write>(
    path: &Path,
    global: &Global,
    printer: &mut Printer<W>,
) -> Result<(), CtlError> {
    let mut shutdown = Shutdown::install()?;
    let text = fs_err::read_to_string(path).map_err(|source| CtlError::File {
        path: path.to_path_buf(),
        source,
    })?;
    let file = Spec::parse(path, &text)?;
    let binary = viewer_binary(
        file.viewer_binary
            .as_deref()
            .map(|binary| beside(path, binary))
            .as_deref(),
    )?;
    let run_dir = match &file.dir {
        Some(dir) => beside(path, dir),
        None => fresh_run_dir("stage")?,
    };
    let sockets = socket_dir().ok_or_else(|| {
        CtlError::NoSocket("XDG_RUNTIME_DIR is not set to put the automation sockets in".to_owned())
    })?;
    let scene = scenarios::scenario(&file.scenario).ok_or_else(|| CtlError::StageFile {
        path: path.to_path_buf(),
        reason: format!("unknown scenario {:?}", file.scenario),
    })?;
    let mut builder = FakeGridBuilder::new()
        .http_port(file.port)
        // A long hold, so the CAPS long-poll does not compete with the
        // renders for the cores.
        .event_queue_hold(Duration::from_secs(2))
        .region(scene.dress(RegionConfig::default()));
    for viewer in &file.viewers {
        builder = builder.account(AccountConfig::new(FIRST_NAME, &viewer.label, PASSWORD));
    }
    let grid = builder
        .start()
        .await
        .map_err(|error| CtlError::Grid(error.to_string()))?;
    let login_uri = grid.login_uri().to_string();
    printer.print(&Outcome::Grid {
        login_uri: login_uri.clone(),
        scenario: file.scenario.clone(),
    })?;
    let mut starting = tokio::task::JoinSet::new();
    for viewer in &file.viewers {
        let dir = ViewerDir::new(run_dir.join(&viewer.label)).map_err(|source| CtlError::File {
            path: run_dir.clone(),
            source,
        })?;
        let credentials = write_credentials(&dir, &viewer.label, &login_uri)?;
        let spec = ViewerStart {
            label: viewer.label.clone(),
            binary: binary.clone(),
            socket: sockets.join(format!("ctl-{}-{}.sock", std::process::id(), viewer.label)),
            dir,
            login: vec![
                "--credentials".to_owned(),
                credentials.display().to_string(),
                "--avatar".to_owned(),
                AVATAR_KEY.to_owned(),
            ],
            watch: viewer.watch,
            web_media: viewer.web_media,
            capture_size: file.capture_size.clone(),
            extra: viewer.args.clone(),
            wait: true,
        };
        let options = driver_options(&viewer.label, global);
        let _task = starting.spawn(async move { start(&spec, options).await });
    }
    let mut ready: Vec<ReadyViewer> = Vec::new();
    let mut failure: Option<CtlError> = None;
    let interrupted = loop {
        tokio::select! {
            joined = starting.join_next() => match joined {
                Some(Ok(Ok(viewer))) => ready.push(viewer),
                Some(Ok(Err(error))) => {
                    failure = Some(error);
                    break false;
                }
                Some(Err(join)) => std::panic::resume_unwind(join.into_panic()),
                None => break false,
            },
            () = shutdown.requested() => break true,
        }
    };
    if interrupted || failure.is_some() {
        // Aborting a start drops its viewer, which asks it to log out.
        starting.abort_all();
        while starting.join_next().await.is_some() {}
        stop(ready, printer).await?;
        drop(grid);
        return failure.map_or(Ok(()), Err);
    }
    ready.sort_by_key(|viewer| {
        file.viewers
            .iter()
            .position(|named| named.label == viewer.label)
    });
    for viewer in &ready {
        printer.print(&viewer.outcome())?;
    }
    tracing::info!("holding the stage; Ctrl-C logs every viewer out");
    hold_then_stop(ready, &mut shutdown, printer).await?;
    drop(grid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::{Spec, ViewerSpec};
    use crate::error::CtlError;

    /// The reason a stage file is refused, or what it parsed as.
    fn refusal(text: &str) -> String {
        match Spec::parse(Path::new("stage.toml"), text) {
            Ok(file) => format!("parsed as {file:?}"),
            Err(CtlError::StageFile { reason, .. }) => reason,
            Err(other) => format!("refused with {other}"),
        }
    }

    #[test]
    fn a_minimal_file_takes_the_defaults() -> Result<(), CtlError> {
        let file = Spec::parse(Path::new("stage.toml"), "[[viewer]]\nlabel = \"a\"\n")?;
        assert_eq!(
            file,
            Spec {
                scenario: "stock".to_owned(),
                port: 0,
                viewer_binary: None,
                dir: None,
                capture_size: "1280x720".to_owned(),
                viewers: vec![ViewerSpec {
                    label: "a".to_owned(),
                    watch: false,
                    web_media: false,
                    args: Vec::new(),
                }],
            }
        );
        Ok(())
    }

    #[test]
    fn a_bad_file_is_refused_with_its_reason() {
        let cases = [
            ("", "it names no [[viewer]]"),
            (
                "scenario = \"nowhere\"\n[[viewer]]\nlabel = \"a\"",
                "unknown scenario",
            ),
            (
                "[[viewer]]\nlabel = \"a\"\n[[viewer]]\nlabel = \"a\"",
                "given twice",
            ),
            ("[[viewer]]\nlabel = \"a b\"", "not a plain word"),
            ("[[viewer]]\nlabel = \"\"", "not a plain word"),
            (
                "[[viewer]]\nlabel = \"a\"\nwach = true",
                "unknown field `wach`",
            ),
        ];
        for (text, reason) in cases {
            let found = refusal(text);
            assert!(found.contains(reason), "{text:?}: {found}");
        }
    }
}
