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
