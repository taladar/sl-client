//! The land, the region and the environment held to the grids: the `land/*`
//! probes run in the VM under each flavour, in a region of this test's own
//! stated as each grid's land was — the parcel's name, owner, flags and
//! counts, the region's name, corner and estate, the ground's height — and
//! every `PROBE` line and `DEBUG_CHANNEL` line set against the transcript's
//! but for the fields the probe's header names as varying. The land is the
//! grids' own (aditi's Mauve sandbox, the local OpenSim's Default Region), so
//! what the test states of it is copied from the transcripts; what is checked
//! is what the calls do with it.

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;

use super::clocks::{run_for, stage};
use super::prim_state::debug_notes;
use super::strings::probe_lines;
use super::*;
use crate::vm::land::{Parcel, PrimCounts, Region, SimData};

/// A parcel of `flags` owned by `owner`, otherwise empty.
pub(super) fn parcel(flags: u32, owner: &str) -> Parcel {
    Parcel {
        id: "9a9a9a9a-0000-4000-8000-000000000001".to_owned(),
        name: String::new(),
        description: String::new(),
        owner: owner.to_owned(),
        group: crate::value::NULL_KEY.to_owned(),
        group_owned: false,
        area: 0,
        flags,
        see_avatars: false,
        landing: (
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        ),
        tp_routing: 0,
        dwell: 0.0,
        claim_date: 0,
        sounds: (false, false),
        music_url: String::new(),
        max_prims: (0, 0),
        capacity: (0, 0),
        counts: (PrimCounts::default(), PrimCounts::default()),
        prim_owners: Vec::new(),
    }
}

