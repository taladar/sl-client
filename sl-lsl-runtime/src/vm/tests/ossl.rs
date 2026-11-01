//! OpenSim's own library held to the grids: `ossl/available` and
//! `ossl/errors` run in the VM under each flavour, in the comms test's region,
//! against the region's stock `[OSSL]` configuration and an owner who is
//! neither estate owner nor manager, as on the local OpenSim; every `PROBE`
//! line and every `DEBUG_CHANNEL` line the runner noted is checked. Second
//! Life compiles neither probe, as aditi did not.
//!
//! What the probes call of a tranche not written yet is answered where they
//! call it, with what the grid answered: the avatar's gender and health, the
//! rezzing object, the link number, the inventory item, the map texture,
//! the wind and LightShare's water colour, the apparent time and the
//! wallclock (each as its transcript has it), `llGetTime` (the `elapsed` the
//! probe prints is compared as present, not as a number), what a listener
//! reads of where the speaker is, and `llResetOtherScript`, which the test
//! does for the leader. Keys minted on either side compare as keys.
//!
//! One line differs, by the value model's rule rather than OSSL's:
//! `(string)` of `osGetRegionSize`'s vector writes Second Life's five
//! decimals on every grid, where OpenSim writes six (`EngineFlavour`).

use pretty_assertions::assert_eq;

use super::comms::{Region, canonical, noted_debug};
use super::strings::probe_lines;
use super::*;
use crate::library::ThreatLevel;
use crate::vm::ossl::Settings;

/// The local OpenSim as the probes saw it.
fn local_opensim() -> Simulator {
    Simulator {
        version: "OpenSim 0.9.2.1 Yeti Dev   36f6d16 (Unix/Mono)".to_owned(),
        grid_name: "the lost continent of hippo".to_owned(),
        grid_nick: "hippogrid".to_owned(),
        physics_engine_type: "ubODE".to_owned(),
        physics_engine_name: "ubODE 1.0-0.13.2".to_owned(),
    }
}

/// The three grids' transcripts of the probe `ossl/<name>`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let files = match name {
        "available" => [
            include_str!("../../../probes/results/aditi/ossl/available.txt"),
            include_str!("../../../probes/results/opensim-yengine/ossl/available.txt"),
            include_str!("../../../probes/results/opensim-xengine/ossl/available.txt"),
        ],
        "strings" => [
            include_str!("../../../probes/results/aditi/ossl/strings.txt"),
            include_str!("../../../probes/results/opensim-yengine/ossl/strings.txt"),
            include_str!("../../../probes/results/opensim-xengine/ossl/strings.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/ossl/errors.txt"),
            include_str!("../../../probes/results/opensim-yengine/ossl/errors.txt"),
            include_str!("../../../probes/results/opensim-xengine/ossl/errors.txt"),
        ],
    };
    let [aditi, yengine, xengine] = files;
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// The value of `key=` in the transcript's `fn=<function>` line, as printed.
fn printed(transcript: &str, function: &str, key: &str) -> String {
    probe_lines(transcript)
        .iter()
        .find(|line| line.contains(&format!(" fn={function} ")))
        .and_then(|line| {
            let start = line.find(&format!(" {key}="))?;
            line.get(start.saturating_add(key.len()).saturating_add(2)..)
                .map(|rest| rest.split(' ').next().unwrap_or_default().to_owned())
        })
        .unwrap_or_default()
}

/// `line` with every key written `<key>` and every `elapsed=` value `*`.
fn normalised(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("elapsed=") {
        out.push_str(rest.get(..at).unwrap_or_default());
        out.push_str("elapsed=*");
        let after = rest.get(at.saturating_add(8)..).unwrap_or_default();
        rest = after
            .get(after.find(' ').unwrap_or(after.len())..)
            .unwrap_or_default();
    }
    out.push_str(rest);
    let mut keyless = String::new();
    let characters: Vec<char> = out.chars().collect();
    let mut index = 0_usize;
    while let Some(character) = characters.get(index) {
        let candidate: String = characters
            .get(index..index.saturating_add(36))
            .map(|window| window.iter().collect())
            .unwrap_or_default();
        if crate::library::dotnet::is_uuid(&candidate) {
            keyless.push_str("<key>");
            index = index.saturating_add(36);
        } else {
            keyless.push(*character);
            index = index.saturating_add(1);
        }
    }
    keyless
}

