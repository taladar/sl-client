//! Judging a run: each expectation of a case held against what was observed,
//! and every way the content went wrong that no expectation asked about — a
//! script that does not compile, a run-time error, an avatar that never got to
//! the end of its steps — written so a person can see what happened without
//! re-running anything.

use sl_fake_grid::{AvatarAction, DialogChoice, ImitatedGrid, Progress, Target};

use crate::case::{Case, Expectation, SaidExpectation};
use crate::run::{AvatarOutcome, Observed};

/// How many lines of what was said a failure quotes before it stops.
const QUOTED_LINES: usize = 12;

/// The verdict on one run of a case on one grid.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// The case's name.
    pub case: String,
    /// The grid it ran on.
    pub grid: ImitatedGrid,
    /// How many ticks it ran.
    pub ticks: u32,
    /// What went wrong, each a paragraph; none is a pass.
    pub failures: Vec<String>,
    /// What is worth knowing but is not a failure.
    pub notes: Vec<String>,
}

impl Verdict {
    /// Whether the case passed.
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// A grid's name as a case file writes it.
#[must_use]
pub const fn grid_name(grid: ImitatedGrid) -> &'static str {
    match grid {
        ImitatedGrid::SecondLife => "second-life",
        ImitatedGrid::OpenSim => "opensim",
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outcome = if self.passed() { "PASS" } else { "FAIL" };
        writeln!(
            f,
            "{outcome} {} [{}] {} ticks",
            self.case,
            grid_name(self.grid),
            self.ticks
        )?;
        for failure in &self.failures {
            writeln!(f, "{}", indented(failure))?;
        }
        for note in &self.notes {
            writeln!(f, "{}", indented(&format!("note: {note}")))?;
        }
        Ok(())
    }
}

