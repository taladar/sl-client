//! Turning a collected run into something a person can act on.
//!
//! [`crate::summary`] says whether there is a comparison to make. This module
//! makes it, in three outputs, in increasing order of how often they actually
//! identify the bug:
//!
//! - a **contact sheet** ([`crate::sheet`]) — the two viewers' frames tiled and
//!   named, which is what gets looked at first and pasted into an issue;
//! - an **image diff** ([`crate::frames`]) — numbers that rank the frames by how
//!   far apart they are, so attention goes to the worst one;
//! - a **scene-dump diff** ([`crate::scene_diff`]) — the structured comparison
//!   that names the cause: a texture id that differs, a level of detail that
//!   differs, a material missing on one side.
//!
//! # It is a developer-facing tool, not a gate
//!
//! Nothing here enters `cargo nextest` and nothing here fails a build. Two
//! viewers, two renderers, two GPUs and two driver versions differ everywhere at
//! once, and a check that fails on a Mesa upgrade is one that gets disabled and
//! then ignored. The tiered harness says *wrong*; this says *different*, and a
//! person decides which viewer is right.
//!
//! Expect a large baseline difference and say so rather than treating it as a
//! finding. The signal is a *change* in the difference between runs, or a
//! difference localised to one object.
//!
//! # The one thing this report must never blur
//!
//! "The viewers differ" and "the run did not happen" are different sentences.
//! A run where Firestorm never got in world is not a run where Firestorm drew
//! something different, and a report that blurs the two sends its reader to hunt
//! a rendering bug in a directory of black frames. So a half whose
//! `harness-status.json` is missing is reported as a run that did not happen,
//! and **nothing is diffed against it**.
//!
//! # "The reference viewer is right" is a prior, not evidence
//!
//! One difference here looked exactly like a bug of ours and was upstream
//! Linden code: Firestorm drew every avatar with no right hand, because
//! `avatarSkinV.glsl` reads one past the end of the matrix palette for
//! `mWristRight` and `NaN * 0` is `NaN`
//! ([secondlife/viewer#6240](https://github.com/secondlife/viewer/issues/6240)).
//! Our viewer rendering the same avatar correctly was the single most
//! informative measurement in that chase, and it came late. When the two
//! viewers disagree, the one to suspect is not automatically ours.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dump::SceneDump;
use crate::frames::{self, Compared};
use crate::launch::Viewer;
use crate::scene_diff::{SceneDiff, Tolerances};
use crate::sheet::{self, Column};
use crate::status::Artefacts;
use crate::summary::RunSummary;

/// What went wrong building a report.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The run directory holds nothing this can read.
    #[error(
        "{path} does not look like a cross-check run: it has no sl-client/ or firestorm/ \
         directory"
    )]
    NotARun {
        /// The directory that was pointed at.
        path: String,
    },
    /// The report directory could not be made.
    #[error("could not create {path}: {source}")]
    Create {
        /// The directory that could not be made.
        path: String,
        /// Why not.
        source: std::io::Error,
    },
    /// A report file could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        /// The file that could not be written.
        path: String,
        /// Why not.
        source: std::io::Error,
    },
}

/// What a report was asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// The run directory to read.
    pub run: PathBuf,
    /// Where the report goes. `<run>/report` by default.
    pub out: PathBuf,
    /// How many rows the contact sheet gets.
    pub rows: usize,
    /// How wide one contact-sheet cell is drawn.
    pub cell_width: u32,
    /// How many findings the printed report lists before it says how many more
    /// there are.
    pub findings: usize,
    /// Whether to write a difference image per compared frame.
    pub heatmaps: bool,
    /// How far apart two numbers may be before the scene diff says so.
    pub tolerances: Tolerances,
}

impl Spec {
    /// A report of `run`, at this crate's defaults.
    #[must_use]
    pub fn new(run: impl Into<PathBuf>) -> Self {
        let run = run.into();
        Self {
            out: run.join("report"),
            run,
            rows: 6,
            cell_width: 640,
            findings: 25,
            heatmaps: true,
            tolerances: Tolerances::default(),
        }
    }
}

/// One viewer's half, as the report sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Half {
    /// Which viewer.
    pub viewer: String,
    /// What it left behind.
    pub artefacts: Artefacts,
    /// Its scene dump, when it wrote one that could be read.
    #[serde(skip)]
    pub dump: Option<SceneDump>,
    /// Why its dump could not be read, when there was one and it could not.
    pub dump_problem: Option<String>,
}

