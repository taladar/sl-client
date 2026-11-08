//! A script's running state through a take and a rez, and through a region
//! restart, against the acceptance of `server-lsl-script-persistence` and the
//! rows of `book/src/gridspec/lsl.md` § Persistence.
//!
//! The probes `persistence/rerez` and `persistence/restart` are not replayed:
//! they read their script time (`llGetTime`, `llResetTime`), the clock
//! (`llGetUnixTime`), the prim's key and description and a repeating sensor,
//! none of which this VM runs yet (`server-lsl-lib-time-timers`,
//! `server-lsl-lib-prim-state`, `server-lsl-lib-detection-sensors`). The
//! script below is the probe with those taken out, and each test holds one
//! grid's rows of it.

use pretty_assertions::assert_eq;

use super::*;

/// The probe's `Probe` script without the functions this VM does not run: on
/// link message 100 it sets every global, goes into `kept`, starts a
/// one-second timer and a listen on 7 and asks the owner for 1040; granted,
/// it prints what it holds, sends itself three link messages and sleeps
/// eight seconds. Going into `kept`, it stops `Stopped` with
/// `llSetScriptState`.
fn probe() -> String {
    format!(
        r#"integer gi;
float gf;
string gs;
key gk;
vector gv;
rotation gr;
list gl;
integer ticks;

string held()
{{
    return "gi=" + (string)gi + " gs=" + gs + " gl=" + llList2CSV(gl)
        + " perms=" + (string)llGetPermissions()
        + " granter=" + (string)llGetPermissionsKey()
        + " stopped=" + (string)llGetScriptState("Stopped");
}}

default
{{
    state_entry()
    {{
        print("entry " + held());
    }}

    on_rez(integer param)
    {{
        print("on_rez default " + held());
    }}

    link_message(integer sender, integer number, string text, key id)
    {{
        if (number == 100)
        {{
            gi = 42;
            gf = 2.5;
            gs = "kept";
            gk = "{OWNER}";
            gv = <1.0, 2.0, 3.0>;
            gr = <0.0, 0.0, 0.70710677, 0.70710677>;
            gl = [1, "two", 3.0];
            state kept;
        }}
    }}
}}

state kept
{{
    state_entry()
    {{
        llSetScriptState("Stopped", FALSE);
        llListen(7, "", NULL_KEY, "");
        llSetTimerEvent(1.0);
        llRequestPermissions("{OWNER}", 1040);
    }}

    run_time_permissions(integer perms)
    {{
        print("before " + held());
        llMessageLinked(LINK_THIS, 1, "queued", NULL_KEY);
        llMessageLinked(LINK_THIS, 2, "queued", NULL_KEY);
        llMessageLinked(LINK_THIS, 3, "queued", NULL_KEY);
        llSleep(8.0);
        print("woke");
    }}

    on_rez(integer param)
    {{
        print("on_rez kept " + held());
    }}

    link_message(integer sender, integer number, string text, key id)
    {{
        if (number < 10)
        {{
            print("link " + (string)number);
        }}
    }}

    listen(integer channel, string name, key id, string message)
    {{
        print("heard " + message);
    }}

    touch_start(integer count)
    {{
        print("touched by " + llDetectedName(0));
    }}

    timer()
    {{
        ++ticks;
    }}

    changed(integer change)
    {{
        print("changed " + (string)change);
    }}
}}
"#
    )
}

/// The probe's `Stopped` script, which the probe stops.
const STOPPED: &str = r#"default
{
    state_entry()
    {
        print("stopped entry");
    }

    on_rez(integer param)
    {
        print("stopped on_rez");
    }
}
"#;

/// What `held()` prints of a script that kept everything, with `Stopped`
/// still stopped.
fn kept_held() -> String {
    format!("gi=42 gs=kept gl=1, two, 3.000000 perms=1040 granter={OWNER} stopped=0")
}

