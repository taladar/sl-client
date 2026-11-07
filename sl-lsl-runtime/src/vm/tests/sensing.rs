//! What a script senses, held to the rules `detection/sensor` and
//! `detection/cast-ray` measured (`book/src/gridspec/lsl.md` § Sensors,
//! raycasts and touches). The probes themselves are not replayed line for
//! line: both grids' regions held other residents' objects at places the
//! transcripts do not record, and those fill the sweeps and their caps. So
//! the scene here is the probe's row with nobody else's objects in it, and
//! each rule a test of its own.

use pretty_assertions::assert_eq;
use sl_types::lsl::{Rotation, Vector};

use super::clocks::{run_for, stage};
use super::*;
use crate::vm::sensing::{
    ACTIVE, AGENT, AGENT_BY_USERNAME, HitKind, PASSIVE, Query, RayHit, SCRIPTED, Sensed, Sweeper,
    sweep,
};

/// The owner.
const OWNER_KEY: &str = super::OWNER;

/// The sweeping prim: at the origin's stand-in, facing east.
fn sweeper() -> Sweeper {
    Sweeper {
        position: at(100.0, 100.0, 30.0),
        rotation: Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        },
        root: "aaaaaaaa-0000-4000-8000-000000000000".to_owned(),
        group: crate::value::NULL_KEY.to_owned(),
    }
}

