//! The VM against the acceptance of `server-lsl-vm-execution`: a runaway
//! loop cannot stall the region, a sleep resumes on its tick, a stopped
//! script keeps its globals, and an error stops only its own script.

use std::sync::Arc;

use core::time::Duration;

use pretty_assertions::assert_eq;

use super::*;
use crate::bytecode::{Position, StateId};
use crate::compile;
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
}

impl Host for Recorder {
    fn print(&mut self, caller: CallerId, text: &str) {
        self.printed.push((self.now, caller, text.to_owned()));
    }

    fn stubbed(&mut self, caller: CallerId, id: BuiltinId) {
        self.stubbed.push((caller, id));
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
        engine.post(A, touch, vec![Value::Integer(1)]),
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
            }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let touch = named("touch_start")?;
    for _ in 0..2 {
        assert_eq!(
            engine.post(A, touch, vec![Value::Integer(1)]),
            Ok(Some(Posted::Queued))
        );
    }
    let reports = run(&mut engine, &mut host, 2);
    // One touch runs and resets; the second, queued behind it, is dropped
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
        let _posted = engine.post(A, touch, vec![Value::Integer(1)]);
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
    assert_eq!(
        engine.post(A, touch, vec![Value::Integer(3)]),
        Ok(Some(Posted::Queued))
    );
    assert_eq!(engine.post(A, timer, vec![]), Ok(Some(Posted::NoHandler)));
    assert_eq!(engine.post(B, timer, vec![]), Ok(None));
    assert_eq!(
        engine.post(A, touch, vec![]),
        Err(PostError::Arity {
            event: "touch_start",
            expected: 1,
            found: 0,
        })
    );
    assert_eq!(
        engine.post(A, touch, vec![Value::Float(1.0)]),
        Err(PostError::Argument {
            event: "touch_start",
            index: 0,
            expected: sl_lsl::ast::TypeName::Integer,
            found: sl_lsl::ast::TypeName::Float,
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
