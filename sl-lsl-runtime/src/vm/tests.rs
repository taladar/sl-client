//! The VM against the acceptance of `server-lsl-vm-execution`: a runaway
//! loop cannot stall the region, a sleep resumes on its tick, a stopped
//! script keeps its globals, and an error stops only its own script.

use std::sync::Arc;

use core::time::Duration;

use pretty_assertions::assert_eq;

use super::*;
use crate::bytecode::{Position, StateId};
use crate::compile;
use sl_lsl::ast::TypeName;

use crate::library::{BuiltinId, Event, event};
use crate::value::Value;

/// A tenth of a second: ten ticks to the second, so `llSleep(2.0)` is twenty.
const STEP: Duration = Duration::from_millis(100);

/// A host that records what scripts print, with the tick they printed at.
#[derive(Debug, Default)]
struct Recorder {
    /// The tick being run.
    now: u64,
    /// `(tick, caller, text)` per `print`.
    printed: Vec<(u64, CallerId, String)>,
    /// Stub notices.
    stubbed: Vec<(CallerId, BuiltinId)>,
    /// `(tick, caller)` per state left.
    left: Vec<(u64, CallerId)>,
    /// The scripts `llGetScriptState` can name, all in one prim.
    scripts: std::collections::BTreeMap<String, CallerId>,
}

impl Host for Recorder {
    fn print(&mut self, caller: CallerId, text: &str) {
        self.printed.push((self.now, caller, text.to_owned()));
    }

    fn stubbed(&mut self, caller: CallerId, id: BuiltinId) {
        self.stubbed.push((caller, id));
    }

    fn left_state(&mut self, caller: CallerId) {
        self.left.push((self.now, caller));
    }

    fn script_named(&self, _caller: CallerId, name: &str) -> Option<CallerId> {
        self.scripts.get(name).copied()
    }
}

impl Recorder {
    /// What `caller` printed, with the tick of each.
    fn from(&self, caller: CallerId) -> Vec<(u64, &str)> {
        self.printed
            .iter()
            .filter(|(_, who, _)| *who == caller)
            .map(|(tick, _, text)| (*tick, text.as_str()))
            .collect()
    }

    /// What `caller` printed, text only.
    fn texts(&self, caller: CallerId) -> Vec<&str> {
        self.from(caller)
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }
}

/// A compiled script, shared.
fn program(source: &str) -> Result<Arc<crate::bytecode::Program>, String> {
    compile(source).map(Arc::new).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// A region with the default budgets for [`STEP`].
fn engine() -> Engine {
    Engine::new(EngineConfig::for_step(STEP))
}

/// Run `ticks` ticks, returning each report.
fn run(engine: &mut Engine, host: &mut Recorder, ticks: u64) -> Vec<TickReport> {
    core::iter::repeat_with(|| {
        host.now = engine.now().0.saturating_add(1);
        engine.tick(host)
    })
    .take(usize::try_from(ticks).unwrap_or(usize::MAX))
    .collect()
}

/// The event called `name`.
fn named(name: &str) -> Result<&'static Event, String> {
    event(name).ok_or_else(|| format!("no event {name}"))
}

/// The outcomes `caller`'s slices in `report` ended with.
fn outcomes(report: &TickReport, caller: CallerId) -> Vec<Outcome> {
    report
        .slices
        .iter()
        .filter(|(who, _)| *who == caller)
        .map(|(_, slice)| slice.outcome.clone())
        .collect()
}

const A: CallerId = CallerId(1);
const B: CallerId = CallerId(2);

#[test]
fn a_script_initialises_its_globals_then_enters_default() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer n = 4;
            string s = "x";
            default { state_entry() { print(s + (string)(n * 2)); } }"#,
        )?,
    );
    let reports = run(&mut engine, &mut host, 2);
    assert_eq!(host.from(A), vec![(1, "x8")]);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Finished, Outcome::Finished])
    );
    // An idle script is not runnable and costs nothing.
    assert_eq!(reports.get(1).map(|report| report.runnable), Some(0));
    Ok(())
}

