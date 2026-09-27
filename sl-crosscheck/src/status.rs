//! Reading back what a viewer left behind: its frames, its scene dump, and the
//! status file that says whether the run happened at all.
//!
//! Both viewers write `harness-status.json` into their capture directory before
//! they log out, with the same keys — this crate's reader is the one parser for
//! both. The distinction that matters is between *a status that says the run
//! failed* and *no status at all*: the first is a viewer reporting honestly, the
//! second is a run that never reached the point of reporting, and telling a
//! person "firestorm: failed" when the truth is "firestorm never started" sends
//! them looking for a rendering bug in a run that produced no rendering.
//!
//! The same distinction runs one level down, in `day_position`: a viewer that
//! reports an unhonoured pin has told you its frames are not of the scene you
//! asked for, while a viewer that reports **nothing** about the pin is a build
//! from before the field existed and has told you nothing at all. Both are
//! printed, and neither reads as success.
//!
//! And again in `window_size`, which only a UI capture carries: the reference's
//! snapshot path cannot draw its interface at any size but its window's, so a
//! UI frame from a window the compositor would not resize holds the interface
//! at the wrong scale — or the same grab stitched across the frame. Firestorm
//! reports what the window system gave it; this viewer lays its interface out
//! at the capture size and has no window in the question, so it says nothing,
//! and only from Firestorm is silence a gap.
//!
//! Beside what the viewers *say*, [`FrameSizes`] is what they *wrote*: the
//! pixel size of every frame, read back from the files. A frame of the wrong
//! size is a run that did not do as it was told, whatever its status says.
//!
//! Nothing here judges the frames' contents. Whether the two viewers drew the same thing
//! is a separate question with a separate answer; this module answers only
//! "is there something to compare".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The contents of `harness-status.json`, as both viewers write it.
///
/// (Not `Eq`: a recorded day position is an `f32`.)
#[expect(
    clippy::module_name_repetitions,
    reason = "the type is named after the file it parses, which both viewers write under that \
              name; renaming it here would only hide the correspondence"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// What became of a pinned sun, when the run pinned one — absent when it did
    /// not, and absent from a viewer build that predates the field.
    #[serde(default)]
    pub day_position: Option<DayPositionStatus>,
    /// What the window system made of a UI capture's size, when the viewer's
    /// UI capture depends on its window — Firestorm's does, and writes this
    /// block for every UI capture. Absent otherwise.
    #[serde(default)]
    pub window_size: Option<WindowSizeStatus>,
}

/// Whether a UI capture's window was the size the run asked for, as Firestorm
/// reports it.
#[expect(
    clippy::module_name_repetitions,
    reason = "named for the `window_size` key of the status file this module parses, as \
              `DayPositionStatus` is named for `day_position`"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSizeStatus {
    /// The size the run asked for, `WIDTHxHEIGHT`.
    pub requested: String,
    /// Whether the window was that size when the first frame was taken.
    pub honoured: bool,
    /// Prose saying what happened — on a refusal, the size the window was.
    pub detail: String,
}

/// The pixel sizes of one viewer's frames, read back from the files.
///
/// Counted by size rather than listed per frame: the question is only ever
/// "were they all the size the run asked for", and a run of thirty frames at
/// one size is one entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameSizes {
    /// How many frames were found at each `WIDTHxHEIGHT`.
    pub by_size: BTreeMap<String, usize>,
    /// How many frames could not be read as an image at all.
    pub unreadable: usize,
}

impl FrameSizes {
    /// Read the size of every frame in `frames`. Only the header is read, so
    /// this costs nothing next to the run that wrote them.
    #[must_use]
    pub fn read(frames: &[PathBuf]) -> Self {
        let mut sizes = Self::default();
        for frame in frames {
            match image::image_dimensions(frame) {
                Ok((width, height)) => {
                    let count = sizes
                        .by_size
                        .entry(format!("{width}x{height}"))
                        .or_insert(0);
                    *count = count.saturating_add(1);
                }
                Err(_unreadable) => sizes.unreadable = sizes.unreadable.saturating_add(1),
            }
        }
        sizes
    }

    /// Whether every frame is readable and `size` (`WIDTHxHEIGHT`) — vacuously
    /// true of no frames, which is a different failure reported elsewhere.
    #[must_use]
    pub fn all_at(&self, size: &str) -> bool {
        self.unreadable == 0 && self.by_size.keys().all(|found| found == size)
    }