/// A region imitating `flavour`, merging events as that grid does.
fn region(flavour: EngineFlavour) -> Engine {
    let mut config = EngineConfig::for_step(STEP);
    config.flavour = flavour;
    if flavour.is_opensim() {
        config.coalescing = Coalescing::Never;
    }
    Engine::new(config)
}

/// The probe set up in a region imitating `flavour`: `A` is the probe and
/// `B` the script it stops, both in one prim, and the probe has been asleep
/// two seconds of its eight when this returns.
fn asleep(flavour: EngineFlavour) -> Result<(Engine, Recorder), String> {
    let mut engine = region(flavour);
    let mut host = Recorder {
        scripts: [("Stopped".to_owned(), B)].into_iter().collect(),
        prims: [(A, 1), (B, 1)].into_iter().collect(),
        ..Recorder::default()
    };
    let _none = engine.add(A, program_for(&probe(), flavour)?);
    let _none = engine.add(B, program_for(STOPPED, flavour)?);
    let _reports = run(&mut engine, &mut host, 2);
    assert_eq!(link(&mut engine, A, 100, "")?, Some(Posted::Queued));
    let _reports = run(&mut engine, &mut host, 2);
    assert_eq!(host.questions.len(), 1, "the probe asked the owner");
    assert_eq!(engine.answer(A, OWNER, 1040), Ok(Some(Posted::Queued)));
    let _reports = run(&mut engine, &mut host, 20);
    assert_eq!(
        host.texts(A).last().copied(),
        Some(format!("before {}", kept_held()).as_str()),
        "{:?}",
        host.said
    );
    let probe = engine.instance(A).ok_or("no probe")?;
    assert!(probe.is_busy(), "the probe sleeps in its handler");
    assert!(!engine.instance(B).is_some_and(Instance::is_running));
    Ok((engine, host))
}

/// What `id` printed after `tick`, with the tick of each.
fn since(host: &Recorder, id: CallerId, tick: u64) -> Vec<(u64, String)> {
    host.from(id)
        .into_iter()
        .filter(|(at, _)| *at > tick)
        .map(|(at, text)| (at, text.to_owned()))
        .collect()
}

