//! Chat, listens, dialogs and link messages held to the grids: the `comms/*`
//! probes run in the VM under each flavour, in a small region of this test's
//! own that routes what the scripts say to the scripts in earshot, puts the
//! dialogs to two avatars that answer as the probe's runner did, and checks
//! every `PROBE` line and the dialogs and `DEBUG_CHANNEL` lines the grid's
//! runner noted (see `strings.rs` for how a probe is run offline).
//!
//! What a probe calls of a tranche not written yet is changed where it calls
//! it: `llGetKey()` is its prim's key, `llGetOwner()` and `llKey2Name` know
//! the two avatars, an avatar's size is never asked of an object, and the
//! link probe's prim is linked to nothing.

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;

use super::strings::{offline, probe_lines};
use super::*;

/// The probes' owner, the owner of every object.
const OWNER: &str = super::OWNER;

/// The other avatar.
pub(super) const OTHER: &str = "0b0b0b0b-0000-4000-8000-000000000002";

/// The name an avatar's key stands for, as `llKey2Name` gives it.
pub(super) fn name_of(agent: &str) -> &'static str {
    match agent {
        OWNER => "Probe Owner",
        OTHER => "Probe Other",
        _ => "",
    }
}

/// The key of the prim numbered `prim`.
fn prim_key(prim: u64) -> String {
    format!("00000000-0000-4000-8000-{prim:012}")
}

/// `source` offline as the script `script`, in the prim numbered `prim`, with
/// the calls of the tranches not written yet answered.
fn rigged(source: &str, script: &str, prim: u64) -> String {
    let helpers = format!(
        "string key2name(key id)\n{{\n    if (id == \"{OWNER}\") {{ return \"{}\"; }}\n    \
         if (id == \"{OTHER}\") {{ return \"{}\"; }}\n    return \"\";\n}}\n\n",
        name_of(OWNER),
        name_of(OTHER),
    );
    let source = offline(source, script)
        .replace("llGetKey()", &format!("((key)\"{}\")", prim_key(prim)))
        .replace("llGetOwner()", &format!("((key)\"{OWNER}\")"))
        .replace("llKey2Name(", "key2name(")
        .replace("llGetAgentSize(id) != ZERO_VECTOR", "FALSE")
        .replace("llGetLinkNumber()", "0")
        .replace("llGetNumberOfPrims()", "1");
    source.replacen("default\n{", &format!("{helpers}default\n{{"), 1)
}

/// A region of prims, each with its scripts, that routes what they say.
pub(super) struct Region {
    /// The scripts.
    pub(super) engine: Engine,
    /// What they did.
    pub(super) host: Recorder,
    /// Every script, in the order added.
    scripts: Vec<CallerId>,
    /// Each prim's position, by number.
    places: BTreeMap<u64, Vector>,
    /// How many of the host's `said` and `said_to` lines are routed.
    routed: (usize, usize),
}

impl Region {
    /// An empty region imitating `flavour`.
    pub(super) fn new(flavour: EngineFlavour) -> Self {
        Self {
            engine: engine_as(flavour),
            host: Recorder::default(),
            scripts: Vec::new(),
            places: BTreeMap::new(),
            routed: (0, 0),
        }
    }

    /// Put the prim numbered `prim` at `(x, y, z)`.
    pub(super) fn place(&mut self, prim: u64, x: f32, y: f32, z: f32) {
        let _old = self.places.insert(prim, Vector { x, y, z });
        let _old = self.host.prim_keys.insert(prim, prim_key(prim));
    }

    /// Add the probe script `source` as `name`, the script `id` in the prim
    /// numbered `prim`.
    pub(super) fn add(
        &mut self,
        id: CallerId,
        name: &str,
        prim: u64,
        source: &str,
    ) -> Result<(), String> {
        let flavour = self.engine.config().flavour;
        let _none = self
            .engine
            .add(id, program_for(&rigged(source, name, prim), flavour)?);
        let _none = self.host.scripts.insert(name.to_owned(), id);
        let _none = self.host.prims.insert(id, prim);
        let _none = self.host.prim_keys.insert(prim, prim_key(prim));
        self.scripts.push(id);
        Ok(())
    }

