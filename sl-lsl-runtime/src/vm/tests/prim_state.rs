//! Reading and writing the prim held to the grids: the `prim/*` probes run
//! in the VM under each flavour, in a region of this test's own whose prims
//! are the runtime's own [`Prim`](crate::vm::prim::Prim)s, and every `PROBE`
//! line and every `DEBUG_CHANNEL` line the runner noted set against what the
//! grid printed (see `strings.rs` for how a probe is run offline).
//!
//! What a probe reads of a tranche not written yet is written in where it
//! reads it: `llGround` is the height the grid's ground had under the probe,
//! and `llSetStatus(STATUS_PHANTOM, …)` the `PRIM_PHANTOM` rule, which raises
//! nothing either. Every other difference is a named deviation, with its
//! reason.

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;

use super::comms::{Region, canonical};
use super::strings::probe_lines;
use super::*;

/// The three flavours, each with its transcript of the probe `name`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let [aditi, yengine, xengine] = match name {
        "params" => [
            include_str!("../../../probes/results/aditi/prim/params.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/params.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/params.txt"),
        ],
        "params-rules" => [
            include_str!("../../../probes/results/aditi/prim/params-rules.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/params-rules.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/params-rules.txt"),
        ],
        "faces" => [
            include_str!("../../../probes/results/aditi/prim/faces.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/faces.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/faces.txt"),
        ],
        "identity" => [
            include_str!("../../../probes/results/aditi/prim/identity.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/identity.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/identity.txt"),
        ],
        "placement" => [
            include_str!("../../../probes/results/aditi/prim/placement.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/placement.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/placement.txt"),
        ],
        "placement-settle" => [
            include_str!("../../../probes/results/aditi/prim/placement-settle.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/placement-settle.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/placement-settle.txt"),
        ],
        "changed" => [
            include_str!("../../../probes/results/aditi/prim/changed.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/changed.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/changed.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/prim/effects.txt"),
            include_str!("../../../probes/results/opensim-yengine/prim/effects.txt"),
            include_str!("../../../probes/results/opensim-xengine/prim/effects.txt"),
        ],
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// The text of every `DEBUG_CHANNEL` line the runner noted as sent to the
/// owner, in order.
pub(super) fn debug_notes(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter_map(|line| line.strip_prefix("# debug-channel: "))
        .filter_map(|note| {
            let start = note.find(" text=\"")?.saturating_add(7);
            let text = note.get(start..note.len().saturating_sub(1))?;
            // OpenSim's exception lines carry Mono's stack trace after the
            // message, which is OpenSim's own code and no script's.
            Some(text.split("\\n").next().unwrap_or(text).to_owned())
        })
        .collect()
}

/// `lines` with the key in a media complaint (`PRIM_MEDIA error: face 9 on
/// object <key> not found`) written as `<key>`: the grid's prim had its own.
pub(super) fn unkeyed(lines: Vec<String>) -> Vec<String> {
    const MARK: &str = " on object ";
    lines
        .into_iter()
        .map(|line| match line.find(MARK) {
            Some(at) => {
                let start = at.saturating_add(MARK.len());
                let end = start.saturating_add(36);
                match (line.get(..start), line.get(end..)) {
                    (Some(before), Some(after)) => format!("{before}<key>{after}"),
                    _ => line,
                }
            }
            None => line,
        })
        .collect()
}

