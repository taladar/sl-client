//! Second Life's error flood limit held to the grids: the `errors/flood-kinds`,
//! `errors/flood-paths` and `errors/owner-lines` probes run in the VM under
//! each flavour, every `PROBE` line and every `DEBUG_CHANNEL` line the runner
//! noted set against what the grid printed (see `prim_state.rs` for the
//! region they run in).

use pretty_assertions::assert_eq;

use super::prim_state::{compared, debug_notes, hold, home_of, region, run_out, unkeyed, unvaried};
use super::*;

/// The three flavours, each with its transcript of the probe `name` under
/// `errors/`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let [aditi, yengine, xengine] = match name {
        "flood-kinds" => [
            include_str!("../../../probes/results/aditi/errors/flood-kinds.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-kinds.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-kinds.txt"),
        ],
        "flood-paths" => [
            include_str!("../../../probes/results/aditi/errors/flood-paths.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-paths.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-paths.txt"),
        ],
        "flood-window" => [
            include_str!("../../../probes/results/aditi/errors/flood-window.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-window.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-window.txt"),
        ],
        "flood-epoch" => [
            include_str!("../../../probes/results/aditi/errors/flood-epoch.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-epoch.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-epoch.txt"),
        ],
        "flood-repeat" => [
            include_str!("../../../probes/results/aditi/errors/flood-repeat.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-repeat.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-repeat.txt"),
        ],
        "flood-records" => [
            include_str!("../../../probes/results/aditi/errors/flood-records.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/flood-records.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/flood-records.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/errors/owner-lines.txt"),
            include_str!("../../../probes/results/opensim-yengine/errors/owner-lines.txt"),
            include_str!("../../../probes/results/opensim-xengine/errors/owner-lines.txt"),
        ],
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// `lines` with an OpenSim fault's whereabouts left out — everything from
/// its `(script:` on, which names the grid's prim key and position.
fn unplaced(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| match line.find("(script:") {
            Some(at) => line.get(..at).unwrap_or(&line).trim_end().to_owned(),
            None => line,
        })
        .collect()
}

/// Holds `ours` to `theirs` but for `extra`, lines the gate passed that the
/// grid dropped, and `missing`, lines the grid passed that the gate dropped:
/// the turns of Second Life's limit that no model holds (see
/// [`crate::vm::FLOOD_WINDOW_SECONDS`]). Each forgiven line must be there;
/// of lines alike, the last is forgiven.
fn held_but(
    mut ours: Vec<String>,
    mut theirs: Vec<String>,
    extra: &[String],
    missing: &[String],
    context: &str,
) {
    for (lines, forgiven, side) in [(&mut ours, extra, "ours"), (&mut theirs, missing, "theirs")] {
        for line in forgiven {
            let at = lines.iter().rposition(|found| found == line);
            assert!(at.is_some(), "{context}: {side} lack the forgiven {line:?}");
            if let Some(at) = at {
                lines.remove(at);
            }
        }
    }
    assert_eq!(ours, theirs, "{context}");
}

/// The complaint `PRIM_MATERIAL` `value` makes on Second Life, told the
/// owner alone.
fn material(value: u32) -> String {
    format!(
        "llSetPrimitiveParams error running rule #1 (PRIM_MATERIAL): bounds error; {value} is not in (0, 7)."
    )
}

/// The complaint a missing sound `name` makes on Second Life, said in the
/// open.
fn sound(name: &str) -> String {
    format!("Could not find sound '{name}'.")
}

/// The seconds the `cost` record of `call` says it cost, in `lines`.
fn cost_of(lines: &[String], call: &str) -> Option<f64> {
    lines
        .iter()
        .filter_map(|line| crate::probe::Line::parse(line).ok())
        .find(|line| line.rec == "cost" && line.field("call") == Some(call))
        .and_then(|line| line.field("cost")?.parse::<f64>().ok())
}

