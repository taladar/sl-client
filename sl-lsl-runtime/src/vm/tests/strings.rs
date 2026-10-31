//! The string and list library held to the grids: the probes' own scripts
//! run in the VM under each flavour, and every line they print set against
//! the line the grid printed — `strings/functions`, `strings/unicode`,
//! `strings/conversions` and `strings/newer` whole, and each of the
//! `errors/bounds` and `errors/bounds-newer` cases as a script of its own,
//! with what OpenSim's engines say when the library throws.
//!
//! A probe is changed only where it reads the world: `llOwnerSay` becomes
//! `print`, and `llGetRegionName`, `llGetEnv` and `llGetScriptName` become
//! the text they answered. The `start` record, which carries those, is not
//! compared. Every other difference is a named deviation below, with its
//! reason.

use pretty_assertions::assert_eq;

use super::*;

/// `source` with what it reads of the world written in: `llOwnerSay` as
/// `print`, `llGetEnv(…)` and `llGetRegionName()` as `""`, and
/// `llGetScriptName()` as `script`.
pub(super) fn offline(source: &str, script: &str) -> String {
    let mut text = source
        .replace("llOwnerSay(", "print(")
        .replace("llGetRegionName()", "\"\"")
        .replace("llGetScriptName()", &format!("\"{script}\""));
    while let Some(at) = text.find("llGetEnv(") {
        let end = text
            .get(at..)
            .and_then(|rest| rest.find(')'))
            .map_or(text.len(), |close| {
                at.saturating_add(close).saturating_add(1)
            });
        text.replace_range(at..end, "\"\"");
    }
    text
}

/// Run `source` under `flavour` for up to `ticks` ticks, or until it printed
/// its `end` record; the engine, for what it said and told, and every
/// `PROBE` line printed.
pub(super) fn replay(
    source: &str,
    flavour: EngineFlavour,
    ticks: u64,
) -> Result<(Recorder, Vec<String>), String> {
    let mut engine = engine_as(flavour);
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    for _ in 0..ticks {
        let _reports = run(&mut engine, &mut host, 1);
        if host.texts(A).iter().any(|line| line.contains(" rec=end")) {
            break;
        }
    }
    assert!(host.stubbed.is_empty(), "{:?}", host.stubbed);
    let lines = host
        .texts(A)
        .into_iter()
        .filter(|line| line.starts_with("PROBE "))
        .map(str::to_owned)
        .collect();
    Ok((host, lines))
}

/// The `PROBE` lines of a transcript.
pub(super) fn probe_lines(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| line.starts_with("PROBE "))
        .map(str::to_owned)
        .collect()
}

/// The `rec` and `case` of a `PROBE` line, which name it.
fn name_of(line: &str) -> (String, String) {
    crate::probe::Line::parse(line).map_or_else(
        |_| (String::new(), String::new()),
        |parsed| {
            (
                parsed.rec.clone(),
                parsed.field("case").unwrap_or_default().to_owned(),
            )
        },
    )
}

/// Hold `ours` to `theirs` line by line, but for the `start` record and the
/// `(rec, case)` pairs in `deviations`, which must each differ — a
/// deviation that has come to agree is no deviation, and should go.
pub(super) fn hold(ours: &[String], theirs: &[String], deviations: &[(&str, &str)]) {
    let named = |line: &String| {
        let (rec, case) = name_of(line);
        rec != "start"
            && !deviations.iter().any(|(deviating_rec, deviating_case)| {
                rec == *deviating_rec && case == *deviating_case
            })
    };
    let ours_kept: Vec<&String> = ours.iter().filter(|line| named(line)).collect();
    let theirs_kept: Vec<&String> = theirs.iter().filter(|line| named(line)).collect();
    assert_eq!(ours_kept, theirs_kept);
    for (rec, case) in deviations {
        let find = |lines: &[String]| {
            lines
                .iter()
                .find(|line| name_of(line) == ((*rec).to_owned(), (*case).to_owned()))
                .cloned()
        };
        let (mine, grid) = (find(ours), find(theirs));
        assert!(
            mine.is_some() && grid.is_some(),
            "deviation {rec} {case} names no line"
        );
        assert!(mine != grid, "deviation {rec} {case} agrees now: {mine:?}");
    }
}

/// Second Life's answer that differs from run to run: `llBase64ToInteger` of
/// text with no Base64 at its front (15204949, then 12523626 on aditi). The
/// runtime answers `0`.
const UNDEFINED_ON_SECOND_LIFE: (&str, &str) = ("base64-integer", "junk");

#[test]
fn strings_functions_print_what_aditi_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/strings/functions.lsl"), "");
    let (_, ours) = replay(&source, EngineFlavour::SecondLife, 400)?;
    let aditi = probe_lines(include_str!(
        "../../../probes/results/aditi/strings/functions.txt"
    ));
    hold(&ours, &aditi, &[UNDEFINED_ON_SECOND_LIFE]);
    Ok(())
}