/// A point.
const fn at(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

/// An object of the scene: `name` at `position`, scripted or bare.
fn object(number: u32, name: &str, position: Vector, scripted: bool) -> Sensed {
    Sensed {
        key: format!("bbbbbbbb-0000-4000-8000-{number:012}"),
        name: name.to_owned(),
        username: String::new(),
        owner: OWNER_KEY.to_owned(),
        group: crate::value::NULL_KEY.to_owned(),
        position,
        rotation: sweeper().rotation,
        velocity: at(0.0, 0.0, 0.0),
        agent: false,
        physical: false,
        scripted,
    }
}

/// The probe's scene: scripted Targets B, V and U 2, 3 and 4 m east, bare
/// prims every metre from 5 m to 22 m, Target W 3.5 m up, Target X 100 m up,
/// and the owner 9.7 m away.
fn scene() -> Vec<Sensed> {
    let mut scene = vec![
        object(1, "Target B", at(102.0, 100.0, 30.0), true),
        object(2, "Target V", at(103.0, 100.0, 30.0), true),
        object(3, "Target U", at(104.0, 100.0, 30.0), true),
        object(4, "Target W", at(100.0, 100.0, 33.5), true),
        object(5, "Target X", at(100.0, 100.0, 130.0), true),
    ];
    for metre in 5_u16..=22 {
        scene.push(object(
            100 + u32::from(metre),
            "Object",
            at(100.0 + f32::from(metre), 100.0, 30.0),
            false,
        ));
    }
    scene.push(Sensed {
        key: OWNER_KEY.to_owned(),
        name: "Probe Owner".to_owned(),
        username: "probe.owner".to_owned(),
        owner: OWNER_KEY.to_owned(),
        group: crate::value::NULL_KEY.to_owned(),
        position: at(100.0, 109.7, 30.0),
        rotation: sweeper().rotation,
        velocity: at(0.0, 0.0, 0.0),
        agent: true,
        physical: true,
        scripted: false,
    });
    scene
}

/// The names a sweep found, nearest first; `None` for no event.
fn names(
    flavour: EngineFlavour,
    mask: i32,
    (name, range, arc): (&str, f32, f32),
    scene: &[Sensed],
) -> Option<Vec<String>> {
    let query = Query {
        name: name.to_owned(),
        key: crate::value::NULL_KEY.to_owned(),
        mask,
        range,
        arc,
    };
    sweep(&query, &sweeper(), scene, flavour)
        .map(|found| found.into_iter().map(|detected| detected.name).collect())
}

/// The flavours, Second Life's first.
const FLAVOURS: [EngineFlavour; 3] = [
    EngineFlavour::SecondLife,
    EngineFlavour::YEngine,
    EngineFlavour::XEngine,
];

#[test]
fn scripted_narrows_the_mask_on_second_life_alone() {
    let all = ACTIVE | PASSIVE | SCRIPTED;
    let pi = core::f32::consts::PI;
    for flavour in FLAVOURS {
        let found = names(flavour, all, ("", 96.0, pi), &scene()).unwrap_or_default();
        if flavour.is_opensim() {
            assert_eq!(found.len(), 16, "{flavour:?}: the union, capped");
            assert_eq!(found.first().map(String::as_str), Some("Target B"));
        } else {
            assert_eq!(
                found,
                vec!["Target B", "Target V", "Target W", "Target U"],
                "{flavour:?}: scripted objects only, nearest first"
            );
        }
        let passive = names(flavour, PASSIVE, ("", 96.0, pi), &scene()).unwrap_or_default();
        assert!(
            passive.iter().all(|name| name == "Object"),
            "{flavour:?}: PASSIVE finds the bare prims alone"
        );
    }
}

#[test]
fn a_sweep_returns_32_or_16_nearest_first() {
    let pi = core::f32::consts::PI;
    let mut crowd = Vec::new();
    for metre in 1_u16..=40 {
        crowd.push(object(
            200 + u32::from(metre),
            &format!("Crowd {metre}"),
            at(100.0 + f32::from(41 - metre) * 0.5, 100.0, 30.0),
            false,
        ));
    }
    for flavour in FLAVOURS {
        let found = names(flavour, PASSIVE, ("", 96.0, pi), &crowd).unwrap_or_default();
        let cap = if flavour.is_opensim() { 16 } else { 32 };
        assert_eq!(found.len(), cap, "{flavour:?}");
        assert_eq!(found.first().map(String::as_str), Some("Crowd 40"));
    }
}

#[test]
fn the_range_and_the_arc_are_read_as_each_grid_reads_them() {
    let pi = core::f32::consts::PI;
    let scripted = ACTIVE | SCRIPTED;
    for flavour in FLAVOURS {
        let opensim = flavour.is_opensim();
        let edge = names(flavour, scripted, ("Target V", 3.0, pi), &scene());
        assert_eq!(
            edge.map(|found| found.len()),
            Some(usize::from(opensim)),
            "{flavour:?}: a target exactly at the range"
        );
        let none = names(flavour, scripted, ("", 0.0, pi), &scene());
        assert_eq!(none, opensim.then(Vec::new), "{flavour:?}: a range of 0");
        let far = names(flavour, scripted, ("Target X", 200.0, pi), &scene());
        assert_eq!(far, Some(Vec::new()), "{flavour:?}: past 96 m");
        let narrow = names(flavour, scripted, ("Target W", 96.0, pi / 4.0), &scene());
        assert_eq!(narrow, Some(Vec::new()), "{flavour:?}: W is outside 45°");
        let zero = names(flavour, scripted, ("Target W", 96.0, 0.0), &scene());
        assert_eq!(
            zero.map(|found| found.len()),
            Some(usize::from(!opensim)),
            "{flavour:?}: an arc of 0"
        );
        let negative = names(flavour, scripted, ("", 96.0, -1.0), &scene());
        assert_eq!(
            negative.map(|found| found.is_empty()),
            Some(opensim),
            "{flavour:?}: an arc of -1"
        );
    }
}

#[test]
fn an_avatar_is_found_by_its_names_and_typed_as_each_grid_types_it() {
    let pi = core::f32::consts::PI;
    for flavour in FLAVOURS {
        let query = Query {
            name: String::new(),
            key: crate::value::NULL_KEY.to_owned(),
            mask: AGENT,
            range: 96.0,
            arc: pi,
        };
        let found = sweep(&query, &sweeper(), &scene(), flavour).unwrap_or_default();
        let kinds: Vec<i32> = found.iter().map(|detected| detected.kind).collect();
        assert_eq!(kinds, vec![if flavour.is_opensim() { 1 } else { 3 }]);
        let by_username = names(
            flavour,
            AGENT_BY_USERNAME,
            ("probe.owner", 96.0, pi),
            &scene(),
        );
        assert_eq!(
            by_username.map(|found| found.len()),
            Some(usize::from(!flavour.is_opensim())),
            "{flavour:?}"
        );
        let scripted = names(flavour, ACTIVE | SCRIPTED, ("Target B", 96.0, pi), &scene());
        let typed = sweep(
            &Query {
                name: "Target B".to_owned(),
                key: crate::value::NULL_KEY.to_owned(),
                mask: ACTIVE | SCRIPTED,
                range: 96.0,
                arc: pi,
            },
            &sweeper(),
            &scene(),
            flavour,
        );
        assert_eq!(scripted.map(|found| found.len()), Some(1));
        assert_eq!(
            typed.and_then(|found| found.first().map(|detected| detected.kind)),
            Some(12),
            "an idle scripted prim is PASSIVE | SCRIPTED"
        );
    }
}

/// How many `sensor` events a script `source` printed in `seconds` under
/// `flavour`, at its grid's frame rate, with the probe's scene to sweep.
fn sweeps_of(flavour: EngineFlavour, source: &str, seconds: u32) -> Result<usize, String> {
    let mut region = stage(flavour, &[(A, "Probe", source)], 0)?;
    region.host.sensed = scene();
    run_for(&mut region, flavour, seconds)?;
    Ok(region
        .lines()
        .iter()
        .filter(|line| line.contains("swept"))
        .count())
}

/// A script that sweeps for the owner as `start` says, printing each answer.
fn sweeping(start: &str) -> String {
    format!(
        "default {{\n    state_entry() {{ {start} }}\n    \
         sensor(integer n) {{ llOwnerSay(\"PROBE x rec=swept\"); }}\n    \
         no_sensor() {{ llOwnerSay(\"PROBE x rec=swept none\"); }}\n}}\n"
    )
}

#[test]
fn a_repeating_sweep_runs_as_each_grid_runs_it() -> Result<(), String> {
    for flavour in FLAVOURS {
        let opensim = flavour.is_opensim();
        // Every 0.5 s for 2.6 s: five sweeps, the first at 0.51 s, on Second
        // Life; four, the first at 0.6 s, on OpenSim.
        let repeat = sweeping(
            "llSensorRepeat(\"\", NULL_KEY, AGENT, 96.0, PI, 0.5); llSetTimerEvent(2.6); } \
             timer() { llSensorRemove(); llSetTimerEvent(0.0);",
        );
        assert_eq!(
            sweeps_of(flavour, &repeat, 4)?,
            if opensim { 4 } else { 5 },
            "{flavour:?}"
        );
        // An interval of 0: one sweep on Second Life, none on OpenSim.
        let zero = sweeping("llSensorRepeat(\"\", NULL_KEY, AGENT, 96.0, PI, 0.0);");
        assert_eq!(sweeps_of(flavour, &zero, 2)?, usize::from(!opensim));
        // Three in one event: answered once on Second Life, thrice on OpenSim.
        let three = sweeping(
            "llSensor(\"\", NULL_KEY, AGENT, 96.0, PI); llSensor(\"\", NULL_KEY, AGENT, 96.0, PI); \
             llSensor(\"\", NULL_KEY, AGENT, 96.0, PI);",
        );
        assert_eq!(sweeps_of(flavour, &three, 2)?, if opensim { 3 } else { 1 });
        // A sweep while a repeat runs ends it on Second Life.
        let replaced = sweeping(
            "llSensorRepeat(\"\", NULL_KEY, AGENT, 96.0, PI, 1.0); \
             llSensor(\"\", NULL_KEY, AGENT, 96.0, PI);",
        );
        let swept = sweeps_of(flavour, &replaced, 4)?;
        if opensim {
            assert!(swept >= 4, "{flavour:?}: the repeat runs on ({swept})");
        } else {
            assert_eq!(swept, 1, "{flavour:?}: the one sweep replaced the repeat");
        }
    }
    Ok(())
}

/// A hit of the ray fixture.
fn hit(kind: HitKind, key: &str, x: f32, inside: bool, host: bool) -> RayHit {
    RayHit {
        kind,
        key: key.to_owned(),
        root: key.to_owned(),
        link: 0,
        position: at(x, 128.0, 27.5),
        normal: at(-1.0, 0.0, 0.0),
        from_inside: inside,
        host,
    }
}

/// The ray of `detection/cast-ray` from prim A's centre: prim A where it
/// leaves it, the wall's face at 124.75, the phantom ghost's at 127.25, the
/// bare prim's at 129.75.
fn ray_scene() -> Vec<RayHit> {
    let prim = HitKind::Prim {
        physical: false,
        phantom: false,
    };
    vec![
        hit(
            prim,
            "aaaaaaaa-0000-4000-8000-00000000000a",
            120.25,
            true,
            true,
        ),
        hit(
            prim,
            "aaaaaaaa-0000-4000-8000-00000000000b",
            124.75,
            false,
            false,
        ),
        hit(
            HitKind::Prim {
                physical: false,
                phantom: true,
            },
            "aaaaaaaa-0000-4000-8000-00000000000c",
            127.25,
            false,
            false,
        ),
        hit(
            prim,
            "aaaaaaaa-0000-4000-8000-00000000000d",
            129.75,
            false,
            false,
        ),
    ]
}

/// What `llCastRay(start, end, options)` printed under `flavour`, and the
/// debug lines it said.
fn cast(flavour: EngineFlavour, options: &str) -> Result<(String, Vec<String>), String> {
    let source = format!(
        "default {{ state_entry() {{ llOwnerSay(\"PROBE x rec=ray \" + llList2CSV(llCastRay(<120, \
         128, 27.5>, <135, 128, 27.5>, [{options}]))); }} }}"
    );
    let mut region = stage(flavour, &[(A, "Probe", &source)], 0)?;
    region.host.ray_hits = ray_scene();
    run_for(&mut region, flavour, 1)?;
    Ok((
        region.lines().first().cloned().unwrap_or_default(),
        region.debug_lines(),
    ))
}

#[test]
fn a_ray_is_answered_as_each_grid_answers_it() -> Result<(), String> {
    let (plain, _) = cast(EngineFlavour::SecondLife, "")?;
    assert_eq!(
        plain,
        "PROBE x rec=ray aaaaaaaa-0000-4000-8000-00000000000b, <124.750000, 128.000000, 27.500000>, 1"
    );
    let (plain, _) = cast(EngineFlavour::YEngine, "")?;
    assert_eq!(
        plain, "PROBE x rec=ray aaaaaaaa-0000-4000-8000-00000000000a, <120.25, 128, 27.5>, 1",
        "OpenSim hits the prim the ray starts in, its position a string"
    );
    let (four, _) = cast(EngineFlavour::SecondLife, "RC_MAX_HITS, 4")?;
    assert!(four.ends_with(", 2"), "{four}: the wall and the bare prim");
    let (ghost, _) = cast(
        EngineFlavour::SecondLife,
        "RC_MAX_HITS, 4, RC_DETECT_PHANTOM, TRUE",
    )?;
    assert!(ghost.ends_with(", 3"), "{ghost}");
    for (options, words) in [
        (
            "RC_MAX_HITS, 0",
            "You must request at least one result from llCastRay.",
        ),
        (
            "RC_MAX_HITS, 300",
            "Error. No more than 256 hits can be return by llCastRay.",
        ),
        ("99, 1", "Invalid filter settings."),
        (
            "RC_REJECT_TYPES, 0x20000000",
            "Invalid filter settings for llCastRay.",
        ),
        (
            "RC_MAX_HITS",
            "Syntax error in llCastRay call. Incorrect number of elements in param list.",
        ),
    ] {
        let (answer, said) = cast(EngineFlavour::SecondLife, options)?;
        assert_eq!(answer, "PROBE x rec=ray ", "{options}: an empty list");
        assert_eq!(said, vec![words.to_owned()], "{options}");
    }
    let (zero, _) = cast(EngineFlavour::YEngine, "RC_MAX_HITS, 0")?;
    assert!(zero.ends_with(", 1"), "{zero}: OpenSim takes 0 for 1");
    let (unknown, _) = cast(EngineFlavour::YEngine, "99, 1")?;
    assert!(unknown.ends_with(", 1"), "{unknown}: OpenSim ignores it");
    let (missing, said) = cast(EngineFlavour::YEngine, "RC_MAX_HITS")?;
    assert_eq!(missing, "", "OpenSim's engine throws");
    assert!(said.is_empty(), "YEngine tells the owner: {said:?}");
    Ok(())
}
