//! The linkset data store held to aditi: the probe `data/linkset-data`'s
//! three scripts — the Leader and the Watcher in one linkset, the Stranger in
//! an object of its own — run in the VM, every line they print set against
//! the line aditi printed.
//!
//! The probe talks between its scripts by link message and chat, which this
//! runtime does not have yet, so each script is changed where it does
//! (besides `strings.rs`'s `offline`): `llMessageLinked(LINK_THIS, …)` and
//! `llRegionSay` print what they would send, which the test then delivers —
//! a link message to the prim's other script (the Leader handles none of
//! its own), the chat to the other object's script as a `listen` event —
//! after the tick it
//! was sent in; `llListen` does nothing, and `llGetOwnerKey(id) ==
//! llGetOwner()` is true (every script is the owner's).

use pretty_assertions::assert_eq;

use super::strings::{offline, probe_lines};
use super::*;

/// The Watcher, in the Leader's linkset.
const W: CallerId = CallerId(2);

/// The Stranger, in an object of its own.
const S: CallerId = CallerId(3);

/// `source` offline, with the probe's link messages and chat printed for the
/// test to deliver.
fn rigged(source: &str) -> String {
    let helpers = "linked(integer num, string text, key id)\n\
        {\n    print(\"LINKED \" + (string)num + \" \" + text);\n}\n\
        region_say(integer channel, string text)\n\
        {\n    print(\"SAY \" + (string)channel + \" \" + text);\n}\n\
        integer listen_to(integer channel, string name, key id, string text)\n\
        {\n    return 1;\n}\n\n";
    let source = offline(source, "")
        .replace("llMessageLinked(LINK_THIS, ", "linked(")
        .replace("llRegionSay(", "region_say(")
        .replace("llListen(", "listen_to(")
        .replace("llGetOwnerKey(id) == llGetOwner()", "TRUE");
    source.replacen("default\n{", &format!("{helpers}default\n{{"), 1)
}