#[test]
fn a_runaway_loop_uses_its_budget_and_the_tick_still_completes() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("integer n; default { state_entry() { while (TRUE) { ++n; } } }")?,
    );
    let _none = engine.add(
        B,
        program(
            "default { state_entry() {
                integer i;
                for (i = 0; i < 3; ++i) { print((string)i); llSleep(0.1); }
            } }",
        )?,
    );
    let budget = engine.config().script_budget;
    let reports = run(&mut engine, &mut host, 4);
    // The neighbour runs one iteration per tick, the loop notwithstanding.
    assert_eq!(host.from(B), vec![(1, "0"), (2, "1"), (3, "2")]);
    for report in &reports {
        assert_eq!(
            outcomes(report, A).last(),
            Some(&Outcome::Yielded),
            "tick {:?}",
            report.tick
        );
        let used: u32 = report
            .slices
            .iter()
            .filter(|(who, _)| *who == A)
            .map(|(_, slice)| slice.used)
            .sum();
        // Spent to the last instruction, and past it by at most one
        // back-edge.
        assert!(
            (budget..=budget.saturating_add(BACKWARD_JUMP_COST)).contains(&used),
            "tick {:?} used {used} of {budget}",
            report.tick
        );
    }
    // And it really is running: the counter moves every tick.
    let counter = |engine: &Engine| match engine.instance(A).and_then(|a| a.global("n")) {
        Some(Value::Integer(n)) => *n,
        _ => -1,
    };
    let before = counter(&engine);
    let _reports = run(&mut engine, &mut host, 1);
    assert!(counter(&engine) > before, "the loop made no progress");
    Ok(())
}

#[test]
fn a_sleep_resumes_on_its_tick_and_not_before() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default { state_entry() {
                print("before"); llSleep(2.0); print("after"); llSleep(0.25); print("end");
            } }"#,
        )?,
    );
    let reports = run(&mut engine, &mut host, 30);
    // Two seconds are twenty ticks; a quarter second rounds up to three.
    assert_eq!(
        host.from(A),
        vec![(1, "before"), (21, "after"), (24, "end")]
    );
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Finished, Outcome::Sleeping(Tick(21))])
    );
    // Asleep is not runnable.
    assert!(
        reports
            .iter()
            .filter(|report| (2..21).contains(&report.tick.0))
            .all(|report| report.runnable == 0)
    );
    Ok(())
}

#[test]
fn a_stopped_script_keeps_its_globals_and_resumes_where_it_was() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer n;
            default {
                state_entry() { while (TRUE) { ++n; print((string)n); llSleep(0.1); } }
                touch_start(integer count) { print(\"touched\"); }
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 3);
    assert_eq!(host.texts(A), vec!["1", "2", "3"]);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Stopped))
    );
    let reports = run(&mut engine, &mut host, 5);
    assert!(reports.iter().all(|report| report.runnable == 0));
    assert_eq!(host.texts(A), vec!["1", "2", "3"]);
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(
        instance.is_busy(),
        "stopping dropped the handler in progress"
    );
    assert_eq!(instance.global("n"), Some(&Value::Integer(3)));
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 2);
    // It carries on counting from where it was, not from a reset.
    assert_eq!(host.texts(A), vec!["1", "2", "3", "4", "5"]);
    // The touch posted while stopped never arrives.
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

#[test]
fn a_run_time_error_stops_one_script_and_its_neighbours_run_on() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer zero;
default {
    state_entry() {
        print(\"start\");
        print((string)(1 / zero));
        print(\"unreached\");
    }
}",
        )?,
    );
    let _none = engine.add(
        B,
        program("default { state_entry() { while (TRUE) { print(\"b\"); llSleep(0.1); } } }")?,
    );
    let reports = run(&mut engine, &mut host, 3);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::MathError,
                position: Some(Position {
                    line: 5,
                    column: 24
                }),
            }),
        ])
    );
    assert_eq!(host.texts(A), vec!["start"]);
    assert_eq!(host.texts(B), vec!["b", "b", "b"]);
    let instance = engine.instance(A).ok_or("no instance")?;
    assert!(!instance.is_running());
    assert_eq!(instance.state(), StateId::DEFAULT);
    Ok(())
}

