//! `llJson*` held to the grids: the probe `data/json` run in the VM under
//! each flavour, every line it prints set against the line the grid printed
//! (see `strings.rs` for how a probe is run offline). Every difference is a
//! named deviation below, with its reason.

use super::strings::{hold, offline, probe_lines, replay};
use super::*;

/// The probe, offline.
fn source() -> String {
    offline(include_str!("../../../probes/data/json.lsl"), "")
}

#[test]
fn data_json_prints_what_aditi_printed() -> Result<(), String> {
    let (_, ours) = replay(&source(), EngineFlavour::SecondLife, 2_000)?;
    let aditi = probe_lines(include_str!("../../../probes/results/aditi/data/json.txt"));
    hold(&ours, &aditi, &[]);
    Ok(())
}

/// What only the value model makes OpenSim's answers differ in: its floats
/// are doubles, ours `f32`, so `llList2Json` of `1.0e20` writes the float's
/// digits (`100000002004087734272.000000`, not `100000000000000000000.000000`),
/// of `-0.0` keeps the sign a double's `ToString` drops, and of `1e39`
/// writes an infinity a double does not overflow to.
const OPENSIM_FLOATS: (&str, &str) = ("list2json", "floats");

/// A lone surrogate, which OpenSim's `\ud83d` decodes to and a Rust string
/// cannot hold: it reads back as U+FFFD, so `llEscapeURL` answers its three
/// bytes where OpenSim's threw (the record prints the exception's text).
const OPENSIM_LONE_SURROGATE: (&str, &str) = ("json2list", "lone-surrogate");

/// The same limit, met by the probe itself: it prints a value one character
/// at a time, and on OpenSim a character is a UTF-16 unit, so an emoji is
/// printed as its two halves — each U+FFFD here, while OpenSim's `llEscapeURL`
/// of a half throws and the probe prints `?`. The values themselves agree:
/// both records' `_esc` fields, the whole string escaped, are OpenSim's.
const OPENSIM_EMOJI_HALVES: [(&str, &str); 2] =
    [("list2json", "strings"), ("json2list", "surrogates")];

/// OpenSim adds `0` to a specifier list it is given that is empty — the
/// caller's own list variable, since an LSL list is a .NET reference there
/// (`llJsonGetValue` of a scalar, `llJsonSetValue` of an empty path). Lists
/// are values in this VM, so the caller's list is never changed; the
/// `shared-list` records, which count the variable's length after the call,
/// read 0 here and 1 there.
const OPENSIM_SHARED_LIST: (&str, &str) = ("shared-list", "");

#[test]
fn data_json_prints_what_opensim_printed() -> Result<(), String> {
    for (flavour, transcript) in [
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/data/json.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/data/json.txt"),
        ),
    ] {
        let (_, ours) = replay(&source(), flavour, 2_000)?;
        hold(
            &ours,
            &probe_lines(transcript),
            &[
                OPENSIM_FLOATS,
                OPENSIM_LONE_SURROGATE,
                OPENSIM_EMOJI_HALVES[0],
                OPENSIM_EMOJI_HALVES[1],
                OPENSIM_SHARED_LIST,
            ],
        );
    }
    Ok(())
}

/// The dialects: OpenSim compiles none of the linkset data functions,
/// constants or the event, and its `JSON_APPEND` is a string.
#[test]
fn opensim_has_no_linkset_data_and_a_string_append() {
    let opensim = |source: &str| crate::compile_for(source, crate::Dialect::OpenSim).is_ok();
    let second_life = |source: &str| crate::compile(source).is_ok();
    let call = "default { state_entry() { integer n = llLinksetDataAvailable(); } }";
    let constant = "default { state_entry() { integer n = LINKSETDATA_OK; } }";
    let handler = "default { linkset_data(integer a, string n, string v) { } }";
    for source in [call, constant, handler] {
        assert!(second_life(source), "{source}");
        assert!(!opensim(source), "{source}");
    }
    let string_append = "default { state_entry() { string s = JSON_APPEND; } }";
    let integer_append = "default { state_entry() { integer i = JSON_APPEND; } }";
    assert!(opensim(string_append) && !opensim(integer_append));
    assert!(second_life(integer_append) && !second_life(string_append));
}
