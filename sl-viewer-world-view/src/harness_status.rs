//! `harness-status.json`: the one file a driving harness reads to decide
//! whether a capture run happened at all.
//!
//! A cross-check run photographs this viewer and Firestorm against the same
//! grid and puts the frames side by side. Two outcomes must never be reported
//! the same way: *the viewers drew different things* is a finding, and *one of
//! them never got in world* is a broken run. An exit code cannot carry that
//! distinction — a viewer's shutdown path is a logout, a grace period and a
//! window teardown, none of which reliably survive into a status — and a
//! directory of frames cannot either, because a viewer that never logged in
//! still writes a full set of them, black and on schedule.
//!
//! So both viewers write this file into their `--screenshot-dir` before they
//! quit, with the same five keys. Firestorm's half is `FSTestHarness::
//! writeStatus`; this is ours, and the schema is its schema:
//!
//! ```json
//! {
//!   "ok": true,
//!   "reason": "complete",
//!   "frames_written": 30,
//!   "frames_expected": 30,
//!   "viewer": "sl-client"
//! }
//! ```
//!
//! `reason` is prose for a person reading a failed run, not a code to match on:
//! the runner prints it. `viewer` names which half of the pair wrote the file,
//! so a directory that was copied or collected out of order still says what it
//! holds.
//!
//! **A missing file is itself the answer**: the run did not reach the point of
//! writing one (a crash, a `SIGKILL`, a viewer that never started). That is why
//! nothing here has a default and why the writer runs before the logout rather
//! than after it.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// What this viewer calls itself in the `viewer` field. Firestorm writes
/// `"firestorm"` in the same place.
pub const VIEWER_NAME: &str = "sl-client";

/// The contents of `harness-status.json` — see [the module docs](self).
///
/// [`Deserialize`] as well as [`Serialize`] so the schema has one definition
/// and its own tests can read back what they wrote; the cross-check runner
/// parses the same five keys from both viewers' files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStatus {
    /// Whether the run did what it was asked to do. A run that captured its
    /// frames is `true` even if the scene never went quiet — the frames are
    /// still comparable, and `reason` says what happened.
    pub ok: bool,
    /// Prose for whoever reads a failed run: what the harness was doing when it
    /// stopped.
    pub reason: String,
    /// How many frames actually reached the disk.
    pub frames_written: usize,
    /// How many frames the run was asked for, so a short run is visible without
    /// counting files.
    pub frames_expected: usize,
    /// Which viewer wrote this file: [`VIEWER_NAME`] here, `"firestorm"` there.
    pub viewer: String,
}

impl HarnessStatus {
    /// A status from this viewer, with [`VIEWER_NAME`] filled in.
    #[must_use]
    pub fn new(ok: bool, reason: impl Into<String>, written: usize, expected: usize) -> Self {
        Self {
            ok,
            reason: reason.into(),
            frames_written: written,
            frames_expected: expected,
            viewer: VIEWER_NAME.to_owned(),
        }
    }

    /// Write `harness-status.json` into `dir`.
    ///
    /// # Errors
    ///
    /// Returns the serialisation or write error. The caller logs it rather than
    /// failing the run: the frames are already on disk, and a harness that
    /// cannot read a status treats the run as one that did not happen — which
    /// is the right conclusion from an unwritable directory anyway.
    pub fn write(&self, dir: &Path) -> Result<(), StatusError> {
        let path = dir.join("harness-status.json");
        let json = serde_json::to_string_pretty(self)?;
        fs_err::write(&path, json.as_bytes())?;
        Ok(())
    }
}

/// Why a [`HarnessStatus`] could not be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StatusError {
    /// The status could not be serialised.
    #[error("serialising the harness status: {0}")]
    Serialise(#[from] serde_json::Error),
    /// The status file could not be written.
    #[error("writing harness-status.json: {0}")]
    Write(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{HarnessStatus, VIEWER_NAME};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// The five keys are the schema Firestorm's `writeStatus` emits: a rename
    /// here silently halves a cross-check, because the runner reads both files
    /// with one parser.
    #[test]
    fn the_status_carries_firestorms_five_keys() -> Result<(), TestError> {
        let status = HarnessStatus::new(true, "complete", 30, 30);
        let value: serde_json::Value = serde_json::from_str(&serde_json::to_string(&status)?)?;
        let object = value.as_object().ok_or("the status is a JSON object")?;
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "frames_expected",
                "frames_written",
                "ok",
                "reason",
                "viewer"
            ]
        );
        assert_eq!(
            object.get("viewer").and_then(serde_json::Value::as_str),
            Some(VIEWER_NAME)
        );
        Ok(())
    }

    /// A failed run still carries its frame counts: "it wrote 4 of 30" is the
    /// first thing worth knowing about one, and the runner prints it.
    #[test]
    fn a_failed_run_still_counts_its_frames() -> Result<(), TestError> {
        let status = HarnessStatus::new(false, "login not completed", 4, 30);
        let round_trip: HarnessStatus = serde_json::from_str(&serde_json::to_string(&status)?)?;
        assert_eq!(round_trip, status);
        assert_eq!(round_trip.frames_written, 4);
        assert_eq!(round_trip.frames_expected, 30);
        Ok(())
    }

    /// The file lands in the directory the frames went to, under the name both
    /// viewers agree on.
    #[test]
    fn the_file_is_written_beside_the_frames() -> Result<(), TestError> {
        let dir =
            std::env::temp_dir().join(format!("sl-viewer-harness-status-{}", std::process::id()));
        fs_err::create_dir_all(&dir)?;
        HarnessStatus::new(true, "complete", 2, 2).write(&dir)?;
        let text = fs_err::read_to_string(dir.join("harness-status.json"))?;
        fs_err::remove_dir_all(&dir)?;
        let parsed: HarnessStatus = serde_json::from_str(&text)?;
        assert!(parsed.ok);
        assert_eq!(parsed.reason, "complete");
        Ok(())
    }
}