#[test]
fn get_script_state_reads_the_flag_a_fault_clears() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder {
        scripts: [("Watcher", A), ("Divider", B)]
            .into_iter()
            .map(|(name, id)| (name.to_owned(), id))
            .collect(),
        ..Recorder::default()
    };
    let _none = engine.add(
        A,
        program(
            r#"report() {
                print((string)llGetScriptState("Watcher")
                    + (string)llGetScriptState("Divider")
                    + (string)llGetScriptState("Nobody"));
            }
            default {
                state_entry() { report(); llSetTimerEvent(0.5); }
                timer() { report(); llSetTimerEvent(0.0); }
            }"#,
        )?,
    );
    let _none = engine.add(
        B,
        program("integer zero; default { state_entry() { llSleep(0.2); zero = 1 / zero; } }")?,
    );
    let _reports = run(&mut engine, &mut host, 6);
    assert_eq!(host.texts(A), vec!["110", "100"]);
    // Restarted, it reads as running again: one flag, not a copy.
    engine
        .instance_mut(B)
        .ok_or("no instance")?
        .set_running(true);
    engine.instance_mut(A).ok_or("no instance")?.reset();
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["110", "100", "110"]);
    Ok(())
}

#[test]
fn a_body_mono_refuses_faults_before_its_first_line() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { state_entry() { integer i = 3; print(\"x\"); float g = i *= 1.5; } }")?,
    );
    let reports = run(&mut engine, &mut host, 1);
    assert!(host.texts(A).is_empty());
    assert!(matches!(
        reports.first().map(|report| outcomes(report, A)).as_deref(),
        Some([
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::InvalidProgram,
                ..
            })
        ])
    ));
    Ok(())
}

#[test]
fn runaway_recursion_is_a_stack_heap_collision_not_a_host_crash() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("integer f(integer n) { return f(n + 1); } default { state_entry() { f(0); } }")?,
    );
    let reports = run(&mut engine, &mut host, 10);
    let faults: Vec<RuntimeError> = reports
        .iter()
        .flat_map(|report| outcomes(report, A))
        .filter_map(|outcome| match outcome {
            Outcome::Faulted(fault) => Some(fault.error),
            _ => None,
        })
        .collect();
    assert_eq!(faults, vec![RuntimeError::StackHeapCollision]);
    Ok(())
}