/// Deliver what `text`, printed by `from`, says it sent: a link message to
/// the other script of the Leader's prim (the Leader handles none of its
/// own), chat on channel 7 to the other object's script.
fn deliver(engine: &mut Engine, from: CallerId, text: &str) -> Result<(), String> {
    if let Some(rest) = text.strip_prefix("LINKED ") {
        let (num, message) = rest.split_once(' ').unwrap_or((rest, ""));
        let num: i32 = num.parse().map_err(|error| format!("{error}"))?;
        let to = if from == W { A } else { W };
        let _posted = link(engine, to, num, message)?;
    } else if let Some(rest) = text.strip_prefix("SAY 7 ") {
        let to = if from == S { A } else { S };
        let _posted = engine
            .post(
                to,
                named("listen")?,
                vec![
                    Value::Integer(7),
                    Value::String("Object".to_owned()),
                    Value::Key(crate::value::NULL_KEY.to_owned()),
                    Value::String(rest.to_owned()),
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Every `PROBE` line the three scripts print, in the order they print them.
fn run_probe() -> Result<Vec<String>, String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = host.linksets.insert(S, 1);
    let _none = engine.add(
        A,
        program(&rigged(include_str!(
            "../../../probes/data/linkset-data/leader.lsl"
        )))?,
    );
    let _none = engine.add(
        W,
        program(&rigged(include_str!(
            "../../../probes/data/linkset-data/watcher.lsl"
        )))?,
    );
    let _none = engine.add(
        S,
        program(&rigged(include_str!(
            "../../../probes/data/linkset-data/stranger.lsl"
        )))?,
    );
    let mut seen = 0_usize;
    for _ in 0..2_000 {
        let _reports = run(&mut engine, &mut host, 1);
        let sent: Vec<(CallerId, String)> = host
            .printed
            .get(seen..)
            .unwrap_or_default()
            .iter()
            .map(|(_, who, text)| (*who, text.clone()))
            .collect();
        seen = host.printed.len();
        for (from, text) in sent {
            deliver(&mut engine, from, &text)?;
        }
        if host.texts(A).iter().any(|line| line.contains(" rec=end")) {
            break;
        }
    }
    assert!(host.stubbed.is_empty(), "{:?}", host.stubbed);
    Ok(host
        .printed
        .iter()
        .map(|(_, _, text)| text.clone())
        .filter(|line| line.starts_with("PROBE "))
        .collect())
}

/// Whether `line` is one of the Watcher's records.
fn watchers(line: &str) -> bool {
    line.contains(" rec=watcher-")
}

#[test]
fn data_linkset_data_prints_what_aditi_printed() -> Result<(), String> {
    let ours = run_probe()?;
    let aditi = probe_lines(include_str!(
        "../../../probes/results/aditi/data/linkset-data.txt"
    ));
    // The Leader's and the Stranger's records, but for one: the Leader counts
    // the events it gets while it fills the store, and on aditi its own link
    // message telling the Watcher took one of the 64 places of its queue (63
    // events); here no link message reaches the Leader, so 64 do.
    let fill_events = |line: &&String| line.contains(" rec=fill-events ");
    let theirs: Vec<&String> = aditi
        .iter()
        .filter(|line| !watchers(line) && !fill_events(line) && !line.contains(" rec=start "))
        .collect();
    let mine: Vec<&String> = ours
        .iter()
        .filter(|line| !watchers(line) && !fill_events(line) && !line.contains(" rec=start "))
        .collect();
    assert_eq!(mine, theirs);
    assert!(
        ours.iter()
            .any(|line| line.ends_with(" rec=fill-events count=64")),
        "{ours:?}"
    );
    // The Watcher saw every change of the store before the fill, in the
    // order of the calls, as aditi's did. From the fill on what it sees is
    // how far it keeps up (`vary: got`), and here, where the test delivers
    // the Leader's "filling" only after the tick it was sent in, its queue
    // is full of the fill's events before the message comes.
    let before_fill = |line: &&String| {
        line.contains(" rec=watcher-event ")
            && line
                .split(" n=")
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n <= 24)
    };
    let theirs: Vec<&String> = aditi.iter().filter(before_fill).collect();
    let mine: Vec<&String> = ours.iter().filter(before_fill).collect();
    assert_eq!(theirs.len(), 25);
    assert_eq!(mine, theirs);
    Ok(())
}

/// A change raises `linkset_data` in every script of the linkset — the
/// writer too — and in no other object's; a reset after the write in the
/// same event drops the writer's own event and not the others'.
#[test]
fn a_change_reaches_the_linkset_and_no_further() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = host.linksets.insert(S, 1);
    let listener = "default { linkset_data(integer a, string n, string v) { \
        print((string)a + \" \" + n + \" \" + v); } }";
    let _none = engine.add(
        A,
        program(
            "integer once; default { state_entry() { \
            if (llLinksetDataRead(\"k\") == \"\") { llLinksetDataWrite(\"k\", \"v\"); } \
            else if (llLinksetDataRead(\"j\") == \"\") { \
            llLinksetDataWrite(\"j\", \"w\"); llResetScript(); } } \
            linkset_data(integer a, string n, string v) { \
            print((string)a + \" \" + n + \" \" + v); \
            if (!once) { once = TRUE; llLinksetDataWrite(\"x\", \"y\"); } } }",
        )?,
    );
    let _none = engine.add(W, program(listener)?);
    let _none = engine.add(S, program(listener)?);
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(host.texts(A), vec!["1 k v", "1 x y"]);
    assert_eq!(host.texts(W), vec!["1 k v", "1 x y"]);
    assert_eq!(host.texts(S), Vec::<&str>::new());
    // A reset drops what the writer queued for itself; the Watcher still
    // gets it, and the store keeps the pair.
    if let Some(instance) = engine.instance_mut(A) {
        instance.reset();
    }
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(host.texts(A), vec!["1 k v", "1 x y"]);
    assert_eq!(host.texts(W), vec!["1 k v", "1 x y", "1 j w"]);
    assert_eq!(host.linkset_data(A).read("j", ""), "w");
    assert_eq!(host.linkset_data(S).count_keys(), 0);
    Ok(())
}