#[test]
fn available_answers_as_each_engine_did() -> Result<(), String> {
    let source = include_str!("../../../probes/ossl/available.lsl");
    for (flavour, transcript) in transcripts("available") {
        let rigged = source
            .replace("osGetGender(owner)", "\"female\"")
            .replace("osGetHealth(owner)", "100.0")
            .replace("osGetRezzingObject()", "((key)NULL_KEY)")
            .replace("osWindActiveModelPluginName()", "\"SimpleRandomWind\"")
            .replace("osGetWindParam(\"SimpleRandomWind\", \"strength\")", "1.0")
            .replace("osIsNpc(owner)", "0")
            .replace("osGetLinkNumber(llGetObjectName())", "-1")
            .replace(
                "(string)osGetMapTexture()",
                &format!(
                    "\"{}\"",
                    printed(transcript, "osGetMapTexture", "map_texture")
                ),
            )
            .replace(
                "osGetApparentTimeString(TRUE)",
                &printed(transcript, "osGetApparentTimeString", "apparent"),
            )
            .replace(
                "(string)osGetPSTWallclock()",
                &format!(
                    "\"{}\"",
                    printed(transcript, "osGetPSTWallclock", "wallclock")
                ),
            )
            .replace("osGetInventoryItemKey(\"nope\")", "((key)NULL_KEY)");
        let mut region = Region::new(flavour);
        region.host.simulator = local_opensim();
        let added = region.add(A, "Probe", 1, &rigged);
        if !flavour.is_opensim() {
            assert!(transcript.contains("# compile error in \"Probe\""));
            assert!(added.is_err(), "Second Life compiled ossl/available");
            continue;
        }
        added?;
        for _ in 0..200 {
            region.tick()?;
            if region.ended() {
                break;
            }
        }
        let deviating = |line: &String| line.contains(" fn=osGetRegionSize ");
        let (ours_deviating, ours): (Vec<String>, Vec<String>) = canonical(&region.lines(), &[])
            .into_iter()
            .partition(deviating);
        let (theirs_deviating, theirs): (Vec<String>, Vec<String>) =
            canonical(&probe_lines(transcript), &[])
                .into_iter()
                .partition(deviating);
        assert_eq!(ours, theirs, "{flavour:?}");
        assert_eq!(
            ours_deviating,
            vec!["PROBE ossl/available rec=always fn=osGetRegionSize value=\"<256.00000, 256.00000, 0.00000>\"".to_owned()],
            "{flavour:?}"
        );
        assert_eq!(
            theirs_deviating,
            vec!["PROBE ossl/available rec=always fn=osGetRegionSize value=\"<256.000000, 256.000000, 0.000000>\"".to_owned()],
            "{flavour:?}"
        );
        assert_eq!(region.debug_lines(), Vec::<String>::new(), "{flavour:?}");
    }
    Ok(())
}

/// The `ossl/errors` scripts in their saving order: the script's name, its
/// prim, its source.
const ERRORS: [(&str, u64, &str); 11] = [
    (
        "Threat",
        1,
        include_str!("../../../probes/ossl/errors/threat.lsl"),
    ),
    (
        "Listed",
        1,
        include_str!("../../../probes/ossl/errors/listed.lsl"),
    ),
    (
        "Disabled",
        1,
        include_str!("../../../probes/ossl/errors/disabled.lsl"),
    ),
    (
        "Runtime",
        1,
        include_str!("../../../probes/ossl/errors/runtime.lsl"),
    ),
    (
        "Deprecated",
        1,
        include_str!("../../../probes/ossl/errors/deprecated.lsl"),
    ),
    (
        "Shout",
        1,
        include_str!("../../../probes/ossl/errors/shout.lsl"),
    ),
    (
        "LightShare",
        1,
        include_str!("../../../probes/ossl/errors/lightshare.lsl"),
    ),
    (
        "Mod",
        1,
        include_str!("../../../probes/ossl/errors/mod.lsl"),
    ),
    (
        "Listener near",
        3,
        include_str!("../../../probes/ossl/errors/listener-near.lsl"),
    ),
    (
        "Listener far",
        2,
        include_str!("../../../probes/ossl/errors/listener-far.lsl"),
    ),
    (
        "Leader",
        1,
        include_str!("../../../probes/ossl/errors/leader.lsl"),
    ),
];

