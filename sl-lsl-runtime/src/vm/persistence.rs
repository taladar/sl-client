//! What of a running script survives its object being taken into inventory
//! and rezzed again, or its region going down and coming back: the
//! [`SavedScript`].
//!
//! On a real grid a script's running state is part of its object, not of the
//! simulator: a vendor taken and rezzed a week later is still in the state it
//! was in, its globals, timer, listens, permissions and queued events intact
//! (`book/src/gridspec/lsl.md` § Persistence). A [`SavedScript`] is all of
//! that — and the body in progress, down to its program counter, call frames
//! and operand stack — as plain data, free of any engine's clock: a sleep or
//! a timer is kept as the time left of it, never as a tick, so it can be
//! brought back in a region whose clock reads anything.
//!
//! It holds no program. The compiled script is the inventory item's, and is
//! handed back on the restore, which checks the saved state against it —
//! a global, a state, a body or an event the program does not have is a
//! [`RestoreError`], never a script running off the end of its code.
//!
//! [`SavedScript::encode`] writes it in a compact form of this crate's own
//! (not OpenSim's `ScriptState` XML: that form describes OpenSim's engines'
//! internals, and a fake grid's script never runs under them), and
//! [`SavedScript::decode`] reads it back, for the grid to keep with the item.
//!
//! What a take, a rez and a restart do with it differs by grid, and is the
//! [`Engine`](crate::vm::Engine)'s: [`Engine::take`](crate::vm::Engine::take),
//! [`Engine::rez_saved`](crate::vm::Engine::rez_saved),
//! [`Engine::shut_down`](crate::vm::Engine::shut_down) and
//! [`Engine::start_up`](crate::vm::Engine::start_up).

mod codec;

use core::time::Duration;

use sl_lsl::ast::TypeName;

use crate::bytecode::{FunctionId, StateId};
use crate::operand::Operand;
use crate::value::Value;
use crate::vm::comms::Listens;
use crate::vm::detected::Detected;
use crate::vm::flavour::EngineFlavour;
use crate::vm::instance::PostError;
use crate::vm::objects::{ListId, ListObjects};
use crate::vm::permissions::Permissions;

pub use codec::DecodeError;

/// `CHANGED_REGION_START`: the bit of the `changed` every script of a region
/// is raised once the region is back up after a restart.
pub const CHANGED_REGION_START: i32 = 0x400;

/// The bit OpenSim raises in a `changed` of its own before
/// [`CHANGED_REGION_START`] after a restart — its `CHANGED_POSITION`, which
/// Second Life has no constant for (`persistence/restart`, both engines,
/// 2026-10-26).
pub const OPENSIM_CHANGED_POSITION: i32 = 0x8000;

/// A script's running state, saved: everything of an
/// [`Instance`](crate::vm::Instance) but its program and the region's
/// accounting of the time it took.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedScript {
    /// The engine it ran under, whose memory measure its accounting is in.
    pub(crate) flavour: EngineFlavour,
    /// One value per global.
    pub(crate) globals: Vec<Value>,
    /// The list objects it held and which each global holds, where lists are
    /// objects ([`crate::vm::objects`]).
    pub(crate) objects: ListObjects,
    /// The state it was in.
    pub(crate) state: StateId,
    /// The run flag.
    pub(crate) running: bool,
    /// Held by a run-time error until a reset.
    pub(crate) held: bool,
    /// The events waiting for their handlers, oldest first.
    pub(crate) queue: Vec<SavedEvent>,
    /// The body in progress.
    pub(crate) activity: Option<SavedActivity>,
    /// A state change the body in progress asked for.
    pub(crate) pending_state: Option<StateId>,
    /// What was left of a sleep; zero for a script awake.
    pub(crate) asleep: Duration,
    /// The repeating timer.
    pub(crate) timer: Option<SavedTimer>,
    /// `llGetStartParameter`.
    pub(crate) start_parameter: i32,
    /// The script's time (`llGetTime`) when it was saved.
    pub(crate) script_time: Duration,
    /// The detected block of the event in progress.
    pub(crate) detected: Vec<Detected>,
    /// The listens.
    pub(crate) listens: Listens,
    /// The permissions held and the questions waiting for answers.
    pub(crate) permissions: Permissions,
    /// What of the memory accounting is the script's own.
    pub(crate) memory: SavedMemory,
}

/// A queued event, saved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedEvent {
    /// The event's name, looked up in the library table again on the
    /// restore.
    pub(crate) event: String,
    /// Its arguments.
    pub(crate) args: Vec<Value>,
    /// Its detected block, for a detection event.
    pub(crate) detected: Option<Vec<Detected>>,
}

/// Which body a saved frame executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SavedBody {
    /// The global initialisers.
    Init,
    /// A user function.
    Function(FunctionId),
    /// The handler at `index` of `state`.
    Handler {
        /// The state.
        state: StateId,
        /// The handler's index in it.
        index: u32,
    },
}

/// Why the bottom body of a saved activity runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SavedPhase {
    /// The global initialisers.
    Init,
    /// A queued event's handler.
    Event,
    /// `state_entry`.
    Entry,
    /// `state_exit`, on the way to the state given.
    Exit(StateId),
}

/// One call in progress, saved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedFrame {
    /// The body.
    pub(crate) body: SavedBody,
    /// The next instruction.
    pub(crate) pc: u32,
    /// The parameters and locals.
    pub(crate) locals: Vec<Value>,
    /// The list object each local holds.
    pub(crate) objects: Vec<Option<ListId>>,
    /// What each local holds toward running out.
    pub(crate) costs: Vec<u32>,
}