/// The texts of [`since`].
fn texts_since(host: &Recorder, id: CallerId, tick: u64) -> Vec<String> {
    since(host, id, tick)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

/// Take the prim — both scripts — and rez it again `away` later, the probe's
/// saved state through its encoded form. Returns the tick of the rez.
fn rerez(
    engine: &mut Engine,
    flavour: EngineFlavour,
    away: Duration,
) -> Result<(u64, Posted, Posted), String> {
    let taken = engine.take(A).ok_or("no probe to take")?;
    let stopped = engine.take(B).ok_or("no stopped script to take")?;
    assert!(
        engine.instance(A).is_none(),
        "a taken script leaves the region"
    );
    let taken = SavedScript::decode(&taken.encode()).map_err(|error| error.to_string())?;
    let rezzed = engine.now().0;
    let posted_stopped = engine
        .rez_saved(B, program_for(STOPPED, flavour)?, &stopped, away, 0)
        .map_err(|error| error.to_string())?;
    let posted_probe = engine
        .rez_saved(A, program_for(&probe(), flavour)?, &taken, away, 0)
        .map_err(|error| error.to_string())?;
    Ok((rezzed, posted_probe, posted_stopped))
}

/// A line on channel 7 from the owner.
fn after() -> Heard {
    Heard {
        channel: 7,
        name: "Owner".to_owned(),
        speaker: OWNER.to_owned(),
        message: "after".to_owned(),
    }
}

#[test]
fn second_life_keeps_everything_and_runs_on_rez_before_the_queue() -> Result<(), String> {
    let flavour = EngineFlavour::SecondLife;
    let (mut engine, mut host) = asleep(flavour)?;
    let (rezzed, probe, stopped) = rerez(&mut engine, flavour, Duration::from_secs(3))?;
    assert_eq!((probe, stopped), (Posted::Queued, Posted::Stopped));
    let _reports = run(&mut engine, &mut host, 40);
    // Eight seconds asleep, the three away among them.
    let slept = host
        .from(A)
        .into_iter()
        .find(|(_, text)| text.starts_with("before"))
        .map(|(tick, _)| tick)
        .ok_or("the probe never slept")?;
    assert_eq!(
        since(&host, A, rezzed).first(),
        Some(&(slept.saturating_add(50), "woke".to_owned()))
    );
    assert_eq!(
        texts_since(&host, A, rezzed),
        vec![
            "woke".to_owned(),
            format!("on_rez kept {}", kept_held()),
            "link 1".to_owned(),
            "link 2".to_owned(),
            "link 3".to_owned(),
        ]
    );
    assert_eq!(texts_since(&host, B, 0), vec!["stopped entry"]);
    assert_eq!(engine.hear(A, &after()), Ok(Some(vec![Posted::Queued])));
    let ticks = engine
        .instance(A)
        .and_then(|probe| probe.global("ticks"))
        .cloned();
    let _reports = run(&mut engine, &mut host, 20);
    assert_eq!(
        texts_since(&host, A, rezzed).last().map(String::as_str),
        Some("heard after")
    );
    pretty_assertions::assert_ne!(
        engine
            .instance(A)
            .and_then(|probe| probe.global("ticks"))
            .cloned(),
        ticks,
        "the timer ticks on"
    );
    Ok(())
}

#[test]
fn yengine_ends_the_sleep_at_the_take_and_runs_the_queue_before_on_rez() -> Result<(), String> {
    let flavour = EngineFlavour::YEngine;
    let (mut engine, mut host) = asleep(flavour)?;
    let (rezzed, _, stopped) = rerez(&mut engine, flavour, Duration::from_secs(3))?;
    assert_eq!(stopped, Posted::Stopped);
    let _reports = run(&mut engine, &mut host, 10);
    assert_eq!(
        since(&host, A, rezzed).first(),
        Some(&(rezzed.saturating_add(1), "woke".to_owned()))
    );
    assert_eq!(
        texts_since(&host, A, rezzed),
        vec![
            "woke".to_owned(),
            "link 1".to_owned(),
            "link 2".to_owned(),
            "link 3".to_owned(),
            format!("on_rez kept {}", kept_held()),
        ]
    );
    assert_eq!(texts_since(&host, B, 0), vec!["stopped entry"]);
    assert_eq!(engine.hear(A, &after()), Ok(Some(vec![Posted::Queued])));
    Ok(())
}

#[test]
fn xengine_resets_both_scripts_on_the_rez() -> Result<(), String> {
    let flavour = EngineFlavour::XEngine;
    let (mut engine, mut host) = asleep(flavour)?;
    let (rezzed, _, _) = rerez(&mut engine, flavour, Duration::from_secs(3))?;
    let _reports = run(&mut engine, &mut host, 10);
    let fresh = format!(
        "gi=0 gs= gl= perms=0 granter={} stopped=1",
        crate::value::NULL_KEY
    );
    assert_eq!(
        texts_since(&host, A, rezzed),
        vec![format!("entry {fresh}"), format!("on_rez default {fresh}")]
    );
    assert_eq!(
        texts_since(&host, B, rezzed),
        vec!["stopped entry", "stopped on_rez"]
    );
    assert!(engine.instance(B).is_some_and(Instance::is_running));
    assert_eq!(
        engine.hear(A, &after()),
        Ok(Some(Vec::new())),
        "the reset closed the listen"
    );
    Ok(())
}

#[test]
fn a_restart_keeps_the_scripts_and_raises_region_start() -> Result<(), String> {
    let link_lines = ["link 1", "link 2", "link 3"].map(str::to_owned);
    for (flavour, woke, changes) in [
        (EngineFlavour::SecondLife, true, vec!["changed 1024"]),
        (
            EngineFlavour::YEngine,
            true,
            vec!["changed 32768", "changed 1024"],
        ),
        (
            EngineFlavour::XEngine,
            false,
            vec!["changed 32768", "changed 1024"],
        ),
    ] {
        let (mut engine, mut host) = asleep(flavour)?;
        let saved = engine.shut_down();
        assert!(
            engine.instance(A).is_none(),
            "{flavour:?}: the region is empty"
        );
        let scripts = saved
            .into_iter()
            .map(|(id, saved)| {
                let source = if id == A { probe() } else { STOPPED.to_owned() };
                program_for(&source, flavour).map(|program| (id, program, saved))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let restarted = engine.now().0;
        engine
            .start_up(scripts, Duration::from_secs(20))
            .map_err(|(id, error)| format!("{id:?}: {error}"))?;
        let _reports = run(&mut engine, &mut host, 10);
        let mut expected: Vec<String> = Vec::new();
        if woke {
            expected.push("woke".to_owned());
        }
        expected.extend(link_lines.iter().cloned());
        expected.extend(changes.into_iter().map(str::to_owned));
        assert_eq!(texts_since(&host, A, restarted), expected, "{flavour:?}");
        assert_eq!(
            texts_since(&host, B, restarted),
            Vec::<String>::new(),
            "{flavour:?}"
        );
        assert_eq!(
            engine.instance(B).is_some_and(Instance::is_running),
            flavour == EngineFlavour::XEngine,
            "{flavour:?}: only XEngine runs the stopped script again"
        );
        let probe = engine.instance(A).ok_or("no probe")?;
        assert_eq!(probe.global("gi"), Some(&Value::Integer(42)), "{flavour:?}");
        assert_eq!(probe.permissions().mask(), 1040, "{flavour:?}");
        assert!(probe.has_timer(), "{flavour:?}");
        assert_eq!(engine.hear(A, &after()), Ok(Some(vec![Posted::Queued])));
    }
    Ok(())
}

/// A timer of five seconds set at the first tick, taken two seconds later
/// and rezzed ten seconds after that: the tick it next fires at, counted
/// from the rez.
fn timer_after_rerez(flavour: EngineFlavour) -> Result<u64, String> {
    const SOURCE: &str = r#"default
{
    state_entry()
    {
        llSetTimerEvent(5.0);
    }

    timer()
    {
        print("timer");
    }
}
"#;
    let mut engine = region(flavour);
    let mut host = Recorder::default();
    let _none = engine.add(A, program_for(SOURCE, flavour)?);
    let _reports = run(&mut engine, &mut host, 20);
    let saved = engine.take(A).ok_or("no script to take")?;
    let rezzed = engine.now().0;
    let _posted = engine
        .rez_saved(
            A,
            program_for(SOURCE, flavour)?,
            &saved,
            Duration::from_secs(10),
            0,
        )
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 60);
    since(&host, A, rezzed)
        .first()
        .map(|(tick, _)| tick.saturating_sub(rezzed))
        .ok_or_else(|| "the timer never fired".to_owned())
}

#[test]
fn the_time_away_counts_against_the_timer_on_second_life_only() -> Result<(), String> {
    // Set at tick 1, due at 51, taken at 20: 31 ticks left.
    assert_eq!(timer_after_rerez(EngineFlavour::SecondLife)?, 1);
    assert_eq!(timer_after_rerez(EngineFlavour::YEngine)?, 31);
    Ok(())
}

#[test]
fn a_deliberate_reset_is_what_loses_the_globals() -> Result<(), String> {
    const SOURCE: &str = r"integer g = 1;
default
{
    link_message(integer sender, integer number, string text, key id)
    {
        if (number == 1) g = 5;
        else llResetScript();
    }
}
";
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(SOURCE)?);
    let _reports = run(&mut engine, &mut host, 2);
    let _posted = link(&mut engine, A, 1, "")?;
    let _reports = run(&mut engine, &mut host, 2);
    let kept = engine.take(A).ok_or("no script to take")?;
    assert_eq!(kept.globals(), [Value::Integer(5)]);
    let _posted = engine
        .rez_saved(A, program(SOURCE)?, &kept, Duration::ZERO, 0)
        .map_err(|error| error.to_string())?;
    let _posted = link(&mut engine, A, 2, "")?;
    let _reports = run(&mut engine, &mut host, 2);
    let reset = engine.take(A).ok_or("no script to take")?;
    assert_eq!(reset.globals(), [Value::Integer(1)]);
    Ok(())
}

#[test]
fn the_saved_form_round_trips_every_part_of_a_script() -> Result<(), String> {
    let (mut engine, _host) = asleep(EngineFlavour::SecondLife)?;
    assert_eq!(
        engine.post_detected(A, named("touch_start")?, vec![toucher("Toucher")]),
        Ok(Some(Posted::Queued))
    );
    let saved = engine.take(A).ok_or("no probe to take")?;
    assert!(saved.is_busy());
    assert!(saved.has_timer());
    assert_eq!(
        saved.queued(),
        5,
        "three link messages, a timer and a touch"
    );
    assert_eq!(saved.listens().len(), 1);
    assert_eq!(saved.permissions().granter(), Some(OWNER));
    assert!(
        saved.asleep() > Duration::from_secs(5),
        "{:?}",
        saved.asleep()
    );
    let bytes = saved.encode();
    assert_eq!(SavedScript::decode(&bytes), Ok(saved));
    Ok(())
}

#[test]
fn every_value_survives_the_saved_form_a_nan_included() -> Result<(), String> {
    const SOURCE: &str = r#"integer i = -7;
float nan;
float inf;
string s = "tëxt";
key k = "0a0a0a0a-0000-4000-8000-000000000001";
vector v = <1.5, -2.0, 3.25>;
rotation r = <0.0, 0.0, 0.70710677, 0.70710677>;
list l = [1, 2.5, "three", <4.0, 5.0, 6.0>, <0.0, 0.0, 0.0, 1.0>];
default
{
    state_entry()
    {
        inf = (float)"inf";
        nan = inf - inf;
        l += [(key)k];
    }
}
"#;
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(SOURCE)?);
    let _reports = run(&mut engine, &mut host, 2);
    let saved = engine.take(A).ok_or("no script to take")?;
    assert!(
        matches!(saved.globals().get(1), Some(Value::Float(nan)) if nan.is_nan()),
        "{:?}",
        saved.globals()
    );
    let bytes = saved.encode();
    let decoded = SavedScript::decode(&bytes).map_err(|error| error.to_string())?;
    // A NaN is not equal to itself, so the comparison is of the bytes.
    assert_eq!(decoded.encode(), bytes);
    let globals = decoded.globals();
    assert_eq!(globals.get(3), Some(&Value::String("tëxt".to_owned())));
    assert!(matches!(globals.get(2), Some(Value::Float(inf)) if inf.is_infinite()));
    Ok(())
}