#[test]
fn errors_are_thrown_and_shouted_as_each_engine_did() -> Result<(), String> {
    for (flavour, transcript) in transcripts("errors") {
        let mut region = Region::new(flavour);
        region.host.simulator = local_opensim();
        region.place(1, 120.0, 128.0, 27.5);
        region.place(2, 170.0, 128.0, 27.5);
        region.place(3, 130.0, 128.0, 27.5);
        let mut ids: Vec<(String, CallerId)> = Vec::new();
        let mut refused = None;
        for (index, (name, prim, source)) in ERRORS.into_iter().enumerate() {
            let distance = if prim == 2 { "50.000000" } else { "10.000000" };
            let rigged = source
                .replace("float t = llGetTime();", "float t = 0.0;")
                .replace("(string)(llGetTime() - t)", "\"*\"")
                .replace(
                    "llResetOtherScript(\"Threat\");",
                    "print(\"RESET Threat\");",
                )
                .replace(
                    "llList2CSV(lsGetWindlightScene([WL_WATER_COLOR]))",
                    "llList2CSV([0, <4.0, 38.0, 64.0>])",
                )
                .replace(
                    "vector at = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);",
                    "",
                )
                .replace(
                    "(string)llVecDist(at, llGetPos())",
                    &format!("\"{distance}\""),
                )
                .replace("(llGetOwnerKey(id) == llGetOwner())", "TRUE");
            let id = CallerId(u64::try_from(index).unwrap_or(0).saturating_add(1));
            match region.add(id, name, prim, &rigged) {
                Ok(()) => ids.push((name.to_owned(), id)),
                Err(error) => {
                    refused.get_or_insert((name, error));
                }
            }
        }
        if !flavour.is_opensim() {
            // Aditi stopped at the first script it could not compile.
            assert!(transcript.contains("compile error"), "{flavour:?}");
            assert!(
                matches!(refused, Some(("Threat", _))),
                "Second Life compiled ossl/errors: {refused:?}"
            );
            continue;
        }
        if let Some(refused) = refused {
            return Err(format!("{flavour:?} refused {refused:?}"));
        }
        let mut resets = 0_usize;
        for _ in 0..2_000 {
            region.tick()?;
            let asked: Vec<String> = region
                .host
                .printed
                .iter()
                .filter_map(|(_, _, text)| text.strip_prefix("RESET ").map(str::to_owned))
                .collect();
            for name in asked.iter().skip(resets) {
                let id = ids
                    .iter()
                    .find(|(script, _)| script == name)
                    .map(|(_, id)| *id)
                    .ok_or_else(|| format!("no script {name}"))?;
                region
                    .engine
                    .instance_mut(id)
                    .ok_or("the reset script is gone")?
                    .reset();
            }
            resets = asked.len();
            if region.ended() {
                break;
            }
        }
        let ours: Vec<String> = canonical(&region.lines(), &["answer", "heard"])
            .iter()
            .map(|line| normalised(line))
            .collect();
        let theirs: Vec<String> = canonical(&probe_lines(transcript), &["answer", "heard"])
            .iter()
            .map(|line| normalised(line))
            .collect();
        assert_eq!(ours, theirs, "{flavour:?}");
        let said: Vec<String> = region
            .debug_lines()
            .iter()
            .map(|line| normalised(line))
            .collect();
        let noted: Vec<String> = noted_debug(transcript)
            .iter()
            .map(|line| normalised(line))
            .collect();
        assert_eq!(said, noted, "{flavour:?}");
    }
    Ok(())
}

/// A script that calls `call` as it starts and prints what it returned.
fn caller(call: &str) -> String {
    format!("default {{ state_entry() {{ print(\"got \" + (string)({call})); }} }}")
}

