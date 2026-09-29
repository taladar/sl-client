//! What a viewer is launched with: its program, its arguments, its
//! environment, and the directory it is confined to.
//!
//! # Confining a run
//!
//! A viewer keeps settings, caches, logs and credential stores in per-user
//! directories, and a harness run that shares those with the operator's real
//! session is a bad neighbour three ways over: it rewrites settings a person
//! tuned by hand (this workspace's viewer saves its settings on the way out),
//! it serves textures from a cache filled by an earlier run — which is how a
//! fixture whose pixels changed under a stable id goes unnoticed — and two runs
//! at once fight over the same files.
//!
//! So each viewer is pointed inside a directory of its own: this workspace's
//! viewer resolves its config, data, state and cache trees through the four
//! `XDG_*` roots, and [`confined_env`] sets all four. (Firestorm is confined by
//! `FIRESTORM_X64_USER_DIR` instead, which its launcher's caller sets.)

use std::path::{Path, PathBuf};

/// The four `XDG_*` roots this workspace's viewer resolves its trees through,
/// and the leaf each is pointed at inside a confined state directory.
///
/// All four, not just the cache: the viewer writes its settings back on the
/// way out, and a harness run must not be able to edit the operator's.
const XDG_ROOTS: [(&str, &str); 4] = [
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_STATE_HOME", "state"),
    ("XDG_CACHE_HOME", "cache"),
];

/// The environment entries that confine this workspace's viewer to `state`:
/// each `XDG_*` root pointed at a leaf directory below it.
#[must_use]
pub fn confined_env(state: &Path) -> Vec<(String, String)> {
    XDG_ROOTS
        .iter()
        .map(|(key, leaf)| ((*key).to_owned(), state.join(leaf).display().to_string()))
        .collect()
}

/// One viewer's run directory: its private state and its log.
///
/// Everything a viewer of a harness run writes lives under
/// [`root`](Self::root), so a run is kept, copied or deleted as one thing, and
/// nothing it did survives outside it. Several viewers of one run take one
/// directory each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerDir {
    /// The directory itself.
    pub root: PathBuf,
}

impl ViewerDir {
    /// A viewer directory at `root`, made absolute against the current
    /// directory.
    ///
    /// Absolute because every path below it is handed to a viewer, and a
    /// viewer need not start where this process stands: a launcher script may
    /// change into its own install tree first, and then resolves a relative
    /// path somewhere else entirely.
    ///
    /// # Errors
    ///
    /// Returns the error from reading the current directory, or the one for an
    /// empty `root`.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        Ok(Self {
            root: std::path::absolute(root.into())?,
        })
    }

    /// The private state directory the viewer is confined to — its settings,
    /// caches and logs, kept out of the operator's own.
    #[must_use]
    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The viewer's own log (its standard output and error): when a run fails,
    /// this is the file that says why, and it must not be a terminal's
    /// scrollback, least of all one several viewers share.
    #[must_use]
    pub fn log(&self) -> PathBuf {
        self.root.join("viewer.log")
    }

    /// The environment entries confining this workspace's viewer to
    /// [`state`](Self::state).
    #[must_use]
    pub fn confined_env(&self) -> Vec<(String, String)> {
        confined_env(&self.state())
    }

    /// Create the directory and its state directory.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error, named after the directory.
    pub fn create(&self) -> Result<(), std::io::Error> {
        fs_err::create_dir_all(self.state())
    }
}

/// A viewer, ready to be spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The name its log lines and errors call it by: which viewer of a run
    /// this is.
    pub name: String,
    /// The executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment entries **added** to the inherited environment.
    pub env: Vec<(String, String)>,
    /// Where its standard output and error are written.
    pub log: PathBuf,
}

impl Launch {
    /// `program`, called `name`, logging to `log`, with no arguments and the
    /// inherited environment.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        program: impl Into<PathBuf>,
        log: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            log: log.into(),
        }
    }

    /// This launch in `dir`: logging to its log, and confined to its state.
    #[must_use]
    pub fn in_dir(name: impl Into<String>, program: impl Into<PathBuf>, dir: &ViewerDir) -> Self {
        Self::new(name, program, dir.log()).envs(dir.confined_env())
    }

    /// One more argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// More arguments, in order.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// One more environment entry.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// More environment entries — an environment block several viewers of a
    /// run share, say.
    #[must_use]
    pub fn envs<I, K, V>(mut self, entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env.extend(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::{Launch, ViewerDir};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A relative viewer directory is made absolute: a viewer resolves the
    /// paths it is handed from wherever it starts.
    #[test]
    fn a_relative_viewer_directory_is_made_absolute() -> Result<(), TestError> {
        let dir = ViewerDir::new("e2e/viewer-a")?;
        assert!(dir.root.is_absolute());
        assert_eq!(dir.root, std::env::current_dir()?.join("e2e/viewer-a"));
        assert!(dir.state().is_absolute());
        assert!(dir.log().is_absolute());
        Ok(())
    }

    /// A viewer launched in its directory writes nothing outside it: every
    /// `XDG_*` root it reads points inside, and so does its log.
    #[test]
    fn a_launch_in_a_directory_is_confined_to_it() -> Result<(), TestError> {
        let dir = ViewerDir::new("/tmp/run/viewer-a")?;
        let launch = Launch::in_dir("a", "viewer", &dir);
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
        ] {
            let value = launch
                .env
                .iter()
                .find(|(name, _value)| name == key)
                .map(|(_name, value)| value.as_str())
                .ok_or_else(|| format!("{key} is not set"))?;
            assert!(
                Path::new(value).starts_with(&dir.root),
                "{key} points outside the directory at {value}"
            );
        }
        assert!(launch.log.starts_with(&dir.root));
        Ok(())
    }

    /// Two viewers of one run are confined apart: sharing a state directory
    /// is two viewers fighting over one settings file.
    #[test]
    fn two_viewer_directories_share_no_state() -> Result<(), TestError> {
        let first = ViewerDir::new("/tmp/run/viewer-a")?;
        let second = ViewerDir::new("/tmp/run/viewer-b")?;
        for ((key, one), (_key, other)) in first.confined_env().iter().zip(&second.confined_env()) {
            assert!(one != other, "{key} is shared: {one}");
        }
        assert!(first.log() != second.log());
        Ok(())
    }

    /// Arguments and environment entries keep the order they were added in:
    /// a flag and its value are two arguments, and swapping them is a
    /// different command.
    #[test]
    fn arguments_and_environment_keep_their_order() {
        let launch = Launch::new("a", "viewer", "/tmp/viewer.log")
            .arg("--headless")
            .args(["--automation-socket", "/run/a.sock"])
            .env("A", "1")
            .envs([("B", "2")]);
        assert_eq!(
            launch.args,
            ["--headless", "--automation-socket", "/run/a.sock"]
        );
        assert_eq!(
            launch.env,
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
    }
}
