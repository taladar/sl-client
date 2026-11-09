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

/// How many error lines one object may say in the open within
/// [`FLOOD_WINDOW_SECONDS`] on Second Life: aditi passed eight a second apart
/// and put the notice in place of the ninth (`errors/flood`,
/// `errors/flood-kinds`, `errors/flood-paths`), whatever the lines were —
/// refusals of any function and faults count alike, a fault's two lines as
/// one (`errors/runtime-errors`: four faults and a refusal within the window
/// all passed).
pub const FLOOD_LINES: usize = 8;

/// The line Second Life says in place of the first one a flood drops.
pub const FLOOD_NOTICE: &str = "Too many errors... dropping further messages until the flood stops";

/// The window, in seconds, over which an object's error lines are counted.
/// Aditi's own turns about a second either way from one phase to the next:
/// eight lines and a ninth 16 seconds after the first flooded in one phase
/// and passed in the next three alike, and at 17 seconds the same
/// (`errors/flood-window`, `errors/flood-repeat`); nine lines 1.5 seconds
/// apart passed nine to eleven (`errors/flood-records`), and a script's own
/// `llOwnerSay` lines did not change that. Sixteen gets the fewest of those
/// lines wrong; the replays name the rest.
pub const FLOOD_WINDOW_SECONDS: f64 = 16.0;

/// How long a flood lasts at least, in seconds from the line that started
/// it: every error line of the object is dropped meanwhile, and the object
/// starts afresh after it once [`FLOOD_GAP_SECONDS`] passed with no line. Aditi still dropped a line 32 seconds into a flood and
/// passed one 37 seconds in, ten and eleven seconds after the last line it
/// had dropped (`errors/flood-kinds`' `A11`, run before and after its other
/// refusals changed), and passed eight lines in a row starting 34.9 seconds
/// after one (`errors/flood-paths`' `X2`): the length is the flood's, not a
/// quiet's. Thirty-four is a decision inside the measured range; aditi kept
/// two floods with a line every 2.2 and 3.2 seconds going 56 and 49 seconds,
/// and one with a line every 2.7 seconds 36 (`errors/flood-repeat`).
pub const FLOOD_SECONDS: f64 = 34.0;

/// The quiet, in seconds since the object's previous error line, a flood
/// needs before it may end: a complaint every 1.5 seconds kept one going
/// for ten minutes (`errors/flood-epoch`), where one every 2.2 to 3.2
/// seconds let it end (`errors/flood-repeat`'s `G2` to `G3`).
pub const FLOOD_GAP_SECONDS: f64 = 2.0;

/// The longest error line OpenSim's engines send, in characters; they cut
/// anything longer (`XMRInstRun.SendScriptErrorMessage`,
/// `ScriptInstance.EventProcessor`).
const OPENSIM_MAX_LINE: usize = 1000;

