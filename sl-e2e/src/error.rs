//! Why a stage could not be set up, run or taken down.

use std::path::PathBuf;

use crate::backend::Backend;

/// What a test body returns when it fails: any error, boxed.
pub type BodyError = Box<dyn core::error::Error + Send + Sync>;

/// Why a stage failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StageError {
    /// `SL_E2E_BACKEND` names no backend.
    #[error("SL_E2E_BACKEND={0:?} is not one of process, in-process, both")]
    BackendVariable(String),
    /// A viewer label is not usable as an account's last name.
    #[error("viewer label {0:?} must be non-empty ASCII letters and digits, and unique")]
    Label(String),
    /// The stage names no viewer by this label.
    #[error("the stage has no viewer {0:?}")]
    UnknownViewer(String),
    /// The process backend was asked for, but the stage has no viewer binary.
    #[error("the process backend needs the viewer binary: call StageBuilder::viewer_binary")]
    NoBinary,
    /// The artifact directory could not be found or made.
    #[error("the artifact directory {path}: {source}")]
    Artifacts {
        /// The directory.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The tokio runtime could not be built.
    #[error("building the stage's runtime: {0}")]
    Runtime(#[source] std::io::Error),
    /// The fake grid failed.
    #[error("the fake grid: {0}")]
    Grid(#[from] sl_fake_grid::Error),
    /// The grid has no account for a viewer it was just given.
    #[error("the grid has no account for viewer {0:?}")]
    NoAccount(String),
    /// An in-process viewer could not be hosted.
    #[error("the in-process host: {0}")]
    Host(#[from] sl_viewer_automation::HostError),
    /// A viewer process could not be launched.
    #[error("launching viewer {viewer}: {source}")]
    Launch {
        /// The viewer.
        viewer: String,
        /// Why.
        source: sl_viewer_launch::Error,
    },
    /// A viewer process ended before its automation socket answered.
    #[error(
        "viewer {viewer} ended ({ending}) before its automation socket answered; its log is {log}"
    )]
    EndedEarly {
        /// The viewer.
        viewer: String,
        /// How it ended.
        ending: String,
        /// Its log.
        log: PathBuf,
    },
    /// A viewer's automation socket did not answer in time.
    #[error("viewer {viewer}'s automation socket did not answer within {seconds} s: {last}")]
    NoSocket {
        /// The viewer.
        viewer: String,
        /// How long was waited.
        seconds: u64,
        /// The last connection error.
        last: String,
    },
    /// A request to a viewer failed: the login wait, a marker wait.
    #[error("viewer {viewer}: {source}")]
    Driver {
        /// The viewer.
        viewer: String,
        /// Why.
        #[source]
        source: sl_viewer_driver::DriverError,
    },
    /// A marker could not be sent to a viewer's session.
    #[error("sending marker {marker:?} to viewer {viewer}: {reason}")]
    Mark {
        /// The viewer.
        viewer: String,
        /// The marker.
        marker: String,
        /// Why.
        reason: String,
    },
    /// A viewer did not log out and exit when asked.
    #[error("viewer {viewer} did not log out when asked: {reason}")]
    NoLogout {
        /// The viewer.
        viewer: String,
        /// What happened instead.
        reason: String,
    },
    /// The grid still holds sessions once every viewer has stopped.
    #[error("the grid still holds a session for {0:?} after every viewer stopped")]
    Stranded(Vec<String>),
    /// The test body failed on one backend.
    #[error("the test body failed on the {backend} backend: {source}")]
    Body {
        /// The backend.
        backend: Backend,
        /// Why.
        source: BodyError,
    },
}