#[test]
fn a_reset_restarts_the_initialisers_and_drops_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer g = 5;
            integer resets;
            default {
                state_entry() { print((string)g); g = 7; }
                touch_start(integer count) {
                    print(\"touch \" + (string)g);
                    llResetScript();
                    print(\"unreached\");
                }
                changed(integer change) { print(\"stale change\"); }
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(engine.changed(A, 1), Ok(Some(Posted::Queued)));
    let reports = run(&mut engine, &mut host, 2);
    // The touch runs and resets; the change, queued behind it, is dropped
    // with the queue; the initialisers put `g` back to 5.
    assert_eq!(host.texts(A), vec!["5", "touch 7", "5"]);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![Outcome::Reset, Outcome::Finished, Outcome::Finished])
    );
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

#[test]
fn a_state_change_runs_exit_then_entry_and_discards_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer f() { if (TRUE) state two; return 1; }
            default {
                state_entry() { print("entry"); }
                touch_start(integer count) {
                    integer x = f();
                    print("after " + (string)x);
                }
                state_exit() { print("exit"); }
            }
            state two {
                state_entry() { print("two"); state two; }
                touch_start(integer count) { print("touch two"); }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    for _ in 0..2 {
        let _posted = engine.post_detected(A, touch, vec![toucher("a")]);
    }
    let reports = run(&mut engine, &mut host, 2);
    // The function's `state` yields 0 and the handler carries on; the
    // transition waits for its end, then discards the second touch. `state
    // two` inside `two` is no transition. All three as aditi does them
    // (2026-09-28): a `state default;` in `default` ran neither `state_exit`
    // nor `state_entry`, and a function's `state other;` returned 0 to a
    // caller that carried on before `default`'s `state_exit` ran.
    assert_eq!(host.texts(A), vec!["entry", "after 0", "exit", "two"]);
    let two = StateId(1);
    assert_eq!(
        reports.first().map(|report| outcomes(report, A)),
        Some(vec![
            Outcome::Finished,
            Outcome::StateChanged(two),
            Outcome::Finished,
        ])
    );
    assert_eq!(engine.instance(A).map(Instance::state), Some(two));
    assert_eq!(engine.instance(A).map(Instance::queued), Some(0));
    Ok(())
}

#[test]
fn events_reach_only_a_handler_of_the_current_state() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default { touch_start(integer count) { print("touched " + (string)count); } }"#,
        )?,
    );
    let touch = named("touch_start")?;
    let timer = named("timer")?;
    let link = named("link_message")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a"), toucher("b"), toucher("c")]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(engine.post(A, timer, vec![]), Ok(Some(Posted::NoHandler)));
    assert_eq!(engine.post(B, timer, vec![]), Ok(None));
    assert_eq!(
        engine.post(A, link, vec![]),
        Err(PostError::Arity {
            event: "link_message",
            expected: 4,
            found: 0,
        })
    );
    assert_eq!(
        engine.post(
            A,
            link,
            vec![
                Value::Float(1.0),
                Value::Integer(0),
                Value::String(String::new()),
                Value::Key(String::new()),
            ]
        ),
        Err(PostError::Argument {
            event: "link_message",
            index: 0,
            expected: TypeName::Integer,
            found: TypeName::Float,
        })
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["touched 3"]);
    Ok(())
}

#[test]
fn an_over_budget_region_serves_its_scripts_in_turn() -> Result<(), String> {
    let config = EngineConfig::for_step(STEP);
    // Room for one script's share per tick, and three busy scripts.
    let mut engine = Engine::new(EngineConfig {
        region_budget: config.script_budget,
        ..config
    });
    let mut host = Recorder::default();
    let busy = program("default { state_entry() { while (TRUE) { } } }")?;
    let c = CallerId(3);
    for id in [A, B, c] {
        let _none = engine.add(id, Arc::clone(&busy));
    }
    let reports = run(&mut engine, &mut host, 6);
    let served: Vec<CallerId> = reports
        .iter()
        .filter_map(|report| report.slices.first().map(|(id, _)| *id))
        .collect();
    assert_eq!(served, vec![A, B, c, A, B, c]);
    for report in &reports {
        assert_eq!((report.runnable, report.served), (3, 1));
        let budget = u64::from(config.script_budget);
        assert!(
            (budget..=budget.saturating_add(u64::from(BACKWARD_JUMP_COST))).contains(&report.used),
            "used {} of {budget}",
            report.used
        );
    }
    Ok(())
}

#[test]
fn every_value_type_survives_a_round_trip_through_the_machine() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"vector v = <1, 2, 3>;
            rotation r;
            list l = [1, 2.5, "s"];
            default { state_entry() {
                v.y = 5;
                r.s = 0.5;
                l += v;
                key k = "k";
                print((string)v + " " + (string)r.s + " " + (string)llGetListLength(l)
                    + " " + (string)l + " " + (string)k + " " + (string)(v.x + v.z));
            } }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(
        host.texts(A),
        vec![
            "<1.00000, 5.00000, 3.00000> 0.500000 4 12.500000s<1.000000, 5.000000, 3.000000> k 4.000000"
        ]
    );
    assert!(host.stubbed.is_empty());
    Ok(())
}

