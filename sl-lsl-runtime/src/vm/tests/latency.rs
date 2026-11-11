//! The moment each kind of event is raised and run, per flavour
//! (`events/latency`, 2026-11-08): Second Life runs a link message a frame
//! after it was sent, a setter's `changed` and a sweep's answer two frames
//! after the call, and a line said to a script at once; OpenSim runs the
//! first three at once and hands a line over at its next async command pass.

use pretty_assertions::assert_eq;

use super::*;

/// An engine of `flavour` at a step of `step`, queueing as the fake grid
/// has it queue (first in, first out on OpenSim), with `scripts` added, all
/// in one prim; and the host they run against.
fn staged(
    flavour: EngineFlavour,
    step: Duration,
    scripts: &[(CallerId, &str)],
) -> Result<(Engine, Recorder), String> {
    let mut engine = Engine::new(EngineConfig {
        coalescing: if flavour.is_opensim() {
            Coalescing::Never
        } else {
            Coalescing::Reference
        },
        ..EngineConfig::for_step(step, flavour)
    });
    let mut host = Recorder::default();
    let _none = host
        .prim_keys
        .insert(1, "00000000-0000-4000-8000-000000000001".to_owned());
    for (id, source) in scripts {
        let _none = engine.add(*id, program_for(source, flavour)?);
        let _none = host.prims.insert(*id, 1);
    }
    Ok((engine, host))
}

/// What `id` printed, each line with the tick it was printed in.
fn printed_by(host: &Recorder, id: CallerId) -> Vec<(u64, String)> {
    host.from(id)
        .into_iter()
        .map(|(tick, text)| (tick, text.to_owned()))
        .collect()
}

/// What `A` printed, each line with the tick it was printed in, after
/// `ticks` ticks of `source` under `flavour` at a step of `step`.
fn printed(
    flavour: EngineFlavour,
    step: Duration,
    source: &str,
    ticks: u64,
) -> Result<Vec<(u64, String)>, String> {
    let (mut engine, mut host) = staged(flavour, step, &[(A, source)])?;
    let _reports = run(&mut engine, &mut host, ticks);
    Ok(printed_by(&host, A))
}

/// `lines`, each as `text@tick`.
fn stamped(lines: &[(u64, String)]) -> Vec<String> {
    lines
        .iter()
        .map(|(tick, text)| format!("{text}@{tick}"))
        .collect()
}

#[test]
fn a_link_message_runs_a_frame_later_on_second_life_alone() -> Result<(), String> {
    let source = r#"default {
        state_entry() { print("sent"); llMessageLinked(LINK_THIS, 0, "", ""); }
        link_message(integer sender, integer num, string text, key id) { print("link"); }
    }"#;
    assert_eq!(
        stamped(&printed(EngineFlavour::SecondLife, STEP, source, 4)?),
        vec!["sent@1", "link@2"]
    );
    for flavour in [EngineFlavour::YEngine, EngineFlavour::XEngine] {
        assert_eq!(
            stamped(&printed(flavour, STEP, source, 4)?),
            vec!["sent@1", "link@1"],
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn a_setters_changed_runs_two_frames_later_on_second_life_and_in_the_call_on_opensim()
-> Result<(), String> {
    // The link message is sent after the colour: OpenSim queues the
    // `changed` in the call, ahead of it (`events/order` round 2).
    let source = r#"default {
        state_entry() {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
            llMessageLinked(LINK_THIS, 0, "", "");
        }
        changed(integer change) { print("changed " + (string)change); }
        link_message(integer sender, integer num, string text, key id) { print("link"); }
    }"#;
    assert_eq!(
        stamped(&printed(EngineFlavour::SecondLife, STEP, source, 5)?),
        vec!["link@2", "changed 2@3"]
    );
    assert_eq!(
        stamped(&printed(EngineFlavour::YEngine, STEP, source, 5)?),
        vec!["changed 2@1", "link@1"]
    );
    Ok(())
}

#[test]
fn a_sweep_is_answered_two_frames_on_on_second_life_and_in_the_call_on_opensim()
-> Result<(), String> {
    let source = r#"default {
        state_entry() { llSensor("", NULL_KEY, AGENT, 20.0, PI); }
        no_sensor() { print("no_sensor"); }
    }"#;
    assert_eq!(
        stamped(&printed(EngineFlavour::SecondLife, STEP, source, 5)?),
        vec!["no_sensor@3"]
    );
    assert_eq!(
        stamped(&printed(EngineFlavour::YEngine, STEP, source, 5)?),
        vec!["no_sensor@1"]
    );
    Ok(())
}

