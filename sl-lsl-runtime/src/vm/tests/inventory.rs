//! What a prim carries held to the grids: the `inventory/*` probes run in the
//! VM under each flavour, in a region of this test's own whose prims hold the
//! items the probe's runner put in them, and every `PROBE` line and every
//! `DEBUG_CHANNEL` line the runner noted set against what the grid printed
//! (see `strings.rs` for how a probe is run offline).
//!
//! The region does what the grid does with what a script hands it: a script
//! loaded into another prim starts there a tick after it lands, stopped until
//! then; a rezzed object comes out of the item it was taken into, its scripts
//! as they were saved, and its rezzer's `object_rez` follows its `on_rez`.
//!
//! The lines are compared script by script — the leader's in order, and the
//! other scripts' in order — since which of two scripts prints first in one
//! frame is the grid's scheduling, not the library's. What a probe reads of a
//! tranche not written yet is written in where it reads it: `llSensor` finds
//! prim B. Every other difference is a named deviation, with its reason.

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use sl_types::lsl::Vector;

use super::comms::{OTHER, Region, canonical, name_of, note_field, rigged};
use super::prim_state::{debug_notes, home_of, region};
use super::strings::probe_lines;
use super::*;
use crate::vm::contents::{
    INVENTORY_NOTECARD, INVENTORY_OBJECT, INVENTORY_SCRIPT, PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY,
    PARCEL_FLAG_ALLOW_CREATE_OBJECTS, PARCEL_FLAG_ALLOW_SCRIPTS,
};

/// When the items were acquired: 2026-10-27, the day the probes ran.
const ACQUIRED: i64 = 1_793_059_200;

/// The three flavours, each with its transcript of the probe `name`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let [aditi, yengine, xengine] = match name {
        "listing" => [
            include_str!("../../../probes/results/aditi/inventory/listing.txt"),
            include_str!("../../../probes/results/opensim-yengine/inventory/listing.txt"),
            include_str!("../../../probes/results/opensim-xengine/inventory/listing.txt"),
        ],
        "notecards" => [
            include_str!("../../../probes/results/aditi/inventory/notecards.txt"),
            include_str!("../../../probes/results/opensim-yengine/inventory/notecards.txt"),
            include_str!("../../../probes/results/opensim-xengine/inventory/notecards.txt"),
        ],
        "rez" => [
            include_str!("../../../probes/results/aditi/inventory/rez.txt"),
            include_str!("../../../probes/results/opensim-yengine/inventory/rez.txt"),
            include_str!("../../../probes/results/opensim-xengine/inventory/rez.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/inventory/give.txt"),
            include_str!("../../../probes/results/opensim-yengine/inventory/give.txt"),
            include_str!("../../../probes/results/opensim-xengine/inventory/give.txt"),
        ],
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// An item of the owner's, of every right to the owner and the next owner,
/// numbered `number` for its keys.
fn item(number: u64, name: &str, kind: i32) -> Item {
    Item {
        name: name.to_owned(),
        kind,
        item: format!("1e1e1e1e-0000-4000-8000-{number:012}"),
        asset: format!("a5a5a5a5-0000-4000-8000-{number:012}"),
        creator: OWNER.to_owned(),
        last_owner: crate::value::NULL_KEY.to_owned(),
        masks: Masks::full(),
        description: String::new(),
        acquired: ACQUIRED,
    }
}

/// The names the runner's notes say each notecard of the probe was stored
/// under: the name it was made with, or the one the grid kept (`named=`).
fn noted_notecards(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| line.starts_with("# notecard \""))
        .filter_map(|line| {
            let made = line.get(12..)?.split('"').next()?.to_owned();
            Some(note_field(line, "named").unwrap_or(made))
        })
        .collect()
}

