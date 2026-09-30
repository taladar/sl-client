//! What can go wrong driving a viewer, and what a failure leaves behind.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use sl_automation_proto::{AutomationError, FailureReport};

/// Why a driver call did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// The viewer's automation socket could not be reached.
    #[error("could not connect to the automation socket {}: {source}", .path.display())]
    Connect {
        /// The socket.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The viewer speaks another version of the protocol.
    #[error("viewer {viewer} speaks automation protocol {found}, this driver speaks {expected}")]
    Protocol {
        /// The viewer.
        viewer: String,
        /// The version this driver speaks.
        expected: u32,
        /// The version the viewer reported.
        found: u32,
    },
    /// The connection to the viewer closed with the request unanswered: the
    /// viewer exited, or its host stopped.
    #[error("the connection to viewer {viewer} closed before {what} was answered")]
    Closed {
        /// The viewer.
        viewer: String,
        /// What was asked.
        what: String,
    },
    /// The viewer did not answer in time — long past the request's own
    /// deadline, so it has stopped answering.
    #[error("viewer {viewer} gave no answer to {what} within {}s", .waited.as_secs_f32())]
    NoAnswer {
        /// The viewer.
        viewer: String,
        /// What was asked.
        what: String,
        /// How long the driver waited.
        waited: Duration,
    },
    /// The viewer answered with something other than what the request
    /// answers with.
    #[error("viewer {viewer} answered {what} with {got}")]
    Unexpected {
        /// The viewer.
        viewer: String,
        /// What was asked.
        what: String,
        /// What came back, printed.
        got: String,
    },
    /// The viewer carried out the request and it failed — or an
    /// expectation did not come to hold. Carries the viewer's error, its
    /// report and the artifacts saved.
    #[error("{0}")]
    Failed(Box<Failure>),
}

impl DriverError {
    /// The viewer's own error, for a failure the viewer reported.
    #[must_use]
    pub fn automation_error(&self) -> Option<&AutomationError> {
        match self {
            Self::Failed(failure) => Some(&failure.error),
            Self::Connect { .. }
            | Self::Protocol { .. }
            | Self::Closed { .. }
            | Self::NoAnswer { .. }
            | Self::Unexpected { .. } => None,
        }
    }

    /// The failure, with its report and artifacts, for a failure the viewer
    /// reported.
    #[must_use]
    pub fn failure(&self) -> Option<&Failure> {
        match self {
            Self::Failed(failure) => Some(failure),
            Self::Connect { .. }
            | Self::Protocol { .. }
            | Self::Closed { .. }
            | Self::NoAnswer { .. }
            | Self::Unexpected { .. } => None,
        }
    }
}

/// A request or an expectation that failed in the viewer: what was being
/// done, the viewer's error, the report it sent with it, and the files saved
/// to explain it.
#[derive(Debug)]
pub struct Failure {
    /// The viewer, by its label.
    pub viewer: String,
    /// What was being done: `click button[name_key=build-apply]`,
    /// `expect [test_id=floater:inventory] to be visible`.
    pub action: String,
    /// The viewer's error.
    pub error: AutomationError,
    /// What the viewer sent with it: the tree near the scope, the event tail,
    /// the warnings logged while it ran.
    pub report: Option<FailureReport>,
    /// The files saved for it.
    pub artifacts: Artifacts,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "viewer {}: {} failed: {}",
            self.viewer, self.action, self.error
        )?;
        write!(f, "{}", self.artifacts)
    }
}

/// The files a failure left in the viewer's artifact directory: a screenshot
/// with the locator's matches outlined, the semantic tree around the scope,
/// and the event tail with the warnings logged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Artifacts {
    /// The directory they are in; `None` when the viewer has none, and then
    /// nothing is saved.
    pub dir: Option<PathBuf>,
    /// The screenshot, or why there is none.
    pub screenshot: Option<Result<PathBuf, String>>,
    /// The semantic tree, as text, or why there is none.
    pub tree: Option<Result<PathBuf, String>>,
    /// The event tail and the warnings logged, as text, or why there are
    /// none.
    pub events: Option<Result<PathBuf, String>>,
}

impl fmt::Display for Artifacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dir.is_none() {
            return f.write_str("\n  (no artifact directory: nothing saved)");
        }
        for (label, artifact) in [
            ("screenshot", &self.screenshot),
            ("tree", &self.tree),
            ("events", &self.events),
        ] {
            match artifact {
                Some(Ok(path)) => write!(f, "\n  {label}: {}", path.display())?,
                Some(Err(reason)) => write!(f, "\n  {label}: not saved ({reason})")?,
                None => {}
            }
        }
        Ok(())
    }
}