/// The transcripts of the probe `name`, per flavour.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    macro_rules! three {
        ($probe:literal) => {
            [
                include_str!(concat!(
                    "../../../probes/results/aditi/land/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-yengine/land/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-xengine/land/",
                    $probe,
                    ".txt"
                )),
            ]
        };
    }
    let [aditi, yengine, xengine] = match name {
        "env" => three!("env"),
        "parcel" => three!("parcel"),
        "region" => three!("region"),
        "ground" => three!("ground"),
        _ => three!("day-cycle"),
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// The probes' group, on Second Life the owner of the group-owned sandbox.
const GROUP: &str = "9a9a9a9a-0000-4000-8000-0000000000ff";

/// Each grid's land as the transcripts describe it: the parcel the probe
/// stood on, the parcel at the west edge, and the region.
fn land(flavour: EngineFlavour) -> (Parcel, Option<Parcel>, Region) {
    if flavour.is_opensim() {
        let mut here = parcel(671_096_907, "0c0c0c0c-0000-4000-8000-000000000003");
        here.name = "Your Parcel".to_owned();
        here.area = 256 * 256;
        here.dwell = 56.0;
        here.max_prims = (15_000, 15_000);
        here.counts = (
            PrimCounts {
                total: 42,
                owner: 34,
                group: 0,
                other: 8,
                selected: 0,
                temporary: 0,
            },
            PrimCounts {
                total: 42,
                ..PrimCounts::default()
            },
        );
        here.prim_owners = vec![
            (super::OWNER.to_owned(), 8),
            ("0c0c0c0c-0000-4000-8000-000000000003".to_owned(), 34),
        ];
        let region = Region {
            name: "Default Region".to_owned(),
            channel: "OpenSim".to_owned(),
            corner: (256_000.0, 256_000.0),
            flags: 335_544_326,
            rating: "PG".to_owned(),
            estate: (101, "My Estate".to_owned()),
            product: (String::new(), "OpenSim".to_owned()),
            capacity: (15_000, 1.0),
            agent_limit: 40,
            start_time: 1_792_997_270,
            hostname: "127.0.0.1".to_owned(),
            agents: 2,
            water: 20.0,
            neighbours: [true, true, false, false],
        };
        return (here, None, region);
    }
    let mut here = parcel(908_100_683, GROUP);
    here.name = "PUBLIC SANDBOX - All Welcome! (PG) No weapons. No Selling".to_owned();
    let head = "SL Orientation, Building Classes, Help for Newbies, Free Stu";
    here.description = format!("{head}{}", "-".repeat(144_usize.saturating_sub(head.len())));
    here.group = GROUP.to_owned();
    here.group_owned = true;
    here.area = 14_176;
    here.landing = (
        Vector {
            x: 116.431,
            y: 95.109,
            z: 35.1953,
        },
        Vector {
            x: 0.12894,
            y: -0.991_652,
            z: 0.0,
        },
    );
    here.tp_routing = 1;
    here.max_prims = (22_510, 22_510);
    here.capacity = (78_532, 1_398);
    let mut edge = parcel(1_040_187_499, "9a9a9a9a-0000-4000-8000-0000000000ee");
    edge.id = "9a9a9a9a-0000-4000-8000-000000000002".to_owned();
    edge.area = 3392;
    edge.max_prims = (11_650, 22_510);
    edge.counts.0.total = 19;
    let region = Region {
        name: "Mauve".to_owned(),
        channel: "Luau".to_owned(),
        corner: (255_744.0, 257_792.0),
        flags: 0x0010_0000,
        rating: "ADULT".to_owned(),
        estate: (1, "mainland".to_owned()),
        product: ("Mainland / Full Region".to_owned(), "023".to_owned()),
        capacity: (22_510, 10.0),
        agent_limit: 55,
        start_time: 1_788_286_336,
        hostname: "simhost-08ead0889a544632b.aditi.secondlife.io".to_owned(),
        agents: 9,
        water: 20.0,
        neighbours: [false; 4],
    };
    (here, Some(edge), region)
}

/// A region of `flavour` stated as its grid's land was, the probe's scripts
/// `(id, name, source)` in prim A standing where the probe stood.
fn land_stage(
    flavour: EngineFlavour,
    scripts: &[(CallerId, &str, &str)],
) -> Result<super::comms::Region, String> {
    let mut region = stage(flavour, &[], 1_793_179_656_000_000)?;
    for (id, name, source) in scripts {
        region.add_unrigged(*id, name, 1, source)?;
    }
    let (here, edge, info) = land(flavour);
    region.host.parcel = Some(here);
    region.host.edge_parcel = edge;
    region.host.region = info;
    let (home, ground) = super::prim_state::home_of(flavour);
    region.host.ground = ground;
    for (id, _, _) in scripts {
        let _placed = region.host.placements.insert(
            *id,
            crate::vm::seating::Placement {
                position: home.clone(),
                local: home.clone(),
                root: home.clone(),
            },
        );
    }
    region.host.simulator = Simulator {
        version: if flavour.is_opensim() {
            "OpenSim 0.9.2.1 Yeti Dev   36f6d16 (Unix/Mono)".to_owned()
        } else {
            "2026-07-24.30110090421".to_owned()
        },
        grid_name: String::new(),
        grid_nick: if flavour.is_opensim() {
            String::new()
        } else {
            "aditi".to_owned()
        },
        physics_engine_type: String::new(),
        physics_engine_name: String::new(),
    };
    Ok(region)
}

/// The fields of a `PROBE` line, quoted values kept whole.
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in line.chars() {
        match character {
            '\\' if quoted && !escaped => {
                escaped = true;
                current.push(character);
                continue;
            }
            '"' if !escaped => quoted = !quoted,
            ' ' if !quoted => {
                out.push(core::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        escaped = false;
        current.push(character);
    }
    out.push(current);
    out
}

/// `lines` without the start record and without the fields named in
/// `vary`, grouped by record when `by_record` (two scripts' lines of one
/// frame may come in either order).
fn shaped(lines: &[String], vary: &[&str], by_record: bool) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .filter(|line| !line.contains(" rec=start "))
        .map(|line| {
            fields(line)
                .into_iter()
                .filter(|field| {
                    !vary
                        .iter()
                        .any(|name| field.starts_with(&format!("{name}=")))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    if by_record {
        out.sort_by_key(|line| {
            line.split(' ')
                .find_map(|field| field.strip_prefix("rec="))
                .unwrap_or_default()
                .to_owned()
        });
    }
    out
}

/// Run `region` and hold it to `transcript` but for `vary` and the lines
/// named in `deviations` (each a line of ours and the grid's that must
/// differ).
fn hold(
    region: &mut super::comms::Region,
    flavour: EngineFlavour,
    transcript: &str,
    (vary, by_record): (&[&str], bool),
    deviations: &[&str],
) -> Result<(), String> {
    run_for(region, flavour, 60)?;
    let deviating = |line: &String| deviations.iter().any(|mark| line.contains(mark));
    let ours: Vec<String> = shaped(&region.lines(), vary, by_record)
        .into_iter()
        .filter(|line| !deviating(line))
        .collect();
    let theirs: Vec<String> = shaped(&probe_lines(transcript), vary, by_record)
        .into_iter()
        .filter(|line| !deviating(line))
        .collect();
    assert_eq!(ours, theirs, "{flavour:?}");
    let ours_debug: Vec<String> = region
        .debug_lines()
        .into_iter()
        .map(|line| line.split('\n').next().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(ours_debug, debug_notes(transcript), "{flavour:?}");
    Ok(())
}

#[test]
fn get_env_answers_each_grids_keys() -> Result<(), String> {
    let source = include_str!("../../../probes/land/env.lsl");
    for (flavour, transcript) in transcripts("env") {
        let mut region = land_stage(flavour, &[(A, "Probe", source)])?;
        hold(
            &mut region,
            flavour,
            transcript,
            (&["reading", "frames_per_second"], false),
            &[],
        )?;
    }
    Ok(())
}

#[test]
fn parcel_queries_read_the_land_as_each_grid_did() -> Result<(), String> {
    let leader = include_str!("../../../probes/land/parcel/leader.lsl");
    let outside = [
        (
            "Outside flags",
            include_str!("../../../probes/land/parcel/outside-flags.lsl"),
        ),
        (
            "Outside details",
            include_str!("../../../probes/land/parcel/outside-details.lsl"),
        ),
        (
            "Outside owner",
            include_str!("../../../probes/land/parcel/outside-owner.lsl"),
        ),
        (
            "Outside danger",
            include_str!("../../../probes/land/parcel/outside-danger.lsl"),
        ),
        (
            "Outside max prims",
            include_str!("../../../probes/land/parcel/outside-max-prims.lsl"),
        ),
        (
            "Outside prim count",
            include_str!("../../../probes/land/parcel/outside-prim-count.lsl"),
        ),
    ];
    for (flavour, transcript) in transcripts("parcel") {
        let mut scripts: Vec<(CallerId, &str, &str)> = outside
            .iter()
            .zip(1_u64..)
            .map(|((name, source), id)| (CallerId(id), *name, *source))
            .collect();
        scripts.push((CallerId(20), "Leader", leader));
        let mut region = land_stage(flavour, &scripts)?;
        // The dwell moved by one between the two OpenSim runs.
        if flavour == EngineFlavour::XEngine
            && let Some(parcel) = region.host.parcel.as_mut()
        {
            parcel.dwell = 57.0;
        }
        hold(
            &mut region,
            flavour,
            transcript,
            (&["count", "used"], true),
            &[],
        )?;
    }
    Ok(())
}

#[test]
fn region_queries_answer_as_each_grid_did() -> Result<(), String> {
    // `llGetAgentSize` is the avatar tranche's: an avatar of the list is
    // one by its key.
    let source = include_str!("../../../probes/land/region.lsl").replace(
        "llGetAgentSize(llList2Key(got, 0)) != ZERO_VECTOR",
        "llList2Key(got, 0) != \"INVALID_SCOPE\"",
    );
    for (flavour, transcript) in transcripts("region") {
        let mut region = land_stage(flavour, &[(A, "Probe", &source)])?;
        let info = region.host.region.clone();
        let _own = region.host.sims.insert(
            info.name.to_lowercase(),
            SimData {
                corner: info.corner,
                status: "up".to_owned(),
                rating: info.rating.clone(),
            },
        );
        // The avatars of `agent-list`: the owner, and the others of the scope.
        region.host.sensed = vec![crate::vm::sensing::Sensed {
            key: super::OWNER.to_owned(),
            name: "Probe Owner".to_owned(),
            username: "probe.owner".to_owned(),
            owner: super::OWNER.to_owned(),
            group: crate::value::NULL_KEY.to_owned(),
            position: Vector {
                x: 50.0,
                y: 60.0,
                z: 30.0,
            },
            rotation: sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            velocity: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            agent: true,
            physical: true,
            scripted: false,
        }];
        // OpenSim writes a vector cast to a string with six decimals, the
        // value model Second Life's five on every grid (`EngineFlavour`): the
        // directions and the corner are compared as the answers beside them.
        let vary: &[&str] = if flavour.is_opensim() {
            &[
                "fps", "dilation", "agents", "others", "hostname", "dir", "corner",
            ]
        } else {
            &["fps", "dilation", "agents", "others", "hostname"]
        };
        hold(&mut region, flavour, transcript, (vary, true), &[])?;
    }
    Ok(())
}

#[test]
fn the_ground_and_the_water_read_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/land/ground.lsl");
    for (flavour, transcript) in transcripts("ground") {
        let mut region = land_stage(flavour, &[(A, "Probe", source)])?;
        let (home, _) = super::prim_state::home_of(flavour);
        if flavour.is_opensim() {
            region.host.ground = home.z - 6.5;
            region.host.edges = Some([home.z - 6.5, home.z + 11.5, home.z - 6.5, home.z - 6.5]);
        } else {
            region.host.ground = home.z - 2.163;
            region.host.edges = Some([
                home.z - 9.457,
                home.z - 7.217,
                home.z - 5.437,
                home.z - 6.637,
            ]);
        }
        region.host.wind = Some(Vector {
            x: -0.5,
            y: 1.0,
            z: 0.0,
        });
        // Second Life read a position beyond an edge 0.035 m above the edge
        // itself (its terrain clamped a hair inside), so not as the clamped
        // position; the runtime reads the edge, as OpenSim does.
        // OpenSim writes a vector cast to a string with six decimals, the
        // value model Second Life's five on every grid (`EngineFlavour`).
        let deviations: &[&str] = if flavour.is_opensim() {
            &["rec=weather", "rec=surface-flat"]
        } else {
            &[
                "rec=ground-west",
                "rec=weather",
                "offset=west-out",
                "offset=east-out",
                "offset=south-out",
                "offset=north-out",
            ]
        };
        hold(
            &mut region,
            flavour,
            transcript,
            (&["wind", "cloud"], false),
            deviations,
        )?;
    }
    Ok(())
}

#[test]
fn the_day_cycle_reads_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/land/day-cycle.lsl");
    for (flavour, transcript) in transcripts("day-cycle") {
        let mut region = land_stage(flavour, &[(A, "Probe", source)])?;
        // Second Life's parcel and region suns were read a moment apart and
        // differ in the fifth place, and its parcel moon's up axis was a hair
        // off the sun's (`dot_up=-0.003`); the test's are one sun and its
        // exact opposite.
        let deviations: &[&str] = if flavour.is_opensim() {
            &[]
        } else {
            &["rec=agree", "which=moon scope=parcel"]
        };
        hold(
            &mut region,
            flavour,
            transcript,
            (&["time_of_day", "sun", "moon", "fraction"], false),
            deviations,
        )?;
    }
    Ok(())
}
