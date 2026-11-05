//! The timer held to the grids: `events/timer-reset` run in the VM under
//! each flavour (see `prim_state.rs` for how a probe of the prim is run
//! offline) and every `PROBE` line set against what the grid printed. It is
//! the probe that told why `prim/changed` counted the media setters'
//! `CHANGED_MEDIA` a window late on Second Life: a timer does not fall due
//! there while its own handler runs.

use super::prim_state::{compared, hold, home_of, region, run_out};
use super::*;

/// `lines` without their `took` field, a measurement (`vary: took`); its
/// tenths are kept as `tenths`.
fn untimed(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line.find(" took=") {
            Some(start) => {
                let rest = line.get(start.saturating_add(1)..).unwrap_or_default();
                let end = rest.find(' ').map_or(line.len(), |end| {
                    start.saturating_add(1).saturating_add(end)
                });
                format!(
                    "{}{}",
                    line.get(..start).unwrap_or_default(),
                    line.get(end..).unwrap_or_default()
                )
            }
            None => line.clone(),
        })
        .collect()
}

#[test]
fn timer_falls_due_as_each_grid_let_it() -> Result<(), String> {
    let probe = include_str!("../../../probes/events/timer-reset/probe.lsl");
    let clock = include_str!("../../../probes/events/timer-reset/clock.lsl");
    let transcripts = [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/events/timer-reset.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/events/timer-reset.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/events/timer-reset.txt"),
        ),
    ];
    for (flavour, transcript) in transcripts {
        let (home, ground) = home_of(flavour);
        let scripts = [
            (B, "Clock", 1, clock.to_owned()),
            (A, "Probe", 1, probe.to_owned()),
        ];
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 1500)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(
            &untimed(&ours),
            &untimed(&theirs),
            &[],
            &format!("{flavour:?}"),
        );
    }
    Ok(())
}