#[test]
fn bytes_that_are_not_a_saved_script_are_refused() -> Result<(), String> {
    let (mut engine, _host) = asleep(EngineFlavour::SecondLife)?;
    let bytes = engine.take(A).ok_or("no probe to take")?.encode();
    assert_eq!(SavedScript::decode(b"nope"), Err(DecodeError::Mark));
    let mut later = bytes.clone();
    if let Some(version) = later.get_mut(4) {
        *version = 6;
    }
    assert_eq!(SavedScript::decode(&later), Err(DecodeError::Version(6)));
    let cut = bytes
        .get(..bytes.len().saturating_sub(1))
        .unwrap_or_default();
    assert_eq!(SavedScript::decode(cut), Err(DecodeError::Truncated));
    let mut longer = bytes;
    longer.push(0);
    assert_eq!(SavedScript::decode(&longer), Err(DecodeError::Trailing(1)));
    Ok(())
}

#[test]
fn a_saved_script_is_not_brought_back_with_another_program_or_engine() -> Result<(), String> {
    let (mut engine, _host) = asleep(EngineFlavour::SecondLife)?;
    let saved = engine.take(A).ok_or("no probe to take")?;
    assert_eq!(
        engine.rez_saved(
            A,
            program("default { state_entry() {} }")?,
            &saved,
            Duration::ZERO,
            0
        ),
        Err(RestoreError::Globals)
    );
    assert!(engine.instance(A).is_none(), "nothing is added");
    let mut elsewhere = region(EngineFlavour::YEngine);
    assert_eq!(
        elsewhere.rez_saved(A, program(&probe())?, &saved, Duration::ZERO, 0),
        Err(RestoreError::Flavour {
            saved: EngineFlavour::SecondLife,
            region: EngineFlavour::YEngine,
        })
    );
    Ok(())
}