#[test]
fn strings_functions_print_what_opensim_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/strings/functions.lsl"), "");
    for (flavour, transcript) in [
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/strings/functions.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/strings/functions.txt"),
        ),
    ] {
        let (_, ours) = replay(&source, flavour, 400)?;
        hold(&ours, &probe_lines(transcript), &[]);
    }
    Ok(())
}

#[test]
fn strings_unicode_prints_what_each_grid_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/strings/unicode.lsl"), "");
    for (flavour, transcript) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/strings/unicode.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/strings/unicode.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/strings/unicode.txt"),
        ),
    ] {
        let (_, ours) = replay(&source, flavour, 400)?;
        hold(&ours, &probe_lines(transcript), &[]);
    }
    Ok(())
}

#[test]
fn strings_conversions_print_what_aditi_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/strings/conversions.lsl"), "");
    let (_, ours) = replay(&source, EngineFlavour::SecondLife, 400)?;
    let aditi = probe_lines(include_str!(
        "../../../probes/results/aditi/strings/conversions.txt"
    ));
    hold(&ours, &aditi, &[]);
    Ok(())
}

#[test]
fn strings_newer_print_what_aditi_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/strings/newer.lsl"), "");
    let (host, ours) = replay(&source, EngineFlavour::SecondLife, 400)?;
    let aditi = include_str!("../../../probes/results/aditi/strings/newer.txt");
    hold(&ours, &probe_lines(aditi), &[]);
    // The refusals of the unknown algorithms, in the grid's words.
    let refused: Vec<&str> = aditi
        .lines()
        .filter_map(|line| line.strip_prefix("# debug-channel: from=\"Object\" text=\""))
        .filter_map(|text| text.strip_suffix('"'))
        .collect();
    assert_eq!(host.heard(A), refused);
    Ok(())
}