    /// Where the prim `caller` is in stands.
    fn place_of(&self, caller: CallerId) -> Vector {
        self.places
            .get(&self.host.prim_of(caller))
            .cloned()
            .unwrap_or(crate::value::ZERO_VECTOR)
    }

    /// Run one tick, then offer every line said in it to the scripts in
    /// earshot of it: within the line's range of its prim, never in its own
    /// prim; a `llRegionSayTo` to the scripts of the prim it names.
    pub(super) fn tick(&mut self) -> Result<(), String> {
        let _reports = run(&mut self.engine, &mut self.host, 1);
        let said: Vec<(CallerId, Speech)> = self
            .host
            .said
            .get(self.routed.0..)
            .unwrap_or_default()
            .iter()
            .map(|(_, from, speech)| (*from, speech.clone()))
            .collect();
        let said_to: Vec<(CallerId, String, i32, String)> = self
            .host
            .said_to
            .get(self.routed.1..)
            .unwrap_or_default()
            .iter()
            .map(|(_, from, target, channel, text)| (*from, target.clone(), *channel, text.clone()))
            .collect();
        self.routed = (self.host.said.len(), self.host.said_to.len());
        for (from, speech) in said {
            let Some(volume) = speech.to_scripts else {
                continue;
            };
            let range = match volume {
                ChatVolume::Whisper => 10.0,
                ChatVolume::Say => 20.0,
                ChatVolume::Shout => 100.0,
                ChatVolume::RegionSay => f32::INFINITY,
            };
            let at = self.place_of(from);
            let heard = Heard {
                channel: speech.channel,
                name: "Object".to_owned(),
                speaker: prim_key(self.host.prim_of(from)),
                message: speech.text.clone(),
            };
            for to in self.scripts.clone() {
                let there = self.place_of(to);
                let distance = ((there.x - at.x).powi(2)
                    + (there.y - at.y).powi(2)
                    + (there.z - at.z).powi(2))
                .sqrt();
                if self.host.prim_of(to) != self.host.prim_of(from) && distance <= range {
                    let _posted = self
                        .engine
                        .hear(to, &heard)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        for (from, target, channel, message) in said_to {
            let heard = Heard {
                channel,
                name: "Object".to_owned(),
                speaker: prim_key(self.host.prim_of(from)),
                message,
            };
            for to in self.scripts.clone() {
                if prim_key(self.host.prim_of(to)) == target {
                    let _posted = self
                        .engine
                        .hear(to, &heard)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }

    /// Every `PROBE` line printed, in order.
    pub(super) fn lines(&self) -> Vec<String> {
        self.host
            .printed
            .iter()
            .map(|(_, _, text)| text.clone())
            .filter(|line| line.starts_with("PROBE "))
            .collect()
    }

    /// Whether a script has printed its probe's `end` record.
    pub(super) fn ended(&self) -> bool {
        self.lines().iter().any(|line| line.contains(" rec=end"))
    }

    /// The `DEBUG_CHANNEL` lines said, in order.
    pub(super) fn debug_lines(&self) -> Vec<String> {
        self.host
            .said
            .iter()
            .filter(|(_, _, speech)| speech.channel == DEBUG_CHANNEL)
            .map(|(_, _, speech)| speech.text.clone())
            .collect()
    }
}

/// The `rec` of a `PROBE` line.
fn rec_of(line: &str) -> String {
    crate::probe::Line::parse(line).map_or_else(|_| String::new(), |parsed| parsed.rec)
}

/// `lines` with the start record left out and every run of lines whose
/// `rec` is in `unordered` sorted — the probe's header says the grid may
/// print those in any order.
pub(super) fn canonical(lines: &[String], unordered: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<String> = Vec::new();
    for line in lines {
        let rec = rec_of(line);
        if rec == "start" {
            continue;
        }
        if unordered.contains(&rec.as_str()) {
            run.push(line.clone());
        } else {
            run.sort();
            out.append(&mut run);
            out.push(line.clone());
        }
    }
    run.sort();
    out.append(&mut run);
    out
}

/// `lines` without the start record, split into the lines in order and,
/// sorted, those whose `rec` is in `unordered` — which a grid may print
/// anywhere among the others, its scripts running side by side.
fn apart(lines: &[String], unordered: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut loose, kept): (Vec<String>, Vec<String>) = lines
        .iter()
        .filter(|line| rec_of(line) != "start")
        .cloned()
        .partition(|line| unordered.contains(&rec_of(line).as_str()));
    loose.sort();
    (kept, loose)
}

/// The value of `key="…"` in a runner's `#` note.
pub(super) fn note_field(note: &str, key: &str) -> Option<String> {
    let start = note.find(&format!(" {key}=\""))?;
    let rest = note.get(start.saturating_add(key.len()).saturating_add(3)..)?;
    let mut value = String::new();
    let mut characters = rest.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => value.push(characters.next()?),
            '"' => return Some(value),
            other => value.push(other),
        }
    }
    None
}

/// The value of the bare `key=…` in a runner's `#` note.
fn note_bare(note: &str, key: &str) -> Option<String> {
    let start = note.find(&format!(" {key}="))?;
    let rest = note.get(start.saturating_add(key.len()).saturating_add(2)..)?;
    rest.split(' ').next().map(str::to_owned)
}

/// What a transcript's runner noted of each dialog put to an avatar, in
/// order: whether it went to the other avatar, the channel, the message's
/// length and the buttons.
fn noted_dialogs(transcript: &str) -> Vec<(bool, String, String, String)> {
    transcript
        .lines()
        .filter(|line| line.starts_with("# dialog ") && !line.starts_with("# dialog-reply"))
        .map(|line| {
            (
                line.starts_with("# dialog by=other"),
                note_bare(line, "channel").unwrap_or_default(),
                note_bare(line, "message_length").unwrap_or_default(),
                note_field(line, "buttons").unwrap_or_default(),
            )
        })
        .collect()
}

/// The `DEBUG_CHANNEL` lines a transcript's runner noted, in order.
pub(super) fn noted_debug(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| line.starts_with("# debug-channel:"))
        .filter_map(|line| note_field(line, "text"))
        .collect()
}

/// The three grids' transcripts of the probe `name`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let files = match name {
        "listen-limit" => [
            include_str!("../../../probes/results/aditi/comms/listen-limit.txt"),
            include_str!("../../../probes/results/opensim-yengine/comms/listen-limit.txt"),
            include_str!("../../../probes/results/opensim-xengine/comms/listen-limit.txt"),
        ],
        "link-messages" => [
            include_str!("../../../probes/results/aditi/comms/link-messages.txt"),
            include_str!("../../../probes/results/opensim-yengine/comms/link-messages.txt"),
            include_str!("../../../probes/results/opensim-xengine/comms/link-messages.txt"),
        ],
        "object-chat" => [
            include_str!("../../../probes/results/aditi/comms/object-chat.txt"),
            include_str!("../../../probes/results/opensim-yengine/comms/object-chat.txt"),
            include_str!("../../../probes/results/opensim-xengine/comms/object-chat.txt"),
        ],
        "dialog" => [
            include_str!("../../../probes/results/aditi/comms/dialog.txt"),
            include_str!("../../../probes/results/opensim-yengine/comms/dialog.txt"),
            include_str!("../../../probes/results/opensim-xengine/comms/dialog.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/comms/textbox.txt"),
            include_str!("../../../probes/results/opensim-yengine/comms/textbox.txt"),
            include_str!("../../../probes/results/opensim-xengine/comms/textbox.txt"),
        ],
    };
    let [aditi, yengine, xengine] = files;
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// Run `region` until a script prints `end`, or `ticks` ticks.
fn run_out(region: &mut Region, ticks: u32) -> Result<(), String> {
    for _ in 0..ticks {
        region.tick()?;
        if region.ended() {
            break;
        }
    }
    assert!(region.host.stubbed.is_empty(), "{:?}", region.host.stubbed);
    Ok(())
}

#[test]
fn comms_listen_limit_prints_what_each_grid_printed() -> Result<(), String> {
    for (flavour, transcript) in transcripts("listen-limit") {
        let mut region = Region::new(flavour);
        region.add(
            A,
            "Leader",
            1,
            include_str!("../../../probes/comms/listen-limit/leader.lsl"),
        )?;
        region.add(
            B,
            "Filler",
            1,
            include_str!("../../../probes/comms/listen-limit/filler.lsl"),
        )?;
        run_out(&mut region, 400)?;
        assert_eq!(
            canonical(&region.lines(), &[]),
            canonical(&probe_lines(transcript), &[]),
            "{flavour:?}"
        );
        // Second Life's error names the script and the prim; the prim's
        // name is the host's, "Object" on the grid.
        let ours: Vec<String> = region
            .debug_lines()
            .iter()
            .map(|line| line.replace("Faulty prim [", "Object ["))
            .collect();
        assert_eq!(ours, noted_debug(transcript), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn comms_link_messages_prints_what_each_grid_printed() -> Result<(), String> {
    for (flavour, transcript) in transcripts("link-messages") {
        let mut region = Region::new(flavour);
        region.add(
            A,
            "Sender",
            1,
            include_str!("../../../probes/comms/link-messages/sender.lsl"),
        )?;
        region.add(
            B,
            "Receiver",
            1,
            include_str!("../../../probes/comms/link-messages/receiver.lsl"),
        )?;
        run_out(&mut region, 400)?;
        // On aditi the Receiver ran while the Sender was still in its
        // handler, so its first lines came before the Sender's last `sent`;
        // here a handler runs to its end within its share of a tick.
        assert_eq!(
            apart(&region.lines(), &["got"]),
            apart(&probe_lines(transcript), &["got"]),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn comms_object_chat_prints_what_each_grid_printed() -> Result<(), String> {
    let listeners: [(&str, &str, f32, f32); 6] = [
        (
            "At9",
            include_str!("../../../probes/comms/object-chat/at-9.lsl"),
            9.0,
            0.0,
        ),
        (
            "At11",
            include_str!("../../../probes/comms/object-chat/at-11.lsl"),
            11.0,
            0.0,
        ),
        (
            "At19",
            include_str!("../../../probes/comms/object-chat/at-19.lsl"),
            19.0,
            0.0,
        ),
        (
            "At21",
            include_str!("../../../probes/comms/object-chat/at-21.lsl"),
            21.0,
            0.0,
        ),
        (
            "At99",
            include_str!("../../../probes/comms/object-chat/at-99.lsl"),
            0.0,
            99.0,
        ),
        (
            "At101",
            include_str!("../../../probes/comms/object-chat/at-101.lsl"),
            0.0,
            101.0,
        ),
    ];
    for (flavour, transcript) in transcripts("object-chat") {
        let mut region = Region::new(flavour);
        region.place(1, 100.0, 100.0, 30.0);
        region.add(
            A,
            "Speaker",
            1,
            include_str!("../../../probes/comms/object-chat/speaker.lsl"),
        )?;
        region.add(
            B,
            "Self",
            1,
            include_str!("../../../probes/comms/object-chat/self.lsl"),
        )?;
        for (index, (name, source, east, up)) in listeners.iter().enumerate() {
            let prim = u64::try_from(index).unwrap_or_default().saturating_add(2);
            region.place(prim, 100.0 + east, 100.0, 30.0 + up);
            region.add(CallerId(prim.saturating_add(10)), name, prim, source)?;
        }
        run_out(&mut region, 800)?;
        assert_eq!(
            canonical(&region.lines(), &["heard", "ready"]),
            canonical(&probe_lines(transcript), &["heard", "ready"]),
            "{flavour:?}"
        );
        assert_eq!(region.debug_lines(), noted_debug(transcript), "{flavour:?}");
    }
    Ok(())
}

/// Run a dialog probe — `comms/dialog` or `comms/textbox` — answering as its
/// runner did: each `waiting drive=dialog` record names the reply, which the
/// avatar it names says on the dialog's channel once the dialog is put to
/// it (cut to the 254 bytes a reply's field holds), and `waiting drive=touch`
/// is the other avatar touching.
fn run_dialogs(source: &str, flavour: EngineFlavour) -> Result<Region, String> {
    let mut region = Region::new(flavour);
    region.add(A, "Probe", 1, source)?;
    let mut read = 0_usize;
    let mut asked = 0_usize;
    let mut pending: Option<(String, String)> = None;
    for _ in 0..2_000 {
        region.tick()?;
        let lines = region.lines();
        for line in lines.get(read..).unwrap_or_default() {
            let Ok(parsed) = crate::probe::Line::parse(line) else {
                continue;
            };
            if parsed.rec != "waiting" {
                continue;
            }
            let agent = if parsed.field("by") == Some("other") {
                OTHER
            } else {
                OWNER
            };
            match parsed.field("drive") {
                Some("dialog") => {
                    pending = Some((
                        agent.to_owned(),
                        parsed.field("reply").unwrap_or_default().to_owned(),
                    ));
                }
                Some("touch") => {
                    let touch = Detected {
                        key: OTHER.to_owned(),
                        name: name_of(OTHER).to_owned(),
                        owner: OTHER.to_owned(),
                        kind: 1,
                        ..Detected::blank()
                    };
                    let _posted = region
                        .engine
                        .post_detected(A, named("touch_start")?, vec![touch])
                        .map_err(|error| error.to_string())?;
                }
                _ => {}
            }
        }
        read = lines.len();
        let dialogs: Vec<Dialog> = region
            .host
            .dialogs
            .get(asked..)
            .unwrap_or_default()
            .iter()
            .map(|(_, _, dialog)| dialog.clone())
            .collect();
        asked = region.host.dialogs.len();
        for dialog in dialogs {
            if let Some((agent, reply)) = pending.take_if(|(agent, _)| *agent == dialog.agent) {
                let heard = Heard {
                    channel: dialog.channel,
                    name: name_of(&agent).to_owned(),
                    speaker: agent,
                    message: crate::vm::comms::cut_bytes(&reply, 254).to_owned(),
                };
                let _posted = region
                    .engine
                    .hear(A, &heard)
                    .map_err(|error| error.to_string())?;
            }
        }
        if region.ended() {
            break;
        }
    }
    assert!(region.host.stubbed.is_empty(), "{:?}", region.host.stubbed);
    Ok(region)
}

/// Hold a dialog probe's run to a transcript: its `PROBE` lines, the
/// dialogs put to each avatar, and the `DEBUG_CHANNEL` lines.
fn hold_dialogs(region: &Region, transcript: &str, flavour: EngineFlavour) {
    assert_eq!(
        canonical(&region.lines(), &[]),
        canonical(&probe_lines(transcript), &[]),
        "{flavour:?}"
    );
    let put: Vec<(bool, String, String, String)> = region
        .host
        .dialogs
        .iter()
        .map(|(_, _, dialog)| {
            (
                dialog.agent == OTHER,
                dialog.channel.to_string(),
                dialog.message.chars().count().to_string(),
                dialog.buttons.join("|"),
            )
        })
        .collect();
    assert_eq!(put, noted_dialogs(transcript), "{flavour:?}");
    assert_eq!(region.debug_lines(), noted_debug(transcript), "{flavour:?}");
}

#[test]
fn comms_dialog_puts_and_refuses_what_each_grid_did() -> Result<(), String> {
    for (flavour, transcript) in transcripts("dialog") {
        let region = run_dialogs(include_str!("../../../probes/comms/dialog.lsl"), flavour)?;
        hold_dialogs(&region, transcript, flavour);
    }
    Ok(())
}

#[test]
fn comms_textbox_puts_and_refuses_what_each_grid_did() -> Result<(), String> {
    for (flavour, transcript) in transcripts("textbox") {
        let region = run_dialogs(include_str!("../../../probes/comms/textbox.lsl"), flavour)?;
        hold_dialogs(&region, transcript, flavour);
    }
    Ok(())
}

/// The acceptance's conversation: two scripts in one region, no viewer,
/// talk back and forth on channel 7 — each hears the other and neither
/// itself.
#[test]
fn two_scripts_hold_a_conversation_on_a_channel() -> Result<(), String> {
    let mut region = Region::new(EngineFlavour::SecondLife);
    region.place(1, 10.0, 10.0, 20.0);
    region.place(2, 15.0, 10.0, 20.0);
    let talker = |opening: &str| {
        format!(
            "integer n;\ndefault\n{{\n    state_entry() {{ llListen(7, \"\", NULL_KEY, \"\"); {opening} }}\n    \
             listen(integer channel, string name, key id, string message)\n    {{\n        \
             llOwnerSay(\"heard \" + message);\n        if (++n < 3) {{ llSay(7, message + \"!\"); }}\n    }}\n}}\n"
        )
    };
    region.add(A, "Ping", 1, &talker("llSay(7, \"ping\");"))?;
    region.add(B, "Pong", 2, &talker(""))?;
    for _ in 0..20 {
        region.tick()?;
    }
    assert_eq!(
        region.host.texts(B),
        vec!["heard ping", "heard ping!!", "heard ping!!!!"]
    );
    assert_eq!(region.host.texts(A), vec!["heard ping!", "heard ping!!!"]);
    Ok(())
}

/// The acceptance's state change: a listen opened in one state hears
/// nothing after the script moves to another, and one opened in
/// `state_exit` is gone too (`events/state-drops`).
#[test]
fn a_state_change_drops_every_listen() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default { state_entry() { llListen(7, \"\", NULL_KEY, \"\"); } \
             listen(integer c, string n, key k, string m) { print(\"default \" + m); state other; } \
             state_exit() { llListen(8, \"\", NULL_KEY, \"\"); } } \
             state other { listen(integer c, string n, key k, string m) { print(\"other \" + m); } }",
        )?,
    );
    let heard = |channel, message: &str| Heard {
        channel,
        name: "Someone".to_owned(),
        speaker: OWNER.to_owned(),
        message: message.to_owned(),
    };
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(engine.instance(A).map(|i| i.listens().len()), Some(1));
    let _posted = engine.hear(A, &heard(7, "one"));
    let _reports = run(&mut engine, &mut host, 2);
    assert_eq!(
        engine.instance(A).map(|i| i.listens().is_empty()),
        Some(true)
    );
    for channel in [7, 8] {
        assert_eq!(engine.hear(A, &heard(channel, "two")), Ok(Some(Vec::new())));
    }
    let _reports = run(&mut engine, &mut host, 2);
    assert_eq!(host.texts(A), vec!["default one"]);
    Ok(())
}

#[test]
fn a_listen_hears_once_per_admitting_listen_and_not_when_off() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "integer h; default { state_entry() { h = llListen(7, \"\", NULL_KEY, \"\"); \
             llListen(7, \"Bob\", NULL_KEY, \"\"); } \
             listen(integer c, string n, key k, string m) { print(n + \" \" + m); \
             if (m == \"off\") { llListenControl(h, FALSE); } } }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    let line = |name: &str, message: &str| Heard {
        channel: 7,
        name: name.to_owned(),
        speaker: OTHER.to_owned(),
        message: message.to_owned(),
    };
    let _posted = engine.hear(A, &line("Bob", "both"));
    let _posted = engine.hear(A, &line("Amy", "off"));
    let _reports = run(&mut engine, &mut host, 2);
    let _posted = engine.hear(A, &line("Amy", "unheard"));
    let _posted = engine.hear(A, &line("Bob", "named"));
    let _reports = run(&mut engine, &mut host, 2);
    assert_eq!(
        host.texts(A),
        vec!["Bob both", "Bob both", "Amy off", "Bob named"]
    );
    Ok(())
}

#[test]
fn the_say_functions_say_as_far_as_they_carry_and_region_say_refuses_channel_0()
-> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::XEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program_for(
                "default { state_entry() { llWhisper(1, \"w\"); llSay(2, \"s\"); \
                 llShout(3, \"h\"); llRegionSay(4, \"r\"); llRegionSay(0, \"no\"); \
                 llOwnerSay(\"o\"); } }",
                flavour,
            )?,
        );
        let _reports = run(&mut engine, &mut host, 1);
        let said: Vec<(i32, &str, Option<ChatVolume>)> = host
            .said
            .iter()
            .map(|(_, _, speech)| (speech.channel, speech.text.as_str(), speech.to_scripts))
            .collect();
        let refusal = if flavour.is_opensim() {
            "llRegionSay: Cannot use on channel 0"
        } else {
            "Cannot use llRegionSay() on channel 0."
        };
        let refused_to = if flavour.is_opensim() {
            ChatVolume::Shout
        } else {
            ChatVolume::Say
        };
        assert_eq!(
            said,
            vec![
                (1, "w", Some(ChatVolume::Whisper)),
                (2, "s", Some(ChatVolume::Say)),
                (3, "h", Some(ChatVolume::Shout)),
                (4, "r", Some(ChatVolume::RegionSay)),
                (DEBUG_CHANNEL, refusal, Some(refused_to)),
            ]
        );
        assert_eq!(
            host.told
                .iter()
                .map(|(_, _, text)| text.as_str())
                .collect::<Vec<_>>(),
            vec!["o"]
        );
    }
    Ok(())
}

#[test]
fn region_say_to_skips_nobody_and_its_own_prim() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let own = "fab01e52-3517-4545-be1b-4cbf6d8ace3b";
    let _none = engine.add(
        A,
        program(&format!(
            "default {{ state_entry() {{ llRegionSayTo(NULL_KEY, 5, \"a\"); \
             llRegionSayTo(\"nobody\", 5, \"b\"); llRegionSayTo(\"{own}\", 5, \"c\"); \
             llRegionSayTo(\"{OTHER}\", 0, \"d\"); }} }}"
        ))?,
    );
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(
        host.said_to,
        vec![(1, A, OTHER.to_owned(), 0, "d".to_owned())]
    );
    Ok(())
}

#[test]
fn an_instant_message_is_cut_costs_two_seconds_and_refuses_no_key() -> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program_for(
                &format!(
                    "default {{ state_entry() {{ string s; integer i; \
                     for (i = 0; i < 110; ++i) {{ s += \"iiiiiiiiii\"; }} \
                     llInstantMessage(\"{OWNER}\", s); llInstantMessage(NULL_KEY, \"x\"); \
                     print(\"done\"); }} }}"
                ),
                flavour,
            )?,
        );
        let _reports = run(&mut engine, &mut host, 60);
        assert_eq!(
            host.messages
                .iter()
                .map(|(tick, _, message)| (*tick, message.agent.as_str(), message.text.len()))
                .collect::<Vec<_>>(),
            vec![(1, OWNER, 1023)]
        );
        // Two seconds each, at ten ticks to the second.
        assert_eq!(host.from(A), vec![(41, "done")]);
        let refusal = if flavour.is_opensim() {
            "llInstantMessage: An invalid key  was passed to llInstantMessage"
        } else {
            "Invalid Key passed to instant message."
        };
        assert_eq!(host.heard(A), vec![refusal]);
    }
    Ok(())
}