/// Run `call` in a YEngine region under `config` for an owner standing as
/// `standing`; what it printed and what was said on `DEBUG_CHANNEL`.
fn called(
    call: &str,
    config: Settings,
    standing: Standing,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut region = Region::new(EngineFlavour::YEngine);
    region.engine.set_ossl(config);
    region.host.standing = standing;
    region.host.simulator = local_opensim();
    region.add(A, "Caller", 1, &caller(call))?;
    region.tick()?;
    for _ in 0..30 {
        region.tick()?;
    }
    let printed = region
        .host
        .printed
        .iter()
        .map(|(_, _, text)| text.clone())
        .collect();
    Ok((printed, region.debug_lines()))
}

/// A configuration of `[OSSL]` lines.
fn config(lines: &str) -> Settings {
    Settings::from_ini(&format!("[OSSL]\n{lines}"))
}

#[test]
fn an_allow_line_overrides_the_threat_level_both_ways() -> Result<(), String> {
    // osGetScriptEngineName is High: over VeryLow it is denied by level...
    let (printed, said) = called("osGetScriptEngineName()", config(""), Standing::default())?;
    assert!(printed.is_empty());
    assert_eq!(
        said.first().map(|line| line
            .split(" (script:")
            .next()
            .unwrap_or_default()
            .to_owned()),
        Some(
            "YEngine: OSSL Permission Error: osGetScriptEngineName permission denied.  Allowed \
             threat level is VeryLow but function threat level is High."
                .to_owned()
        )
    );
    // ...allowed by `true`, and by a level that reaches it.
    let (printed, _) = called(
        "osGetScriptEngineName()",
        config("Allow_osGetScriptEngineName = true"),
        Standing::default(),
    )?;
    assert_eq!(printed, vec!["got YEngine".to_owned()]);
    let (printed, _) = called(
        "osGetScriptEngineName()",
        config("OSFunctionThreatLevel = High"),
        Standing::default(),
    )?;
    assert_eq!(printed, vec!["got YEngine".to_owned()]);
    // osGetAgents is None, under VeryLow, yet a list it is not on denies it.
    let list = "Allow_osGetAgents = ESTATE_MANAGER,ESTATE_OWNER";
    let (printed, said) = called(
        "llGetListLength(osGetAgents())",
        config(list),
        Standing::default(),
    )?;
    assert!(printed.is_empty());
    assert!(said.first().is_some_and(|line| {
        line.starts_with("YEngine: OSSL Permission Error: osGetAgents permission denied. (script:")
    }));
    // An estate owner is on it: the call passes its check, and reaches a
    // function the grid has not written yet.
    let owner = Standing {
        roles: vec![Allowed::EstateOwner],
        ..Standing::default()
    };
    let (_, said) = called("llGetListLength(osGetAgents())", config(list), owner)?;
    assert!(
        said.first()
            .is_some_and(|line| line.contains("osGetAgents is not implemented"))
    );
    // The estate owner is no estate manager.
    let manager_only = "Allow_osGetAgents = ESTATE_MANAGER";
    let both = Standing {
        roles: vec![Allowed::EstateOwner, Allowed::EstateManager],
        ..Standing::default()
    };
    let (_, said) = called("llGetListLength(osGetAgents())", config(manager_only), both)?;
    assert!(
        said.first()
            .is_some_and(|line| line.contains("osGetAgents permission denied."))
    );
    Ok(())
}