impl Half {
    /// Collect one viewer's half out of the run directory.
    fn collect(run: &Path, viewer: Viewer) -> Self {
        let artefacts = Artefacts::collect(&run.join(viewer.name()));
        let (dump, dump_problem) = match &artefacts.scene_dump {
            Some(path) => match SceneDump::read(path) {
                Ok(dump) => (Some(dump), None),
                Err(error) => (None, Some(error.to_string())),
            },
            None => (None, None),
        };
        Self {
            viewer: viewer.name().to_owned(),
            artefacts,
            dump,
            dump_problem,
        }
    }

    /// Whether this half produced something to compare.
    #[must_use]
    pub const fn usable(&self) -> bool {
        self.artefacts.status.happened() && !self.artefacts.frames.is_empty()
    }
}

/// A whole report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The run directory this came from.
    pub run: PathBuf,
    /// What the run was asked to photograph, when `run.json` was readable.
    pub scenario: Option<String>,
    /// Where the camera stood.
    pub camera: Option<String>,
    /// This viewer's half.
    pub left: Half,
    /// The reference's half.
    pub right: Half,
    /// The contact sheet, when one was written.
    pub contact_sheet: Option<PathBuf>,
    /// How many frames the sheet left out.
    pub frames_dropped: usize,
    /// Every compared pair of frames.
    pub frames: Vec<Compared>,
    /// The scene-dump comparison, when both viewers wrote a dump.
    pub scene: Option<SceneDiff>,
}

impl Report {
    /// Whether both halves produced frames — the only case in which anything
    /// was actually compared.
    #[must_use]
    pub const fn comparable(&self) -> bool {
        self.left.usable() && self.right.usable()
    }

    /// The middling image difference across the compared frames, which is what
    /// ranks one run against another.
    ///
    /// The median rather than the mean: a single frame caught mid-load is a long
    /// way from the rest and would decide a mean by itself.
    #[must_use]
    pub fn median_difference(&self) -> Option<f64> {
        let mut means: Vec<f64> = self
            .frames
            .iter()
            .filter_map(|compared| compared.pair().map(|pair| pair.mean_abs))
            .collect();
        if means.is_empty() {
            return None;
        }
        means.sort_by(f64::total_cmp);
        means.get(means.len().checked_div(2)?).copied()
    }

