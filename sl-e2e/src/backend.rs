//! Which way a stage runs its viewers: as processes, or as Apps in the test
//! process — chosen by the environment, never by rewriting a test.

use core::fmt;

use crate::error::StageError;

/// The environment variable that picks the backends a stage runs on.
pub const BACKEND_VARIABLE: &str = "SL_E2E_BACKEND";

/// How a stage's viewers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Each viewer is the real binary, `--headless --automation-socket`,
    /// reached over its socket.
    Process,
    /// Each viewer is the viewer's own builder's App, stepped on an
    /// in-process host's thread and reached over its link.
    InProcess,
}

impl Backend {
    /// The backend's name: in `SL_E2E_BACKEND`, in the artifact directory, in
    /// errors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::InProcess => "in-process",
        }
    }

    /// The backends `value` (an `SL_E2E_BACKEND` value) names; unset is
    /// `both`, so a test covers both unless told otherwise.
    ///
    /// # Errors
    ///
    /// [`StageError::BackendVariable`] for anything but `process`,
    /// `in-process` or `both`.
    pub fn selected(value: Option<&str>) -> Result<Vec<Self>, StageError> {
        match value.map(str::trim) {
            None | Some("both" | "") => Ok(vec![Self::InProcess, Self::Process]),
            Some("process") => Ok(vec![Self::Process]),
            Some("in-process") => Ok(vec![Self::InProcess]),
            Some(other) => Err(StageError::BackendVariable(other.to_owned())),
        }
    }

    /// The backends the environment selects.
    ///
    /// # Errors
    ///
    /// As [`selected`](Self::selected).
    pub fn from_env() -> Result<Vec<Self>, StageError> {
        Self::selected(std::env::var(BACKEND_VARIABLE).ok().as_deref())
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::Backend;
    use crate::error::StageError;

    /// Unset runs both, in-process first; each name runs its own; anything
    /// else is refused rather than silently running one.
    #[test]
    fn the_variable_selects_the_backends() -> Result<(), StageError> {
        assert_eq!(
            Backend::selected(None)?,
            [Backend::InProcess, Backend::Process]
        );
        assert_eq!(
            Backend::selected(Some("both"))?,
            [Backend::InProcess, Backend::Process]
        );
        assert_eq!(Backend::selected(Some("process"))?, [Backend::Process]);
        assert_eq!(
            Backend::selected(Some(" in-process "))?,
            [Backend::InProcess]
        );
        assert!(matches!(
            Backend::selected(Some("inprocess")),
            Err(StageError::BackendVariable(value)) if value == "inprocess"
        ));
        Ok(())
    }
}
