//! Reading back what a viewer left behind: its frames, its scene dump, and the
//! status file that says whether the run happened at all.
//!
//! Both viewers write `harness-status.json` into their capture directory before
//! they log out, with the same five keys — this crate's reader is the one parser
//! for both. The distinction that matters is between *a status that says the run
//! failed* and *no status at all*: the first is a viewer reporting honestly, the
//! second is a run that never reached the point of reporting, and telling a
//! person "firestorm: failed" when the truth is "firestorm never started" sends
//! them looking for a rendering bug in a run that produced no rendering.
//!
//! Nothing here judges the frames. Whether the two viewers drew the same thing
//! is a separate question with a separate answer; this module answers only
//! "is there something to compare".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The contents of `harness-status.json`, as both viewers write it.
#[expect(
    clippy::module_name_repetitions,
    reason = "the type is named after the file it parses, which both viewers write under that \
              name; renaming it here would only hide the correspondence"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStatus {
    /// Whether the run did what it was asked to do.
    pub ok: bool,
    /// Prose saying what the harness was doing when it stopped.
    pub reason: String,
    /// How many frames reached the disk, by the viewer's own count.
    pub frames_written: usize,
    /// How many frames the run asked for.
    pub frames_expected: usize,
    /// Which viewer wrote the file (`sl-client` / `firestorm`).
    pub viewer: String,
}

/// What was found where a viewer's status file should have been.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    /// The viewer reported: the run happened, and this is what it says.
    Reported {
        /// What it said.
        #[serde(flatten)]
        status: HarnessStatus,
    },
    /// No status file. The run did not reach the point of writing one — it
    /// crashed, was killed, or never started.
    Missing,
    /// A status file that could not be read or parsed, which is a broken run
    /// rather than a failed one; the message says what was wrong with it.
    Unreadable {
        /// Why it could not be read.
        problem: String,
    },
}

impl Status {
    /// Whether the run happened *and* the viewer called it a success. A missing
    /// or unreadable status is never a success.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::Reported { status } if status.ok)
    }

    /// Whether a run happened at all, however it went.
    #[must_use]
    pub const fn happened(&self) -> bool {
        matches!(self, Self::Reported { .. })
    }

    /// One line for the printed report.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Reported { status } => format!(
                "{} — {} ({}/{} frames)",
                if status.ok { "ok" } else { "FAILED" },
                status.reason,
                status.frames_written,
                status.frames_expected
            ),
            Self::Missing => {
                "NO STATUS — the run did not happen (no harness-status.json was written)".to_owned()
            }
            Self::Unreadable { problem } => {
                format!("NO STATUS — harness-status.json could not be read: {problem}")
            }
        }
    }
}

/// Everything one viewer left in its capture directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artefacts {
    /// The captured frames, in name order — which is capture order, the files
    /// being numbered.
    pub frames: Vec<PathBuf>,
    /// The structured scene dump, when the viewer wrote one.
    pub scene_dump: Option<PathBuf>,
    /// What the status file said, or that there was none.
    pub status: Status,
}

impl Artefacts {
    /// Collect what is in `dir`.
    ///
    /// A directory that does not exist collects as an empty run with a missing
    /// status, not as an error: "the viewer never wrote anything" is a result
    /// the report should print, not a failure of the collection.
    #[must_use]
    pub fn collect(dir: &Path) -> Self {
        Self {
            frames: frames_in(dir),
            scene_dump: exists(dir.join("scene.json")),
            status: read_status(dir),
        }
    }
}

/// The `frame_NNN.png` files in `dir`, sorted by name.
///
/// By name rather than by modification time: the names are zero-padded and so
/// sort into capture order, while two frames written in the same second are not
/// ordered by their timestamps at all.
fn frames_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs_err::read_dir(dir) else {
        return Vec::new();
    };
    let mut frames: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("frame_")
                        && std::path::Path::new(name)
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
                })
        })
        .collect();
    frames.sort();
    frames
}