/// The body in progress, saved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedActivity {
    /// Why the bottom body runs.
    pub(crate) phase: SavedPhase,
    /// The call stack, the executing frame last.
    pub(crate) frames: Vec<SavedFrame>,
    /// The operand stack.
    pub(crate) stack: Vec<Operand>,
    /// The library call at the executing frame's instruction has had its
    /// forced delay already (XEngine's media functions sleep first).
    pub(crate) slept_first: bool,
}

/// The repeating timer, saved as lengths of time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavedTimer {
    /// The time between two `timer` events.
    pub(crate) interval: Duration,
    /// The time left until the next.
    pub(crate) next: Duration,
}

/// What of a script's memory accounting is its own rather than its
/// program's: the rest is counted again on the restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavedMemory {
    /// `llGetMemoryLimit`.
    pub(crate) limit: u32,
    /// Mono's low-water mark.
    pub(crate) peak: u32,
    /// Whether the profiler is on.
    pub(crate) profiling: bool,
    /// What the profiler found.
    pub(crate) profiled: Option<u32>,
}

/// Why a [`SavedScript`] could not be brought back with the program it was
/// handed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RestoreError {
    /// It was saved under another engine, whose memory measure its
    /// accounting is in.
    #[error("saved under {saved:?}, restored under {region:?}")]
    Flavour {
        /// The engine it was saved under.
        saved: EngineFlavour,
        /// The engine of the region it was brought to.
        region: EngineFlavour,
    },
    /// Its globals are not the program's: another count, or another type in
    /// a slot.
    #[error("the saved globals do not match the program's")]
    Globals,
    /// It names a state the program does not have.
    #[error("the program has no state {0}")]
    State(u32),
    /// It names a function the program does not have.
    #[error("the program has no function {0}")]
    Function(u32),
    /// It names a handler the program does not have, or one for another
    /// event.
    #[error("the program has no handler {index} in state {state}")]
    Handler {
        /// The state.
        state: u32,
        /// The handler's index.
        index: u32,
    },
    /// A frame's program counter is past its body's end, or its locals are
    /// not the body's.
    #[error("a saved frame does not fit its body")]
    Frame,
    /// A body in progress with no frame.
    #[error("a saved body in progress has no frame")]
    NoFrame,
    /// A queued event the library does not know, the state has no handler
    /// for, or whose arguments are not its parameters.
    #[error("the queued `{0}` cannot be handled in the saved state")]
    Event(String),
    /// The event that tells the script it is back (`on_rez`, `changed`) did
    /// not fit its parameters — a bug of this crate's.
    #[error(transparent)]
    Post(#[from] PostError),
}

impl SavedScript {
    /// The engine it ran under.
    #[must_use]
    pub const fn flavour(&self) -> EngineFlavour {
        self.flavour
    }

    /// The globals, in the program's order.
    #[must_use]
    pub fn globals(&self) -> &[Value] {
        &self.globals
    }

    /// The state it was in.
    #[must_use]
    pub const fn state(&self) -> StateId {
        self.state
    }

    /// Whether its run flag was set.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// How many events were queued.
    #[must_use]
    pub const fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Whether a body was in progress.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.activity.is_some()
    }

    /// What was left of a sleep; zero for a script awake.
    #[must_use]
    pub const fn asleep(&self) -> Duration {
        self.asleep
    }

    /// Whether a timer was set.
    #[must_use]
    pub const fn has_timer(&self) -> bool {
        self.timer.is_some()
    }

    /// The listens.
    #[must_use]
    pub const fn listens(&self) -> &Listens {
        &self.listens
    }

    /// The permissions.
    #[must_use]
    pub const fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    /// Write it in this crate's compact form, for the grid to keep with the
    /// inventory item.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        codec::encode(self)
    }

    /// Read back what [`Self::encode`] wrote.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] when `bytes` are not a saved script this crate wrote:
    /// another form, a later version of this one, or cut short.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        codec::decode(bytes)
    }

    /// End the sleep it was in: OpenSim's YEngine ends one at a take and at a
    /// shutdown, and the handler goes on once the script is back.
    pub(crate) const fn wake(&mut self) {
        self.asleep = Duration::ZERO;
    }

    /// Drop the body in progress: XEngine loses a handler a shutdown came in,
    /// though the events queued behind it run.
    pub(crate) fn abandon(&mut self) {
        self.activity = None;
        self.pending_state = None;
        self.asleep = Duration::ZERO;
    }

    /// Give every global a list object of its own: XEngine saves a script's
    /// globals by copying each list (`ScriptBase.GetVars`), so two globals
    /// that held one list before a shutdown hold two equal lists after it.
    pub(crate) fn separate_lists(&mut self) {
        self.objects.globals.fill(None);
    }

    /// `time` passed while it was away: take it off the sleep and the timer.
    pub(crate) const fn elapse(&mut self, time: Duration) {
        self.asleep = self.asleep.saturating_sub(time);
        if let Some(timer) = self.timer.as_mut() {
            timer.next = timer.next.saturating_sub(time);
        }
    }
}

/// Whether `value` is of the type `ty`.
pub(crate) fn is_of(value: &Value, ty: TypeName) -> bool {
    value.type_name() == ty
}

/// `ticks` ticks of `step` each, as a length of time.
pub(crate) fn span(ticks: u64, step: Duration) -> Duration {
    let nanos = step.as_nanos().saturating_mul(u128::from(ticks));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// How many ticks of `step` cover `time`, rounded up: a sleep or a timer
/// never ends before the time it was given.
pub(crate) fn ticks_in(time: Duration, step: Duration) -> u64 {
    let step = step.as_nanos().max(1);
    u64::try_from(time.as_nanos().div_ceil(step)).unwrap_or(u64::MAX)
}