/// A link message to `LINK_THIS` reaches the sender too — which is how a
/// broadcast becomes a loop — and a reset in the same event drops the
/// sender's own while the other script still gets it.
#[test]
fn a_link_message_reaches_its_sender_unless_it_resets() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = host.prims.insert(A, 1);
    let _none = host.prims.insert(B, 1);
    let _none = engine.add(
        A,
        program(
            "default { link_message(integer s, integer num, string m, key k) { \
             print(m + (string)num); \
             if (num < 3) { llMessageLinked(LINK_THIS, num + 1, m, k); } \
             else if (num == 3) { llMessageLinked(LINK_SET, 9, \"last\", k); llResetScript(); } } }",
        )?,
    );
    let _none = engine.add(
        B,
        program(
            "default { state_entry() { llMessageLinked(LINK_THIS, 1, \"loop\", \"\"); } \
             link_message(integer s, integer num, string m, key k) \
             { print(\"b \" + m + (string)num); } }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 10);
    assert_eq!(host.texts(A), vec!["loop1", "loop2", "loop3"]);
    assert_eq!(
        host.texts(B),
        vec!["b loop1", "b loop2", "b loop3", "b last9"]
    );
    Ok(())
}

#[test]
fn map_destination_goes_to_the_toucher_or_the_wearer_and_otherwise_nowhere() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let source = "default { state_entry() { llMapDestination(\"Here\", <1, 2, 3>, ZERO_VECTOR); } \
                  touch_start(integer n) { llMapDestination(\"There\", <4, 5, 6>, ZERO_VECTOR); } }";
    let _none = engine.add(A, program(source)?);
    let _none = engine.add(B, program(source)?);
    let _none = host.worn.insert(B, OWNER.to_owned());
    let _reports = run(&mut engine, &mut host, 15);
    let touch = Detected {
        key: OTHER.to_owned(),
        ..Detected::blank()
    };
    let _posted = engine
        .post_detected(A, named("touch_start")?, vec![touch])
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 15);
    assert_eq!(
        host.destinations
            .iter()
            .map(|(id, to)| (*id, to.agent.as_str(), to.region.as_str()))
            .collect::<Vec<_>>(),
        vec![(B, OWNER, "Here"), (A, OTHER, "There")]
    );
    Ok(())
}

#[test]
fn load_url_checks_the_url_on_opensim_only() -> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(
            A,
            program_for(
                &format!(
                    "default {{ state_entry() {{ llLoadURL(\"{OWNER}\", \"m\", \"ftp://x\"); \
                     llLoadURL(\"{OWNER}\", \"m\", \"nothing\"); \
                     llLoadURL(\"{OWNER}\", \"m\", \"https://example.com/\"); \
                     llLoadURL(NULL_KEY, \"m\", \"https://example.com/\"); }} }}"
                ),
                flavour,
            )?,
        );
        let _reports = run(&mut engine, &mut host, 10);
        let urls: Vec<&str> = host
            .offers
            .iter()
            .map(|(_, offer)| offer.url.as_str())
            .collect();
        if flavour.is_opensim() {
            assert_eq!(urls, vec!["https://example.com/"]);
            assert_eq!(
                host.heard(A),
                vec!["llLoadURL: Invalid url schema", "llLoadURL: Invalid url"]
            );
        } else {
            assert_eq!(urls, vec!["ftp://x", "nothing", "https://example.com/"]);
            assert!(host.heard(A).is_empty());
        }
    }
    Ok(())
}