/// `path` if there is a file there.
fn exists(path: PathBuf) -> Option<PathBuf> {
    fs_err::metadata(&path)
        .is_ok_and(|metadata| metadata.is_file())
        .then_some(path)
}

/// Read `harness-status.json` from `dir`.
fn read_status(dir: &Path) -> Status {
    let path = dir.join("harness-status.json");
    let text = match fs_err::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Status::Missing,
        Err(error) => {
            return Status::Unreadable {
                problem: error.to_string(),
            };
        }
    };
    match serde_json::from_str(&text) {
        Ok(status) => Status::Reported { status },
        Err(error) => Status::Unreadable {
            problem: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Artefacts, Status};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A scratch directory of this test's own.
    fn scratch(name: &str) -> Result<std::path::PathBuf, TestError> {
        let dir = std::env::temp_dir().join(format!(
            "sl-crosscheck-status-{name}-{}",
            std::process::id()
        ));
        let _ignored = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The reader is one parser for both viewers: this is Firestorm's file,
    /// written by its C++ half, and it must read as the same five fields.
    #[test]
    fn firestorms_own_status_file_reads() -> Result<(), TestError> {
        let dir = scratch("firestorm")?;
        fs_err::write(
            dir.join("harness-status.json"),
            br#"{"frames_expected":30,"frames_written":30,"ok":true,"reason":"complete","viewer":"firestorm"}"#,
        )?;
        let artefacts = Artefacts::collect(&dir);
        assert!(artefacts.status.succeeded());
        assert!(artefacts.status.happened());
        let Status::Reported { status } = &artefacts.status else {
            return Err("the status should have been reported".into());
        };
        assert_eq!(status.viewer, "firestorm");
        assert_eq!(status.frames_written, 30);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A run that never wrote a status is not a failed run: it is a run that did
    /// not happen, and the two must not read the same way — one sends a person
    /// hunting a rendering bug, the other a broken launch.
    #[test]
    fn a_missing_status_is_not_a_failed_run() -> Result<(), TestError> {
        let dir = scratch("missing")?;
        let artefacts = Artefacts::collect(&dir);
        assert_eq!(artefacts.status, Status::Missing);
        assert!(!artefacts.status.happened());
        assert!(artefacts.status.describe().contains("did not happen"));

        fs_err::write(
            dir.join("harness-status.json"),
            br#"{"frames_expected":30,"frames_written":0,"ok":false,"reason":"login not completed","viewer":"sl-client"}"#,
        )?;
        let failed = Artefacts::collect(&dir);
        assert!(failed.status.happened(), "a failed run still happened");
        assert!(!failed.status.succeeded());
        assert!(failed.status.describe().contains("login not completed"));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// Frames come back in capture order, and a dump is noticed when there is
    /// one — a viewer that writes no scene dump yet is a fact about the run, not
    /// an error.
    #[test]
    fn frames_are_collected_in_capture_order() -> Result<(), TestError> {
        let dir = scratch("frames")?;
        for name in [
            "frame_002.png",
            "frame_000.png",
            "frame_001.png",
            "notes.txt",
        ] {
            fs_err::write(dir.join(name), b"")?;
        }
        let artefacts = Artefacts::collect(&dir);
        let names: Vec<String> = artefacts
            .frames
            .iter()
            .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert_eq!(names, ["frame_000.png", "frame_001.png", "frame_002.png"]);
        assert_eq!(artefacts.scene_dump, None);

        fs_err::write(dir.join("scene.json"), b"{}")?;
        assert!(Artefacts::collect(&dir).scene_dump.is_some());
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A truncated status file is a broken run, not a silently successful one.
    #[test]
    fn an_unreadable_status_is_not_a_success() -> Result<(), TestError> {
        let dir = scratch("unreadable")?;
        fs_err::write(dir.join("harness-status.json"), b"{ this is not json")?;
        let artefacts = Artefacts::collect(&dir);
        assert!(!artefacts.status.succeeded());
        assert!(!artefacts.status.happened());
        assert!(matches!(artefacts.status, Status::Unreadable { .. }));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