/// `text` indented four spaces, its continuation lines six.
fn indented(text: &str) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    std::iter::once(format!("    {first}"))
        .chain(lines.map(|line| format!("      {line}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Judges `observed`, a run of `case`.
#[must_use]
pub fn judge(case: &Case, observed: &Observed) -> Verdict {
    let mut failures = Vec::new();
    for failure in &observed.compile_failures {
        failures.push(format!(
            "script {:?} in {:?} does not compile:\n{}",
            failure.script,
            failure.object,
            failure.errors.join("\n")
        ));
    }
    let expected_faults: Vec<(&String, Option<&String>)> = case
        .expectations
        .iter()
        .filter_map(|expectation| match expectation {
            Expectation::Fault { error, script } => Some((error, script.as_ref())),
            Expectation::Said(_)
            | Expectation::Transcript { .. }
            | Expectation::Position { .. } => None,
        })
        .collect();
    for fault in &observed.faults {
        let expected = expected_faults.iter().any(|(error, script)| {
            fault.error.contains(error.as_str()) && script.is_none_or(|name| *name == fault.script)
        });
        if !expected {
            failures.push(fault.to_string());
        }
    }
    for (avatar, run) in case.avatars.iter().zip(&observed.avatars) {
        if let Some(failure) = unfinished(avatar, run, observed.ticks) {
            failures.push(failure);
        }
    }
    for expectation in &case.expectations {
        let failure = match expectation {
            Expectation::Said(said) => said_failure(said, observed),
            Expectation::Transcript { avatar, lines } => {
                transcript_failure(avatar, lines, observed)
            }
            Expectation::Fault { error, script } => {
                let seen = observed.faults.iter().any(|fault| {
                    fault.error.contains(error.as_str())
                        && script.as_ref().is_none_or(|name| *name == fault.script)
                });
                (!seen).then(|| {
                    let at = script
                        .as_ref()
                        .map(|name| format!(" in script {name:?}"))
                        .unwrap_or_default();
                    format!("expected a run-time error containing {error:?}{at}; none was raised")
                })
            }
            Expectation::Position { object, at, within } => {
                position_failure(object, at, *within, observed)
            }
        };
        failures.extend(failure);
    }
    let notes = observed
        .unsupplied
        .iter()
        .map(|unsupplied| {
            format!(
                "script {:?} in {:?} names {}, which the case does not supply: the fake grid \
                 answers a fetch of it as not found",
                unsupplied.script, unsupplied.object, unsupplied.key
            )
        })
        .chain(observed.stubbed.iter().map(|call| {
            format!(
                "script {:?} in {:?} called {}, which the fake grid only stubs: it answered \
                 its default and did nothing else",
                call.script, call.object, call.function
            )
        }))
        .collect();
    Verdict {
        case: case.name.clone(),
        grid: observed.grid,
        ticks: observed.ticks,
        failures,
        notes,
    }
}

/// Why `avatar`, which ran as `run`, did not finish its steps within `ticks`,
/// if it did not.
fn unfinished(avatar: &crate::case::Avatar, run: &AvatarOutcome, ticks: u32) -> Option<String> {
    let at = run
        .step
        .and_then(|step| {
            avatar.steps.get(step).map(|(_ticks, action)| {
                format!(
                    " on step {} of {} ({})",
                    step.saturating_add(1),
                    avatar.steps.len(),
                    describe(action)
                )
            })
        })
        .unwrap_or_default();
    let why = match &run.progress {
        Progress::Finished => return None,
        Progress::Stopped(reason) => format!("{} stopped{at}: {reason}", run.name),
        Progress::LoggingIn | Progress::Running => {
            format!(
                "{} had not finished its steps after {ticks} ticks; it was{at}",
                run.name
            )
        }
        other => format!("{} ended {other:?}{at}", run.name),
    };
    let tail = run.transcript.len().saturating_sub(QUOTED_LINES);
    let transcript = run.transcript.get(tail..).unwrap_or_default().join("\n");
    Some(format!("{why}\nits transcript ends:\n{transcript}"))
}

/// A step's action, as a case file writes it.
fn describe(action: &AvatarAction) -> String {
    let name = |target: &Target| match target {
        Target::Named(name) => format!("{name:?}"),
        Target::Key(key) => key.uuid().to_string(),
    };
    match action {
        AvatarAction::Walk { to } => format!("walk = [{}, {}, {}]", to.x, to.y, to.z),
        AvatarAction::Touch(target) => format!("touch = {}", name(target)),
        AvatarAction::Sit(target) => format!("sit = {}", name(target)),
        AvatarAction::Stand => "stand = true".to_owned(),
        AvatarAction::Say { channel, message } => format!("say = {message:?}, channel = {channel}"),
        AvatarAction::Pay { target, amount } => match amount {
            Some(amount) => format!("pay = {}, amount = {amount}", name(target)),
            None => format!("pay = {}", name(target)),
        },
        AvatarAction::AnswerDialog(DialogChoice::Label(label)) => {
            format!("answer_dialog = {label:?}")
        }
        AvatarAction::AnswerDialog(DialogChoice::Index(index)) => {
            format!("answer_dialog_index = {index}")
        }
        AvatarAction::AnswerDialog(DialogChoice::Text(text)) => format!("answer_text = {text:?}"),
        AvatarAction::AnswerPermissions(permissions) if permissions.0 == 0 => {
            "refuse = true".to_owned()
        }
        AvatarAction::AnswerPermissions(permissions) => format!("grant = {:#x}", permissions.0),
        AvatarAction::AwaitChat(wanted) => format!("await_chat = {wanted:?}"),
        other => format!("{other:?}"),
    }
}

/// Why `said` does not hold of `observed`, if it does not.
fn said_failure(said: &SaidExpectation, observed: &Observed) -> Option<String> {
    let speaker = said
        .by
        .as_ref()
        .map_or_else(|| "something".to_owned(), |by| format!("{by:?}"));
    let channel = said
        .channel
        .map(|channel| format!(" on channel {channel}"))
        .unwrap_or_default();
    let (said_lines, matching, to) = match &said.heard_by {
        None => {
            let by_speaker: Vec<String> = observed
                .said
                .iter()
                .filter(|line| said.by.as_ref().is_none_or(|by| *by == line.from_name))
                .map(|line| {
                    format!(
                        "channel {}: {} {:?}",
                        line.channel.0, line.from_name, line.message
                    )
                })
                .collect();
            let matching = observed
                .said
                .iter()
                .filter(|line| said.by.as_ref().is_none_or(|by| *by == line.from_name))
                .filter(|line| said.channel.is_none_or(|channel| channel == line.channel.0))
                .filter(|line| said.text.matches(&line.message))
                .map(|line| format!("channel {}: {:?}", line.channel.0, line.message))
                .collect::<Vec<_>>();
            (by_speaker, matching, String::new())
        }
        Some(avatar) => {
            let heard: Vec<&sl_fake_grid::HeardLine> = observed
                .avatars
                .iter()
                .find(|run| run.name == *avatar)
                .map(|run| run.heard.iter().collect())
                .unwrap_or_default();
            let by_speaker: Vec<String> = heard
                .iter()
                .filter(|line| said.by.as_ref().is_none_or(|by| *by == line.from))
                .map(ToString::to_string)
                .collect();
            let matching = heard
                .iter()
                .filter(|line| said.by.as_ref().is_none_or(|by| *by == line.from))
                .filter(|line| said.text.matches(&line.message))
                .map(ToString::to_string)
                .collect();
            (by_speaker, matching, format!(" to {avatar}"))
        }
    };
    if said.never {
        return (!matching.is_empty()).then(|| {
            format!(
                "expected {speaker} never to say {}{channel}{to}; it said:\n{}",
                said.text,
                quoted(&matching)
            )
        });
    }
    if !matching.is_empty() {
        return None;
    }
    let what = if said_lines.is_empty() {
        match (&said.by, &said.heard_by) {
            (Some(_), _) => "it said nothing".to_owned(),
            (None, Some(avatar)) => format!("{avatar} was told nothing"),
            (None, None) => "nothing was said".to_owned(),
        }
    } else {
        format!("it said:\n{}", quoted(&said_lines))
    };
    Some(format!(
        "expected {speaker} to say {}{channel}{to}; {what}",
        said.text
    ))
}

/// Why the prim `object` does not stand within `within` of `at` at the end of
/// `observed`, if it does not.
fn position_failure(
    object: &str,
    at: &sl_types::lsl::Vector,
    within: f32,
    observed: &Observed,
) -> Option<String> {
    let wanted = format!("<{}, {}, {}>", at.x, at.y, at.z);
    let Some((_name, stands)) = observed.positions.iter().find(|(name, _at)| name == object) else {
        return Some(format!(
            "expected {object:?} at {wanted} (within {within} m); the region holds nothing \
             called that"
        ));
    };
    let (dx, dy, dz) = (stands.x - at.x, stands.y - at.y, stands.z - at.z);
    let off = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
    (off > within).then(|| {
        format!(
            "expected {object:?} at {wanted} (within {within} m); it stands at <{}, {}, {}>, \
             {off} m away",
            stands.x, stands.y, stands.z
        )
    })
}

/// Up to [`QUOTED_LINES`] of `lines`, and how many more there were.
fn quoted(lines: &[String]) -> String {
    let mut shown: Vec<String> = lines.iter().take(QUOTED_LINES).cloned().collect();
    let more = lines.len().saturating_sub(QUOTED_LINES);
    if more > 0 {
        shown.push(format!("… and {more} more"));
    }
    shown.join("\n")
}

/// Why `avatar`'s transcript does not hold `lines` in order, if it does not.
fn transcript_failure(avatar: &str, lines: &[String], observed: &Observed) -> Option<String> {
    let run = observed.avatars.iter().find(|run| run.name == avatar)?;
    let mut rest = run.transcript.iter();
    for (index, wanted) in lines.iter().enumerate() {
        if !rest.any(|line| line == wanted) {
            return Some(format!(
                "expected {avatar}'s transcript to hold, in order, line {} of {}: {wanted:?}; \
                 its transcript:\n{}",
                index.saturating_add(1),
                lines.len(),
                run.transcript.join("\n")
            ));
        }
    }
    None
}

/// Everything observed of a run, for `--verbose`: the region's chat, the
/// faults, and each avatar's transcript and what it was told.
#[must_use]
pub fn transcript(observed: &Observed) -> String {
    let mut out = vec!["  said in the region:".to_owned()];
    out.extend(observed.said.iter().map(|line| {
        format!(
            "    channel {}: {}: {}",
            line.channel.0, line.from_name, line.message
        )
    }));
    if !observed.faults.is_empty() {
        out.push("  run-time errors:".to_owned());
        out.extend(observed.faults.iter().map(|fault| format!("    {fault}")));
    }
    for run in &observed.avatars {
        out.push(format!("  {} ({:?}):", run.name, run.progress));
        out.extend(run.transcript.iter().map(|line| format!("    {line}")));
        out.push(format!("  {} was told:", run.name));
        out.extend(run.heard.iter().map(|line| format!("    {line}")));
    }
    out.join("\n")
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A failure's paragraph is indented under its verdict line.
    #[test]
    fn a_verdict_reads_as_a_report() {
        let verdict = Verdict {
            case: "vendor".to_owned(),
            grid: ImitatedGrid::SecondLife,
            ticks: 42,
            failures: vec![
                "expected \"Vendor\" to say \"hi\"; it said:\nchannel 0: \"no\"".to_owned(),
            ],
            notes: Vec::new(),
        };
        assert_eq!(
            verdict.to_string(),
            "FAIL vendor [second-life] 42 ticks\n    expected \"Vendor\" to say \"hi\"; it \
             said:\n      channel 0: \"no\"\n"
        );
    }

    /// A prim is where it should be within the distance allowed, and a
    /// failure says where it is instead, or that it is not there at all.
    #[test]
    fn a_place_is_checked_within_its_distance() {
        let at = |x: f32| sl_types::lsl::Vector {
            x,
            y: 128.0,
            z: 25.0,
        };
        let observed = Observed {
            grid: ImitatedGrid::SecondLife,
            ticks: 1,
            finished: true,
            said: Vec::new(),
            faults: Vec::new(),
            stubbed: Vec::new(),
            avatars: Vec::new(),
            positions: vec![("Door".to_owned(), at(130.0))],
            compile_failures: Vec::new(),
            unsupplied: Vec::new(),
        };
        assert_eq!(
            position_failure("Door", &at(130.005), 0.01, &observed),
            None
        );
        assert_eq!(
            position_failure("Door", &at(128.0), 0.5, &observed),
            Some(
                "expected \"Door\" at <128, 128, 25> (within 0.5 m); it stands at <130, 128, 25>, \
                 2 m away"
                    .to_owned()
            )
        );
        assert_eq!(
            position_failure("Window", &at(128.0), 0.5, &observed),
            Some(
                "expected \"Window\" at <128, 128, 25> (within 0.5 m); the region holds nothing \
                 called that"
                    .to_owned()
            )
        );
    }

    /// A step reads as the case file writes it.
    #[test]
    fn a_step_reads_as_written() {
        assert_eq!(
            describe(&AvatarAction::AwaitChat("Thank".to_owned())),
            "await_chat = \"Thank\""
        );
        assert_eq!(
            describe(&AvatarAction::Pay {
                target: Target::Named("Vendor".to_owned()),
                amount: None
            }),
            "pay = \"Vendor\""
        );
    }
}