#[test]
fn owners_creators_and_switches_as_ossl_reads_them() -> Result<(), String> {
    const ME: &str = "0a0a0a0a-0000-4000-8000-000000000001";
    let call = "osGetScriptEngineName()";
    // An owner by key, written in capitals.
    let (printed, _) = called(
        call,
        config(&format!(
            "Allow_osGetScriptEngineName = {}",
            ME.to_uppercase()
        )),
        Standing::default(),
    )?;
    assert_eq!(printed, vec!["got YEngine".to_owned()]);
    // `false` disables, whatever a creators line says.
    let (_, said) = called(
        call,
        config(&format!(
            "Allow_osGetScriptEngineName = false\nCreators_osGetScriptEngineName = {ME}"
        )),
        Standing::default(),
    )?;
    assert!(said.first().is_some_and(|line| line.contains(
        "OSSL Permission Error: osGetScriptEngineName disabled in region configuration"
    )));
    // A creator on the list, but a modifiable script of another owner.
    let stranger = Standing {
        creator: "0b0b0b0b-0000-4000-8000-000000000002".to_owned(),
        modifiable: true,
        ..Standing::default()
    };
    let creators = "Creators_osGetScriptEngineName = 0b0b0b0b-0000-4000-8000-000000000002";
    let (_, said) = called(call, config(creators), stranger.clone())?;
    assert!(said.first().is_some_and(|line| {
        line.contains("osGetScriptEngineName permission denied. Script creator is not prim owner.")
    }));
    let no_mod = Standing {
        modifiable: false,
        ..stranger
    };
    let (printed, _) = called(call, config(creators), no_mod)?;
    assert_eq!(printed, vec!["got YEngine".to_owned()]);
    // OSSL off: a checked function throws, an unchecked one runs.
    let off = "AllowOSFunctions = false";
    let (_, said) = called(call, config(off), Standing::default())?;
    assert!(said.first().is_some_and(|line| {
        line.contains("OSSL Permission Error: All unsafe OSSL funtions disabled")
    }));
    let (printed, _) = called("osRound(2.5, 0)", config(off), Standing::default())?;
    assert_eq!(printed, vec!["got 3.000000".to_owned()]);
    Ok(())
}

#[test]
fn a_denial_to_the_owner_goes_to_the_owner_alone() -> Result<(), String> {
    let mut region = Region::new(EngineFlavour::YEngine);
    region
        .engine
        .set_ossl(config("PermissionErrorToOwner = true"));
    region.add(A, "Caller", 1, &caller("osGetScriptEngineName()"))?;
    region.tick()?;
    region.tick()?;
    assert_eq!(region.debug_lines(), Vec::<String>::new());
    let told: Vec<&(u64, CallerId, String, i32, String)> = region.host.said_to.iter().collect();
    assert_eq!(told.len(), 1);
    assert!(told.first().is_some_and(|(_, _, to, channel, text)| {
        to == super::OWNER
            && *channel == DEBUG_CHANNEL
            && text.starts_with("YEngine: OSSL Permission Error: osGetScriptEngineName")
    }));
    Ok(())
}

#[test]
fn a_deprecated_function_warns_sleeps_and_answers() -> Result<(), String> {
    let mut region = Region::new(EngineFlavour::XEngine);
    region.add(A, "Caller", 1, &caller("osSetPenColour(\"\", \"red\")"))?;
    region.tick()?;
    region.tick()?;
    assert_eq!(
        region.debug_lines(),
        vec!["Use of function osSetPenColour is deprecated. Use osSetPenColor instead.".to_owned()]
    );
    assert!(region.host.said.iter().any(|(_, _, speech)| {
        speech.to_avatars == ChatVolume::Shout && speech.to_scripts == Some(ChatVolume::Shout)
    }));
    // A second's sleep, ten ticks, between the shout and the answer.
    for _ in 0..12 {
        region.tick()?;
    }
    let shouted = region.host.said.first().map(|(tick, ..)| *tick);
    let answered = region.host.printed.first().map(|(tick, ..)| *tick);
    assert_eq!(
        answered
            .zip(shouted)
            .map(|(answered, shouted)| answered.saturating_sub(shouted)),
        Some(10)
    );
    assert_eq!(
        region
            .host
            .printed
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["got PenColour red; "]
    );
    Ok(())
}

#[test]
fn overloads_are_told_apart_by_their_arguments() -> Result<(), String> {
    let source = r#"
default
{
    state_entry()
    {
        print((string)osApproxEquals(1, 1.0000001)
            + (string)osApproxEquals(1.0, 1.1, 0.2)
            + (string)osApproxEquals(<1, 2, 3>, <1, 2, 3.5>, 0.4)
            + (string)osApproxEquals(<1, 2, 3>, <1, 2, 3>)
            + (string)osApproxEquals(ZERO_ROTATION, <0, 0, 0, 1.1>)
            + (string)osApproxEquals(ZERO_ROTATION, <0, 0, 0, 1.1>, 0.2));
        print(osStringSubString("abcdef", 2) + "|" + osStringSubString("abcdef", 2, 2));
        print(osDrawLine("", 1, 2) + "|" + osDrawLine("", 1, 2, 3, 4));
        print((string)osSlerp(<1, 0, 0>, <0, 1, 0>, 0.0)
            + (string)osSlerp(ZERO_ROTATION, ZERO_ROTATION, 0.5));
    }
}
"#;
    let mut region = Region::new(EngineFlavour::YEngine);
    region.add(A, "Overloads", 1, source)?;
    for _ in 0..3 {
        region.tick()?;
    }
    assert_eq!(
        region
            .host
            .printed
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "110101",
            "cdef|cd",
            "LineTo 1,2; |MoveTo 1,2; LineTo 3,4; ",
            "<1.00000, 0.00000, 0.00000><0.00000, 0.00000, 0.00000, 1.00000>",
        ]
    );
    // Second Life knows none of it.
    let refused = program_for(source, EngineFlavour::SecondLife).err();
    assert!(refused.is_some());
    Ok(())
}

