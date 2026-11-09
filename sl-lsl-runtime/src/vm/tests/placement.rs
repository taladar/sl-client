//! Moving the prim (`llSetPos`) as each grid moves it: a root goes at most
//! 10 m towards its target in one call on both grids, a child's offset is
//! not capped on Second Life and is capped 10 m from where it was on
//! OpenSim, and the prim reads its new position back at once — the rows of
//! `book/src/gridspec/lsl.md` the `prim/placement` and `prim/link` probes
//! measured.

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;

use super::*;
use crate::vm::seating::Placement;

/// The three flavours.
const FLAVOURS: [EngineFlavour; 3] = [
    EngineFlavour::SecondLife,
    EngineFlavour::YEngine,
    EngineFlavour::XEngine,
];

/// A point `x` metres east of the origin.
const fn east(x: f32) -> Vector {
    Vector { x, y: 0.0, z: 0.0 }
}

/// A linkset of two prims whose second holds the script.
fn as_child() -> Links {
    Links {
        this: 2,
        prims: (1..=2)
            .map(|link| LinkedPrim {
                link,
                key: format!("fab01e52-3517-4545-be1b-4cbf6d8ace3{link}"),
                name: format!("Prim {link}"),
                scripts: if link == 2 { vec![A] } else { Vec::new() },
            })
            .collect(),
        sitters: Vec::new(),
    }
}

/// Runs `source` in a fresh `flavour` engine against `host` for a second of
/// ticks — long enough for each `llSetPos`'s forced delay.
fn ran(source: &str, flavour: EngineFlavour, host: &mut Recorder) -> Result<(), String> {
    let mut engine = engine_as(flavour);
    let _none = engine.add(A, program_for(source, flavour)?);
    let _reports = run(&mut engine, host, 10);
    Ok(())
}

/// A root moves to a target within 10 m, and 10 m towards one farther away,
/// on every flavour; `llGetPos` reads each move back at once.
#[test]
fn a_root_moves_at_most_ten_metres_a_call() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetPos(<5.0, 0.0, 0.0>);
        print((string)llGetPos());
        llSetPos(<55.0, 0.0, 0.0>);
        print((string)llGetPos());
    } }";
    for flavour in FLAVOURS {
        let mut host = Recorder::default();
        ran(source, flavour, &mut host)?;
        assert_eq!(
            host.moves,
            vec![(A, east(5.0)), (A, east(15.0))],
            "{flavour:?}"
        );
        // `(string)vector` has five decimals on Second Life and six on
        // OpenSim (`strings/conversions`).
        let printed = if flavour.is_opensim() {
            vec![
                "<5.000000, 0.000000, 0.000000>",
                "<15.000000, 0.000000, 0.000000>",
            ]
        } else {
            vec![
                "<5.00000, 0.00000, 0.00000>",
                "<15.00000, 0.00000, 0.00000>",
            ]
        };
        assert_eq!(host.texts(A), printed, "{flavour:?}");
    }
    Ok(())
}

/// The cap is a distance, not a per-axis clamp: a diagonal target goes 10 m
/// along the diagonal.
#[test]
fn the_cap_is_ten_metres_along_the_way() -> Result<(), String> {
    let source =
        "default { state_entry() { llSetPos(<30.0, 40.0, 0.0>); print((string)llGetPos()); } }";
    let mut host = Recorder::default();
    ran(source, EngineFlavour::SecondLife, &mut host)?;
    assert_eq!(host.texts(A), vec!["<6.00000, 8.00000, 0.00000>"]);
    Ok(())
}

/// A child set 15 m from its root, 2 m from where it stands: 15 m on Second
/// Life, 12 m on OpenSim (`prim/link`).
#[test]
fn a_child_offset_is_capped_on_opensim_only() -> Result<(), String> {
    let source = "default { state_entry() { llSetPos(<15.0, 0.0, 0.0>); } }";
    for flavour in FLAVOURS {
        let mut host = Recorder {
            linked: Some(as_child()),
            placements: [(
                A,
                Placement {
                    position: east(102.0),
                    local: east(2.0),
                    root: east(100.0),
                },
            )]
            .into_iter()
            .collect(),
            ..Recorder::default()
        };
        ran(source, flavour, &mut host)?;
        let wanted = if flavour.is_opensim() { 12.0 } else { 15.0 };
        assert_eq!(host.moves, vec![(A, east(wanted))], "{flavour:?}");
    }
    Ok(())
}

/// Each call costs the script its 0.2 s: two moves in a row are two ticks
/// apart at a tenth of a second a tick.
#[test]
fn a_move_costs_a_fifth_of_a_second() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetPos(<1.0, 0.0, 0.0>);
        print(\"after one\");
        llSetPos(<2.0, 0.0, 0.0>);
        print(\"after two\");
    } }";
    let mut host = Recorder::default();
    ran(source, EngineFlavour::SecondLife, &mut host)?;
    let ticks: Vec<u64> = host
        .printed
        .iter()
        .filter(|(_tick, caller, _text)| *caller == A)
        .map(|(tick, _caller, _text)| *tick)
        .collect();
    let gaps: Vec<u64> = ticks
        .windows(2)
        .filter_map(|pair| match pair {
            [first, second] => Some(second.saturating_sub(*first)),
            _other => None,
        })
        .collect();
    assert_eq!(gaps, vec![2]);
    Ok(())
}
