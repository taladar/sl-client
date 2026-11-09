//! Position and rotation targets held to the grids: `physics/targets` run in
//! the VM under each flavour, its target phases set against what each grid
//! printed — the handles, which targets raise their events, the ranges, the
//! eight a script keeps. How many events a second ran is a measurement
//! (`vary: at, not_at`), so a count is held only to whether it is zero. On
//! OpenSim a handle is the region's next local id, which anything else in
//! the region may have drawn from between two targets, so the steps between
//! handles are not held there. The probe goes on to `llMoveToTarget` and the look-at functions, which are the
//! physics tranche's, and stops there.

use pretty_assertions::assert_eq;

use super::strings::{offline, probe_lines};
use super::*;

/// The records of the target phases, up to the movers.
const PHASES: [&str; 6] = [
    "targets",
    "ten-targets",
    "ranges",
    "rot-handles",
    "rot-targets",
    "ten-rot-targets",
];

/// `line` with each count of events written as whether it is zero, and on
/// OpenSim without the steps between handles. A ten-target phase's
/// `not_at` is left out: a `not_at_target` queued just before the phase
/// began may run in it or not — XEngine's position phase heard one, its
/// rotation phase none.
fn bucketed(line: &str, opensim: bool) -> String {
    let ten = line.contains(" rec=ten-");
    line.split(' ')
        .filter_map(|field| match field.split_once('=') {
            Some(("not_at", _)) if ten => None,
            Some((name @ ("at" | "not_at"), value)) => Some(format!(
                "{name}={}",
                if value == "0" { "0" } else { "some" }
            )),
            Some(("steps" | "after_position_target", _)) if opensim => None,
            _ => Some(field.to_owned()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The target phases' records of `lines`, bucketed.
fn phases(lines: &[String], opensim: bool) -> Vec<String> {
    lines
        .iter()
        .filter(|line| {
            crate::probe::Line::parse(line)
                .is_ok_and(|parsed| PHASES.contains(&parsed.rec.as_str()))
        })
        .map(|line| bucketed(line, opensim))
        .collect()
}

#[test]
fn targets_are_reached_as_each_grid_reached_them() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/physics/targets.lsl"), "");
    for (flavour, transcript) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/physics/targets.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/physics/targets.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/physics/targets.txt"),
        ),
    ] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program_for(&source, flavour)?);
        // Six phases of a second each, at ten ticks a second.
        let _reports = run(&mut engine, &mut host, 80);
        let ours: Vec<String> = host
            .texts(A)
            .into_iter()
            .filter(|line| line.starts_with("PROBE "))
            .map(str::to_owned)
            .collect();
        let opensim = flavour.is_opensim();
        assert_eq!(
            phases(&ours, opensim),
            phases(&probe_lines(transcript), opensim),
            "{flavour:?}"
        );
    }
    Ok(())
}
