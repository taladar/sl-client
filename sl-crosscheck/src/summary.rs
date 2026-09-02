//! `run.json` and the report printed at the end of a run.
//!
//! Both answer the same question — *is there a comparison here, and of what* —
//! for two different readers: the file for whatever compares the frames next,
//! the printed lines for the person who started the run and is watching it
//! finish.
//!
//! The one rule both follow is that **"the viewers differ" and "the run did not
//! happen" are never phrased the same way**. A run where Firestorm never started
//! is not a run where Firestorm drew something different; a report that blurs
//! them sends its reader to look for a rendering bug in a directory of nothing.
//! So a viewer's line leads with what its status file said, and the closing line
//! is about whether there is anything to compare at all — never about whether
//! the images match, which this crate has not looked at and does not know.

use core::time::Duration;

use serde::{Deserialize, Serialize};

use crate::launch::Viewer;
use crate::plan::RunPlan;
use crate::process::Ending;
use crate::status::Artefacts;

/// One viewer's half of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewerRun {
    /// Which viewer this is.
    pub viewer: String,
    /// Whether this viewer was asked to run at all. A run with one half skipped
    /// is still a run worth keeping — of one viewer.
    pub attempted: bool,
    /// Why it was skipped, when it was.
    pub skipped: Option<String>,
    /// How it ended, when it ran.
    pub ending: Option<String>,
    /// How long it ran, in seconds.
    pub seconds: Option<f64>,
    /// What it left behind.
    pub artefacts: Option<Artefacts>,
}

impl ViewerRun {
    /// A viewer that was not asked to run.
    #[must_use]
    pub fn skipped(viewer: Viewer, why: impl Into<String>) -> Self {
        Self {
            viewer: viewer.name().to_owned(),
            attempted: false,
            skipped: Some(why.into()),
            ending: None,
            seconds: None,
            artefacts: None,
        }
    }

    /// A viewer that ran.
    #[must_use]
    pub fn ran(viewer: Viewer, ending: Ending, took: Duration, artefacts: Artefacts) -> Self {
        Self {
            viewer: viewer.name().to_owned(),
            attempted: true,
            skipped: None,
            ending: Some(format!("{ending:?}")),
            seconds: Some(took.as_secs_f64()),
            artefacts: Some(artefacts),
        }
    }

    /// A viewer that could not be started at all — which is a run that did not
    /// happen, recorded as one rather than as an error that ends the whole run:
    /// the other viewer's half is still worth collecting.
    #[must_use]
    pub fn failed_to_start(viewer: Viewer, why: impl Into<String>) -> Self {
        Self {
            viewer: viewer.name().to_owned(),
            attempted: true,
            skipped: None,
            ending: Some(format!("could not start: {}", why.into())),
            seconds: None,
            artefacts: None,
        }
    }

    /// Whether this half produced something to compare.
    #[must_use]
    pub fn usable(&self) -> bool {
        self.artefacts
            .as_ref()
            .is_some_and(|artefacts| artefacts.status.happened() && !artefacts.frames.is_empty())
    }

    /// The report's line for this viewer.
    #[must_use]
    pub fn describe(&self) -> String {
        let name = &self.viewer;
        let Some(artefacts) = &self.artefacts else {
            return match (&self.skipped, &self.ending) {
                (Some(why), _ending) => format!("  {name}: skipped — {why}"),
                (None, Some(ending)) => format!("  {name}: {ending}"),
                (None, None) => format!("  {name}: did not run"),
            };
        };
        let took = self
            .seconds
            .map_or_else(String::new, |seconds| format!(", {seconds:.0} s"));
        let ending = self
            .ending
            .as_deref()
            .map_or_else(String::new, |ending| format!(", ended {ending}"));
        format!(
            "  {name}: {}\n    {} frame(s){}{}{}",
            artefacts.status.describe(),
            artefacts.frames.len(),
            artefacts
                .scene_dump
                .as_ref()
                .map_or(", no scene dump", |_dump| ", scene dump"),
            took,
            ending
        )
    }
}