#[test]
fn flood_kinds_count_every_line_of_an_object_as_each_grid_did() -> Result<(), String> {
    let flooder = include_str!("../../../probes/errors/flood-kinds/flooder.lsl");
    let faulter = include_str!("../../../probes/errors/flood-kinds/faulter.lsl");
    for (flavour, transcript) in transcripts("flood-kinds") {
        let (home, ground) = home_of(flavour);
        let scripts = [
            (B, "Faulter", 1, faulter.to_owned()),
            (A, "Flooder", 1, flooder.to_owned()),
        ];
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 4000)?;
        let (ours, theirs) = compared(&region, transcript);
        // The Faulter's record comes from a second script of the prim, in
        // the middle of the Flooder's on the grid; each is held in its own
        // order.
        let faulter = |lines: &[String], kept: bool| -> Vec<String> {
            unvaried(
                lines
                    .iter()
                    .filter(|line| line.contains(" rec=fault ") == kept)
                    .cloned()
                    .collect(),
            )
        };
        let context = format!("{flavour:?}");
        hold(
            &faulter(&ours, true),
            &faulter(&theirs, true),
            &[],
            &context,
        );
        hold(
            &faulter(&ours, false),
            &faulter(&theirs, false),
            &[],
            &context,
        );
        assert_eq!(
            unplaced(region.debug_lines()),
            unplaced(debug_notes(transcript)),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn flood_paths_count_and_cost_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/errors/flood-paths.lsl");
    for (flavour, transcript) in transcripts("flood-paths") {
        let (home, ground) = home_of(flavour);
        let mut region = region(
            flavour,
            &[(A, "Probe", 1, source.to_owned())],
            ground,
            &home,
        )?;
        run_out(&mut region, 5000)?;
        let (ours, theirs) = compared(&region, transcript);
        // What a missing sound and a missing item cost, to a frame either way
        // (Second Life's clock moves in 1/45 s, ours in tenths).
        for call in ["sound", "texture"] {
            let (mine, grid) = (cost_of(&ours, call), cost_of(&theirs, call));
            let (Some(mine), Some(grid)) = (mine, grid) else {
                return Err(format!("{flavour:?}: no {call} cost"));
            };
            assert!(
                (mine - grid).abs() < 0.1,
                "{flavour:?} {call}: {mine} against {grid}"
            );
        }
        hold(
            &unvaried(ours),
            &unvaried(theirs),
            &[],
            &format!("{flavour:?}"),
        );
        assert_eq!(
            unkeyed(region.debug_lines()),
            unkeyed(debug_notes(transcript)),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn a_rule_lists_complaints_reach_the_owner_alone_on_second_life() -> Result<(), String> {
    let listener = include_str!("../../../probes/errors/owner-lines/listener.lsl");
    let speaker = include_str!("../../../probes/errors/owner-lines/speaker.lsl");
    for (flavour, transcript) in transcripts("owner-lines") {
        let (home, ground) = home_of(flavour);
        let scripts = [
            (B, "Listener", 2, listener.to_owned()),
            (A, "Speaker", 1, speaker.to_owned()),
        ];
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 400)?;
        let (ours, theirs) = compared(&region, transcript);
        // The listener's lines interleave with the speaker's on the grid, as
        // the runner heard them; each script's are held in their own order.
        let heard = |lines: &[String], kept: bool| -> Vec<String> {
            unkeyed(
                lines
                    .iter()
                    .filter(|line| line.contains(" rec=heard ") == kept)
                    .cloned()
                    .collect(),
            )
        };
        let context = format!("{flavour:?}");
        hold(&heard(&ours, true), &heard(&theirs, true), &[], &context);
        hold(&heard(&ours, false), &heard(&theirs, false), &[], &context);
        assert_eq!(
            unkeyed(region.debug_lines()),
            unkeyed(debug_notes(transcript)),
            "{flavour:?}"
        );
    }
    Ok(())
}

/// Runs the one-script probe `name` (`source`) for `steps` under each
/// flavour, holding its records but those of the kind `untimed` (its timing
/// records, where its phase ends on the clock) and its debug lines to the
/// grid's, but for the Second Life lines `extra` and `missing` (see
/// [`held_but`]).
fn replayed(
    name: &str,
    source: &str,
    steps: u32,
    untimed: Option<&str>,
    extra: &[String],
    missing: &[String],
) -> Result<(), String> {
    for (flavour, transcript) in transcripts(name) {
        let (home, ground) = home_of(flavour);
        let mut region = region(
            flavour,
            &[(A, "Probe", 1, source.to_owned())],
            ground,
            &home,
        )?;
        run_out(&mut region, steps)?;
        let (ours, theirs) = compared(&region, transcript);
        let context = format!("{name} {flavour:?}");
        let timed = |lines: Vec<String>| -> Vec<String> {
            let kind = untimed.map(|kind| format!(" rec={kind} "));
            lines
                .into_iter()
                .filter(|line| {
                    kind.as_ref()
                        .is_none_or(|kind| !line.contains(kind.as_str()))
                })
                .collect()
        };
        hold(
            &unvaried(timed(ours)),
            &unvaried(timed(theirs)),
            &[],
            &context,
        );
        let (extra, missing) = if flavour.is_opensim() {
            (&[][..], &[][..])
        } else {
            (extra, missing)
        };
        held_but(
            region.debug_lines(),
            debug_notes(transcript),
            extra,
            missing,
            &context,
        );
    }
    Ok(())
}

// The four probes below measured where Second Life's limit turns, and it
// turns about a second either way from one phase to the next: the same
// phase passed its late lines in one run and dropped them in the next
// (`errors/flood-repeat`). The gate counts over a fixed window, so the lines
// it passes or drops against the grid's in those turns are named.

#[test]
fn flood_window_turns_as_each_grid_turned() -> Result<(), String> {
    let source = include_str!("../../../probes/errors/flood-window.lsl");
    // O16's late lines came 16 seconds after the first, which flooded here
    // and passed in both of flood-repeat's B16; N17's ninth came 17 seconds
    // after the first, which made the notice here and passed in flood-repeat.
    let extra = [
        material(1610),
        material(1611),
        sound("N17-9"),
        sound("N17-10"),
        sound("N17-11"),
    ];
    replayed(
        "flood-window",
        source,
        7500,
        None,
        &extra,
        &[FLOOD_NOTICE.to_owned()],
    )
}

#[test]
fn flood_epoch_lasts_as_each_grid_kept_it() -> Result<(), String> {
    let source = include_str!("../../../probes/errors/flood-epoch.lsl");
    // Lines 1.5 seconds apart: aditi passed the tenth, as it passed nine to
    // eleven of flood-records' Q, R and X.
    replayed("flood-epoch", source, 7500, None, &[], &[material(1010)])
}

#[test]
fn flood_repeat_turns_as_each_grid_turned() -> Result<(), String> {
    let source = include_str!("../../../probes/errors/flood-repeat.lsl");
    // The turns at 16 and 17 seconds; then the floods of G2 and G3, which
    // aditi kept going 56 and 49 seconds where G25's, and the gate's, ended
    // after about 34.
    let mut extra = vec![
        sound("N16r1-9"),
        sound("N16r1-10"),
        sound("N16r1-11"),
        sound("N17r1-10"),
        sound("N17r1-11"),
        material(21711),
    ];
    extra.extend((31216..=31224).map(material));
    extra.push(material(31227));
    extra.extend((31311..=31314).map(material));
    extra.push(material(31319));
    let missing = [
        material(11611),
        FLOOD_NOTICE.to_owned(),
        FLOOD_NOTICE.to_owned(),
        material(21611),
    ];
    replayed(
        "flood-repeat",
        source,
        13000,
        Some("single"),
        &extra,
        &missing,
    )
}

#[test]
fn flood_records_count_as_each_grid_counted() -> Result<(), String> {
    let source = include_str!("../../../probes/errors/flood-records.lsl");
    // Lines 1.5 seconds apart: aditi passed nine, eleven, nine, eleven and
    // ten of them, with records between or not; the gate passes nine.
    let missing = [210, 211, 410, 411, 510].map(material);
    replayed("flood-records", source, 9500, None, &[], &missing)
}