#[test]
fn a_call_to_a_function_nobody_wrote_stops_the_script() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { state_entry() { llSay(0, \"hi\"); } }")?,
    );
    let reports = run(&mut engine, &mut host, 1);
    assert!(matches!(
        reports.first().map(|report| outcomes(report, A)).as_deref(),
        Some([
            Outcome::Finished,
            Outcome::Faulted(Fault {
                error: RuntimeError::Unimplemented(BuiltinId::LlSay),
                ..
            })
        ])
    ));
    Ok(())
}

#[test]
fn a_sleep_runs_out_while_stopped_and_a_restart_resumes_at_once() -> Result<(), String> {
    // aditi (2026-09-28): a script stopped one second into `llSleep(5.0)`
    // and restarted at nine woke within the restart's frame.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(r#"default { state_entry() { print("sleep"); llSleep(5.0); print("woke"); } }"#)?,
    );
    let _reports = run(&mut engine, &mut host, 10);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let _reports = run(&mut engine, &mut host, 80);
    assert_eq!(host.texts(A), vec!["sleep"]);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.from(A), vec![(1, "sleep"), (91, "woke")]);
    Ok(())
}

#[test]
fn a_restart_before_the_sleep_ends_still_waits_it_out() -> Result<(), String> {
    // aditi (2026-09-28): `llSleep(5.0)` begun at 06.73 s, the script stopped
    // at 07.76 and restarted at 09.82, woke at 11.78 — five seconds after it
    // began. The sleep counts on while the script is stopped.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(r#"default { state_entry() { print("sleep"); llSleep(5.0); print("woke"); } }"#)?,
    );
    let _reports = run(&mut engine, &mut host, 10);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(false);
    let _reports = run(&mut engine, &mut host, 20);
    engine
        .instance_mut(A)
        .ok_or("no instance")?
        .set_running(true);
    let _reports = run(&mut engine, &mut host, 30);
    assert_eq!(host.from(A), vec![(1, "sleep"), (51, "woke")]);
    Ok(())
}

/// A touch by `name`, keyed after it.
fn toucher(name: &str) -> Detected {
    Detected {
        key: format!("{name:0>8}-0000-0000-0000-000000000000"),
        name: name.to_owned(),
        kind: 1,
        touch: Touch {
            face: 2,
            ..Touch::INVALID
        },
        ..Detected::blank()
    }
}

#[test]
fn the_queue_holds_sixty_four_events_and_drops_the_rest() -> Result<(), String> {
    let mut engine = engine();
    let _none = engine.add(
        A,
        program(
            "default { link_message(integer sender, integer number, string text, key id) { } }",
        )?,
    );
    let link = named("link_message")?;
    let args = || {
        vec![
            Value::Integer(1),
            Value::Integer(0),
            Value::String(String::new()),
            Value::Key(String::new()),
        ]
    };
    let posted: Vec<Posted> =
        core::iter::repeat_with(|| engine.post(A, link, args()).ok().flatten())
            .take(70)
            .collect::<Option<_>>()
            .ok_or("a post failed")?;
    assert_eq!(
        posted.iter().filter(|p| **p == Posted::Queued).count(),
        MAX_QUEUED
    );
    assert_eq!(posted.last(), Some(&Posted::QueueFull));
    assert_eq!(engine.instance(A).map(Instance::queued), Some(MAX_QUEUED));
    Ok(())
}

#[test]
fn a_timer_repeats_on_its_ticks_and_never_stacks() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer n;
            default {
                state_entry() { llSetTimerEvent(0.3); }
                timer() {
                    ++n;
                    print("timer " + (string)n);
                    if (n == 2) llSleep(1.0);
                    if (n == 3) llSetTimerEvent(0.0);
                }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 40);
    // Due at 4 and 7; the second handler sleeps ten ticks, through three
    // more due ticks (10, 13, 16), which queue one event, not three. That one
    // runs when the sleep ends, at 17, and stops the timer.
    assert_eq!(
        host.from(A),
        vec![(4, "timer 1"), (7, "timer 2"), (17, "timer 3")]
    );
    assert_eq!(engine.instance(A).map(Instance::has_timer), Some(false));
    Ok(())
}

