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
//! quit, with the same keys. Firestorm's half is `FSTestHarness::
//! writeStatus`; this is ours, and the schema is its schema:
//!
//! ```json
//! {
//!   "ok": true,
//!   "reason": "complete",
//!   "frames_written": 30,
//!   "frames_expected": 30,
//!   "viewer": "sl-client",
//!   "day_position": {
//!     "requested": 0.5,
//!     "honoured": true,
//!     "detail": "sampled the region's day cycle at 0.5"
//!   }
//! }
//! ```
//!
//! `reason` is prose for a person reading a failed run, not a code to match on:
//! the runner prints it. `viewer` names which half of the pair wrote the file,
//! so a directory that was copied or collected out of order still says what it
//! holds.
//!
//! `day_position` is absent when the run pinned no sun, and present whenever it
//! did — **including when the pin could not be honoured**, which is the case it
//! exists for. A capture taken under lighting that is not the lighting that was
//! asked for is not a capture of the requested scene, and the only report of one
//! used to be a line in a viewer's own log, which nobody reads until after they
//! have believed the frames.
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
/// parses the same keys from both viewers' files.
///
/// (Not `Eq`: a recorded day position is an `f32`.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// What became of a pinned sun, when the run pinned one.
    ///
    /// `#[serde(default)]` on the way in, so a status written by a viewer build
    /// that predates this field still parses. The runner tells the two apart:
    /// a run that asked for a day position and gets a status without this key
    /// back has learned nothing about its lighting, which is not the same as
    /// having learned that it was fine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_position: Option<DayPositionStatus>,
}

/// What a run's pinned day position selected, as the status file carries it.
///
/// (Not `Eq`: `requested` is the `f32` position that was asked for.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayPositionStatus {
    /// The position the run asked for, `0.0..=1.0`.
    pub requested: f32,
    /// Whether the frames were taken under the sky the **region** serves at that
    /// position. False when anything stood in for it — a viewer's own substitute
    /// cycle, a fixed sky, or a cycle that cannot be sampled at all.
    pub honoured: bool,
    /// Prose saying what happened, for whoever reads the run.
    pub detail: String,
}

impl HarnessStatus {
    /// A status from this viewer, with [`VIEWER_NAME`] filled in and no sun
    /// pinned; [`with_day_position`](Self::with_day_position) adds one.
    #[must_use]
    pub fn new(ok: bool, reason: impl Into<String>, written: usize, expected: usize) -> Self {
        Self {
            ok,
            reason: reason.into(),
            frames_written: written,
            frames_expected: expected,
            viewer: VIEWER_NAME.to_owned(),
            day_position: None,
        }
    }

    /// Record what the run's pinned day position selected.
    ///
    /// An **unhonoured** pin also fails the status outright: the frames exist,
    /// but they are not frames of the scene that was asked for, and a run that
    /// reported them as a success would be inviting a person to compare two
    /// viewers' skies without either of them having been told which sky to draw.
    #[must_use]
    pub fn with_day_position(mut self, day_position: DayPositionStatus) -> Self {
        if !day_position.honoured {
            self.ok = false;
            self.reason = format!(
                "the run asked for day position {} and did not get it: {}",
                day_position.requested, day_position.detail
            );
        }
        self.day_position = Some(day_position);
        self
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

    use super::{DayPositionStatus, HarnessStatus, VIEWER_NAME};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// The five always-present keys are the schema Firestorm's `writeStatus`
    /// emits: a rename here silently halves a cross-check, because the runner
    /// reads both files with one parser. `day_position` is the sixth and is
    /// written only by a run that pinned one.
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

    /// A pin that could not be honoured fails the run and says why in the
    /// `reason`, so a person reading the printed report meets it there rather
    /// than having to open the JSON — the whole point of moving this out of the
    /// viewer's log.
    #[test]
    fn an_unhonoured_pin_fails_the_run() -> Result<(), TestError> {
        let status =
            HarnessStatus::new(true, "complete", 30, 30).with_day_position(DayPositionStatus {
                requested: 0.5,
                honoured: false,
                detail: "the region's day cycle schedules one sky".to_owned(),
            });
        assert!(!status.ok);
        assert!(status.reason.contains("did not get it"));
        assert!(status.reason.contains("schedules one sky"));
        // The frames are still counted: they exist, they are just not frames of
        // the scene that was asked for.
        assert_eq!(status.frames_written, 30);

        let honoured =
            HarnessStatus::new(true, "complete", 30, 30).with_day_position(DayPositionStatus {
                requested: 0.5,
                honoured: true,
                detail: "sampled the region's day cycle at 0.5".to_owned(),
            });
        assert!(honoured.ok);
        assert_eq!(honoured.reason, "complete");
        let round_trip: HarnessStatus = serde_json::from_str(&serde_json::to_string(&honoured)?)?;
        assert_eq!(round_trip, honoured);
        Ok(())
    }

    /// A status written by a build that predates the day-position key still
    /// parses — and reads as "this run said nothing about its lighting", which
    /// the runner must not confuse with "the lighting was fine".
    #[test]
    fn a_status_without_a_day_position_still_parses() -> Result<(), TestError> {
        let parsed: HarnessStatus = serde_json::from_str(
            r#"{"ok":true,"reason":"complete","frames_written":30,"frames_expected":30,"viewer":"firestorm"}"#,
        )?;
        assert_eq!(parsed.day_position, None);
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
