//! Forced delays and throttles held to the grids: the `throttle/*` probes run
//! in the VM under each flavour (book chapter `gridspec/lsl.md`, § Forced
//! delays and throttles) — each call's delay set against the one the grid
//! measured, the chat each grid let through and the HTTP throttle's window.

use pretty_assertions::assert_eq;

use super::clocks::{frame, run_for};
use super::comms::OTHER;
use super::http::{hold, http_stage, without};
use super::strings::probe_lines;
use super::*;

/// The transcripts of the probe `throttle/<name>`, per flavour.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    macro_rules! three {
        ($probe:literal) => {
            [
                include_str!(concat!(
                    "../../../probes/results/aditi/throttle/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-yengine/throttle/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-xengine/throttle/",
                    $probe,
                    ".txt"
                )),
            ]
        };
    }
    let [aditi, yengine, xengine] = match name {
        "delays" => three!("delays"),
        "chat" => three!("chat"),
        _ => three!("http-window"),
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// The value of the field `name` of a probe line, as a number.
fn number(line: &str, name: &str) -> Option<f64> {
    line.split(' ')
        .find_map(|field| field.strip_prefix(&format!("{name}=")))
        .and_then(|value| value.parse().ok())
}

/// The `rec=delay` lines' function names and the delay to hold ours to: the
/// least of the grid's three calls, which a frame's jitter can only lengthen.
fn delays(lines: &[String], field: &str) -> Vec<(String, f64)> {
    lines
        .iter()
        .filter(|line| line.contains(" rec=delay "))
        .filter_map(|line| {
            let name = line
                .split(' ')
                .find_map(|field| field.strip_prefix("fn="))?
                .to_owned();
            Some((name, number(line, field)?))
        })
        .collect()
}

#[test]
fn each_call_takes_the_forced_delay_its_grid_measured() -> Result<(), String> {
    let source = include_str!("../../../probes/throttle/delays.lsl");
    for (flavour, transcript) in transcripts("delays") {
        let mut region = http_stage(flavour, &[(A, "Probe", source)])?;
        // The owner stood in the region, which the dataserver calls ask of.
        let _previous = region
            .host
            .avatars
            .insert(OWNER.to_owned(), TestAvatar::standing("Probe Owner"));
        run_for(&mut region, flavour, 120)?;
        let ours = region.lines();
        let theirs = probe_lines(transcript);
        // Every other line as the grid printed it.
        let vary = ["seconds", "least", "most"];
        let shape = |lines: &[String]| -> Vec<String> {
            lines
                .iter()
                .filter(|line| !line.contains(" rec=delay "))
                .map(|line| without(line, &vary))
                .collect()
        };
        assert_eq!(shape(&ours), shape(&theirs), "{flavour:?}");
        // Ours is read in whole ticks, as the grid's script time is in
        // frames: within one frame and a little of the grid's least.
        let slack = frame(flavour).as_secs_f64() * 1.5;
        let ours = delays(&ours, "seconds");
        let theirs = delays(&theirs, "least");
        assert_eq!(
            ours.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            theirs.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            "{flavour:?}"
        );
        let off: Vec<String> = ours
            .iter()
            .zip(&theirs)
            .filter(|((_, ours), (_, theirs))| (ours - theirs).abs() > slack)
            .map(|((name, ours), (_, theirs))| format!("{name}: ours {ours}, the grid's {theirs}"))
            .collect();
        assert_eq!(off, Vec::<String>::new(), "{flavour:?}");
    }
    Ok(())
}

/// The `delays` field of the `rec=flood` line of kind 0 — each `llSay`'s
/// time — to a tenth of a second.
fn channel_zero_delays(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.contains(" rec=flood kind=0 "))
        .filter_map(|line| line.split(" delays=\"").nth(1))
        .flat_map(|delays| delays.trim_end_matches('"').split(','))
        .filter_map(|delay| delay.parse::<f64>().ok())
        .map(|delay| format!("{delay:.1}"))
        .collect()
}

