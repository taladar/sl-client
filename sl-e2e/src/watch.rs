//! Whether a person watches the run: `SL_E2E_WATCH` opens each viewer
//! process's `--watch` window, so a live run can be followed on screen.
//!
//! The process backend only. An in-process viewer is stepped by its host
//! rather than run by winit's event loop, so a watch window has nothing to
//! drive it; a run that asks for one there is refused rather than run
//! unwatched.

use crate::backend::Backend;
use crate::error::StageError;

/// The environment variable that opens the watch windows.
pub const WATCH_VARIABLE: &str = "SL_E2E_WATCH";

/// Whether `value` (an `SL_E2E_WATCH` value) asks for watch windows on
/// `backends`; unset, empty and the usual falsey words are no.
///
/// # Errors
///
/// [`StageError::WatchVariable`] for a value that is neither yes nor no,
/// and [`StageError::WatchInProcess`] when it is yes and `backends` include
/// the in-process one.
pub fn selected(value: Option<&str>, backends: &[Backend]) -> Result<bool, StageError> {
    let watch = match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("" | "0" | "false" | "no" | "off") => false,
        Some("1" | "true" | "yes" | "on") => true,
        Some(_other) => {
            return Err(StageError::WatchVariable(
                value.unwrap_or_default().to_owned(),
            ));
        }
    };
    if watch && backends.contains(&Backend::InProcess) {
        return Err(StageError::WatchInProcess);
    }
    Ok(watch)
}

/// Whether the environment asks for watch windows on `backends`.
///
/// # Errors
///
/// As [`selected`].
pub fn from_env(backends: &[Backend]) -> Result<bool, StageError> {
    selected(std::env::var(WATCH_VARIABLE).ok().as_deref(), backends)
}

#[cfg(test)]
mod tests {
    use super::selected;
    use crate::backend::Backend;
    use crate::error::StageError;

    /// Unset and the falsey words are no; a yes opens the windows on the
    /// process backend and is refused where an in-process run would be left
    /// unwatched; anything else is refused.
    #[test]
    fn the_variable_opens_watch_windows_on_the_process_backend() -> Result<(), StageError> {
        let process = [Backend::Process];
        assert!(!selected(None, &process)?);
        assert!(!selected(Some(" Off "), &process)?);
        assert!(selected(Some("1"), &process)?);
        assert!(!selected(Some("0"), &[Backend::InProcess])?);
        assert!(matches!(
            selected(Some("yes"), &[Backend::InProcess, Backend::Process]),
            Err(StageError::WatchInProcess)
        ));
        assert!(matches!(
            selected(Some("maybe"), &process),
            Err(StageError::WatchVariable(value)) if value == "maybe"
        ));
        Ok(())
    }
}
