//! Script memory against the acceptance of `server-lsl-memory-and-limits`,
//! held to the transcripts of the `memory/*` probes: the probes' own scripts
//! run in the VM, each `llOwnerSay` and `llSetObjectDesc` turned into a
//! `print` (one instruction for one), and what they print is set beside what
//! each grid printed.
//!
//! Mono's readings carry the grid's garbage, which wanders by a couple of
//! hundred bytes ([`WANDER`]); YEngine's are its heap tracker's and match
//! exactly.

use pretty_assertions::assert_eq;

use super::*;
use crate::probe::Line;
use crate::value::NULL_KEY;
use crate::value::memory::{MEMORY_LIMIT, YENGINE_HEAP};
use crate::vm::memory::HOST_MEMORY_CAP;

/// How far a Mono reading here may sit from aditi's: the code model's fit
/// (`vm::memory`) and the grid's own garbage.
const WANDER: i64 = 300;

/// A probe script made runnable here: its `llOwnerSay` lines, and the
/// `llSetObjectDesc` this runtime does not have yet, turned into `print`s for
/// the test to read, and its closing link message to its controller, which
/// has no script here, dropped.
fn runnable(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.contains("llMessageLinked"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("llOwnerSay(", "print(")
        .replace("llSetObjectDesc(", "print(")
        .replace("llStringLength(text)", "0")
}

/// What a probe run printed: its `PROBE` lines, the marks it left where the
/// grid's version wrote its description, and how it ended.
#[derive(Debug)]
struct Run {
    /// The `PROBE` lines.
    lines: Vec<Line>,
    /// The numbers it wrote to its description, in order.
    marks: Vec<i64>,
    /// The fault that stopped it, if one did.
    fault: Option<RuntimeError>,
    /// The host, with what the script said and told its owner.
    host: Recorder,
    /// The engine, with the instance as it was left.
    engine: Engine,
}

impl Run {
    /// The `PROBE` lines of `rec`.
    fn records(&self, rec: &str) -> Vec<&Line> {
        self.lines.iter().filter(|line| line.rec == rec).collect()
    }

    /// The field `key` of the first `rec` line, as a number.
    fn first(&self, rec: &str, key: &str) -> Result<i64, String> {
        let line = self
            .records(rec)
            .into_iter()
            .next()
            .ok_or_else(|| format!("no {rec} line"))?;
        number(line, key)
    }

    /// The last mark.
    fn last_mark(&self) -> Result<i64, String> {
        self.marks
            .last()
            .copied()
            .ok_or_else(|| "no mark".to_owned())
    }
}

/// The field `key` of `line`, as a number.
fn number(line: &Line, key: &str) -> Result<i64, String> {
    line.field(key)
        .ok_or_else(|| format!("no {key} in {line:?}"))?
        .trim_matches('"')
        .parse()
        .map_err(|error| format!("{key} of {line:?}: {error}"))
}

/// The probe at `source`, named `name` in its prim, sent link message
/// `num` under `flavour` and run until it is done or stopped.
fn run_probe(source: &str, name: &str, num: i32, flavour: EngineFlavour) -> Result<Run, String> {
    let mut engine = Engine::new(EngineConfig {
        flavour,
        ..EngineConfig::for_step(STEP)
    });
    let mut host = Recorder::default();
    let _none = host.scripts.insert(name.to_owned(), A);
    let _none = engine.add(A, program(&runnable(source))?);
    let _first = run(&mut engine, &mut host, 1);
    let _posted = engine
        .post(
            A,
            named("link_message")?,
            vec![
                Value::Integer(1),
                Value::Integer(num),
                Value::String(String::new()),
                Value::Key(NULL_KEY.to_owned()),
            ],
        )
        .map_err(|error| error.to_string())?;
    let mut fault = None;
    for _ in 0..10_000 {
        let reports = run(&mut engine, &mut host, 1);
        fault = fault.or_else(|| {
            reports
                .iter()
                .flat_map(|report| outcomes(report, A))
                .find_map(|outcome| match outcome {
                    Outcome::Faulted(fault) => Some(fault.error),
                    _ => None,
                })
        });
        let idle = engine
            .instance(A)
            .is_none_or(|instance| !instance.is_runnable(engine.now().after(1)));
        if idle {
            break;
        }
    }
    let printed = host.texts(A);
    let lines = printed
        .iter()
        .filter(|text| text.starts_with("PROBE "))
        .map(|text| Line::parse(text).map_err(|error| format!("{text}: {error:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    let marks = printed
        .iter()
        .filter_map(|text| text.parse::<i64>().ok())
        .collect();
    Ok(Run {
        lines,
        marks,
        fault,
        host,
        engine,
    })
}

/// The reading `key` a transcript's first `rec` line of script `script`
/// holds.
fn transcript_reading(transcript: &str, rec: &str, script: &str, key: &str) -> Result<i64, String> {
    let line = transcript
        .lines()
        .filter_map(|line| Line::parse(line).ok())
        .find(|line| line.rec == rec && line.field("script") == Some(script))
        .ok_or_else(|| format!("no {rec} of {script}"))?;
    number(&line, key)
}

/// `(script field, used, free)` of every reading line of `script` in a
/// transcript, in order.
fn readings(transcript: &str, script: &str) -> Vec<(String, i64, i64)> {
    transcript
        .lines()
        .filter_map(|line| Line::parse(line).ok())
        .filter(|line| line.field("script") == Some(script))
        .filter_map(|line| {
            Some((
                line.rec.clone(),
                number(&line, "used").ok()?,
                number(&line, "free").ok()?,
            ))
        })
        .collect()
}

/// The same readings of a run here.
fn run_readings(run: &Run) -> Vec<(String, i64, i64)> {
    run.lines
        .iter()
        .filter_map(|line| {
            Some((
                line.rec.clone(),
                number(line, "used").ok()?,
                number(line, "free").ok()?,
            ))
        })
        .collect()
}

const RECURSION: &str = include_str!("../../../probes/results/aditi/memory/recursion.txt");
const COLLISION: &str = include_str!("../../../probes/results/aditi/memory/collision.txt");
const COLLISION_YENGINE: &str =
    include_str!("../../../probes/results/opensim-yengine/memory/collision.txt");
const NARROW: &str = include_str!("../../../probes/memory/recursion/narrow.lsl");
const WIDE: &str = include_str!("../../../probes/memory/recursion/wide.lsl");
const LIMITED: &str = include_str!("../../../probes/memory/recursion/limited.lsl");
const DOUBLING: &str = include_str!("../../../probes/memory/collision/doubling.lsl");
const FULL_LIST: &str = include_str!("../../../probes/memory/collision/full-list.lsl");
const LIMITED_LIST: &str = include_str!("../../../probes/memory/collision/limited-list.lsl");

/// Whether `ours` is within [`WANDER`] of `theirs`.
fn near(ours: i64, theirs: i64) -> bool {
    ours.abs_diff(theirs) <= WANDER.unsigned_abs()
}

#[test]
fn the_code_model_reads_within_the_grids_wander_of_the_probes() -> Result<(), String> {
    let globals_none = include_str!("../../../probes/memory/sizes/globals-none.lsl");
    let cases = [
        (NARROW, "Narrow", 1, "narrow", RECURSION),
        (WIDE, "Wide", 2, "wide", RECURSION),
        (LIMITED, "Limited", 3, "limited", RECURSION),
        (DOUBLING, "Doubling", 1, "doubling", COLLISION),
        (FULL_LIST, "Full list", 3, "full-list", COLLISION),
        (LIMITED_LIST, "Limited list", 4, "limited-list", COLLISION),
    ];
    for (source, name, num, script, transcript) in cases {
        let ours = run_probe(source, name, num, EngineFlavour::SecondLife)?;
        let used = ours.first("before", "used")?;
        let theirs = transcript_reading(transcript, "before", script, "used")?;
        assert!(
            near(used, theirs),
            "{script}: {used} here, {theirs} on aditi"
        );
        // Free is the limit less the most used: nothing has been used more.
        assert_eq!(
            ours.first("before", "free")?,
            i64::from(MEMORY_LIMIT) - used,
            "{script}"
        );
    }
    let none = run_probe(globals_none, "Globals none", 1, EngineFlavour::SecondLife)?;
    let printed = none.host.texts(A);
    let used: Vec<i64> = printed
        .first()
        .and_then(|text| text.rsplit("used=").next())
        .map(|reads| {
            reads
                .split(',')
                .filter_map(|read| read.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(used.len(), 2, "{printed:?}");
    assert!(used.iter().all(|read| near(*read, 4_456)), "{used:?}");
    Ok(())
}

#[test]
fn yengine_reads_what_its_heap_tracker_holds() -> Result<(), String> {
    let globals_none = include_str!("../../../probes/memory/sizes/globals-none.lsl");
    for (flavour, expected) in [
        (EngineFlavour::YEngine, "used=32,32"),
        (EngineFlavour::XEngine, "used=65536,65536"),
    ] {
        let run = run_probe(globals_none, "Globals none", 1, flavour)?;
        let printed = run.host.texts(A);
        assert!(
            printed.first().is_some_and(|text| text.ends_with(expected)),
            "{flavour:?}: {printed:?}"
        );
    }
    Ok(())
}

#[test]
fn a_recursion_collides_at_the_depth_aditi_reached() -> Result<(), String> {
    for (source, name, num, depths) in [
        (NARROW, "Narrow", 1, 1_039..=1_116),
        (WIDE, "Wide", 2, 413..=518),
    ] {
        let run = run_probe(source, name, num, EngineFlavour::SecondLife)?;
        let depth = run.last_mark()?;
        assert!(depths.contains(&depth), "{name}: depth {depth}");
        assert_eq!(run.fault, Some(RuntimeError::StackHeapCollision), "{name}");
        assert_eq!(
            run.host.heard(A),
            vec![
                format!("Faulty prim [script:{name}] Script run-time error").as_str(),
                "Stack-Heap Collision"
            ]
        );
        let instance = run.engine.instance(A).ok_or("no instance")?;
        assert!(!instance.reads_running(EngineFlavour::SecondLife), "{name}");
        // The stack never shows in a reading: every one is what it was
        // before the recursion, so free stays the limit less that.
        let before = run.first("before", "used")?;
        assert!(
            run.records("depth")
                .iter()
                .all(|line| number(line, "used") == Ok(before)),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_lowered_limit_cuts_the_recursion() -> Result<(), String> {
    let run = run_probe(LIMITED, "Limited", 3, EngineFlavour::SecondLife)?;
    let used = run.first("before", "used")?;
    let set = run.records("set-limit");
    let set = set.first().ok_or("no set-limit")?;
    assert_eq!(number(set, "result"), Ok(1));
    assert_eq!(number(set, "limit"), Ok(used + 4_096));
    assert_eq!(number(set, "free"), Ok(4_096));
    assert_eq!(run.fault, Some(RuntimeError::StackHeapCollision));
    // Aditi reached 129 to 253; the model, which has no garbage to collect,
    // stops shallower (`vm::memory`). Either way far short of the 1 000 or
    // so the default limit allows.
    let depth = run.last_mark()?;
    assert!((50..=253).contains(&depth), "depth {depth}");
    Ok(())
}

#[test]
fn a_doubling_string_runs_out_where_each_grid_ran_out() -> Result<(), String> {
    // Mono: making 32 768 characters, at step 15.
    let mono = run_probe(DOUBLING, "Doubling", 1, EngineFlavour::SecondLife)?;
    assert_eq!(mono.fault, Some(RuntimeError::StackHeapCollision));
    assert_eq!(mono.last_mark()?, 15);
    let theirs = transcript_reading(COLLISION, "before", "doubling", "used")?;
    let grown = mono.records("grown");
    let fourteen = grown.last().ok_or("no step")?;
    assert_eq!(number(fourteen, "step"), Ok(14));
    let used = number(fourteen, "used")?;
    assert!(
        near(used - mono.first("before", "used")?, 39_542 - theirs),
        "{used}"
    );

    // YEngine: an OutOfHeapException making 524 288, at step 19, told to
    // the owner alone, and every reading on the way its heap tracker's.
    let yengine = run_probe(DOUBLING, "Doubling", 1, EngineFlavour::YEngine)?;
    assert_eq!(
        yengine.fault,
        Some(RuntimeError::OutOfHeap {
            old_total: 0,
            new_total: YENGINE_HEAP / 2,
            limit: YENGINE_HEAP,
        })
    );
    assert_eq!(yengine.last_mark()?, 19);
    assert_eq!(
        run_readings(&yengine),
        readings(COLLISION_YENGINE, "doubling")
    );
    let told: Vec<&str> = yengine
        .host
        .told
        .iter()
        .map(|(_, _, text)| text.as_str())
        .collect();
    assert_eq!(
        told,
        vec![
            "[YEngine]: Exception while running d9cbb4b7-206e-45c2-8c9a-7429643962ac",
            "OutOfHeapException: oldtotal=0, newtotal=524288, limit=1048576",
            "Prim: <Faulty prim>, Script: <Doubling>, Location: Default Region <120,128,27>",
            "Script must be Reset to re-enable.",
        ]
    );
    assert!(yengine.host.heard(A).is_empty(), "nothing on DEBUG_CHANNEL");
    let instance = yengine.engine.instance(A).ok_or("no instance")?;
    assert!(instance.reads_running(EngineFlavour::YEngine));

    // XEngine: never, and every reading is 65536.
    let xengine = run_probe(DOUBLING, "Doubling", 1, EngineFlavour::XEngine)?;
    assert_eq!(xengine.fault, None);
    assert_eq!(xengine.records("no-collision").len(), 1);
    assert!(run_readings(&xengine).iter().all(
        |(_, used, free)| *used == i64::from(MEMORY_LIMIT) && *free == i64::from(MEMORY_LIMIT)
    ));
    Ok(())
}

#[test]
fn a_growing_list_collides_where_aditi_collided() -> Result<(), String> {
    // Mono: making 3 660 elements, ten at a time; the code model's wander
    // moves it by a step either way.
    let mono = run_probe(FULL_LIST, "Full list", 3, EngineFlavour::SecondLife)?;
    assert_eq!(mono.fault, Some(RuntimeError::StackHeapCollision));
    let last = mono.last_mark()?;
    assert!((3_650..=3_670).contains(&last), "made {last}");
    // Each element 16 bytes: 8 000 a 500, as on aditi.
    let counts: Vec<i64> = mono
        .records("grown")
        .iter()
        .filter_map(|line| number(line, "used").ok())
        .collect();
    assert!(
        counts
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if b - a == 8_000)),
        "{counts:?}"
    );

    // YEngine: 4 bytes an integer, never within 20 000, exactly.
    let yengine = run_probe(FULL_LIST, "Full list", 3, EngineFlavour::YEngine)?;
    assert_eq!(yengine.fault, None);
    assert_eq!(
        run_readings(&yengine),
        readings(COLLISION_YENGINE, "full-list")
    );
    Ok(())
}

#[test]
fn a_list_under_a_lowered_limit_collides_early() -> Result<(), String> {
    let mono = run_probe(LIMITED_LIST, "Limited list", 4, EngineFlavour::SecondLife)?;
    let used = mono.first("before", "used")?;
    let set = mono.records("set-limit");
    let set = set.first().ok_or("no set-limit")?;
    assert_eq!(number(set, "result"), Ok(1));
    assert_eq!(number(set, "limit"), Ok(used + 8_192));
    assert_eq!(mono.fault, Some(RuntimeError::StackHeapCollision));
    // Aditi made 540, the garbage it read into `used` buying it a little
    // more room than the model has.
    let last = mono.last_mark()?;
    assert!((500..=540).contains(&last), "made {last}");

    // YEngine refuses the limit and runs on.
    let yengine = run_probe(LIMITED_LIST, "Limited list", 4, EngineFlavour::YEngine)?;
    assert_eq!(yengine.fault, None);
    assert_eq!(
        run_readings(&yengine),
        readings(COLLISION_YENGINE, "limited-list")
    );
    Ok(())
}

/// Run `body` as `state_entry` of a script with `globals` under `flavour`,
/// returning what it printed.
fn printed(globals: &str, body: &str, flavour: EngineFlavour) -> Result<Vec<String>, String> {
    let mut engine = Engine::new(EngineConfig {
        flavour,
        ..EngineConfig::for_step(STEP)
    });
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(&format!(
            "{globals}\ndefault {{ state_entry() {{ {body} }} }}"
        ))?,
    );
    let _reports = run(&mut engine, &mut host, 20);
    Ok(host.texts(A).into_iter().map(str::to_owned).collect())
}

#[test]
fn free_memory_is_a_low_water_mark_on_mono_and_the_heap_left_on_yengine() -> Result<(), String> {
    let globals = "list held;";
    let body = r#"
        integer before = llGetUsedMemory();
        integer n;
        for (n = 0; n < 100; ++n) held += [n];
        integer grown = llGetUsedMemory();
        integer low = llGetFreeMemory();
        held = [];
        print((string)(grown - before) + " " + (string)(llGetUsedMemory() - before) + " "
            + (string)(llGetFreeMemory() - low));
        // One call a statement: LSL evaluates an expression right to left.
        integer set = llSetMemoryLimit(65536);
        integer after = llGetFreeMemory();
        print((string)set + " " + (string)(after - low));
    "#;
    // Mono: the list's 64 and 16 an integer; clearing it lowers the use and
    // leaves free where it was, until setting the limit restarts the mark.
    assert_eq!(
        printed(globals, body, EngineFlavour::SecondLife)?,
        vec!["1664 0 0", "1 1664"]
    );
    // YEngine: 4 an integer, and free rises again at once.
    assert_eq!(
        printed(globals, body, EngineFlavour::YEngine)?,
        vec!["400 0 400", "0 400"]
    );
    assert_eq!(
        printed(globals, body, EngineFlavour::XEngine)?,
        vec!["0 0 0", "0 0"]
    );
    Ok(())
}

#[test]
fn set_memory_limit_takes_what_the_reference_takes() -> Result<(), String> {
    // One call a statement: LSL evaluates an expression right to left.
    let body = r#"
        integer used = llGetUsedMemory();
        string out = (string)llSetMemoryLimit(65537);
        out += " " + (string)llSetMemoryLimit(100000);
        out += " " + (string)llGetMemoryLimit();
        out += " " + (string)llSetMemoryLimit(used - 1);
        out += " " + (string)llSetMemoryLimit(1);
        out += " " + (string)llSetMemoryLimit(0);
        out += " " + (string)llSetMemoryLimit(-1);
        out += " " + (string)(llGetMemoryLimit() == 65536);
        out += " " + (string)llSetMemoryLimit(used + 1024);
        out += " " + (string)(llGetMemoryLimit() == used + 1024);
        out += " " + (string)(llGetFreeMemory() == 1024);
        out += " " + (string)llSetMemoryLimit(16384);
        out += " " + (string)llGetMemoryLimit();
        out += " " + (string)llSetMemoryLimit(65536);
        out += " " + (string)llGetMemoryLimit();
        print(out);
    "#;
    assert_eq!(
        printed("", body, EngineFlavour::SecondLife)?,
        vec!["0 0 65536 0 0 0 0 1 1 1 1 1 16384 1 65536"]
    );
    for flavour in [EngineFlavour::YEngine, EngineFlavour::XEngine] {
        assert_eq!(
            printed("", body, flavour)?,
            vec!["0 0 65536 0 0 0 0 1 0 0 0 0 65536 0 65536"],
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn the_profiler_records_the_most_used_while_it_runs() -> Result<(), String> {
    let globals = "list held;";
    let body = r#"
        integer before = llGetSPMaxMemory();
        llScriptProfiler(PROFILE_SCRIPT_MEMORY);
        integer n;
        for (n = 0; n < 50; ++n) held += [n];
        integer most = llGetUsedMemory();
        held = [];
        llScriptProfiler(PROFILE_NONE);
        for (n = 0; n < 100; ++n) held += [n];
        print((string)before + " " + (string)(llGetSPMaxMemory() == most));
    "#;
    assert_eq!(
        printed(globals, body, EngineFlavour::SecondLife)?,
        vec!["0 1"]
    );
    assert_eq!(
        printed(globals, body, EngineFlavour::YEngine)?,
        vec!["65536 0"]
    );
    Ok(())
}

#[test]
fn a_runaway_allocation_stops_the_script_rather_than_the_host() -> Result<(), String> {
    let source = r#"string s = "a"; default { state_entry() { while (TRUE) s += s; } }"#;
    for (flavour, error) in [
        (EngineFlavour::SecondLife, RuntimeError::StackHeapCollision),
        (
            EngineFlavour::YEngine,
            // With no other global, the heap fills one doubling later than
            // the probe's: 524 288 characters fit exactly.
            RuntimeError::OutOfHeap {
                old_total: 0,
                new_total: YENGINE_HEAP,
                limit: YENGINE_HEAP,
            },
        ),
        (EngineFlavour::XEngine, RuntimeError::HostMemory),
    ] {
        let mut engine = Engine::new(EngineConfig {
            flavour,
            ..EngineConfig::for_step(STEP)
        });
        let mut host = Recorder::default();
        let _none = engine.add(A, program(source)?);
        let mut faults: Vec<RuntimeError> = Vec::new();
        for _ in 0..10_000 {
            faults.extend(
                run(&mut engine, &mut host, 1)
                    .iter()
                    .flat_map(|report| outcomes(report, A))
                    .filter_map(|outcome| match outcome {
                        Outcome::Faulted(fault) => Some(fault.error),
                        _ => None,
                    }),
            );
            if !faults.is_empty() {
                break;
            }
        }
        assert_eq!(faults.first(), Some(&error), "{flavour:?}");
        let held = engine
            .instance(A)
            .and_then(|instance| instance.global("s"))
            .map(|value| match value {
                Value::String(text) => text.len(),
                _ => 0,
            })
            .unwrap_or_default();
        assert!(
            u32::try_from(held).unwrap_or(u32::MAX) <= HOST_MEMORY_CAP,
            "{flavour:?} held {held}"
        );
    }
    Ok(())
}

#[test]
fn the_top_scripts_report_ranks_a_busy_script_above_an_idle_one() -> Result<(), String> {
    for flavour in [
        EngineFlavour::SecondLife,
        EngineFlavour::YEngine,
        EngineFlavour::XEngine,
    ] {
        let mut engine = Engine::new(EngineConfig {
            flavour,
            ..EngineConfig::for_step(STEP)
        });
        let mut host = Recorder::default();
        let _none = engine.add(A, program("default { state_entry() { } }")?);
        let _none = engine.add(
            B,
            program("integer n; default { state_entry() { while (TRUE) ++n; } }")?,
        );
        let limited = CallerId(3);
        let _none = engine.add(
            limited,
            program("default { state_entry() { llSetMemoryLimit(16384); } }")?,
        );
        let _reports = run(&mut engine, &mut host, 100);
        let top = engine.top_scripts();
        assert_eq!(top.len(), 3, "{flavour:?}");
        let busy = top.first().ok_or("empty")?;
        assert_eq!(busy.id, B, "{flavour:?}");
        assert!(busy.time > Duration::ZERO);
        assert!(top.iter().skip(1).all(|usage| usage.time < busy.time));
        // Script memory: the limit on Second Life; a flat 16 384 a running
        // script on OpenSim, nothing for a stopped one.
        let memory = |engine: &Engine, id| engine.usage(id).map(|usage| usage.memory);
        match flavour {
            EngineFlavour::SecondLife => {
                assert_eq!(memory(&engine, A), Some(MEMORY_LIMIT));
                assert_eq!(memory(&engine, limited), Some(0x4000), "its lowered limit");
            }
            EngineFlavour::YEngine | EngineFlavour::XEngine => {
                assert_eq!(memory(&engine, limited), Some(OPENSIM_SCRIPT_MEMORY));
                engine
                    .instance_mut(A)
                    .ok_or("no instance")?
                    .set_running(false);
                assert_eq!(memory(&engine, A), Some(0));
            }
        }
    }
    Ok(())
}

#[test]
fn the_active_script_count_is_an_average_on_second_life_and_a_count_on_opensim()
-> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = Engine::new(EngineConfig {
            flavour,
            ..EngineConfig::for_step(STEP)
        });
        let mut host = Recorder::default();
        for id in [A, B] {
            let _none = engine.add(id, program("default { state_entry() { } }")?);
        }
        engine
            .instance_mut(B)
            .ok_or("no instance")?
            .set_running(false);
        let _reports = run(&mut engine, &mut host, 600);
        let active = engine.active_scripts();
        match flavour {
            // Thirty seconds' average of the one running: past 0.8 after a
            // minute, and never a whole number.
            EngineFlavour::SecondLife => assert!(active > 0.8 && active < 1.0, "{active}"),
            _ => assert!((active - 2.0).abs() < f64::EPSILON, "{active}"),
        }
    }
    Ok(())
}