/// `lines` without the fields the flood probes and `prim/params` name as
/// measurements in their headers (`vary: t, cost`): a clock reading and what
/// a call cost.
pub(super) fn unvaried(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            line.split(' ')
                .filter(|field| !field.starts_with("t=") && !field.starts_with("cost="))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// The name of a `PROBE` line: its `rec` and its `label` or `code`.
fn name_of(line: &str) -> String {
    crate::probe::Line::parse(line).map_or_else(
        |_| String::new(),
        |parsed| {
            let which = parsed
                .field("label")
                .or_else(|| parsed.field("code"))
                .or_else(|| parsed.field("asked"))
                .unwrap_or_default();
            match parsed.field("when") {
                Some(when) => format!("{} {which} {when}", parsed.rec),
                None => format!("{} {which}", parsed.rec),
            }
        },
    )
}

/// Hold `ours` to `theirs` but for the lines named in `deviations`, each of
/// which must differ — a deviation that agrees now should go.
pub(super) fn hold(ours: &[String], theirs: &[String], deviations: &[&str], context: &str) {
    let kept = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter(|line| !deviations.contains(&name_of(line).as_str()))
            .cloned()
            .collect()
    };
    assert_eq!(kept(ours), kept(theirs), "{context}");
    for deviation in deviations {
        let find = |lines: &[String]| {
            lines
                .iter()
                .find(|line| name_of(line) == *deviation)
                .cloned()
        };
        let (mine, grid) = (find(ours), find(theirs));
        assert!(
            mine.is_some() && grid.is_some(),
            "{context}: deviation {deviation} names no line"
        );
        assert!(mine != grid, "{context}: deviation {deviation} agrees now");
    }
}

/// A region of `flavour` with the probe's scripts in it: `scripts` as
/// `(id, name, prim, source)`, `ground` the height of the ground and `home`
/// where each prim stands.
pub(super) fn region(
    flavour: EngineFlavour,
    scripts: &[(CallerId, &str, u64, String)],
    ground: f32,
    home: &Vector,
) -> Result<Region, String> {
    region_after(flavour, scripts, (ground, home), Duration::ZERO)
}

/// [`region`], its prims rezzed at once and their scripts saved `delay`
/// later, as the runner saves them after it rezzes the prims.
fn region_after(
    flavour: EngineFlavour,
    scripts: &[(CallerId, &str, u64, String)],
    (ground, home): (f32, &Vector),
    delay: Duration,
) -> Result<Region, String> {
    let mut region = Region::new(flavour);
    // OpenSim merges nothing and queues first in, first out, as the fake
    // grid configures its OpenSim regions.
    if flavour.is_opensim() {
        let mut config = *region.engine.config();
        config.coalescing = crate::vm::Coalescing::Never;
        region.engine = Engine::new(config);
    }
    region.host.ground = ground;
    region.host.moves_heard = flavour.is_opensim();
    for (id, _name, prim, _source) in scripts {
        region.place(*prim, home.x, home.y, home.z);
        let _placed = region.host.placements.insert(
            *id,
            crate::vm::seating::Placement {
                position: home.clone(),
                local: home.clone(),
                root: home.clone(),
            },
        );
        // A prim with no text reads its alpha as OpenSim stores it: 0.
        let key = region.host.prim_key(*prim);
        let mut fresh = crate::vm::prim::Prim::new_box(&key, super::OWNER);
        if flavour.is_opensim() {
            fresh.text.alpha = 0.0;
        }
        let _fresh = region.host.prim_states.insert(key, fresh);
    }
    let step = region.engine.config().step;
    let idle = delay.as_micros().checked_div(step.as_micros()).unwrap_or(0);
    for _ in 0..idle {
        region.tick()?;
    }
    for (id, name, prim, source) in scripts {
        region.add(*id, name, *prim, source)?;
    }
    Ok(region)
}

/// Runs `region` until a script printed `end` or `ticks` ran out, raising
/// each `changed` a write raised in the scripts of the prim written.
pub(super) fn run_out(region: &mut Region, ticks: u32) -> Result<(), String> {
    for _ in 0..ticks {
        region.tick()?;
        let raised = core::mem::take(&mut region.host.changes);
        for (key, bits) in raised {
            let scripts: Vec<CallerId> = region
                .host
                .prims
                .iter()
                .filter(|(_, prim)| region.host.prim_key(**prim) == key)
                .map(|(id, _)| *id)
                .collect();
            for id in scripts {
                let _posted = region
                    .engine
                    .changed(id, bits)
                    .map_err(|error| error.to_string())?;
            }
        }
        if region.ended() {
            break;
        }
    }
    Ok(())
}