#[test]
fn channel_zero_chat_is_throttled_on_opensim_alone() -> Result<(), String> {
    let ears = [
        include_str!("../../../probes/throttle/chat/ear-0.lsl"),
        include_str!("../../../probes/throttle/chat/ear-1.lsl"),
        include_str!("../../../probes/throttle/chat/ear-2.lsl"),
        include_str!("../../../probes/throttle/chat/ear-3.lsl"),
    ];
    let tally = include_str!("../../../probes/throttle/chat/tally.lsl");
    let flood = include_str!("../../../probes/throttle/chat/flood.lsl");
    for (flavour, transcript) in transcripts("chat") {
        // The Flood in prim A; prim B five metres east holds the Ears and
        // the Tally, saved in the probe's order.
        let mut region = http_stage(flavour, &[])?;
        region.place(2, 133.0, 128.0, 25.0);
        for (ear, source) in (1_u64..).zip(ears) {
            region.add_unrigged(CallerId(ear), &format!("Ear {}", ear - 1), 2, source)?;
        }
        region.add_unrigged(CallerId(5), "Tally", 2, tally)?;
        region.add_unrigged(CallerId(6), "Flood", 1, flood)?;
        run_for(&mut region, flavour, 300)?;
        let ours = region.lines();
        let theirs = probe_lines(transcript);
        let vary = ["seconds", "slowest", "delays"];
        let shape = |lines: &[String]| -> Vec<String> {
            lines.iter().map(|line| without(line, &vary)).collect()
        };
        assert_eq!(shape(&ours), shape(&theirs), "{flavour:?}");
        // Which calls slept, and how long.
        assert_eq!(
            channel_zero_delays(&ours),
            channel_zero_delays(&theirs),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn the_http_throttle_lets_go_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/throttle/http-window.lsl");
    for (flavour, transcript) in transcripts("http-window") {
        let mut region = http_stage(flavour, &[(A, "Probe", source)])?;
        // Second Life's timer runs late by its frames, so the window's
        // length and each wait's end read later there; what was accepted
        // must not. OpenSim's bucket refills by the clock, so where in the
        // pattern its requests go through follows its timer's phase, which
        // whole ticks of 1/55 s (0.509 s for 0.5) move; how many does not.
        let vary: &[&str] = if flavour.is_opensim() {
            &["seconds", "at", "pattern", "first_after"]
        } else {
            &["seconds", "at"]
        };
        hold(&mut region, flavour, transcript, (400, vary), &[])?;
    }
    Ok(())
}

#[test]
fn a_state_change_probe_hears_what_each_grid_heard() -> Result<(), String> {
    let probe = include_str!("../../../probes/events/state-change/probe.lsl");
    let talker = include_str!("../../../probes/events/state-change/talker.lsl");
    for flavour in [
        EngineFlavour::SecondLife,
        EngineFlavour::YEngine,
        EngineFlavour::XEngine,
    ] {
        // The talker starts first, as the runner saves it; on a grid the
        // probe follows it by round trips of a tenth of a second and more.
        // Started in the same tick, the talker's sixth line falls in the
        // tick of the probe's third timer and is heard after it: five, as
        // the fake grid's loopback runs gave (`accepted.toml`).
        for (offset, heard) in [(5, 6), (0, 5)] {
            let mut region = http_stage(flavour, &[])?;
            region.place(2, 133.0, 128.0, 25.0);
            region.add_unrigged(CallerId(1), "Talker", 2, talker)?;
            for _ in 0..offset {
                region.tick()?;
            }
            region.add_unrigged(CallerId(2), "Probe", 1, probe)?;
            run_for(&mut region, flavour, 20)?;
            let lines: Vec<String> = region
                .lines()
                .into_iter()
                .filter(|line| line.contains(" heard="))
                .collect();
            assert_eq!(
                lines,
                vec![
                    format!("PROBE events/state-change rec=default ticks=3 heard={heard}"),
                    "PROBE events/state-change rec=two ticks=3 heard=0 queued=10 linked=0"
                        .to_owned(),
                    "PROBE events/state-change rec=relistened heard=6".to_owned(),
                ],
                "{flavour:?} {offset}"
            );
        }
    }
    Ok(())
}

#[test]
fn opensims_delay_factor_scales_a_forced_delay_alone() -> Result<(), String> {
    let flavour = EngineFlavour::YEngine;
    let mut engine = Engine::new(EngineConfig {
        script_delay_factor: 0.5,
        ..EngineConfig::for_step(STEP, flavour)
    });
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program_for(
            "default { state_entry() { llModPow(2, 10, 1000); print(\"forced\"); \
             llSleep(1.0); print(\"slept\"); llBase64ToString(\"YWJjZA\"); \
             print(\"refused\"); } }",
            flavour,
        )?,
    );
    let _reports = run(&mut engine, &mut host, 40);
    // Ten ticks to the second: `llModPow`'s second halved, `llSleep`'s and
    // the refusal's (`LSL_Api.Error`) whole.
    assert_eq!(
        host.from(A),
        vec![(6, "forced"), (16, "slept"), (26, "refused")]
    );
    Ok(())
}

#[test]
fn opensim_keeps_a_longer_ban_and_takes_no_sleep_for_it() -> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = engine_as(flavour);
        let mut parcel = super::land::parcel(0, OWNER);
        parcel.bans.push(crate::vm::land::AccessEntry {
            agent: OTHER.to_owned(),
            expires: 0,
        });
        let mut host = Recorder {
            parcel: Some(parcel),
            ..Recorder::default()
        };
        let _none = engine.add(
            A,
            program_for(
                &format!(
                    "default {{ state_entry() {{ llAddToLandBanList(\"{OTHER}\", 1.0); \
                     print(\"banned\"); llAddToLandPassList(\"{OTHER}\", 1.0); \
                     print(\"passed\"); }} }}"
                ),
                flavour,
            )?,
        );
        let _reports = run(&mut engine, &mut host, 10);
        // Ten ticks to the second: a ban for good kept on OpenSim, with no
        // sleep; the pass, which the avatar has none of, made after 0.1 s.
        let (banned, changes) = if flavour.is_opensim() {
            (
                1,
                vec![crate::vm::land::LandChange::Pass(OTHER.to_owned(), 1.0)],
            )
        } else {
            (
                2,
                vec![
                    crate::vm::land::LandChange::Ban(OTHER.to_owned(), 1.0),
                    crate::vm::land::LandChange::Pass(OTHER.to_owned(), 1.0),
                ],
            )
        };
        assert_eq!(
            host.from(A),
            vec![(banned, "banned"), (banned + 1, "passed")],
            "{flavour:?}"
        );
        assert_eq!(host.land_changes, changes, "{flavour:?}");
    }
    Ok(())
}