#[test]
fn a_state_change_releases_the_hosts_hold_and_discards_the_queue() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default {
                state_entry() { llSetTimerEvent(0.5); }
                touch_start(integer n) { state two; }
                link_message(integer sender, integer number, string text, key id) {
                    print("stale link message");
                }
            }
            state two {
                timer() { print("timer in two"); llSetTimerEvent(0.0); }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    let link = named("link_message")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(
        engine.post(
            A,
            link,
            vec![
                Value::Integer(1),
                Value::Integer(0),
                Value::String(String::new()),
                Value::Key(String::new()),
            ]
        ),
        Ok(Some(Posted::Queued))
    );
    let _reports = run(&mut engine, &mut host, 10);
    // The link message queued behind the touch is gone with the state; the
    // host was told once; the timer set in `default` carries on in `two`.
    assert_eq!(host.from(A), vec![(6, "timer in two")]);
    assert_eq!(host.left, vec![(2, A)]);
    Ok(())
}

#[test]
fn detected_functions_read_the_block_of_the_event_being_handled_and_only_it() -> Result<(), String>
{
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"string who(integer i) {
                return llDetectedName(i) + "/" + (string)llDetectedType(i) + "/"
                    + (string)llDetectedTouchFace(i) + "/" + (string)llDetectedKey(i);
            }
            default {
                touch_start(integer n) {
                    print((string)n + " " + who(0) + " " + who(1) + " " + who(2));
                    llSetTimerEvent(0.1);
                }
                timer() { llSetTimerEvent(0.0); print("timer " + who(0)); }
            }"#,
        )?,
    );
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    // A second toucher in the same tick joins the queued event.
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("b"), toucher("a")]),
        Ok(Some(Posted::Merged))
    );
    let _reports = run(&mut engine, &mut host, 3);
    let a = "0000000a-0000-0000-0000-000000000000";
    let b = "0000000b-0000-0000-0000-000000000000";
    // Past the block, and in an event without one, everything reads zero —
    // the name as the `NULL_KEY` string, the face as 0 (aditi, 2026-09-28).
    let null = crate::value::NULL_KEY;
    assert_eq!(
        host.texts(A),
        vec![
            format!("2 a/1/2/{a} b/1/2/{b} {null}/0/0/{null}").as_str(),
            format!("timer {null}/0/0/{null}").as_str(),
        ]
    );
    Ok(())
}

#[test]
fn touches_in_different_ticks_are_separate_events() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { touch_start(integer n) { print((string)n); llSleep(0.5); } }")?,
    );
    let touch = named("touch_start")?;
    assert_eq!(
        engine.post_detected(A, touch, vec![toucher("a")]),
        Ok(Some(Posted::Queued))
    );
    let _reports = run(&mut engine, &mut host, 1);
    for name in ["b", "c"] {
        assert_eq!(
            engine.post_detected(A, touch, vec![toucher(name)]),
            Ok(Some(Posted::Queued))
        );
        let _reports = run(&mut engine, &mut host, 1);
    }
    let _reports = run(&mut engine, &mut host, 20);
    assert_eq!(host.texts(A), vec!["1", "1", "1"]);
    assert_eq!(
        engine.post(A, touch, vec![Value::Integer(1)]),
        Err(PostError::Detection("touch_start"))
    );
    assert_eq!(
        engine.post_detected(A, touch, Vec::new()),
        Err(PostError::DetectedCount {
            event: "touch_start",
            found: 0,
        })
    );
    assert_eq!(
        engine.post(A, named("state_entry")?, Vec::new()),
        Err(PostError::Transition("state_entry"))
    );
    Ok(())
}

