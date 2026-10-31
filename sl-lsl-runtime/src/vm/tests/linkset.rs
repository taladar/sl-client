//! The linkset data store held to aditi: the probe `data/linkset-data`'s
//! three scripts — the Leader and the Watcher in one prim, the Stranger in
//! an object of its own 3 m away — run in the VM in the comms tests' region,
//! which carries their link messages and chat, every line they print set
//! against the line aditi printed. `llGetOwnerKey(id) == llGetOwner()` is
//! true (every script is the owner's).

use pretty_assertions::assert_eq;

use super::comms::Region;
use super::strings::probe_lines;
use super::*;

/// The Watcher, in the Leader's prim.
const W: CallerId = CallerId(2);

/// The Stranger, in an object of its own.
const S: CallerId = CallerId(3);

/// Every `PROBE` line the three scripts print, in the order they print them.
fn run_probe() -> Result<Vec<String>, String> {
    let owned = |source: &str| source.replace("llGetOwnerKey(id) == llGetOwner()", "TRUE");
    let mut region = Region::new(EngineFlavour::SecondLife);
    let _none = region.host.linksets.insert(S, 1);
    region.place(1, 100.0, 100.0, 30.0);
    region.place(2, 103.0, 100.0, 30.0);
    region.add(
        A,
        "Leader",
        1,
        &owned(include_str!("../../../probes/data/linkset-data/leader.lsl")),
    )?;
    region.add(
        W,
        "Watcher",
        1,
        &owned(include_str!(
            "../../../probes/data/linkset-data/watcher.lsl"
        )),
    )?;
    region.add(
        S,
        "Stranger",
        2,
        &owned(include_str!(
            "../../../probes/data/linkset-data/stranger.lsl"
        )),
    )?;
    for _ in 0..2_000 {
        region.tick()?;
        if region
            .host
            .texts(A)
            .iter()
            .any(|line| line.contains(" rec=end"))
        {
            break;
        }
    }
    assert!(region.host.stubbed.is_empty(), "{:?}", region.host.stubbed);
    Ok(region.lines())
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
    // Every Leader and Stranger record — `fill-events` included: the
    // Leader's own link message telling the Watcher of the fill reaches the
    // Leader too and takes one of the 64 places of its queue, so 63 of the
    // fill's events do, as on aditi.
    let theirs: Vec<&String> = aditi
        .iter()
        .filter(|line| !watchers(line) && !line.contains(" rec=start "))
        .collect();
    let mine: Vec<&String> = ours
        .iter()
        .filter(|line| !watchers(line) && !line.contains(" rec=start "))
        .collect();
    assert_eq!(mine, theirs);
    // The Watcher saw every change of the store before the fill, in the
    // order of the calls, as aditi's did. From the fill on what it sees is
    // how far it keeps up with the Leader's writes (`vary: got`).
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