#[test]
fn a_regex_listen_hears_lines_that_match() -> Result<(), String> {
    let source = r#"
default
{
    state_entry()
    {
        print((string)osListenRegex(7, "^Ob", NULL_KEY, "^probe [0-9]+$",
            OS_LISTEN_REGEX_NAME | OS_LISTEN_REGEX_MESSAGE));
        print((string)osListenRegex(8, "(", NULL_KEY, "", OS_LISTEN_REGEX_NAME));
    }

    listen(integer channel, string name, key id, string message)
    {
        print("heard " + message);
    }
}
"#;
    let mut region = Region::new(EngineFlavour::YEngine);
    region.add(A, "Listener", 1, source)?;
    for _ in 0..15 {
        region.tick()?;
    }
    for message in ["probe 12", "probe x", "a probe 3"] {
        let _posted = region
            .engine
            .hear(
                A,
                &Heard {
                    channel: 7,
                    name: "Object".to_owned(),
                    speaker: crate::value::NULL_KEY.to_owned(),
                    message: message.to_owned(),
                },
            )
            .map_err(|error| error.to_string())?;
    }
    for _ in 0..3 {
        region.tick()?;
    }
    assert_eq!(
        region
            .host
            .printed
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "-1", "heard probe 12"]
    );
    assert_eq!(
        region.debug_lines(),
        vec!["Name regex is invalid.".to_owned()]
    );
    Ok(())
}

#[test]
fn a_message_reaches_the_scripts_of_another_prim() -> Result<(), String> {
    let receiver = r#"
default
{
    dataserver(key id, string data)
    {
        print("from " + (string)id + " " + data);
    }
}
"#;
    let sender = r#"
default
{
    state_entry()
    {
        osMessageObject("00000000-0000-4000-8000-000000000002", "hello");
        osMessageObject("00000000-0000-4000-8000-000000000009", "lost");
        osMessageObject("nope", "lost");
    }
}
"#;
    let mut region = Region::new(EngineFlavour::YEngine);
    // The stock configuration allows it estate managers and owners.
    region.host.standing.roles.push(Allowed::EstateOwner);
    region.add(A, "Sender", 1, sender)?;
    region.add(CallerId(2), "Receiver", 2, receiver)?;
    for _ in 0..40 {
        region.tick()?;
    }
    assert_eq!(
        region
            .host
            .printed
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["from 00000000-0000-4000-8000-000000000001 hello"]
    );
    assert_eq!(
        region.debug_lines(),
        vec![
            "osMessageObject() cannot send message to 00000000-0000-4000-8000-000000000009, \
             object was not found in scene."
                .to_owned(),
            "osMessageObject() cannot send messages to objects with invalid UUIDs".to_owned(),
        ]
    );
    Ok(())
}

/// The `ossl/strings` scripts in their saving order: the script's name and
/// its source, all in prim A.
const STRINGS: [(&str, &str); 3] = [
    (
        "Strings",
        include_str!("../../../probes/ossl/strings/strings.lsl"),
    ),
    (
        "Parser",
        include_str!("../../../probes/ossl/strings/parser.lsl"),
    ),
    (
        "Leader",
        include_str!("../../../probes/ossl/strings/leader.lsl"),
    ),
];