    /// The report's line when the frames are not all `size`, or `None` when
    /// they are.
    #[must_use]
    pub fn describe_mismatch(&self, size: &str) -> Option<String> {
        if self.all_at(size) {
            return None;
        }
        let mut found: Vec<String> = self
            .by_size
            .iter()
            .map(|(found, count)| format!("{count} at {found}"))
            .collect();
        if self.unreadable > 0 {
            found.push(format!("{} unreadable", self.unreadable));
        }
        Some(format!(
            "FRAMES NOT AT {size} — {}; the run asked for one capture size, so these cannot be \
             paired with the other viewer's",
            found.join(", ")
        ))
    }
}

/// What a run's pinned day position selected, as either viewer reports it.
///
/// (Not `Eq`: `requested` is the `f32` position that was asked for.)
#[expect(
    clippy::module_name_repetitions,
    reason = "named for the `day_position` key of the status file this module parses, as \
              `HarnessStatus` above is named for the file itself; the shared suffix is the \
              correspondence, not an accident of the module it landed in"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayPositionStatus {
    /// The position the run asked for, `0.0..=1.0`.
    pub requested: f32,
    /// Whether the frames were taken under the sky the **region** serves at that
    /// position.
    pub honoured: bool,
    /// Prose saying what happened.
    pub detail: String,
}