/// A whole run: what was asked for, and what each viewer did.
#[expect(
    clippy::module_name_repetitions,
    reason = "this is the summary of a run, and `run.json` is what it is written to; the pair \
              of names is the point"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    /// The scenario every region was dressed with.
    pub scenario: String,
    /// The login URI both viewers were pointed at.
    pub login_uri: String,
    /// The capture size, as `WIDTHxHEIGHT`.
    pub capture_size: String,
    /// Which layers were in the frames.
    pub layers: Vec<String>,
    /// The camera position, when one was pinned.
    pub camera_position: Option<String>,
    /// What the camera looked at, when it was aimed.
    pub camera_look_at: Option<String>,
    /// Each viewer's half.
    pub viewers: Vec<ViewerRun>,
}

impl RunSummary {
    /// The summary of a run of `plan` in which the viewers did `viewers`.
    #[must_use]
    pub fn new(plan: &RunPlan, viewers: Vec<ViewerRun>) -> Self {
        let mut layers = vec!["world".to_owned()];
        for (on, name) in [
            (plan.capture.ui, "ui"),
            (plan.capture.hud, "hud"),
            (plan.capture.gizmos, "gizmos"),
        ] {
            if on {
                layers.push(name.to_owned());
            }
        }
        Self {
            scenario: plan.scenario.clone(),
            login_uri: plan.login_uri.to_string(),
            capture_size: format!("{}x{}", plan.capture.width, plan.capture.height),
            layers,
            camera_position: plan.camera.map(|camera| camera.position.to_string()),
            camera_look_at: plan
                .camera
                .and_then(|camera| camera.look_at)
                .map(|point| point.to_string()),
            viewers,
        }
    }

    /// Whether every viewer that was **asked** to run produced something, and at
    /// least one was. The runner's exit status follows this: it is a statement
    /// about the *run*, never about whether the two viewers agreed.
    ///
    /// Judged on what was asked for rather than on both halves being present,
    /// because `--only sl-client` is a legitimate thing to want — a one-sided
    /// run is not a failed run, and reporting it as one teaches its operator to
    /// ignore the exit status.
    #[must_use]
    pub fn ran_as_asked(&self) -> bool {
        let mut attempted = self
            .viewers
            .iter()
            .filter(|viewer| viewer.attempted)
            .peekable();
        attempted.peek().is_some() && attempted.all(ViewerRun::usable)
    }

    /// Whether there are **two** sets of frames, which is the only case in which
    /// anything can actually be compared.
    #[must_use]
    pub fn comparable(&self) -> bool {
        self.viewers.iter().filter(|viewer| viewer.usable()).count() >= 2
    }