/// The script time a script reads after two seconds in a region, a take, a
/// rez three seconds later and one more tick, under `flavour`.
fn script_time_after_rerez(flavour: EngineFlavour) -> Result<String, String> {
    let source = "default { link_message(integer s, integer n, string m, key i) { print((string)llGetTime()); } }";
    let mut engine = region(flavour);
    let mut host = Recorder::default();
    let _none = engine.add(A, program_for(source, flavour)?);
    let _reports = run(&mut engine, &mut host, 20);
    let taken = engine.take(A).ok_or("no script to take")?;
    let taken = SavedScript::decode(&taken.encode()).map_err(|error| error.to_string())?;
    let _posted = engine
        .rez_saved(
            A,
            program_for(source, flavour)?,
            &taken,
            Duration::from_secs(3),
            0,
        )
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = link(&mut engine, A, 0, "")?;
    let _reports = run(&mut engine, &mut host, 1);
    host.texts(A)
        .last()
        .map(|text| (*text).to_owned())
        .ok_or_else(|| "the script printed nothing".to_owned())
}

#[test]
fn script_time_is_kept_through_a_take_but_not_on_xengine() -> Result<(), String> {
    // `persistence/rerez`: script time counts on through a take and a rez on
    // Second Life and YEngine, the time away not counted; XEngine starts it
    // again with the rez's reset.
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        assert_eq!(script_time_after_rerez(flavour)?, "2.200000", "{flavour:?}");
    }
    assert_eq!(script_time_after_rerez(EngineFlavour::XEngine)?, "0.200000");
    Ok(())
}