/// What was found where a viewer's status file should have been.
///
/// (Not `Eq`: a reported status can carry an `f32` day position.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Whether nothing in this status contradicts the lighting the run asked
    /// for — `asked` being the plan's `--day-position`, or `None` when it pinned
    /// no sun.
    ///
    /// A run that pinned a sun and did **not** report on it is false as surely
    /// as one that reported failing to honour it: a viewer build from before the
    /// field existed cannot tell you which sky it drew, and a comparison of two
    /// skies neither viewer was told to draw is not a comparison of renderers.
    ///
    /// A status that never happened is `true` here, because it is already a
    /// failed run by [`happened`](Self::happened) and saying so twice only
    /// makes the report longer.
    #[must_use]
    pub fn lighting_as_asked(&self, asked: Option<f32>) -> bool {
        if asked.is_none() {
            return true;
        }
        match self {
            Self::Reported { status } => {
                status.day_position.as_ref().is_some_and(|pin| pin.honoured)
            }
            Self::Missing | Self::Unreadable { .. } => true,
        }
    }

    /// The report's line about the run's pinned sun, or `None` when it pinned
    /// none (or never got far enough to say).
    #[must_use]
    pub fn describe_day_position(&self, asked: Option<f32>) -> Option<String> {
        let asked = asked?;
        let Self::Reported { status } = self else {
            return None;
        };
        Some(match &status.day_position {
            Some(pin) if pin.honoured => format!("sun pinned at {asked} — {}", pin.detail),
            Some(pin) => format!("SUN NOT PINNED at {asked} — {}", pin.detail),
            None => format!(
                "SUN NOT REPORTED — the run asked for day position {asked} and this viewer said \
                 nothing about it; it predates the day_position field"
            ),
        })
    }

    /// Whether nothing in this status contradicts the window size a UI capture
    /// needs. `capture_ui` is whether the run captured the interface at all;
    /// `follows_window` is whether this viewer's interface is drawn at its
    /// window's size, which is when silence about the window is a gap rather
    /// than an answer.
    ///
    /// A status that never happened is `true` here, for the reason
    /// [`lighting_as_asked`](Self::lighting_as_asked) gives.
    #[must_use]
    pub fn window_as_asked(&self, capture_ui: bool, follows_window: bool) -> bool {
        if !capture_ui {
            return true;
        }
        match self {
            Self::Reported { status } => status
                .window_size
                .as_ref()
                .map_or(!follows_window, |window| window.honoured),
            Self::Missing | Self::Unreadable { .. } => true,
        }
    }

    /// The report's line about a UI capture's window, or `None` when there is
    /// nothing to say — no UI in the frames, no status, or a viewer whose
    /// interface does not follow its window and did not mention one.
    #[must_use]
    pub fn describe_window_size(&self, capture_ui: bool, follows_window: bool) -> Option<String> {
        if !capture_ui {
            return None;
        }
        let Self::Reported { status } = self else {
            return None;
        };
        match &status.window_size {
            Some(window) if window.honoured => Some(format!(
                "window {} as asked — {}",
                window.requested, window.detail
            )),
            Some(window) => Some(format!(
                "WINDOW NOT {} — {}",
                window.requested, window.detail
            )),
            None if follows_window => Some(
                "WINDOW NOT REPORTED — this viewer draws its interface at its window's size and \
                 said nothing about the window; it predates the window_size field"
                    .to_owned(),
            ),
            None => None,
        }
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
///
/// (Not `Eq`: the status it holds can carry an `f32` day position.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artefacts {
    /// The captured frames, in name order — which is capture order, the files
    /// being numbered.
    pub frames: Vec<PathBuf>,
    /// The pixel sizes of those frames, as the files have them. Defaulted on
    /// the way in so a `run.json` from before the field still reads.
    #[serde(default)]
    pub frame_sizes: FrameSizes,
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
        let frames = frames_in(dir);
        Self {
            frame_sizes: FrameSizes::read(&frames),
            frames,
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

    /// Firestorm's `window_size` block, verbatim from a refused resize: it
    /// parses, it is not a window as asked, and the report quotes its detail.
    #[test]
    fn firestorms_window_size_block_reads() -> Result<(), TestError> {
        let dir = scratch("window")?;
        fs_err::write(
            dir.join("harness-status.json"),
            br#"{"frames_expected":2,"frames_written":2,"ok":true,"reason":"complete","viewer":"firestorm","window_size":{"detail":"the window is 1024x738 and the run asked for 1920x1080","honoured":false,"requested":"1920x1080"}}"#,
        )?;
        let status = Artefacts::collect(&dir).status;
        assert!(status.succeeded(), "the status itself says ok");
        assert!(!status.window_as_asked(true, true));
        assert!(
            status.window_as_asked(false, true),
            "a world-only run does not care"
        );
        let line = status
            .describe_window_size(true, true)
            .ok_or("a refused window must be reported")?;
        assert!(line.contains("WINDOW NOT 1920x1080"), "{line}");
        assert!(line.contains("1024x738"), "{line}");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// Silence about the window is a gap only from a viewer whose interface
    /// follows its window; this one lays its interface out at the capture size.
    #[test]
    fn silence_about_the_window_depends_on_the_viewer() -> Result<(), TestError> {
        let dir = scratch("window-silent")?;
        fs_err::write(
            dir.join("harness-status.json"),
            br#"{"frames_expected":2,"frames_written":2,"ok":true,"reason":"complete","viewer":"sl-client"}"#,
        )?;
        let status = Artefacts::collect(&dir).status;
        assert!(status.window_as_asked(true, false));
        assert_eq!(status.describe_window_size(true, false), None);
        assert!(!status.window_as_asked(true, true));
        assert!(
            status
                .describe_window_size(true, true)
                .is_some_and(|line| line.contains("WINDOW NOT REPORTED"))
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// The frames' sizes come from the files, not from what the viewer says:
    /// a frame of another size, or one that is not an image, is named.
    #[test]
    fn frame_sizes_are_read_from_the_files() -> Result<(), TestError> {
        let dir = scratch("sizes")?;
        image::RgbImage::new(8, 4).save(dir.join("frame_000.png"))?;
        image::RgbImage::new(8, 4).save(dir.join("frame_001.png"))?;
        let artefacts = Artefacts::collect(&dir);
        assert!(artefacts.frame_sizes.all_at("8x4"));
        assert_eq!(artefacts.frame_sizes.describe_mismatch("8x4"), None);

        image::RgbImage::new(4, 4).save(dir.join("frame_002.png"))?;
        fs_err::write(dir.join("frame_003.png"), b"not a png")?;
        let sizes = Artefacts::collect(&dir).frame_sizes;
        assert!(!sizes.all_at("8x4"));
        let line = sizes
            .describe_mismatch("8x4")
            .ok_or("a mismatch must be described")?;
        assert!(line.contains("2 at 8x4"), "{line}");
        assert!(line.contains("1 at 4x4"), "{line}");
        assert!(line.contains("1 unreadable"), "{line}");
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