/// One bounds case: its number and its source.
type Case = (u32, &'static str);

/// What a bounds case's engine reported: the case's number, the lines told
/// the owner and the lines said on `DEBUG_CHANNEL`.
type Report = (u32, Vec<String>, Vec<String>);

/// The `errors/bounds` cases.
const BOUNDS: [Case; 18] = [
    (
        1,
        include_str!("../../../probes/errors/bounds/bounds-01.lsl"),
    ),
    (
        2,
        include_str!("../../../probes/errors/bounds/bounds-02.lsl"),
    ),
    (
        3,
        include_str!("../../../probes/errors/bounds/bounds-03.lsl"),
    ),
    (
        4,
        include_str!("../../../probes/errors/bounds/bounds-04.lsl"),
    ),
    (
        5,
        include_str!("../../../probes/errors/bounds/bounds-05.lsl"),
    ),
    (
        6,
        include_str!("../../../probes/errors/bounds/bounds-06.lsl"),
    ),
    (
        7,
        include_str!("../../../probes/errors/bounds/bounds-07.lsl"),
    ),
    (
        8,
        include_str!("../../../probes/errors/bounds/bounds-08.lsl"),
    ),
    (
        9,
        include_str!("../../../probes/errors/bounds/bounds-09.lsl"),
    ),
    (
        10,
        include_str!("../../../probes/errors/bounds/bounds-10.lsl"),
    ),
    (
        11,
        include_str!("../../../probes/errors/bounds/bounds-11.lsl"),
    ),
    (
        12,
        include_str!("../../../probes/errors/bounds/bounds-12.lsl"),
    ),
    (
        13,
        include_str!("../../../probes/errors/bounds/bounds-13.lsl"),
    ),
    (
        14,
        include_str!("../../../probes/errors/bounds/bounds-14.lsl"),
    ),
    (
        15,
        include_str!("../../../probes/errors/bounds/bounds-15.lsl"),
    ),
    (
        16,
        include_str!("../../../probes/errors/bounds/bounds-16.lsl"),
    ),
    (
        17,
        include_str!("../../../probes/errors/bounds/bounds-17.lsl"),
    ),
    (
        18,
        include_str!("../../../probes/errors/bounds/bounds-18.lsl"),
    ),
];

/// The `errors/bounds-newer` cases, which only Second Life compiles.
const BOUNDS_NEWER: [Case; 4] = [
    (
        1,
        include_str!("../../../probes/errors/bounds-newer/bounds-01.lsl"),
    ),
    (
        2,
        include_str!("../../../probes/errors/bounds-newer/bounds-02.lsl"),
    ),
    (
        3,
        include_str!("../../../probes/errors/bounds-newer/bounds-03.lsl"),
    ),
    (
        4,
        include_str!("../../../probes/errors/bounds-newer/bounds-04.lsl"),
    ),
];

/// A bounds case's script, running its case as it enters `default` rather
/// than when its controller sends it the number.
fn bounds_script(number: u32, source: &str) -> String {
    offline(source, &format!("Bounds {number}")).replacen(
        "default\n{\n",
        "default\n{\n    state_entry() { run(); }\n",
        1,
    )
}

/// The `case` lines a transcript holds for `Bounds <number>`.
fn case_lines(transcript: &str, number: u32) -> Vec<String> {
    let script = format!("script=\"Bounds {number}\" case=");
    probe_lines(transcript)
        .into_iter()
        .filter(|line| line.contains(" rec=case ") && line.contains(&script))
        .collect()
}

/// Each case of `cases` under `flavour` prints the `case` lines the
/// transcript has for it, and nothing after a throw — but for the cases
/// `deviating` names, which must print something else; the lines its engine
/// tells the owner or says on `DEBUG_CHANNEL` for a throw, by case.
fn hold_bounds(
    cases: &[Case],
    flavour: EngineFlavour,
    transcript: &str,
    deviating: &[u32],
) -> Result<Vec<Report>, String> {
    let mut reports = Vec::new();
    let mut mismatched = Vec::new();
    for (number, source) in cases {
        let (host, ours) = replay(&bounds_script(*number, source), flavour, 50)?;
        let theirs = case_lines(transcript, *number);
        if (ours == theirs) == deviating.contains(number) {
            mismatched.push(format!(
                "Bounds {number}:\n  ours  {ours:?}\n  grid  {theirs:?}"
            ));
        }
        let told = host.told.iter().map(|(_, _, text)| text.clone()).collect();
        let said = host.heard(A).into_iter().map(str::to_owned).collect();
        reports.push((*number, told, said));
    }
    assert!(
        mismatched.is_empty(),
        "under {flavour:?}:\n{}",
        mismatched.join("\n")
    );
    Ok(reports)
}

#[test]
fn every_bounds_case_answers_as_on_aditi() -> Result<(), String> {
    let aditi = include_str!("../../../probes/results/aditi/errors/bounds.txt");
    for (number, told, said) in hold_bounds(&BOUNDS, EngineFlavour::SecondLife, aditi, &[])? {
        assert!(
            told.is_empty() && said.is_empty(),
            "Bounds {number}: {told:?} {said:?}"
        );
    }
    let newer = include_str!("../../../probes/results/aditi/errors/bounds-newer.txt");
    let _reports = hold_bounds(&BOUNDS_NEWER, EngineFlavour::SecondLife, newer, &[])?;
    Ok(())
}

/// The cases that print a vector or rotation with `(string)`, which writes
/// five decimals here, as the value model is Second Life's on every grid,
/// and six on OpenSim. Their library answers agree; only the cast differs.
const OPENSIM_VECTOR_CASTS: [u32; 3] = [1, 2, 3];

/// The cases whose library call throws on OpenSim: `llList2ListStrided`,
/// `llListStatistics` and `llInsertString`.
const THROWING: [u32; 3] = [9, 13, 16];

/// YEngine's report of an exception out of the library, as the transcript
/// has it after the item's key: the exception, where, and that a reset is
/// needed — the stack trace after those is not told.
const YENGINE_TOLD: [&str; 2] = [
    "IndexOutOfRangeException: Index was outside the bounds of the array.",
    "Script must be Reset to re-enable.",
];

#[test]
fn every_bounds_case_answers_as_on_yengine_and_a_throw_is_told_to_the_owner() -> Result<(), String>
{
    let transcript = include_str!("../../../probes/results/opensim-yengine/errors/bounds.txt");
    for (number, told, said) in hold_bounds(
        &BOUNDS,
        EngineFlavour::YEngine,
        transcript,
        &OPENSIM_VECTOR_CASTS,
    )? {
        assert!(said.is_empty(), "Bounds {number}: {said:?}");
        if THROWING.contains(&number) {
            let [first, exception, _where, reset] = told.as_slice() else {
                return Err(format!("Bounds {number} told {told:?}"));
            };
            assert!(first.starts_with("[YEngine]: Exception while running "));
            assert_eq!([exception.as_str(), reset.as_str()], YENGINE_TOLD);
            for line in YENGINE_TOLD {
                assert!(
                    transcript.contains(&format!("# owner-say: from=\"Object\" text=\"{line}\""))
                );
            }
        } else {
            assert!(told.is_empty(), "Bounds {number}: {told:?}");
        }
    }
    Ok(())
}

#[test]
fn every_bounds_case_answers_as_on_xengine_and_a_throw_is_said_on_debug() -> Result<(), String> {
    let transcript = include_str!("../../../probes/results/opensim-xengine/errors/bounds.txt");
    for (number, told, said) in hold_bounds(
        &BOUNDS,
        EngineFlavour::XEngine,
        transcript,
        &OPENSIM_VECTOR_CASTS,
    )? {
        assert!(told.is_empty(), "Bounds {number}: {told:?}");
        if THROWING.contains(&number) {
            let [line] = said.as_slice() else {
                return Err(format!("Bounds {number} said {said:?}"));
            };
            // The grid's line goes on with the stack trace.
            assert!(
                transcript.contains(&format!("text=\"{line}\\n  at ")),
                "Bounds {number}: {line}"
            );
        } else {
            assert!(said.is_empty(), "Bounds {number}: {said:?}");
        }
    }
    Ok(())
}