    /// The report's text.
    #[must_use]
    pub fn render(&self, findings: usize) -> String {
        let mut lines = vec![format!("cross-check report for {}", self.run.display())];
        if let Some(scenario) = &self.scenario {
            lines.push(format!("scenario {scenario}"));
        }
        if let Some(camera) = &self.camera {
            lines.push(camera.clone());
        }
        lines.push(String::new());

        // What happened, before what was seen. A half that never got in world
        // has to be said in those words: a reader who meets a difference first
        // goes looking for a rendering bug in a directory of black frames.
        for half in [&self.left, &self.right] {
            lines.push(format!(
                "{}: {} — {} frame(s), {}",
                half.viewer,
                half.artefacts.status.describe(),
                half.artefacts.frames.len(),
                match (&half.dump, &half.dump_problem) {
                    (Some(dump), _problem) => format!(
                        "scene dump of {} object(s) and {} avatar(s)",
                        dump.objects.len(),
                        dump.avatars.len()
                    ),
                    (None, Some(problem)) => format!("an unreadable scene dump: {problem}"),
                    (None, None) => "no scene dump".to_owned(),
                }
            ));
        }
        let failed: Vec<&str> = [&self.left, &self.right]
            .into_iter()
            .filter(|half| !half.usable())
            .map(|half| half.viewer.as_str())
            .collect();
        if !failed.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "the run did not happen for {}: what follows is a capture, not a comparison, and \
                 nothing here says the two viewers agree or differ",
                failed.join(" and ")
            ));
        }

        // The scene dump before the pixels: its settings section explains the
        // pixels, and a reader who meets the images first goes hunting a
        // rendering bug that is a graphics preset.
        lines.push(String::new());
        match &self.scene {
            Some(scene) => {
                lines.push("scene dumps:".to_owned());
                lines.push(scene.render(findings));
            }
            None if !failed.is_empty() => lines.push(
                "no scene-dump comparison: a viewer that never got in world writes a dump of an \
                 empty world, and diffing that would report every object in the scene as missing"
                    .to_owned(),
            ),
            None => lines.push(
                "no scene-dump comparison: it needs a scene.json from both viewers, and the \
                 structured diff is the output that names a cause"
                    .to_owned(),
            ),
        }

        lines.push(String::new());
        if self.frames.is_empty() {
            lines.push("no frames were compared".to_owned());
        } else {
            lines.push(format!(
                "frames ({} compared), worst first — a number, not a verdict:",
                self.frames.len()
            ));
            // Said every time, deliberately. The two viewers do not share a
            // renderer, so tone mapping, exposure, shadow filtering and
            // anti-aliasing differ everywhere at once, and a reader who takes
            // the baseline for a finding chases it for a day.
            lines.push(
                "  expect a large baseline difference: two renderers, two GPUs, two drivers. \
                 What is worth attention is a change in the difference between runs, or a \
                 difference localised to one tile."
                    .to_owned(),
            );
            let mut ranked: Vec<&Compared> = self.frames.iter().collect();
            ranked.sort_by(|first, second| {
                second
                    .pair()
                    .map_or(f64::MAX, |pair| pair.mean_abs)
                    .total_cmp(&first.pair().map_or(f64::MAX, |pair| pair.mean_abs))
            });
            for compared in ranked.iter().take(findings) {
                lines.push(compared.describe());
            }
            if ranked.len() > findings {
                lines.push(format!(
                    "  … and {} more frame(s) not listed",
                    ranked.len().saturating_sub(findings)
                ));
            }
        }

        lines.push(String::new());
        match &self.contact_sheet {
            Some(path) => {
                lines.push(format!("contact sheet: {}", path.display()));
                if self.frames_dropped > 0 {
                    lines.push(format!(
                        "  {} frame(s) are not on it; it is spread across the run rather than \
                         taken from its start",
                        self.frames_dropped
                    ));
                }
            }
            None => lines.push("no contact sheet: neither viewer left a frame to tile".to_owned()),
        }
        lines.join("\n")
    }
}

/// Build the report of one run.
///
/// # Errors
///
/// [`Error::NotARun`] when the directory holds neither viewer's artefacts,
/// and the I/O errors of writing the report out.
pub fn build(spec: &Spec) -> Result<Report, Error> {
    let left = Half::collect(&spec.run, Viewer::SlClient);
    let right = Half::collect(&spec.run, Viewer::Firestorm);
    if !spec.run.join(Viewer::SlClient.name()).is_dir()
        && !spec.run.join(Viewer::Firestorm.name()).is_dir()
    {
        return Err(Error::NotARun {
            path: spec.run.display().to_string(),
        });
    }
    fs_err::create_dir_all(&spec.out).map_err(|source| Error::Create {
        path: spec.out.display().to_string(),
        source,
    })?;

    let asked = read_run_json(&spec.run);
    let scenario = asked.as_ref().map(|summary| summary.scenario.clone());
    let camera = asked.as_ref().and_then(describe_camera);

    // Only ever between two halves that both happened, both for the pixels and
    // for the dumps. A half that never got in world still wrote a full set of
    // frames — black, and on schedule — and would write a dump of an empty
    // world beside them; diffing either manufactures the largest difference of
    // the run out of a viewer that drew nothing.
    let happened = left.usable() && right.usable();
    let frames = if happened {
        frames::compare(
            &left.artefacts.frames,
            &right.artefacts.frames,
            spec.heatmaps.then_some(spec.out.as_path()),
        )
    } else {
        Vec::new()
    };
    let scene = match (happened, &left.dump, &right.dump) {
        (true, Some(ours), Some(theirs)) => Some(SceneDiff::compare(ours, theirs, spec.tolerances)),
        _one_or_neither => None,
    };

    let columns: Vec<Column> = [&left, &right]
        .into_iter()
        .filter(|half| !half.artefacts.frames.is_empty())
        .map(|half| Column {
            viewer: half.viewer.clone(),
            frames: half.artefacts.frames.clone(),
        })
        .collect();
    let (contact_sheet, frames_dropped) = if columns.is_empty() {
        (None, 0)
    } else {
        let mut sheet_spec = sheet::Spec::new(
            match (&scenario, &camera) {
                (Some(scenario), Some(camera)) => format!("{scenario} — {camera}"),
                (Some(scenario), None) => scenario.clone(),
                (None, _camera) => spec.run.display().to_string(),
            },
            columns,
        );
        sheet_spec.rows = spec.rows;
        sheet_spec.cell_width = spec.cell_width;
        match sheet::build(&sheet_spec, &spec.out.join("contact-sheet.png")) {
            Ok(sheet) => (Some(sheet.path), sheet.dropped),
            Err(error) => {
                // A sheet that could not be drawn must not take the numbers down
                // with it: the scene diff is the output that names causes.
                tracing::warn!("no contact sheet: {error}");
                (None, 0)
            }
        }
    };

    let report = Report {
        run: spec.run.clone(),
        scenario,
        camera,
        left,
        right,
        contact_sheet,
        frames_dropped,
        frames,
        scene,
    };
    write(
        &spec.out.join("report.json"),
        &serde_json::to_string_pretty(&report).unwrap_or_else(|error| {
            format!("{{\"error\": \"the report could not be serialised: {error}\"}}")
        }),
    )?;
    write(
        &spec.out.join("report.txt"),
        &format!("{}\n", report.render(spec.findings)),
    )?;
    Ok(report)
}

