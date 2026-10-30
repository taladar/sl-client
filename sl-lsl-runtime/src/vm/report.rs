//! What a script that goes wrong says, and who hears it: the words of a
//! run-time error and of a library call refusing its arguments, in each
//! flavour's measured shape, and Second Life's per-object flood limit on
//! them (book chapter `gridspec/lsl.md`, § Run-time errors).

use std::collections::BTreeMap;

use sl_types::chat::ChatVolume;
use sl_types::lsl::Vector;

use crate::error::Division;
use crate::vm::context::Tick;
use crate::vm::fault::{Fault, RuntimeError};
use crate::vm::flavour::EngineFlavour;

/// `DEBUG_CHANNEL`: the channel errors are said on, and the one content
/// listens on to hear its own.
pub const DEBUG_CHANNEL: i32 = 0x7FFF_FFFF;

/// How many error lines one object may say in a flood on Second Life before
/// the rest are dropped: aditi passed eight and dropped the ninth
/// (`errors/flood`, 2026-10-27).
pub const FLOOD_LINES: u32 = 8;

/// The line Second Life says in place of the first one a flood drops.
pub const FLOOD_NOTICE: &str = "Too many errors... dropping further messages until the flood stops";

/// The quiet, in seconds, after which an object's flood is over and its error
/// lines pass again. Aditi still dropped a line after 10 seconds of quiet and
/// passed one after 30; twenty is a decision inside that measured window.
pub const FLOOD_QUIET_SECONDS: f64 = 20.0;

/// The longest error line OpenSim's engines send, in characters; they cut
/// anything longer (`XMRInstRun.SendScriptErrorMessage`,
/// `ScriptInstance.EventProcessor`).
const OPENSIM_MAX_LINE: usize = 1000;

/// A line of chat a script's object says, and how far it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speech {
    /// The channel; [`DEBUG_CHANNEL`] for an error.
    pub channel: i32,
    /// The text.
    pub text: String,
    /// How far avatars hear it.
    pub to_avatars: ChatVolume,
    /// How far scripts' `listen`s hear it, or [`None`] when no script does —
    /// OpenSim's run-time errors reach avatars only.
    pub to_scripts: Option<ChatVolume>,
}

/// Where a script is, as its error lines name it. The host answers it
/// ([`Host::whereabouts`](crate::vm::Host::whereabouts)); the runtime knows
/// none of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Whereabouts {
    /// The key of the object (the link set) the script is in: Second Life
    /// counts a flood per object.
    pub object: String,
    /// The key of the prim the script is in (OpenSim's `primID`).
    pub prim: String,
    /// The prim's name as it is now: the speaker, and Second Life's prefix.
    pub name: String,
    /// The script's inventory name.
    pub script: String,
    /// The key of the script's inventory item, which YEngine's exception
    /// report names.
    pub item: String,
    /// The name of the region the prim is in.
    pub region: String,
    /// The prim's region position.
    pub position: Vector,
}

/// How far a run-time error carries in `flavour`: on Second Life a script
/// within say range hears it (one at 19 m did, one at 21 m did not); on
/// OpenSim it reaches the avatars near and no script.
#[must_use]
pub(crate) const fn fault_reach(flavour: EngineFlavour) -> Option<ChatVolume> {
    match flavour {
        EngineFlavour::SecondLife => Some(ChatVolume::Say),
        EngineFlavour::YEngine | EngineFlavour::XEngine => None,
    }
}

/// How far a library call's refusal carries to scripts in `flavour`: say
/// range on Second Life; on OpenSim a shout (`LSL_Api.Error` hands it to the
/// listens as one — a script 50 m away heard it).
#[must_use]
pub(crate) const fn refusal_reach(flavour: EngineFlavour) -> ChatVolume {
    if flavour.is_opensim() {
        ChatVolume::Shout
    } else {
        ChatVolume::Say
    }
}

/// The `DEBUG_CHANNEL` lines a fault says in `flavour`, in order; none when
/// the flavour's engine says nothing (OpenSim's runaway recursion takes the
/// simulator down or is swallowed rather than reported).
#[must_use]
pub(crate) fn fault_lines(flavour: EngineFlavour, fault: &Fault, at: &Whereabouts) -> Vec<String> {
    // OpenSim runs a script's global initialisers as it loads it, ahead of
    // `default`'s `state_entry`; an error there is named after that event
    // (not measured: no probe faults in an initialiser).
    let event = fault.event.unwrap_or("state_entry");
    match flavour {
        EngineFlavour::SecondLife => vec![
            format!("{} [script:{}] Script run-time error", at.name, at.script),
            fault.error.to_string(),
        ],
        EngineFlavour::YEngine => opensim_error(flavour, &fault.error)
            .map(|what| {
                cut(format!(
                    "YEngine: {what} (script: {} event: {event} primID: {} at: <{},{},{}>) \
                     Script must be Reset to re-enable.\n",
                    at.script,
                    at.prim,
                    whole_metres(at.position.x),
                    whole_metres(at.position.y),
                    whole_metres(at.position.z),
                ))
            })
            .into_iter()
            .collect(),
        // XEngine says a .NET exception out of the library as `FormatException`
        // words it — the exception wrapped in the reflection call's — with no
        // `(script: …)` after it (`ScriptInstance.EventProcessor`). The stack
        // trace that follows on the grid, a line a frame of its own C#, is
        // left out.
        EngineFlavour::XEngine => match &fault.error {
            RuntimeError::Exception { exception, .. } => vec![cut(format!(
                "System.Reflection.TargetInvocationException: Exception has been thrown by \
                 the target of an invocation. ---> System.{exception}"
            ))],
            error => opensim_error(flavour, error)
                .map(|what| {
                    cut(format!(
                        "{what}(script: {} event: {event} primID:{} at <{}, {}, {}>)",
                        at.script, at.prim, at.position.x, at.position.y, at.position.z,
                    ))
                })
                .into_iter()
                .collect(),
        },
    }
}

