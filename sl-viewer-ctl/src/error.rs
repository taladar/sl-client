//! What can go wrong running a command.

use std::io;
use std::path::PathBuf;

use sl_viewer_driver::DriverError;

/// Why a command did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum CtlError {
    /// Driving the viewer failed: an action, a wait, a read — or the viewer
    /// could not be reached.
    #[error(transparent)]
    Driver(#[from] DriverError),
    /// Printing a result failed.
    #[error("printing: {0}")]
    Output(#[from] io::Error),
    /// No socket was given and none could be found.
    #[error("{0}")]
    NoSocket(String),
    /// A file could not be read or written.
    #[error("{}: {source}", .path.display())]
    File {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// A stage file does not say what it must.
    #[error("the stage file {}: {reason}", .path.display())]
    StageFile {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// A viewer could not be launched, or ended before it was ready.
    #[error("viewer {viewer}: {reason}")]
    Launch {
        /// The viewer.
        viewer: String,
        /// What went wrong.
        reason: String,
    },
    /// The fake grid did not start.
    #[error("starting the fake grid: {0}")]
    Grid(String),
}