    /// The report printed when a run finishes.
    #[must_use]
    pub fn render(&self) -> String {
        let mut lines = vec![
            format!("scenario {} on {}", self.scenario, self.login_uri),
            format!(
                "{} capture, layers: {}",
                self.capture_size,
                self.layers.join(" + ")
            ),
        ];
        if let Some(position) = &self.camera_position {
            lines.push(match &self.camera_look_at {
                Some(look_at) => format!("camera at {position} looking at {look_at}"),
                None => format!("camera at {position}"),
            });
        }
        for viewer in &self.viewers {
            lines.push(viewer.describe());
        }
        // Never "the viewers agree" or "the viewers differ": nothing here has
        // looked at a pixel. This says only whether there is a pair to look at,
        // and — separately — whether what was asked for happened.
        let failed: Vec<&str> = self
            .viewers
            .iter()
            .filter(|viewer| viewer.attempted && !viewer.usable())
            .map(|viewer| viewer.viewer.as_str())
            .collect();
        lines.push(if !failed.is_empty() {
            format!(
                "the run did not happen for {}: there is nothing to compare, and nothing to \
                 conclude about what either viewer drew",
                failed.join(" and ")
            )
        } else if self.comparable() {
            "both viewers produced frames; they are ready to be compared".to_owned()
        } else {
            let skipped: Vec<&str> = self
                .viewers
                .iter()
                .filter(|viewer| !viewer.attempted)
                .map(|viewer| viewer.viewer.as_str())
                .collect();
            format!(
                "a one-sided run as asked: {} did not run, so these frames are a capture \
                 rather than a comparison",
                skipped.join(" and ")
            )
        });
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{RunSummary, ViewerRun};
    use crate::launch::Viewer;
    use crate::plan::{CaptureSpec, RunPlan};
    use crate::process::Ending;
    use crate::status::{Artefacts, HarnessStatus, Status};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A plan against a loopback grid.
    fn plan() -> Result<RunPlan, TestError> {
        Ok(RunPlan {
            scenario: "catalogue".to_owned(),
            login_uri: "http://127.0.0.1:9100/".parse()?,
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
            password: "password".to_owned(),
            capture: CaptureSpec::default(),
            camera: None,
        })
    }

    /// A viewer that captured its frames and said so.
    fn good(viewer: Viewer) -> ViewerRun {
        ViewerRun::ran(
            viewer,
            Ending::Exited(0),
            Duration::from_secs(42),
            Artefacts {
                frames: vec!["frame_000.png".into(), "frame_001.png".into()],
                scene_dump: Some("scene.json".into()),
                status: Status::Reported {
                    status: HarnessStatus {
                        ok: true,
                        reason: "complete".to_owned(),
                        frames_written: 2,
                        frames_expected: 2,
                        viewer: viewer.name().to_owned(),
                    },
                },
            },
        )
    }

    /// Two good halves are a comparison, and the report says so without claiming
    /// anything about what is in the frames.
    #[test]
    fn two_good_halves_are_a_comparison() -> Result<(), TestError> {
        let summary = RunSummary::new(
            &plan()?,
            vec![good(Viewer::SlClient), good(Viewer::Firestorm)],
        );
        assert!(summary.comparable());
        assert!(summary.ran_as_asked());
        let report = summary.render();
        assert!(report.contains("ready to be compared"));
        assert!(
            !report.contains("differ"),
            "nothing here has looked at a pixel"
        );
        assert!(!report.contains("agree"));
        Ok(())
    }

    /// A half that never wrote a status is reported as a run that did not
    /// happen, and the run as a whole is not a comparison — the mistake this
    /// whole module exists to prevent.
    #[test]
    fn a_half_that_did_not_happen_is_not_a_difference() -> Result<(), TestError> {
        let missing = ViewerRun::ran(
            Viewer::Firestorm,
            Ending::Killed,
            Duration::from_secs(300),
            Artefacts {
                frames: Vec::new(),
                scene_dump: None,
                status: Status::Missing,
            },
        );
        let summary = RunSummary::new(&plan()?, vec![good(Viewer::SlClient), missing]);
        assert!(!summary.comparable());
        assert!(!summary.ran_as_asked());
        let report = summary.render();
        assert!(report.contains("firestorm"));
        assert!(report.contains("did not happen"));
        assert!(
            !report.contains("differ") && !report.contains("agree"),
            "a run that did not happen says nothing about whether the viewers agreed"
        );
        Ok(())
    }

    /// A skipped half is neither a failure nor a comparison: a run of one viewer
    /// is a legitimate thing to ask for, and says so.
    #[test]
    fn a_skipped_half_says_it_was_skipped() -> Result<(), TestError> {
        let summary = RunSummary::new(
            &plan()?,
            vec![
                good(Viewer::SlClient),
                ViewerRun::skipped(Viewer::Firestorm, "no --firestorm binary was given"),
            ],
        );
        // Not a comparison — there is only one set of frames — but the run did
        // what it was asked to do, and the exit status must not call it a
        // failure.
        assert!(!summary.comparable());
        assert!(summary.ran_as_asked());
        let report = summary.render();
        assert!(report.contains("skipped — no --firestorm binary"));
        assert!(report.contains("one-sided run as asked"));
        Ok(())
    }

    /// The summary records what was asked for, not only what happened: a run
    /// directory is read weeks later, and "which scene, which size, which
    /// layers" is the first thing its reader needs.
    #[test]
    fn the_summary_records_what_was_asked_for() -> Result<(), TestError> {
        let mut plan = plan()?;
        plan.capture.hud = true;
        let summary = RunSummary::new(&plan, Vec::new());
        assert_eq!(summary.capture_size, "1920x1080");
        assert_eq!(summary.layers, vec!["world".to_owned(), "hud".to_owned()]);
        assert_eq!(summary.scenario, "catalogue");
        Ok(())
    }
}