/// What an OpenSim engine calls `error`, or [`None`] for one it does not
/// report. The errors that are this runtime's own keep their own words.
fn opensim_error(flavour: EngineFlavour, error: &RuntimeError) -> Option<String> {
    match (flavour, error) {
        (EngineFlavour::XEngine, RuntimeError::MathError(Division::Integer)) => {
            Some("Integer division by Zero".to_owned())
        }
        (EngineFlavour::XEngine, RuntimeError::MathError(Division::Float)) => {
            Some("Float division by zero".to_owned())
        }
        (_, RuntimeError::MathError(Division::Vector)) => {
            Some("Vector division by zero".to_owned())
        }
        (_, RuntimeError::MathError(Division::Integer | Division::Float)) => {
            Some("Division by Zero".to_owned())
        }
        // An exhausted heap, and on YEngine an exception out of the library,
        // is told to the owner instead ([`owner_lines`]); XEngine's exception
        // has its own words ([`fault_lines`]).
        (
            _,
            RuntimeError::StackHeapCollision
            | RuntimeError::OutOfHeap { .. }
            | RuntimeError::Exception { .. },
        ) => None,
        (
            _,
            other @ (RuntimeError::InvalidProgram
            | RuntimeError::HostMemory
            | RuntimeError::Unimplemented(_)
            | RuntimeError::Internal(_)),
        ) => Some(other.to_string()),
    }
}

/// The lines a fault tells the script's owner alone in `flavour`, in order,
/// for a fault reported that way rather than on `DEBUG_CHANNEL`: YEngine's
/// `OutOfHeapException` (`memory/collision`, 2026-10-26) and an exception
/// out of a library function (`errors/bounds`, 2026-10-27) — the item, the
/// exception, where, and that a reset is needed. YEngine sends the engine's
/// stack trace after them, a line a frame of its own C#, which is left out.
#[must_use]
pub(crate) fn owner_lines(flavour: EngineFlavour, fault: &Fault, at: &Whereabouts) -> Vec<String> {
    match (flavour, &fault.error) {
        (
            EngineFlavour::YEngine,
            error @ (RuntimeError::OutOfHeap { .. } | RuntimeError::Exception { .. }),
        ) => vec![
            format!("[YEngine]: Exception while running {}", at.item),
            error.to_string(),
            format!(
                "Prim: <{}>, Script: <{}>, Location: {} <{},{},{}>",
                at.name,
                at.script,
                at.region,
                whole_metres(at.position.x),
                whole_metres(at.position.y),
                whole_metres(at.position.z),
            ),
            "Script must be Reset to re-enable.".to_owned(),
        ],
        _ => Vec::new(),
    }
}

/// YEngine's position: each coordinate rounded down to a whole metre, and
/// written without a fraction.
const fn whole_metres(coordinate: f32) -> f32 {
    coordinate.floor()
}

/// Cut `line` to [`OPENSIM_MAX_LINE`] characters.
fn cut(line: String) -> String {
    if line.chars().count() > OPENSIM_MAX_LINE {
        line.chars().take(OPENSIM_MAX_LINE).collect()
    } else {
        line
    }
}

/// What the flood gate does with one error line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Say it.
    Pass,
    /// Say [`FLOOD_NOTICE`] in its place.
    Notice,
    /// Say nothing.
    Drop,
}

/// One object's run of error lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flood {
    /// Lines in the run, passed or not.
    lines: u32,
    /// The tick of the last of them.
    last: Tick,
}

/// Second Life's flood limit on error lines, per object: [`FLOOD_LINES`]
/// pass, the next is [`FLOOD_NOTICE`], the rest are dropped until the object
/// has been quiet for [`FLOOD_QUIET_SECONDS`]. A dropped line keeps the
/// flood going. Another object's lines are not affected.
#[derive(Debug, Clone)]
pub(crate) struct FloodGate {
    /// The runs under way, by object key.
    objects: BTreeMap<String, Flood>,
}

impl FloodGate {
    /// No object flooding.
    pub(crate) const fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
        }
    }

    /// What to do with a line `object` says at `now`; a run ends after
    /// `quiet` ticks without one.
    pub(crate) fn admit(&mut self, object: &str, now: Tick, quiet: u64) -> Admission {
        self.objects
            .retain(|_, flood| flood.last.after(quiet) > now);
        let flood = self.objects.entry(object.to_owned()).or_insert(Flood {
            lines: 0,
            last: now,
        });
        flood.lines = flood.lines.saturating_add(1);
        flood.last = now;
        match flood.lines {
            lines if lines <= FLOOD_LINES => Admission::Pass,
            lines if lines == FLOOD_LINES.saturating_add(1) => Admission::Notice,
            _ => Admission::Drop,
        }
    }
}