/// What YEngine told the owner of each script the two refused searches
/// stopped, as the transcript has it after the item's key and but for where
/// the prim is: the exception a line at a time, and that a reset is needed —
/// the stack trace after those is not told.
const YENGINE_TOLD: [[&str; 3]; 2] = [
    [
        "ArgumentOutOfRangeException: Count must be positive and count must refer to a \
         location within the string/array/collection.",
        "Parameter name: count",
        "Script must be Reset to re-enable.",
    ],
    [
        "ArgumentOutOfRangeException: Index was out of range. Must be non-negative and less \
         than the size of the collection.",
        "Parameter name: startIndex",
        "Script must be Reset to re-enable.",
    ],
];

#[test]
fn strings_search_and_sort_as_each_engine_did() -> Result<(), String> {
    for (flavour, transcript) in transcripts("strings") {
        let mut region = Region::new(flavour);
        let mut refused = None;
        for (index, (name, source)) in STRINGS.into_iter().enumerate() {
            let id = CallerId(u64::try_from(index).unwrap_or(0).saturating_add(1));
            if let Err(error) = region.add(id, name, 1, source) {
                refused.get_or_insert((name, error));
            }
        }
        if !flavour.is_opensim() {
            // Aditi stopped at the first script it could not compile.
            assert!(transcript.contains("# compile error in \"Strings\""));
            assert!(
                matches!(refused, Some(("Strings", _))),
                "Second Life compiled ossl/strings: {refused:?}"
            );
            continue;
        }
        if let Some(refused) = refused {
            return Err(format!("{flavour:?} refused {refused:?}"));
        }
        for _ in 0..2_000 {
            region.tick()?;
            if region.ended() {
                break;
            }
        }
        super::strings::hold(
            &canonical(&region.lines(), &["answer"]),
            &canonical(&probe_lines(transcript), &["answer"]),
            &[],
        );
        let told: Vec<String> = region
            .host
            .told
            .iter()
            .map(|(_, _, text)| text.clone())
            .collect();
        let said = region.debug_lines();
        if flavour == EngineFlavour::YEngine {
            assert!(said.is_empty(), "YEngine said {said:?}");
            let [
                first,
                count,
                count_parameter,
                _strings_prim,
                count_reset,
                second,
                index,
                index_parameter,
                _parser_prim,
                index_reset,
            ] = told.as_slice()
            else {
                return Err(format!("YEngine told {told:?}"));
            };
            for header in [first, second] {
                assert!(header.starts_with("[YEngine]: Exception while running "));
            }
            assert_eq!(
                [
                    [count.as_str(), count_parameter, count_reset],
                    [index.as_str(), index_parameter, index_reset],
                ],
                YENGINE_TOLD
            );
            for line in YENGINE_TOLD.iter().flatten() {
                assert!(
                    transcript.contains(&format!("# owner-say: from=\"Object\" text=\"{line}\"")),
                    "{line}"
                );
            }
        } else {
            assert!(told.is_empty(), "XEngine told {told:?}");
            let noted = noted_debug(transcript);
            assert_eq!(said.len(), 2, "XEngine said {said:?}");
            for (ours, theirs) in said.iter().zip(&noted) {
                // The grid's line goes on with the stack trace.
                assert!(
                    theirs.starts_with(&format!("{ours}\n  at ")),
                    "{ours:?} against {theirs:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn the_stock_configuration_is_the_local_grids() {
    let stock = Settings::stock();
    assert!(stock.families.ossl && stock.families.lightshare && stock.families.modules);
    assert!(!stock.permission_error_to_owner);
    assert_eq!(stock.threat_level, ThreatLevel::VeryLow);
    assert_eq!(
        stock
            .permits
            .get("osGetAgents")
            .map(|permit| permit.allowed.clone()),
        Some(vec![Allowed::EstateManager, Allowed::EstateOwner])
    );
    assert!(
        stock
            .permits
            .get("osAvatarStopAnimation")
            .is_some_and(|permit| permit.disabled)
    );
    assert!(
        stock
            .permits
            .get("osGetAgentIP")
            .is_some_and(|permit| permit.everyone)
    );
    // The include of an `osslEnable.ini` beside it is not followed.
    assert_eq!(
        stock.ignored,
        vec!["Include-osslEnable = config-include/osslEnable.ini".to_owned()]
    );
}