#[test]
fn changed_carries_the_bits_of_whatever_raised_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program("default { changed(integer change) { print((string)change); } }")?,
    );
    let bit = |name: &str| match crate::library::constant(name).map(|c| c.value.to_value()) {
        Some(Value::Integer(bit)) => Ok(bit),
        _ => Err(format!("no constant {name}")),
    };
    let raised = [
        "CHANGED_INVENTORY",
        "CHANGED_COLOR",
        "CHANGED_SCALE",
        "CHANGED_LINK",
        "CHANGED_OWNER",
        "CHANGED_REGION_START",
    ];
    for (index, name) in raised.into_iter().enumerate() {
        let expected = if index == 0 {
            Posted::Queued
        } else {
            Posted::Merged
        };
        assert_eq!(engine.changed(A, bit(name)?), Ok(Some(expected)));
    }
    let _reports = run(&mut engine, &mut host, 1);
    // One tick's changes are one event with every bit.
    assert_eq!(host.texts(A), vec!["1195"]);
    // Changes a tick apart are events of their own.
    for name in ["CHANGED_TEXTURE", "CHANGED_SHAPE"] {
        assert_eq!(engine.changed(A, bit(name)?), Ok(Some(Posted::Queued)));
        let _reports = run(&mut engine, &mut host, 1);
    }
    assert_eq!(host.texts(A), vec!["1195", "16", "4"]);
    Ok(())
}

#[test]
fn on_rez_brings_the_start_parameter_and_a_reset_keeps_it() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"integer resets;
            default {
                state_entry() { print("start " + (string)llGetStartParameter()); }
                on_rez(integer param) {
                    print("rez " + (string)param + " " + (string)llGetStartParameter());
                    llResetScript();
                }
            }"#,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(engine.rez(A, 42), Ok(Some(Posted::Queued)));
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.texts(A), vec!["start 0", "rez 42 42", "start 42"]);
    Ok(())
}

#[test]
fn the_engines_own_events_are_in_the_table() {
    for name in ["timer", "changed", "on_rez"] {
        assert!(event(name).is_some(), "{name}");
    }
}

/// A sample argument of type `ty` and how `(string)` prints it.
fn sample(ty: TypeName) -> Value {
    match ty {
        TypeName::Integer => Value::Integer(7),
        TypeName::Float => Value::Float(1.5),
        TypeName::String => Value::String("s".to_owned()),
        TypeName::Key => Value::Key(crate::value::NULL_KEY.to_owned()),
        TypeName::Vector => Value::Vector(sl_types::lsl::Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }),
        TypeName::Rotation => Value::Rotation(crate::value::ZERO_ROTATION),
        TypeName::List => Value::List(vec![crate::value::Element::Integer(4)]),
    }
}