/// The probe's lines and the transcript's, without the start record: the
/// lines of the scripts in other prims first, as the runner's transcript has
/// them (they are saved, and print, before the leader starts), then the
/// leader's.
pub(super) fn compared(region: &Region, transcript: &str) -> (Vec<String>, Vec<String>) {
    let probe = |line: &&(u64, CallerId, String)| line.2.starts_with("PROBE ");
    let mut callers: Vec<CallerId> = region
        .host
        .printed
        .iter()
        .map(|(_, caller, _)| *caller)
        .filter(|caller| *caller != A)
        .collect();
    callers.sort();
    callers.dedup();
    let others: Vec<String> = callers
        .iter()
        .flat_map(|other| {
            region
                .host
                .printed
                .iter()
                .filter(probe)
                .filter(move |(_, caller, _)| caller == other)
                .map(|(_, _, line)| line.clone())
        })
        .collect();
    let leader: Vec<String> = region
        .host
        .printed
        .iter()
        .filter(probe)
        .filter(|(_, caller, _)| *caller == A)
        .map(|(_, _, line)| line.clone())
        .collect();
    let ours: Vec<String> = others.into_iter().chain(leader).collect();
    (
        canonical(&ours, &[]),
        canonical(&probe_lines(transcript), &[]),
    )
}

/// Where the probe's prim stood on each grid, and the ground under it
/// (`prim/placement`'s `home` record and the moves that hit the ground;
/// aditi's ground to the millimetre from `prim/placement-settle`, since
/// `prim/placement` prints it to the decimetre).
pub(super) fn home_of(flavour: EngineFlavour) -> (Vector, f32) {
    if flavour.is_opensim() {
        (
            Vector {
                x: 122.0,
                y: 128.0,
                z: 27.5,
            },
            21.0,
        )
    } else {
        (
            Vector {
                x: 52.0,
                y: 62.0,
                z: 34.62,
            },
            32.455,
        )
    }
}

#[test]
fn params_round_trip_as_each_grid_stored_them() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/params/probe.lsl");
    let shadows = include_str!("../../../probes/prim/params/shadows.lsl");
    for (flavour, transcript) in transcripts("params") {
        let (home, ground) = home_of(flavour);
        // The Shadows script is `skip-on: opensim-xengine` (its set never
        // returns there), so XEngine's transcript holds nothing of it.
        let mut scripts = vec![(A, "Probe", 1, source.to_owned())];
        if flavour != EngineFlavour::XEngine {
            scripts.insert(0, (B, "Shadows", 2, shadows.to_owned()));
        }
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 3000)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(
            &unvaried(ours),
            &unvaried(theirs),
            &[],
            &format!("{flavour:?}"),
        );
        // The Shadows' refusal of rule 24 on Second Life is said before the
        // runner starts recording the debug channel, so the transcript lacks
        // it, though it counts towards the flood gate like any other line.
        let mut notes = debug_notes(transcript);
        if !flavour.is_opensim() {
            notes.insert(
                0,
                "llSetPrimitiveParams error running rule #1: unknown rule.".to_owned(),
            );
            // A deviation: aditi passed the complaint of `text-300`, 30.16
            // seconds in, in two later runs on 2026-11-05, where an earlier
            // run that day dropped it (and the projector's after it) at the
            // same clock — one of the limit's turns no count holds. The gate
            // drops it.
            let text_300 = "llSetPrimitiveParams error running rule #1 (PRIM_TEXT): bounds error; 2 is not in (0, 1).";
            let before = notes.len();
            notes.retain(|note| note != text_300);
            assert_eq!(
                before - notes.len(),
                1,
                "the text-300 deviation names no line"
            );
        }
        assert_eq!(region.debug_lines(), notes, "{flavour:?}");
    }
    Ok(())
}