/// A worn object's script that travels with its wearer keeps what it held and
/// hears `changed(CHANGED_REGION | CHANGED_TELEPORT)` on Second Life and
/// YEngine; XEngine runs it again from `state_entry`, its globals lost
/// (`events/changed-worn`, 2026-11-03).
#[test]
fn a_travelling_script_hears_its_arrival_as_its_engine_tells_it() -> Result<(), String> {
    const SOURCE: &str = r#"integer g = 1;
default
{
    state_entry()
    {
        print("entry " + (string)g);
    }

    link_message(integer sender, integer number, string text, key id)
    {
        g = number;
    }

    changed(integer change)
    {
        print("changed " + (string)change + " " + (string)g);
    }
}
"#;
    let change = crate::vm::CHANGED_REGION | crate::vm::CHANGED_TELEPORT;
    for (flavour, expected) in [
        (EngineFlavour::SecondLife, "changed 768 5"),
        (EngineFlavour::YEngine, "changed 768 5"),
        (EngineFlavour::XEngine, "entry 1"),
    ] {
        let mut engine = region(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program_for(SOURCE, flavour)?);
        let _reports = run(&mut engine, &mut host, 2);
        let _posted = link(&mut engine, A, 5, "")?;
        let _reports = run(&mut engine, &mut host, 2);
        let saved = engine.take(A).ok_or("no script to take")?;
        let arrived = engine.now().0;
        let _posted = engine
            .travel_saved(
                A,
                program_for(SOURCE, flavour)?,
                &saved,
                Duration::ZERO,
                change,
            )
            .map_err(|error| error.to_string())?;
        let _reports = run(&mut engine, &mut host, 4);
        assert_eq!(
            texts_since(&host, A, arrived),
            vec![expected.to_owned()],
            "{flavour:?}"
        );
    }
    Ok(())
}