/// Write one of the report's files.
fn write(path: &Path, text: &str) -> Result<(), Error> {
    fs_err::write(path, text).map_err(|source| Error::Write {
        path: path.display().to_string(),
        source,
    })
}

/// Read the runner's own `run.json`, when there is a readable one.
///
/// Only for what the run was *asked* for — the scene, the camera. What each
/// viewer actually left is re-collected from the directory rather than trusted
/// to the file, because the paths in it are relative to wherever the runner was
/// started and a run directory is read from somewhere else weeks later.
fn read_run_json(run: &Path) -> Option<RunSummary> {
    let text = fs_err::read_to_string(run.join("run.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// The camera line, when the run pinned one.
fn describe_camera(summary: &RunSummary) -> Option<String> {
    let position = summary.camera_position.as_ref()?;
    Some(match &summary.camera_look_at {
        Some(look_at) => format!("camera at {position} looking at {look_at}"),
        None => format!("camera at {position}"),
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Spec, build};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A scratch run directory of this test's own.
    fn scratch(name: &str) -> Result<std::path::PathBuf, TestError> {
        let dir = std::env::temp_dir().join(format!(
            "sl-crosscheck-report-{name}-{}",
            std::process::id()
        ));
        let _ignored = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Lay out one viewer's half of a run: a status, some frames, and a dump.
    fn half(
        run: &std::path::Path,
        viewer: &str,
        frames: usize,
        colour: [u8; 3],
        dump: Option<&str>,
    ) -> Result<(), TestError> {
        let dir = run.join(viewer);
        fs_err::create_dir_all(&dir)?;
        fs_err::write(
            dir.join("harness-status.json"),
            format!(
                r#"{{"ok":true,"reason":"complete","frames_written":{frames},
                     "frames_expected":{frames},"viewer":"{viewer}"}}"#
            ),
        )?;
        for index in 0..frames {
            image::RgbImage::from_pixel(64, 36, image::Rgb(colour))
                .save(dir.join(format!("frame_{index:03}.png")))?;
        }
        if let Some(dump) = dump {
            fs_err::write(dir.join("scene.json"), dump)?;
        }
        Ok(())
    }

    /// A dump naming one object with one face.
    fn dump(viewer: &str, texture: &str) -> String {
        format!(
            r#"{{ "schema_version": 1, "context": {{ "viewer": "{viewer}" }},
                  "render": {{ "draw_distance": 128.0 }},
                  "objects": [ {{ "id": "aaaa0000-0000-0000-0000-000000000001",
                                  "local_id": 7, "pcode": "volume",
                                  "position": [1.0, 2.0, 3.0],
                                  "faces": [ {{ "index": 0, "texture": "{texture}" }} ] }} ] }}"#
        )
    }

    /// A complete pair: three outputs, and the one that names the cause names
    /// it.
    #[test]
    fn a_complete_pair_produces_all_three_outputs() -> Result<(), TestError> {
        let run = scratch("complete")?;
        half(
            &run,
            "sl-client",
            3,
            [40, 40, 40],
            Some(&dump("sl-client", "11111111-0000-0000-0000-000000000000")),
        )?;
        half(
            &run,
            "firestorm",
            3,
            [60, 40, 40],
            Some(&dump("firestorm", "22222222-0000-0000-0000-000000000000")),
        )?;
        let report = build(&Spec::new(&run))?;
        assert!(report.comparable());
        assert!(report.contact_sheet.is_some());
        assert_eq!(report.frames.len(), 3);
        let scene = report.scene.as_ref().ok_or("no scene diff")?;
        let finding = scene.divergences().next().ok_or("no divergence")?;
        assert_eq!(finding.field, "texture");
        let text = report.render(20);
        assert!(text.contains("baseline difference"));
        for written in [
            "report/report.txt",
            "report/report.json",
            "report/diff_000.png",
        ] {
            fs_err::metadata(run.join(written))?;
        }
        fs_err::remove_dir_all(&run)?;
        Ok(())
    }

    /// The mistake the whole crate is organised around: a half that never got in
    /// world still wrote a full set of frames, black and on schedule. They are
    /// not diffed, and the report says the run did not happen rather than
    /// reporting the largest difference of the day.
    #[test]
    fn a_half_that_did_not_happen_is_never_diffed() -> Result<(), TestError> {
        let run = scratch("did-not-happen")?;
        half(
            &run,
            "sl-client",
            3,
            [40, 40, 40],
            Some(&dump("sl-client", "11111111-0000-0000-0000-000000000000")),
        )?;
        // Everything a viewer that never got in world leaves behind: a full set
        // of frames, black and on schedule, a dump of an empty world, and no
        // status file to say any of it happened.
        let dir = run.join("firestorm");
        fs_err::create_dir_all(&dir)?;
        for index in 0..3 {
            image::RgbImage::from_pixel(64, 36, image::Rgb([0, 0, 0]))
                .save(dir.join(format!("frame_{index:03}.png")))?;
        }
        fs_err::write(
            dir.join("scene.json"),
            r#"{ "schema_version": 1, "context": { "viewer": "firestorm" },
                 "objects": [], "avatars": [] }"#,
        )?;
        let report = build(&Spec::new(&run))?;
        assert!(!report.comparable());
        assert!(report.frames.is_empty(), "nothing may be diffed against it");
        assert!(
            report.scene.is_none(),
            "nor its dump: an empty world would report every object as missing"
        );
        let text = report.render(20);
        assert!(text.contains("did not happen for firestorm"));
        assert!(
            text.contains("a capture, not a comparison"),
            "a run that did not happen says nothing about what either viewer drew"
        );
        assert!(
            text.contains("no frames were compared"),
            "and it ranks nothing, because there is nothing to rank"
        );
        assert!(
            text.contains("a viewer that never got in world writes a dump of an empty world"),
            "and it says which output it withheld, and why"
        );
        // A sheet is still drawn: a capture is worth looking at.
        assert!(report.contact_sheet.is_some());
        fs_err::remove_dir_all(&run)?;
        Ok(())
    }

    /// One viewer without a dump loses the output that names causes, and the
    /// report says which output is missing rather than printing nothing.
    #[test]
    fn a_missing_scene_dump_is_named_not_silently_skipped() -> Result<(), TestError> {
        let run = scratch("no-dump")?;
        half(&run, "sl-client", 1, [40, 40, 40], None)?;
        half(
            &run,
            "firestorm",
            1,
            [40, 40, 40],
            Some(&dump("firestorm", "22222222-0000-0000-0000-000000000000")),
        )?;
        let report = build(&Spec::new(&run))?;
        assert!(report.scene.is_none());
        assert!(report.render(20).contains("no scene-dump comparison"));
        fs_err::remove_dir_all(&run)?;
        Ok(())
    }

    /// A directory that is not a run says so, rather than producing an empty
    /// report that reads like a run with no differences.
    #[test]
    fn a_directory_that_is_not_a_run_says_so() -> Result<(), TestError> {
        let run = scratch("not-a-run")?;
        let error = build(&Spec::new(&run))
            .err()
            .ok_or("a directory that is not a run should not report as one")?;
        assert!(
            error
                .to_string()
                .contains("does not look like a cross-check run")
        );
        fs_err::remove_dir_all(&run)?;
        Ok(())
    }
}