#[test]
fn params_rules_read_as_each_grid_read_them() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/params-rules/probe.lsl");
    let face_100 = include_str!("../../../probes/prim/params-rules/face-100.lsl");
    let face_minus_2 = include_str!("../../../probes/prim/params-rules/face-minus-2.lsl");
    let code_as_string = include_str!("../../../probes/prim/params-rules/code-as-string.lsl");
    for (flavour, transcript) in transcripts("params-rules") {
        let (home, ground) = home_of(flavour);
        let scripts = [
            (C, "CodeAsString", 3, code_as_string.to_owned()),
            (D, "Face100", 4, face_100.to_owned()),
            (E, "FaceMinus2", 5, face_minus_2.to_owned()),
            (A, "Probe", 1, source.to_owned()),
        ];
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 400)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(&ours, &theirs, &[], &format!("{flavour:?}"));
        assert_eq!(region.debug_lines(), debug_notes(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn faces_read_and_write_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/faces.lsl");
    for (flavour, transcript) in transcripts("faces") {
        let (home, ground) = home_of(flavour);
        let mut region = region(
            flavour,
            &[(A, "Probe", 1, source.to_owned())],
            ground,
            &home,
        )?;
        run_out(&mut region, 400)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(&ours, &theirs, &[], &format!("{flavour:?}"));
        assert_eq!(region.debug_lines(), debug_notes(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn identity_reads_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/identity.lsl");
    for (flavour, transcript) in transcripts("identity") {
        let (home, ground) = home_of(flavour);
        let mut region = region(
            flavour,
            &[(A, "Probe", 1, source.to_owned())],
            ground,
            &home,
        )?;
        // The owner as the region knows it: the prims it wears, on Second
        // Life (`prim/identity`: 22 there, 0 on OpenSim).
        let worn = if flavour.is_opensim() { 0 } else { 22 };
        let owner = region
            .host
            .object_details(A, &region.host.prim_key(1))
            .map(|prim| crate::vm::prim::ObjectDetails {
                avatar: true,
                root: super::OWNER.to_owned(),
                creator: crate::value::NULL_KEY.to_owned(),
                prim_count: worn,
                ..prim
            });
        if let Some(owner) = owner {
            let _owner = region.host.details.insert(super::OWNER.to_owned(), owner);
        }
        if flavour.is_opensim() {
            region.host.detail_costs = Some((1.0, 1.0, 0.1, 0.0));
            region.host.last_owner_is_owner = true;
        }
        run_out(&mut region, 400)?;
        let (ours, theirs) = compared(&region, transcript);
        // OpenSim answers `OBJECT_VELOCITY` of a prim at rest with a value
        // `llGetListEntryType` calls `TYPE_INVALID` (its own `Vector3`, not
        // an LSL vector); the runtime answers a vector, as Second Life does.
        let deviations: &[&str] = if flavour.is_opensim() {
            &["detail 5"]
        } else {
            &[]
        };
        hold(&ours, &theirs, deviations, &format!("{flavour:?}"));
        assert_eq!(region.debug_lines(), debug_notes(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn placement_moves_as_each_grid_moved() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/placement.lsl");
    for (flavour, transcript) in transcripts("placement") {
        let (home, ground) = home_of(flavour);
        let rigged = source.replace("llGround(ZERO_VECTOR)", &format!("{ground:.3}"));
        let mut region = region(flavour, &[(A, "Probe", 1, rigged)], ground, &home)?;
        run_out(&mut region, 600)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(&ours, &theirs, &[], &format!("{flavour:?}"));
        assert_eq!(region.debug_lines(), debug_notes(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn boxes_and_ground_stops_settle_as_each_grid_settled() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/placement-settle.lsl");
    let cadence = [
        "polls=", "set=", "seen=", "after=", "rez=", "stamp=", "wall=", "t=",
    ];
    let unvaried = |lines: Vec<String>| -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                line.split(' ')
                    .filter(|field| !cadence.iter().any(|name| field.starts_with(name)))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    };
    for (flavour, transcript) in transcripts("placement-settle") {
        let (home, _ground) = home_of(flavour);
        let (below, delay) = settle_home(transcript)?;
        // Second Life's box keeps to a beat about a second apart, counted
        // from the prim's rez, and the runner saved the script a second and
        // a half after it rezzed the prim.
        let mut region = region_after(
            flavour,
            &[(A, "Probe", 1, source.to_owned())],
            (home.z + below, &home),
            delay,
        )?;
        run_out(&mut region, 4000)?;
        let (ours, theirs) = compared(&region, transcript);
        // Counted so, the beat falls where aditi's did for every box read in
        // the first 20 s (the `settled` reads half a second apart alternate
        // between a fresh box and a stale one, and agree). aditi's beat came
        // every 1.01 s by its own clocks (the `cadence` records' `seen`) and
        // the runtime's every second, so by the `lag` reads the runtime's
        // beat comes a little early: before `sleep-0.05` reads, where aditi's
        // came after `params-fast`. `sleep-0.05` and `params-fast` read the
        // 3 m box where aditi still read the 1 m cube, and `params-delayed`
        // a box one rescale newer than aditi's.
        let deviations: &[&str] = if flavour.is_opensim() {
            &[]
        } else {
            &["lag sleep-0.05", "lag params-fast", "lag params-delayed"]
        };
        hold(
            &unvaried(ours),
            &unvaried(theirs),
            deviations,
            &format!("{flavour:?}"),
        );
    }
    Ok(())
}

/// `prim/placement-settle`'s `home` record: the ground below the prim (it
/// differs from run to run on aditi), and how long after the prim was rezzed
/// its script started (`stamp` less `t` less `rez`).
fn settle_home(transcript: &str) -> Result<(f32, Duration), String> {
    let line = probe_lines(transcript)
        .into_iter()
        .find(|line| line.contains(" rec=home "))
        .ok_or("no home record")?;
    let parsed = crate::probe::Line::parse(&line).map_err(|error| error.to_string())?;
    let field = |name: &str| {
        parsed
            .field(name)
            .ok_or_else(|| format!("no {name} in {line}"))
    };
    let number = |name: &str| -> Result<f64, String> {
        field(name)?
            .parse()
            .map_err(|error| format!("{name} in {line}: {error}"))
    };
    // The second of the day of a timestamp, `YYYY-MM-DDThh:mm:ss[.f]Z`.
    let second_of_day = |name: &str| -> Result<f64, String> {
        let stamp = field(name)?;
        let clock = stamp
            .split_once('T')
            .map(|(_, clock)| clock.trim_end_matches('Z'))
            .ok_or_else(|| format!("{name} in {line}"))?;
        clock.split(':').try_fold(0.0, |sum, part| {
            part.parse::<f64>()
                .map(|value| sum * 60.0 + value)
                .map_err(|error| format!("{name} in {line}: {error}"))
        })
    };
    let below: f32 = field("ground")?
        .parse()
        .map_err(|error| format!("ground in {line}: {error}"))?;
    let delay = second_of_day("stamp")? - number("t")? - second_of_day("rez")?;
    Ok((
        below,
        Duration::try_from_secs_f64(delay).unwrap_or(Duration::ZERO),
    ))
}

#[test]
fn setters_raise_changed_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/changed.lsl")
        .replace(
            "llSetStatus(STATUS_PHANTOM, TRUE)",
            "llSetPrimitiveParams([PRIM_PHANTOM, TRUE])",
        )
        .replace(
            "llSetStatus(STATUS_PHANTOM, FALSE)",
            "llSetPrimitiveParams([PRIM_PHANTOM, FALSE])",
        );
    for (flavour, transcript) in transcripts("changed") {
        let (home, ground) = home_of(flavour);
        let mut region = region(flavour, &[(A, "Probe", 1, source.clone())], ground, &home)?;
        run_out(&mut region, 1200)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(&ours, &theirs, &[], &format!("{flavour:?}"));
        assert_eq!(region.debug_lines(), debug_notes(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn effects_complain_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/prim/effects/probe.lsl");
    let missing = include_str!("../../../probes/prim/effects/missing.lsl");
    for (flavour, transcript) in transcripts("effects") {
        let (home, ground) = home_of(flavour);
        let scripts = [
            (B, "Missing", 2, missing.to_owned()),
            (A, "Probe", 1, source.to_owned()),
        ];
        let mut region = region(flavour, &scripts, ground, &home)?;
        run_out(&mut region, 600)?;
        let (ours, theirs) = compared(&region, transcript);
        hold(&ours, &theirs, &[], &format!("{flavour:?}"));
        assert_eq!(
            unkeyed(region.debug_lines()),
            unkeyed(debug_notes(transcript)),
            "{flavour:?}"
        );
    }
    Ok(())
}