#[test]
fn second_life_ranks_a_sweeps_answer_behind_a_later_timer() -> Result<(), String> {
    // `events/latency` `sensor-timer`: the answer, raised two frames after
    // the call, still runs after a timer that fell due a second later in
    // the same sleep; OpenSim runs them in the order raised.
    let source = r#"default {
        state_entry() {
            llSetTimerEvent(1.0);
            llSensor("", NULL_KEY, AGENT, 20.0, PI);
            llSleep(2.0);
        }
        timer() { llSetTimerEvent(0.0); print("timer"); }
        no_sensor() { print("no_sensor"); }
    }"#;
    let order = |flavour| -> Result<Vec<String>, String> {
        Ok(printed(flavour, STEP, source, 40)?
            .into_iter()
            .map(|(_tick, text)| text)
            .collect())
    };
    assert_eq!(
        order(EngineFlavour::SecondLife)?,
        vec!["timer", "no_sensor"]
    );
    assert_eq!(order(EngineFlavour::YEngine)?, vec!["no_sensor", "timer"]);
    Ok(())
}

#[test]
fn a_link_message_waiting_for_its_frame_lets_a_ready_event_go_first() -> Result<(), String> {
    // `events/timer-reset` `in-timer-media-keep` (aditi): a `changed` raised
    // while the handler ran is free to run when it ends, and runs ahead of
    // the link message the handler sent itself last.
    let source = r#"default {
        state_entry() {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
            llSleep(0.5);
            llMessageLinked(LINK_THIS, 0, "", "");
        }
        changed(integer change) { print("changed"); }
        link_message(integer sender, integer num, string text, key id) { print("link"); }
    }"#;
    assert_eq!(
        stamped(&printed(EngineFlavour::SecondLife, STEP, source, 10)?),
        vec!["changed@6", "link@7"]
    );
    Ok(())
}

#[test]
fn opensim_hands_a_line_over_at_its_next_async_pass() -> Result<(), String> {
    // Ticks of 20 ms: OpenSim's pass falls every fifth tick (100 ms).
    let step = Duration::from_millis(20);
    let source = r#"default {
        state_entry() { llListen(7, "", NULL_KEY, ""); print("listening"); }
        listen(integer channel, string name, key id, string text) { print(text); }
    }"#;
    let heard = Heard {
        channel: 7,
        name: "Talker".to_owned(),
        speaker: "00000000-0000-0000-0000-0000000000aa".to_owned(),
        message: "hello".to_owned(),
    };
    let lines = |flavour| -> Result<Vec<String>, String> {
        let (mut engine, mut host) = staged(flavour, step, &[(A, source)])?;
        let _reports = run(&mut engine, &mut host, 1);
        engine.overhear(A, &heard);
        let _reports = run(&mut engine, &mut host, 8);
        Ok(stamped(&printed_by(&host, A)))
    };
    assert_eq!(
        lines(EngineFlavour::SecondLife)?,
        vec!["listening@1", "hello@2"]
    );
    assert_eq!(
        lines(EngineFlavour::YEngine)?,
        vec!["listening@1", "hello@5"]
    );
    Ok(())
}

#[test]
fn opensim_runs_an_answer_in_the_tick_it_was_sent() -> Result<(), String> {
    // `events/latency` `link-echo`: the echo, served before the asker in the
    // tick, answers on what is left of its share; Second Life takes a frame
    // for each hop.
    let echo = r#"default {
        link_message(integer sender, integer num, string text, key id) {
            if (num == 1) llMessageLinked(LINK_THIS, 2, "", "");
        }
    }"#;
    let asker = r#"default {
        state_entry() { llSleep(0.3); print("ask"); llMessageLinked(LINK_THIS, 1, "", ""); }
        link_message(integer sender, integer num, string text, key id) {
            if (num == 2) print("answer");
        }
    }"#;
    let lines = |flavour| -> Result<Vec<String>, String> {
        let (mut engine, mut host) = staged(flavour, STEP, &[(A, echo), (B, asker)])?;
        let _reports = run(&mut engine, &mut host, 10);
        Ok(stamped(&printed_by(&host, B)))
    };
    assert_eq!(lines(EngineFlavour::SecondLife)?, vec!["ask@4", "answer@6"]);
    assert_eq!(lines(EngineFlavour::YEngine)?, vec!["ask@4", "answer@4"]);
    Ok(())
}