#[test]
fn every_event_reaches_a_handler_with_its_parameters() -> Result<(), String> {
    let mut checked = 0_usize;
    for event in &crate::library::EVENTS {
        if matches!(event.name, "state_entry" | "state_exit") {
            continue;
        }
        let params: Vec<String> = event
            .args
            .iter()
            .enumerate()
            .map(|(i, arg)| format!("{} a{i}", arg.ty.keyword()))
            .collect();
        let printed: Vec<String> = (0..event.args.len())
            .map(|i| format!("(string)a{i}"))
            .collect();
        let body = if printed.is_empty() {
            format!("print({:?});", event.name)
        } else {
            format!("print({});", printed.join(" + \"|\" + "))
        };
        let source = format!(
            "default {{ {}({}) {{ {body} }} }}",
            event.name,
            params.join(", ")
        );
        let mut engine = engine();
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program(&source).map_err(|e| format!("{}: {e}", event.name))?,
        );
        let expected = if is_detection_event(event.name) {
            let _posted = engine
                .post_detected(A, event, vec![Detected::blank()])
                .map_err(|e| e.to_string())?;
            "1".to_owned()
        } else {
            let args: Vec<Value> = event.args.iter().map(|arg| sample(arg.ty)).collect();
            let expected = if args.is_empty() {
                event.name.to_owned()
            } else {
                args.iter()
                    .map(|value| match crate::cast(value.clone(), TypeName::String) {
                        Ok(Value::String(text)) => text,
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            };
            assert_eq!(
                engine.post(A, event, args),
                Ok(Some(Posted::Queued)),
                "{}",
                event.name
            );
            expected
        };
        let _reports = run(&mut engine, &mut host, 1);
        assert_eq!(host.texts(A), vec![expected.as_str()], "{}", event.name);
        checked = checked.saturating_add(1);
    }
    assert_eq!(checked, crate::library::EVENTS.len().saturating_sub(2));
    Ok(())
}

/// Three changes — two in one tick, one in the next — and two touches in
/// one tick, under `coalescing`; what the script saw.
fn arrivals(coalescing: Coalescing) -> Result<Vec<String>, String> {
    let mut engine = Engine::new(EngineConfig {
        coalescing,
        ..EngineConfig::for_step(STEP)
    });
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            r#"default {
                state_entry() { llSleep(0.3); }
                changed(integer change) { print("changed " + (string)change); }
                touch_start(integer n) { print("touch " + (string)n); }
            }"#,
        )?,
    );
    // The script sleeps through ticks 1-3, so everything below queues.
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    let posts = [
        engine.changed(A, 2),
        engine.changed(A, 8),
        engine.post_detected(A, touch, vec![toucher("a")]),
        engine.post_detected(A, touch, vec![toucher("b")]),
    ];
    let _reports = run(&mut engine, &mut host, 1);
    let late = engine.changed(A, 16);
    let _reports = run(&mut engine, &mut host, 5);
    let mut seen: Vec<String> = posts
        .iter()
        .chain([&late])
        .map(|posted| format!("{posted:?}"))
        .collect();
    seen.extend(host.texts(A).into_iter().map(str::to_owned));
    Ok(seen)
}

#[test]
fn a_test_can_force_every_arrival_shape_a_script_may_meet() -> Result<(), String> {
    let queued = "Ok(Some(Queued))";
    let merged = "Ok(Some(Merged))";
    // The reference: one tick's raises merge; the late change finds the
    // queued `changed` next in line and does not.
    assert_eq!(
        arrivals(Coalescing::Reference)?,
        vec![
            queued,
            merged,
            queued,
            merged,
            queued,
            "changed 10",
            "touch 2",
            "changed 16"
        ]
    );
    // The most split-up arrival.
    assert_eq!(
        arrivals(Coalescing::Never)?,
        vec![
            queued,
            queued,
            queued,
            queued,
            queued,
            "changed 2",
            "changed 8",
            "touch 1",
            "touch 1",
            "changed 16"
        ]
    );
    // The most merged: the late change joins the one still queued.
    assert_eq!(
        arrivals(Coalescing::WhileQueued)?,
        vec![
            queued,
            merged,
            queued,
            merged,
            merged,
            "changed 26",
            "touch 2"
        ]
    );
    Ok(())
}

#[test]
fn a_later_change_joins_a_queued_one_unless_it_is_next_in_line() -> Result<(), String> {
    // aditi (2026-09-28), four touches alike: while the handler was busy,
    // scale, colour, (texture, which raised nothing) and scale again, a fifth
    // of a second apart, arrived as `8` and then `10`.
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default {
                state_entry() { llSleep(1.0); }
                changed(integer change) { print((string)change); }
            }",
        )?,
    );
    let mut posted = Vec::new();
    for bits in [8, 2, 8] {
        let _reports = run(&mut engine, &mut host, 2);
        posted.push(engine.changed(A, bits));
    }
    let _reports = run(&mut engine, &mut host, 10);
    assert_eq!(
        posted,
        vec![
            Ok(Some(Posted::Queued)),
            Ok(Some(Posted::Queued)),
            Ok(Some(Posted::Merged)),
        ]
    );
    assert_eq!(host.texts(A), vec!["8", "10"]);
    Ok(())
}
