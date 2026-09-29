//! [`FailureReport`]: what a failed request's response carries beyond the
//! error itself, so a failure explains itself without a second round trip.

use serde::{Deserialize, Serialize};

use crate::probe::{DiagnosticLine, LogEntry};
use crate::snapshot::UiNode;

/// The context of a failure, attached to every error response.
///
/// The error says what failed — the check, the candidates, the last nodes or
/// state observed. The report says what was around it:
///
/// - **`tree`**: for a failure on a UI locator, an excerpt of the semantic
///   tree around its scope — the scope's subtree when the locator has a scope
///   that resolves, else the top of the whole tree — cut off a few levels
///   down and after a few hundred nodes. Empty for any other failure.
/// - **`events`**: the tail of the event log (session events, outbound
///   commands, UI actions) as it stood when the request failed, oldest first;
///   empty in a viewer that keeps no event log.
/// - **`diagnostics`**: the warnings and errors the viewer logged while the
///   request ran, oldest first.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FailureReport {
    /// The semantic tree around the scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tree: Vec<UiNode>,
    /// The last entries of the event log.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LogEntry>,
    /// The warnings and errors logged while the request ran.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticLine>,
}
