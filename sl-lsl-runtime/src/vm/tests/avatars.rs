//! Avatar control held to the grids: the `avatar/animation`,
//! `avatar/controls` and `avatar/camera` probes run in the VM under each
//! flavour, the money tests' runner answering their questions, touching,
//! sitting, standing and holding keys down as the grids' runner did; every
//! `PROBE` line, `DEBUG_CHANNEL` line and question set against the
//! transcript's, and what the scripts sent the avatars' viewers
//! (`ScriptControlChange`, the follow camera) against the runner's notes.

use pretty_assertions::assert_eq;

use super::comms::{OTHER, canonical, name_of, noted_debug};
use super::money::{Runner, noted};
use super::strings::probe_lines;
use super::*;

/// The three grids' transcripts of the probe `avatar/<name>`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let files = match name {
        "animation" => [
            include_str!("../../../probes/results/aditi/avatar/animation.txt"),
            include_str!("../../../probes/results/opensim-yengine/avatar/animation.txt"),
            include_str!("../../../probes/results/opensim-xengine/avatar/animation.txt"),
        ],
        "controls" => [
            include_str!("../../../probes/results/aditi/avatar/controls.txt"),
            include_str!("../../../probes/results/opensim-yengine/avatar/controls.txt"),
            include_str!("../../../probes/results/opensim-xengine/avatar/controls.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/avatar/camera.txt"),
            include_str!("../../../probes/results/opensim-yengine/avatar/camera.txt"),
            include_str!("../../../probes/results/opensim-xengine/avatar/camera.txt"),
        ],
    };
    let [aditi, yengine, xengine] = files;
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// A runner of `flavour` with both avatars standing in the region and the
/// probe's prim where the grid's stood.
fn avatar_runner(flavour: EngineFlavour) -> Runner {
    let mut runner = Runner::new(flavour);
    // The grids' own frames: a held key repeats by them.
    let mut config = EngineConfig::for_step(super::clocks::frame(flavour));
    config.flavour = flavour;
    if flavour.is_opensim() {
        config.coalescing = crate::vm::Coalescing::Never;
    }
    runner.region.engine = Engine::new(config);
    for agent in [super::OWNER, OTHER] {
        let _previous = runner
            .region
            .host
            .avatars
            .insert(agent.to_owned(), TestAvatar::standing(name_of(agent)));
    }
    if flavour.is_opensim() {
        runner.region.place(1, 120.0, 128.0, 27.5);
    } else {
        runner.region.place(1, 52.0, 62.0, 34.606_415);
    }
    runner
}

/// The runner's notes of what the scripts sent viewers, in `transcript`.
fn viewer_notes(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| {
            line.starts_with("# script-control-change") || line.starts_with("# follow-cam")
        })
        .map(str::to_owned)
        .collect()
}

/// `line` without the field `name`.
fn without_field(line: &str, name: &str) -> String {
    line.split(' ')
        .filter(|field| !field.starts_with(&format!("{name}=")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hold `runner`, run, to `transcript`: every probe line but for `vary`,
/// every question, every `DEBUG_CHANNEL` line, and what went to viewers.
fn hold(runner: &Runner, flavour: EngineFlavour, transcript: &str, vary: &[&str]) {
    let shape = |lines: Vec<String>| -> Vec<String> {
        canonical(&lines, &[])
            .into_iter()
            .map(|line| {
                vary.iter()
                    .fold(line, |line, name| without_field(&line, name))
            })
            .collect()
    };
    assert_eq!(
        shape(runner.region.lines()),
        shape(probe_lines(transcript)),
        "{flavour:?}"
    );
    assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
    assert_eq!(
        runner.region.debug_lines(),
        noted_debug(transcript),
        "{flavour:?}"
    );
    assert_eq!(
        runner.region.host.avatar_notes,
        viewer_notes(transcript),
        "{flavour:?}"
    );
}

#[test]
fn animations_play_stop_and_override_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/avatar/animation.lsl");
    for (flavour, transcript) in transcripts("animation") {
        let mut runner = avatar_runner(flavour);
        runner.add(A, "Probe", 1, source)?;
        runner.run(9_000)?;
        hold(&runner, flavour, transcript, &[]);
    }
    Ok(())
}

#[test]
fn controls_are_taken_heard_and_released_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/avatar/controls.lsl");
    for (flavour, transcript) in transcripts("controls") {
        let mut runner = avatar_runner(flavour);
        runner.add(A, "Probe", 1, source)?;
        runner.run(9_000)?;
        // How many times Second Life repeated a held key in a second varied
        // between holds (21, 23); the test's frames repeat it every second
        // one, 22 times.
        let vary: &[&str] = if flavour.is_opensim() {
            &[]
        } else {
            &["repeats"]
        };
        hold(&runner, flavour, transcript, vary);
    }
    Ok(())
}

#[test]
fn the_camera_is_set_cleared_and_tracked_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/avatar/camera.lsl");
    for (flavour, transcript) in transcripts("camera") {
        let mut runner = avatar_runner(flavour);
        if flavour.is_opensim() {
            // OpenSim's camera is never turned: its rotation reads identity.
            if let Some(owner) = runner.region.host.avatars.get_mut(super::OWNER) {
                owner.camera.1 = sl_types::lsl::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                };
            }
        }
        runner.add(A, "Probe", 1, source)?;
        // Where the probe's prim stood: its focus is set 2 m above it.
        let here = if flavour.is_opensim() {
            sl_types::lsl::Vector {
                x: 120.0,
                y: 128.0,
                z: 27.5,
            }
        } else {
            sl_types::lsl::Vector {
                x: 52.0,
                y: 62.0,
                z: 34.606_415,
            }
        };
        let _previous = runner.region.host.placements.insert(
            A,
            crate::vm::seating::Placement {
                position: here.clone(),
                local: here.clone(),
                root: here,
            },
        );
        runner.run(9_000)?;
        hold(&runner, flavour, transcript, &[]);
    }
    Ok(())
}