/// `lines` without the fields named in `vary` — what the probe's header says
/// changes from run to run.
fn without(lines: Vec<String>, vary: &[&str]) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            line.split(' ')
                .filter(|field| {
                    !vary
                        .iter()
                        .any(|name| field.starts_with(&format!("{name}=")))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// The `rec` of a `PROBE` line.
fn rec(line: &str) -> String {
    crate::probe::Line::parse(line).map_or_else(|_| String::new(), |parsed| parsed.rec)
}

/// `lines`, the start record left out, split into the leader's and those of
/// the records in `others`, each in the order printed.
fn split(lines: &[String], others: &[&str]) -> (Vec<String>, Vec<String>) {
    lines
        .iter()
        .filter(|line| rec(line) != "start")
        .cloned()
        .partition(|line| !others.contains(&rec(line).as_str()))
}

/// The first line of each of `lines`: the runner notes no more of an
/// exception's than its message's first line.
fn first_lines(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| line.split('\n').next().unwrap_or_default().to_owned())
        .collect()
}

/// A probe's region, with the items its prims hold and what they do.
struct Stage {
    /// The region.
    region: Region,
    /// The scripts each object item carries, by the item's name: each
    /// script's name and source.
    objects: BTreeMap<String, Vec<(String, String)>>,
    /// The source of each script item a script may load elsewhere, by name.
    sources: BTreeMap<String, String>,
    /// The last id given a script.
    last_id: u64,
    /// The scripts a load put in a prim, to start, with their start
    /// parameters, once the next tick is over.
    starting: Vec<(CallerId, bool, i32)>,
    /// How many of the leader's lines have been read for a stimulus.
    read: usize,
}

impl Stage {
    /// A region of `flavour` with `scripts` (`(id, name, prim, source)`)
    /// running in their prims, each prim holding what `contents` says.
    fn new(
        flavour: EngineFlavour,
        scripts: &[(CallerId, &str, u64, String)],
        contents: BTreeMap<u64, Vec<Item>>,
    ) -> Result<Self, String> {
        let (home, ground) = home_of(flavour);
        let mut region = region(flavour, scripts, ground, &home)?;
        region.host.contents = contents;
        for (id, name, _prim, _source) in scripts {
            let _name = region.host.names.insert(*id, (*name).to_owned());
        }
        let _name = region.host.prim_names.insert(1, "Object".to_owned());
        Ok(Self {
            region,
            objects: BTreeMap::new(),
            sources: BTreeMap::new(),
            last_id: 100,
            starting: Vec::new(),
            read: 0,
        })
    }

    /// The flavour the region imitates.
    fn flavour(&self) -> EngineFlavour {
        self.region.engine.config().flavour
    }

    /// Run one tick, then do what the scripts handed the host.
    fn tick(&mut self) -> Result<(), String> {
        self.region.tick()?;
        for (id, running, start) in core::mem::take(&mut self.starting) {
            if let Some(instance) = self.region.engine.instance_mut(id) {
                instance.set_start_parameter(start);
                instance.set_running(running);
            }
        }
        for (key, bits) in core::mem::take(&mut self.region.host.changes) {
            let scripts: Vec<CallerId> = self
                .region
                .host
                .prims
                .iter()
                .filter(|(_, prim)| self.region.host.prim_key(**prim) == key)
                .map(|(id, _)| *id)
                .collect();
            for id in scripts {
                let _posted = self
                    .region
                    .engine
                    .changed(id, bits)
                    .map_err(|error| error.to_string())?;
            }
        }
        for (_caller, load) in core::mem::take(&mut self.region.host.loads) {
            self.load(&load)?;
        }
        for (caller, rez, key) in core::mem::take(&mut self.region.host.rezzes) {
            self.rez(caller, &rez, &key)?;
        }
        Ok(())
    }

    /// A script `load` put in a prim: an instance of its source there,
    /// replacing one of that name, stopped until the next tick is over.
    fn load(&mut self, load: &crate::vm::RemoteLoad) -> Result<(), String> {
        let prim = self
            .region
            .host
            .prim_numbered(&load.target)
            .ok_or("a load into no prim")?;
        let source = self
            .sources
            .get(&load.script)
            .cloned()
            .ok_or("a load of a script with no source")?;
        let held = self
            .region
            .host
            .names
            .iter()
            .find(|(id, name)| {
                name.as_str() == load.script && self.region.host.prim_of(**id) == prim
            })
            .map(|(id, _)| *id);
        let id = held.unwrap_or_else(|| {
            self.last_id = self.last_id.saturating_add(1);
            CallerId(self.last_id)
        });
        self.region.add(id, &load.script, prim, &source)?;
        let _name = self.region.host.names.insert(id, load.script.clone());
        if let Some(instance) = self.region.engine.instance_mut(id) {
            instance.set_running(false);
        }
        self.starting.push((id, load.running, load.start_parameter));
        Ok(())
    }

    /// The object `rez` names, rezzed as `key`: its prim placed and turned
    /// as the rez says, and its scripts brought back as they were saved when
    /// the object was taken, each having run its `state_entry` before.
    fn rez(&mut self, rezzer: CallerId, rez: &crate::vm::Rez, key: &str) -> Result<(), String> {
        let flavour = self.flavour();
        let prim = self
            .region
            .host
            .prim_numbered(key)
            .ok_or("a rez of no prim")?;
        let scripts = self.objects.get(&rez.item).cloned().unwrap_or_default();
        let at = &rez.position;
        self.region.place(prim, at.x, at.y, at.z);
        let mut state = crate::vm::prim::Prim::new_box(key, OWNER);
        state.rotation = rez.rotation.clone();
        if flavour.is_opensim() {
            state.text.alpha = 0.0;
        }
        if rez.die_at_edge {
            state.status |= 128;
        }
        let _state = self.region.host.prim_states.insert(key.to_owned(), state);
        for (name, source) in scripts {
            self.last_id = self.last_id.saturating_add(1);
            let id = CallerId(self.last_id);
            let program = program_for(&rigged(&source, &name, prim), flavour)?;
            let saved = saved_after_entry(&program, flavour)?;
            self.region.add(id, &name, prim, &source)?;
            let _name = self.region.host.names.insert(id, name.clone());
            let _placed = self.region.host.placements.insert(
                id,
                crate::vm::seating::Placement {
                    position: at.clone(),
                    local: at.clone(),
                    root: at.clone(),
                },
            );
            let _posted = self
                .region
                .engine
                .rez_saved(id, program, &saved, Duration::ZERO, rez.start_parameter)
                .map_err(|error| error.to_string())?;
            if let Some(instance) = self.region.engine.instance_mut(id) {
                instance.set_start_string(rez.start_string.clone());
            }
        }
        self.region.object_rezzes.push((rezzer, key.to_owned()));
        Ok(())
    }

    /// Run until a script printed `end` or `ticks` ran out, touching prim A
    /// as the other avatar when the leader waits for it.
    fn run(&mut self, ticks: u32) -> Result<(), String> {
        for _ in 0..ticks {
            self.tick()?;
            let lines = self.region.lines();
            for line in lines.get(self.read..).unwrap_or_default() {
                if line.contains(" rec=waiting drive=touch") {
                    let touch = crate::vm::Detected {
                        key: OTHER.to_owned(),
                        name: name_of(OTHER).to_owned(),
                        owner: OTHER.to_owned(),
                        kind: 1,
                        ..crate::vm::Detected::blank()
                    };
                    let _posted = self
                        .region
                        .engine
                        .post_detected(A, named("touch_start")?, vec![touch])
                        .map_err(|error| error.to_string())?;
                }
            }
            self.read = lines.len();
            if self.region.ended() {
                break;
            }
        }
        Ok(())
    }

    /// Every `PROBE` line printed, in order.
    fn lines(&self) -> Vec<String> {
        self.region.lines()
    }
}

/// What a script of `program` saves when its object is taken after it ran
/// its `state_entry` where it was made — what a rez of that object brings it
/// back from.
fn saved_after_entry(
    program: &std::sync::Arc<crate::bytecode::Program>,
    flavour: EngineFlavour,
) -> Result<crate::vm::SavedScript, String> {
    let mut engine = engine_as(flavour);
    let mut host = Recorder::default();
    let id = CallerId(1);
    let _none = engine.add(id, std::sync::Arc::clone(program));
    let _reports = run(&mut engine, &mut host, 5);
    engine
        .take(id)
        .ok_or_else(|| "no instance to take".to_owned())
}

/// Hold the leader's lines and the other scripts' lines of `stage` to the
/// transcript's, each without the `vary` fields; `others` names the records
/// the other scripts print.
fn hold_lines(
    stage: &Stage,
    transcript: &str,
    (others, vary, unordered): (&[&str], &[&str], &[&str]),
    context: &str,
) {
    let (ours_leader, ours_others) = split(&stage.lines(), others);
    let (their_leader, their_others) = split(&probe_lines(transcript), others);
    assert_eq!(
        without(canonical(&ours_leader, unordered), vary),
        without(canonical(&their_leader, unordered), vary),
        "{context}: the leader's lines"
    );
    assert_eq!(
        without(ours_others, vary),
        without(their_others, vary),
        "{context}: the other scripts' lines"
    );
}

#[test]
fn listing_reads_and_changes_the_contents_as_each_grid_did() -> Result<(), String> {
    let probe = include_str!("../../../probes/inventory/listing/probe.lsl");
    let helper = include_str!("../../../probes/inventory/listing/helper.lsl");
    let ranges = [
        (
            "Range minus one",
            include_str!("../../../probes/inventory/listing/range-minus-one.lsl"),
        ),
        (
            "Range past end",
            include_str!("../../../probes/inventory/listing/range-past-end.lsl"),
        ),
        (
            "Range all 100",
            include_str!("../../../probes/inventory/listing/range-all-100.lsl"),
        ),
        (
            "Range no type",
            include_str!("../../../probes/inventory/listing/range-no-type.lsl"),
        ),
    ];
    for (flavour, transcript) in transcripts("listing") {
        // The scripts run in the order the runner saved them: the helper,
        // the range scripts, the leader.
        let mut scripts = vec![
            (CallerId(20), "Probe", 1, probe.to_owned()),
            (CallerId(9), "aaa Helper", 1, helper.to_owned()),
        ];
        let mut in_b = Vec::new();
        for (index, (name, source)) in (10_u64..).zip(ranges) {
            scripts.push((CallerId(index), name, 2, source.to_owned()));
            in_b.push(item(index, name, INVENTORY_SCRIPT));
        }
        let mut in_a: Vec<Item> = (1_u64..)
            .zip(noted_notecards(transcript))
            .map(|(number, name)| item(number, &name, INVENTORY_NOTECARD))
            .collect();
        let mut boxed = item(20, "Box", INVENTORY_OBJECT);
        boxed.masks.next = 532_480;
        in_a.extend([
            boxed,
            item(21, "Probe", INVENTORY_SCRIPT),
            item(22, "aaa Helper", INVENTORY_SCRIPT),
        ]);
        if flavour.is_opensim() {
            // The masks as an OpenSim region holds them, export and the
            // object's own bits in: `PermissionMaskToLSLPerm` reads them as
            // the probe printed.
            for held in &mut in_a {
                let raw = |mask: i32| {
                    if mask == crate::vm::contents::PERM_ALL {
                        0x0009_E000
                    } else {
                        mask | 0xF
                    }
                };
                held.masks.base = raw(held.masks.base);
                held.masks.owner = raw(held.masks.owner);
                held.masks.next = raw(held.masks.next);
            }
        }
        let contents = BTreeMap::from([(1, in_a), (2, in_b)]);
        let mut stage = Stage::new(flavour, &scripts, contents)?;
        // Mauve runs no outside scripts; the local grid's parcel runs all.
        let mut flags = PARCEL_FLAG_ALLOW_CREATE_OBJECTS | PARCEL_FLAG_ALLOW_ALL_OBJECT_ENTRY;
        if flavour.is_opensim() {
            flags |= PARCEL_FLAG_ALLOW_SCRIPTS;
        }
        stage.region.host.parcel = Some(super::land::parcel(flags, OTHER));
        stage.run(400)?;
        hold_lines(
            &stage,
            transcript,
            (&["helper-entry", "helper-pong", "name"], &[], &[]),
            &format!("{flavour:?}"),
        );
        assert_eq!(
            first_lines(stage.region.debug_lines()),
            debug_notes(transcript),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn notecards_answer_over_the_dataserver_as_each_grid_did() -> Result<(), String> {
    let probe = include_str!("../../../probes/inventory/notecards/probe.lsl");
    let cards = [
        (
            "Lines",
            include_str!("../../../probes/inventory/notecards/Lines.notecard"),
        ),
        (
            "Empty",
            include_str!("../../../probes/inventory/notecards/Empty.notecard"),
        ),
        (
            "NoNewline",
            include_str!("../../../probes/inventory/notecards/NoNewline.notecard"),
        ),
        (
            "Many",
            include_str!("../../../probes/inventory/notecards/Many.notecard"),
        ),
    ];
    for (flavour, transcript) in transcripts("notecards") {
        let mut in_a = vec![item(9, "Probe", INVENTORY_SCRIPT)];
        let mut texts = BTreeMap::new();
        for (number, (name, text)) in (1_u64..).zip(cards) {
            let card = item(number, name, INVENTORY_NOTECARD);
            let _text = texts.insert(card.asset.clone(), text.to_owned());
            in_a.push(card);
        }
        let mut stage = Stage::new(
            flavour,
            &[(A, "Probe", 1, probe.to_owned())],
            BTreeMap::from([(1, in_a)]),
        )?;
        stage.region.host.notecards = texts;
        stage.run(1200)?;
        hold_lines(
            &stage,
            transcript,
            (&[], &["delay", "reply"], &[]),
            &format!("{flavour:?}"),
        );
        assert_eq!(
            first_lines(stage.region.debug_lines()),
            debug_notes(transcript),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn rezzing_from_the_contents_answers_as_each_grid_did() -> Result<(), String> {
    let probe = include_str!("../../../probes/inventory/rez/probe.lsl").to_owned();
    let child = include_str!("../../../probes/inventory/rez/child.lsl");
    for (flavour, transcript) in transcripts("rez") {
        let in_a = vec![
            item(1, "Child", INVENTORY_OBJECT),
            item(2, "Notes", INVENTORY_NOTECARD),
            item(3, "Probe", INVENTORY_SCRIPT),
        ];
        let mut stage = Stage::new(
            flavour,
            &[(A, "Probe", 1, probe.clone())],
            BTreeMap::from([(1, in_a)]),
        )?;
        let _child = stage.objects.insert(
            "Child".to_owned(),
            vec![("Child".to_owned(), child.to_owned())],
        );
        stage.run(800)?;
        hold_lines(
            &stage,
            transcript,
            (&[], &["delay", "seconds"], &["burst-rez"]),
            &format!("{flavour:?}"),
        );
        assert_eq!(
            first_lines(stage.region.debug_lines()),
            debug_notes(transcript),
            "{flavour:?}"
        );
    }
    Ok(())
}

/// What a transcript's runner noted of each offer an avatar was sent:
/// whether it went to the other avatar, its dialog, whether it was marked
/// offline, its length and the start of its message.
fn noted_offers(transcript: &str) -> Vec<(bool, String, String, usize, String)> {
    transcript
        .lines()
        .filter(|line| line.starts_with("# im "))
        .map(|line| {
            let bare = |key: &str| {
                line.split(' ')
                    .find_map(|field| field.strip_prefix(&format!("{key}=")))
                    .unwrap_or_default()
                    .to_owned()
            };
            let message = note_field(line, "message").unwrap_or_default();
            (
                line.starts_with("# im by=other"),
                bare("dialog"),
                bare("offline"),
                bare("length").parse().unwrap_or(0),
                message.trim_end_matches('…').to_owned(),
            )
        })
        .collect()
}

#[test]
fn giving_from_the_contents_answers_as_each_grid_did() -> Result<(), String> {
    let receiver = include_str!("../../../probes/inventory/give/receiver.lsl");
    let loaded = include_str!("../../../probes/inventory/give/loaded.lsl");
    let passed = include_str!("../../../probes/inventory/give/passed.lsl");
    let probe = include_str!("../../../probes/inventory/give/probe.lsl").replace(
        "llSensor(\"Receiver\", NULL_KEY, PASSIVE | ACTIVE | SCRIPTED, 10.0, PI);",
        "receiver = \"00000000-0000-4000-8000-000000000002\"; emit(\"found\", \" n=1\"); \
         llSetTimerEvent(2.0);",
    );
    for (flavour, transcript) in transcripts("give") {
        let in_a = vec![
            item(1, "Gift", INVENTORY_NOTECARD),
            item(2, "Passed", INVENTORY_SCRIPT),
            item(3, "Loaded", INVENTORY_SCRIPT),
            item(4, "Probe", INVENTORY_SCRIPT),
        ];
        let in_b = vec![item(5, "Receiver", INVENTORY_SCRIPT)];
        let mut stage = Stage::new(
            flavour,
            &[
                (B, "Receiver", 2, receiver.to_owned()),
                (A, "Probe", 1, probe.clone()),
            ],
            BTreeMap::from([(1, in_a), (2, in_b)]),
        )?;
        let _loaded = stage.sources.insert("Loaded".to_owned(), loaded.to_owned());
        let _passed = stage.sources.insert("Passed".to_owned(), passed.to_owned());
        stage.region.host.replace_raises_twice = flavour.is_opensim();
        if !flavour.is_opensim() {
            stage.region.host.at = Some((
                "Mauve".to_owned(),
                Vector {
                    x: 52.0,
                    y: 62.0,
                    z: 34.62,
                },
            ));
        }
        stage.run(800)?;
        let others: &[&str] = &["receiver-changed", "loaded", "passed-runs"];
        if flavour == EngineFlavour::XEngine {
            // A deviation: the stopped script loaded over the running one
            // raised two `changed` on both OpenSim engines, the remove and the
            // add, and XEngine read it as running in the second though it
            // printed nothing; YEngine read it stopped, which the runtime
            // imitates on both.
            let (ours_leader, ours_others) = split(&stage.lines(), others);
            let (their_leader, their_others) = split(&probe_lines(transcript), others);
            assert_eq!(
                without(ours_leader, &["delay"]),
                without(their_leader, &["delay"]),
                "XEngine: the leader's lines"
            );
            let last = "PROBE inventory/give rec=receiver-changed change=1 \
                        items=\"Gift:7|Gift 1:7|Gift 2:7|Loaded:10:1|Passed:10:0|Receiver:10:1\"";
            assert_eq!(their_others.last().map(String::as_str), Some(last));
            assert_eq!(
                ours_others.get(..ours_others.len().saturating_sub(1)),
                their_others.get(..their_others.len().saturating_sub(1)),
                "XEngine: the other scripts' lines"
            );
        } else {
            hold_lines(
                &stage,
                transcript,
                (others, &["delay"], &[]),
                &format!("{flavour:?}"),
            );
        }
        // Each call's forced delay, to half a second (a grid's frames add a
        // little): OpenSim sleeps for a gift to an avatar alone.
        let delays = |lines: &[String]| -> Vec<(String, f64)> {
            lines
                .iter()
                .filter_map(|line| crate::probe::Line::parse(line).ok())
                .filter(|line| line.rec == "called")
                .filter_map(|line| {
                    let label = line.field("label")?.to_owned();
                    let delay: f64 = line.field("delay")?.parse().ok()?;
                    Some((label, (delay * 2.0).round()))
                })
                .collect()
        };
        assert_eq!(
            delays(&stage.lines()),
            delays(&probe_lines(transcript)),
            "{flavour:?}: the delays"
        );
        let offers: Vec<(bool, String, String, usize, String)> = stage
            .region
            .host
            .offers_made
            .iter()
            .map(|(_, offer)| {
                (
                    offer.agent == OTHER,
                    "9".to_owned(),
                    if offer.offline { "1" } else { "0" }.to_owned(),
                    offer.message.chars().count(),
                    offer.message.clone(),
                )
            })
            .collect();
        let noted = noted_offers(transcript);
        assert_eq!(offers.len(), noted.len(), "{flavour:?}: the offers");
        for (ours, theirs) in offers.iter().zip(&noted) {
            assert_eq!(
                (&ours.0, &ours.1, &ours.2, &ours.3),
                (&theirs.0, &theirs.1, &theirs.2, &theirs.3),
                "{flavour:?}"
            );
            assert!(
                ours.4.starts_with(&theirs.4),
                "{flavour:?}: {} is not {}",
                ours.4,
                theirs.4
            );
        }
        assert_eq!(
            first_lines(stage.region.debug_lines()),
            debug_notes(transcript),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn the_newer_functions_answer_as_second_life_did() -> Result<(), String> {
    let probe = include_str!("../../../probes/inventory/newer/probe.lsl");
    let child = include_str!("../../../probes/inventory/newer/child.lsl");
    let transcript = include_str!("../../../probes/results/aditi/inventory/newer.txt");
    let mut lines = item(1, "Lines", INVENTORY_NOTECARD);
    lines.description = "sl-conformance lsl-probes".to_owned();
    let mut object = item(2, "Child", INVENTORY_OBJECT);
    object.description = "(No Description)".to_owned();
    let in_a = vec![lines.clone(), object, item(3, "Probe", INVENTORY_SCRIPT)];
    let mut stage = Stage::new(
        EngineFlavour::SecondLife,
        &[(A, "Probe", 1, probe.to_owned())],
        BTreeMap::from([(1, in_a)]),
    )?;
    let _text = stage
        .region
        .host
        .notecards
        .insert(lines.asset, "one\ntwo\nthree\n".to_owned());
    let _child = stage.objects.insert(
        "Child".to_owned(),
        vec![("Child".to_owned(), child.to_owned())],
    );
    stage.run(400)?;
    hold_lines(&stage, transcript, (&[], &[], &[]), "SecondLife");
    assert_eq!(
        first_lines(stage.region.debug_lines()),
        debug_notes(transcript)
    );
    Ok(())
}

/// OpenSim compiles none of the six task-inventory functions Second Life
/// added after its library was written: its engines refused the probe's
/// child at `llGetStartString` (`inventory/newer`), and none of the six is in
/// its `LSL_Api`.
#[test]
fn opensim_compiles_none_of_the_newer_functions() {
    let child = include_str!("../../../probes/inventory/newer/child.lsl");
    let opensim = crate::compile_for(child, crate::Dialect::OpenSim).ok();
    let second_life = crate::compile_for(child, crate::Dialect::SecondLife).ok();
    assert!(
        opensim.is_none(),
        "OpenSim compiles none of the newer functions"
    );
    assert!(second_life.is_some(), "Second Life compiles them all");
    for call in [
        "llGetInventoryDesc(\"x\");",
        "llGetInventoryAcquireTime(\"x\");",
        "llGetNotecardLineSync(\"x\", 0);",
        "llRezObjectWithParams(\"x\", [REZ_PARAM, 1]);",
        "llDerezObject(NULL_KEY, DEREZ_DIE);",
        "llOwnerSay(NAK);",
    ] {
        let source = format!("default {{ state_entry() {{ {call} }} }}");
        assert!(
            crate::compile_for(&source, crate::Dialect::OpenSim).is_err(),
            "{call}"
        );
        assert!(
            crate::compile_for(&source, crate::Dialect::SecondLife).is_ok(),
            "{call}"
        );
    }
}