/// A line a library call says on `DEBUG_CHANNEL` without stopping the
/// script: a refusal of its arguments, or OpenSim's shouted OSSL warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The text, worded for the flavour.
    pub text: String,
    /// Whether it is shouted to avatars and scripts alike, as OpenSim's
    /// `OSSLShoutError` does, rather than carried as the flavour carries a
    /// refusal (`refusal_reach`).
    pub shouted: bool,
    /// Whether Second Life tells it the owner alone rather than saying it,
    /// as it tells the complaints of a rule list — `llSetPrimitiveParams`,
    /// `llGetPrimitiveParams`, `llParticleSystem`, `llSetPrimMediaParams` —
    /// where OpenSim says them as any other (`errors/owner-lines`).
    pub to_owner: bool,
}

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
        (_, RuntimeError::Thrown { thrown, .. }) => Some(thrown.text.clone()),
        (
            _,
            other @ (RuntimeError::InvalidProgram
            | RuntimeError::TooManyListens
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
/// exception, where, and that a reset is needed. An exception whose message
/// has several lines is told a line at a time (`ossl/strings`'
/// `ArgumentOutOfRangeException`, 2026-11-01). YEngine sends the engine's
/// stack trace after them, a line a frame of its own C#, which is left out.
#[must_use]
pub(crate) fn owner_lines(flavour: EngineFlavour, fault: &Fault, at: &Whereabouts) -> Vec<String> {
    match (flavour, &fault.error) {
        (
            EngineFlavour::YEngine,
            error @ (RuntimeError::OutOfHeap { .. } | RuntimeError::Exception { .. }),
        ) => core::iter::once(format!("[YEngine]: Exception while running {}", at.item))
            .chain(error.to_string().lines().map(str::to_owned))
            .chain([
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
            ])
            .collect(),
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

/// Who an error line is said to, as the flood gate tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Audience {
    /// Said in the open, to every avatar and script in range.
    Near,
    /// Told the owner alone, as Second Life tells a rule list's complaints
    /// (`errors/owner-lines`).
    Owner,
}

/// One object's recent error lines.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flood {
    /// The ticks of the lines passed within the window, oldest first.
    passed: Vec<Tick>,
    /// The tick the object's flood may end, while it floods.
    until: Option<Tick>,
    /// The tick of the object's last line, passed or dropped.
    last: Option<Tick>,
}

/// Second Life's flood limit on error lines, per object. A line said in the
/// open passes while fewer than [`FLOOD_LINES`] passed within the last
/// [`FLOOD_WINDOW_SECONDS`]; the one that would make more is replaced by
/// [`FLOOD_NOTICE`], and every line of the object is dropped for
/// [`FLOOD_SECONDS`] and until it has been quiet for [`FLOOD_GAP_SECONDS`],
/// after which it starts afresh. A line told the owner
/// alone is let one line deeper, and overflows silently: aditi passed nine
/// such lines a fifth of a second apart, then nothing and no notice
/// (`errors/flood-kinds`' `C`, `errors/flood-paths`' `S`), while one of them
/// as the eighth line still made the next open one the notice
/// (`errors/flood-paths`' `X1` to `X5`). Another object's lines are not
/// affected.
#[derive(Debug, Clone)]
pub(crate) struct FloodGate {
    /// The objects that passed a line within the window or flood, by key.
    objects: BTreeMap<String, Flood>,
}

/// How long the flood gate's counts run, in ticks of the engine's step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloodTicks {
    /// The window lines are counted over.
    pub(crate) window: u64,
    /// The length of a flood.
    pub(crate) flood: u64,
    /// The quiet a flood needs before it ends.
    pub(crate) gap: u64,
}

impl FloodGate {
    /// No object flooding.
    pub(crate) const fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
        }
    }

    /// What to do with a line `object` says to `audience` at `now`.
    pub(crate) fn admit(
        &mut self,
        object: &str,
        audience: Audience,
        now: Tick,
        ticks: FloodTicks,
    ) -> Admission {
        let recent = |tick: &Tick| tick.after(ticks.window) > now;
        let flooding = |flood: &Flood| match flood.until {
            Some(until) => {
                until > now || flood.last.is_some_and(|last| last.after(ticks.gap) > now)
            }
            None => false,
        };
        self.objects
            .retain(|_, flood| flooding(flood) || flood.passed.iter().any(recent));
        let flood = self.objects.entry(object.to_owned()).or_insert(Flood {
            passed: Vec::new(),
            until: None,
            last: None,
        });
        let still = flooding(flood);
        flood.last = Some(now);
        if still {
            return Admission::Drop;
        }
        flood.until = None;
        flood.passed.retain(recent);
        let limit = match audience {
            Audience::Near => FLOOD_LINES,
            Audience::Owner => FLOOD_LINES.saturating_add(1),
        };
        if flood.passed.len() < limit {
            flood.passed.push(now);
            return Admission::Pass;
        }
        flood.passed.clear();
        flood.until = Some(now.after(ticks.flood));
        match audience {
            Audience::Near => Admission::Notice,
            Audience::Owner => Admission::Drop,
        }
    }
}
