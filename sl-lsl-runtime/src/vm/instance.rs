//! One running script: its instance state, and the interpreter that advances
//! it a bounded number of instructions at a time.

use std::collections::VecDeque;
use std::sync::Arc;

use core::time::Duration;

use sl_lsl::ast::{BinaryOp, TypeName};
use sl_types::lsl::{Rotation, Vector};

use crate::bytecode::{Body, CodeOffset, Component, FunctionId, Instr, Program, StateId};
use crate::library::{self, Called, Event};
use crate::num::ticks_for;
use crate::operand::{self, Operand};
use crate::value::{Element, Value};
use crate::vm::comms::Listens;
use crate::vm::context::{Peers, Raised, Request, ScriptCtx, ScriptData, Tick};
use crate::vm::cost;
use crate::vm::delays;
use crate::vm::detected::{Detected, MAX_DETECTED, is_detection_event};
use crate::vm::fault::{Fault, RuntimeError};
use crate::vm::flavour::EngineFlavour;
use crate::vm::host::{CallerId, Host};
use crate::vm::memory::{Memory, Stack, frame_cost};
use crate::vm::objects::{self, ListId, ListObjects};
use crate::vm::ossl::{self, Admitted};
use crate::vm::permissions::Permissions;
use crate::vm::report::Warning;

mod saved;

pub(crate) use saved::{restore, save};

/// Instructions charged for calling a library function, on top of the one the
/// `CallBuiltin` instruction itself costs (book chapter
/// `simulator/lsl-engine.md`, section 3).
///
/// Measured on aditi (Mono, 2026-09-28, loops of 400 000 and 1 000 000
/// iterations, about two seconds each, so `llGetTime`'s whole-frame steps
/// are a 1% error): a loop calling `llAbs` runs at 226 400 iterations a
/// second where the same loop empty runs at 500 000, so one of its iterations
/// costs 2.21 empty ones — 1148 charged instructions against 520. The `llAbs`
/// iteration is eleven instructions and a back-edge (523) plus this.
pub const BUILTIN_CALL_COST: u32 = 625;

/// Instructions charged for a jump taken backwards — a loop going round
/// again — on top of the one the jump itself costs.
///
/// Measured on aditi (the same runs as [`BUILTIN_CALL_COST`]): an empty `for`
/// loop, eight of our instructions an iteration, runs at 500 000 iterations
/// a second, and one that also assigns, multiplies and adds (fourteen) at
/// 494 300 — six more instructions cost barely one per cent. Mono's time is in
/// the scheduler check at each loop back-edge, not in the arithmetic between
/// them. With this, the two loops charge 520 and 526 an iteration, which
/// predicts 494 300 for the second exactly.
pub const BACKWARD_JUMP_COST: u32 = 512;

/// The deepest the call stack may grow. A frame costs a Mono script at least
/// sixteen bytes of its 64 KiB, so no script can recurse deeper than this on
/// the grid; past it the script stops with a stack-heap collision rather than
/// growing the host's memory without bound. Mono's memory accounting stops a
/// recursion well before it (`vm::memory`'s stack model); this is the
/// backstop for OpenSim's engines, whose recursion takes the simulator down
/// rather than stopping at a depth anyone measured.
pub const MAX_CALL_DEPTH: usize = 4096;

/// What one running script adds to its object's `OBJECT_SCRIPT_MEMORY` on
/// OpenSim, whatever it holds: both engines answer a flat 16 384.
pub const OPENSIM_SCRIPT_MEMORY: u32 = 0x4000;

/// The most events a script's queue holds; one posted past it is dropped.
/// Second Life's documented limit.
pub const MAX_QUEUED: usize = 64;

/// The detection events that coalesce: posted again in the tick an earlier
/// one of the same event is still queued in, they merge into it — the
/// detected lists joined, up to [`MAX_DETECTED`] — rather than queue a
/// second event, so every avatar touching in one frame arrives as one
/// `touch_start` with a count. A sensor sweep is one event already.
const COALESCING: [&str; 6] = [
    "collision",
    "collision_end",
    "collision_start",
    "touch",
    "touch_end",
    "touch_start",
];

/// Which queued event a coalescing event — a touch, a collision, a `changed`
/// — may merge into.
///
/// On the grid it is frame timing that decides whether two changes or two
/// touches arrive as one event or two, and a script cannot choose; so a
/// script has to handle both shapes, and a test has to be able to force
/// each. [`Self::Reference`] is Second Life's behaviour and the default;
/// the other two are the extremes a script may meet, for a content test to
/// run a scenario under both and check it copes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coalescing {
    /// Second Life's rule, measured on aditi (2026-09-28). Everything raised
    /// in one tick merges — the grid gathers one frame's changes and
    /// touches into one event. A `changed` raised later also merges into the
    /// newest `changed` still queued, **unless that one is next in line**:
    /// four changes a fifth of a second apart, made while the script was
    /// busy, arrived as the first alone and the other three as one, every
    /// time (and three half a second apart, queued behind another event,
    /// as one: `events/merging`, 2026-10-25). A touch raised later merges
    /// into the same touch event still queued, wherever it stands: three
    /// touches a second apart, made while the script slept, arrived as one
    /// `touch_start`, one `touch` and one `touch_end` (`events/merging`).
    /// Collisions merge within the tick only here, which is not all aditi
    /// does: a collision that began and ended while the script slept
    /// arrived not at all (`events/collision`) — the raiser's task,
    /// `server-world-collision-and-physics`, carries that.
    ///
    /// The queue is not first in, first out either (`events/order`): a
    /// sweep's answer waits behind every other kind of event queued with it
    /// (`events/latency`: behind a timer and a link message raised 1.5 s
    /// after it), a touch behind those and the answers, and a `changed`
    /// behind everything, whenever each was raised.
    #[default]
    Reference,
    /// Never merge: every raise is an event of its own, the most split-up
    /// arrival a script can see. OpenSim's rule, on both engines
    /// (`events/merging`, 2026-10-25): five colour changes in one handler
    /// are five `changed`, three touches three `touch_start`s.
    Never,
    /// Merge into the same event while it is still queued, whatever tick it
    /// was raised in — the most merged arrival a script can see.
    WhileQueued,
}

impl Coalescing {
    /// Whether an event posted at `now` may merge into one posted at
    /// `queued`; `behind_head` says the queued one is not next in line and
    /// its kind follows the reference's queue rule (only `changed` does).
    const fn merges(self, queued: Tick, now: Tick, behind_head: bool) -> bool {
        match self {
            Self::Reference => queued.0 == now.0 || behind_head,
            Self::Never => false,
            Self::WhileQueued => true,
        }
    }

    /// Whether a touch event may merge into the same touch event still
    /// queued: on the reference wherever it was raised.
    const fn merges_touch(self) -> bool {
        match self {
            Self::Reference | Self::WhileQueued => true,
            Self::Never => false,
        }
    }

    /// Where an event named `event` waits in a queue under this rule: behind
    /// every queued event of a lower rank and ahead of every one of a higher.
    /// One rank for all of them is first in, first out; the reference ranks
    /// a sweep's answers behind the rest, touches behind those and a
    /// `changed` behind everything (`events/order`, `events/latency`).
    fn rank(self, event: &str) -> u8 {
        match self {
            Self::Reference if event == "changed" => 3,
            Self::Reference if TOUCHES.contains(&event) => 2,
            Self::Reference if SWEEP_ANSWERS.contains(&event) => 1,
            Self::Reference | Self::Never | Self::WhileQueued => 0,
        }
    }
}

/// The three touch events.
const TOUCHES: [&str; 3] = ["touch", "touch_end", "touch_start"];

/// The two events a sweep answers with.
const SWEEP_ANSWERS: [&str; 2] = ["no_sensor", "sensor"];

/// When an event is posted and how it may merge: the tick, the region's
/// [`Coalescing`], and the tick its handler may start in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    /// The tick the event is posted in.
    pub tick: Tick,
    /// Which queued event it may merge into.
    pub coalescing: Coalescing,
    /// The first tick its handler may start in. An event is queued at the
    /// moment it is raised, so a state change or a reset after it drops it,
    /// yet Second Life runs a link message no sooner than the next frame and
    /// a setter's `changed` and a sweep's answer two frames on
    /// (`events/latency`, `events/state-change`).
    pub ready: Tick,
}

impl Arrival {
    /// This arrival, its handler held back `ticks` ticks after the tick it is
    /// posted in.
    #[must_use]
    pub const fn ready_after(self, ticks: u64) -> Self {
        Self {
            ready: self.tick.after(ticks),
            ..self
        }
    }
}

/// The events that never stack: one already queued, a second is dropped. A
/// timer that falls due while its last event is still waiting fires once,
/// not twice.
const NON_STACKING: [&str; 1] = ["timer"];

/// What one call of [`Instance::run_slice`] ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to run: the script is stopped, asleep, or has no body in
    /// progress and no event queued.
    Idle,
    /// The budget ran out in the middle of a body; the next slice resumes at
    /// exactly that instruction.
    Yielded,
    /// A body ran to its end — the global initialisers, an event handler,
    /// `state_entry` or `state_exit` — without the state changing.
    Finished,
    /// The script suspended itself (`llSleep`, or a library function's forced
    /// delay) and is not runnable before this tick.
    Sleeping(Tick),
    /// The script started another (`llSetScriptState`) that was stopped in
    /// the middle of a body whose sleep has run out, and stepped aside for it:
    /// the engine runs that script's turn before this one's next slice, which
    /// resumes after the call.
    Handed(CallerId),
    /// The state changed to this one: `state_exit` of the old state has run,
    /// the event queue is empty, and `state_entry` of the new one is next.
    StateChanged(StateId),
    /// The script reset itself (`llResetScript`): globals, stack and queue
    /// are gone and the initialisers run next, then `default`'s
    /// `state_entry`.
    Reset,
    /// The script called `llDie` and the host is removing its object: the
    /// body in progress and the queue are gone and the script is stopped, so
    /// nothing of it runs again whatever the host does next.
    Died,
    /// A run-time error ended the body in progress. What became of the
    /// script is the flavour's: on Second Life it is stopped and held
    /// ([`Instance::is_held`]), keeping its state and its globals; on YEngine
    /// it is held but reads as running; on XEngine it runs on with the next
    /// event.
    Faulted(Fault),
}

/// The result of one [`Instance::run_slice`].
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// How it ended.
    pub outcome: Outcome,
    /// Instructions charged, counting each library call and each loop
    /// back-edge at its cost. May exceed the budget by the cost of the one
    /// instruction that crossed it, never by more.
    pub used: u32,
    /// What library calls of the slice said refusing their arguments
    /// (`Could not find script 'Nope'.`), in order, for the engine to say on
    /// `DEBUG_CHANNEL`.
    pub refusals: Vec<Warning>,
    /// What library calls of the slice raised in other scripts — changes of
    /// the linkset's data store, link messages — in the order of the calls,
    /// for the engine to post.
    pub raised: Vec<Raised>,
}

/// What became of a posted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    /// Queued behind any earlier events.
    Queued,
    /// Dropped: the current state has no handler for it. An event nobody
    /// handles is never queued, so it cannot fire after a later state change.
    NoHandler,
    /// Dropped: the script is not running, or is held by a run-time error
    /// until it is reset. Such a script receives no events at all.
    Stopped,
    /// Merged into the same event already queued this tick: a touch's or
    /// collision's detected list grew, or a `changed`'s bits were OR-ed in,
    /// instead of a second event queuing.
    Merged,
    /// Dropped: an event of a kind that never stacks is already queued.
    AlreadyQueued,
    /// Dropped: the queue holds [`MAX_QUEUED`] events already.
    QueueFull,
}

/// A posted event whose arguments do not match the event's parameters — a
/// host bug.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostError {
    /// The wrong number of arguments.
    #[error("`{event}` takes {expected} arguments, got {found}")]
    Arity {
        /// The event.
        event: &'static str,
        /// The number of parameters it has.
        expected: usize,
        /// The number of arguments posted.
        found: usize,
    },
    /// `state_entry` and `state_exit` are the VM's own, run on a state
    /// change; nothing posts them.
    #[error("`{0}` is raised by a state change, not posted")]
    Transition(&'static str),
    /// A detection event posted without its detected block, or an event
    /// without one posted with one
    /// ([`Instance::post_detected`] is for the first kind only).
    #[error("`{0}` carries a detected block exactly when it is a detection event")]
    Detection(&'static str),
    /// A detected block that is empty or longer than [`MAX_DETECTED`].
    #[error("`{event}` needs 1 to 16 detections, got {found}")]
    DetectedCount {
        /// The event.
        event: &'static str,
        /// How many were posted.
        found: usize,
    },
    /// An argument of the wrong type.
    #[error("argument {index} of `{event}` must be {expected:?}, got {found:?}")]
    Argument {
        /// The event.
        event: &'static str,
        /// The zero-based argument position.
        index: usize,
        /// The parameter's type.
        expected: TypeName,
        /// The argument's type.
        found: TypeName,
    },
}

/// Which body a frame executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyRef {
    /// The global initialisers.
    Init,
    /// A user function.
    Function(FunctionId),
    /// An event handler: the handler at `index` of `state`.
    Handler {
        /// The state it belongs to.
        state: StateId,
        /// Its index in the state's handlers.
        index: usize,
    },
}

/// Why the body at the bottom of the call stack is running, which decides
/// what happens when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The global initialisers; `state_entry` of the current state follows.
    Init,
    /// A queued event's handler.
    Event,
    /// `state_entry`.
    Entry,
    /// `state_exit`, on the way to the state given.
    Exit(StateId),
}

/// One call in progress.
#[derive(Debug, Clone)]
struct Frame {
    /// The body it executes.
    body: BodyRef,
    /// The next instruction to execute.
    pc: u32,
    /// The body's parameters and locals.
    locals: Vec<Value>,
    /// The list object each local holds, where lists are objects
    /// ([`crate::vm::objects`]).
    objects: Vec<Option<ListId>>,
    /// What each local, as stored, holds toward running out
    /// ([`Memory::local_cost`]); a parameter counts nothing until it is
    /// stored to, holding what its caller holds.
    costs: Vec<u32>,
    /// What the frame itself costs toward running out ([`frame_cost`]).
    cost: u32,
}

/// The body in progress: the call stack and the operand stack.
#[derive(Debug, Clone)]
struct Activity {
    /// Why the bottom body runs.
    phase: Phase,
    /// The call stack; the last frame is executing.
    frames: Vec<Frame>,
    /// The operand stack, shared by every frame (a frame's operands sit
    /// above its caller's). A float on it may still be a double
    /// ([`Operand`]).
    stack: Vec<Operand>,
    /// What the frames and their locals hold toward running out, all
    /// together.
    held: Stack,
    /// The library call at the executing frame's instruction has had its
    /// forced delay already, before it ran: XEngine sleeps first in the
    /// functions of [`SLEEPS_FIRST`], so the call runs when it wakes.
    slept_first: bool,
}

impl Activity {
    /// A body with `frame` at the bottom of its call stack.
    fn new(phase: Phase, frame: Frame) -> Self {
        let held = Stack {
            frames: frame.cost,
            locals: 0,
        };
        Self {
            phase,
            frames: vec![frame],
            stack: Vec::new(),
            held,
            slept_first: false,
        }
    }
}

/// The library functions whose forced delay OpenSim takes before the call
/// rather than after it (`ScriptSleep` first in `LSL_Api`): the media ones.
/// XEngine blocks its thread there, so what the call does — the `changed`
/// it raises — comes after a timer that fell due in the delay
/// (`events/timer-reset`); YEngine suspends a script only when the call has
/// returned, which puts every delay after its call.
const SLEEPS_FIRST: [&str; 6] = [
    "llClearLinkMedia",
    "llClearPrimMedia",
    "llGetLinkMedia",
    "llGetPrimMediaParams",
    "llSetLinkMedia",
    "llSetPrimMediaParams",
];

/// The library functions that decide as they run whether to take a delay
/// before what they do (XEngine blocks its thread in `ScriptSleep` there):
/// OpenSim's chat throttle sleeps an `llSay` or `llShout` before the line is
/// said ([`delays::SayThrottle`]).
const DECIDES_SLEEP_FIRST: [&str; 2] = ["llSay", "llShout"];

/// An event waiting for its handler.
#[derive(Debug, Clone)]
struct Queued {
    /// The event.
    event: &'static Event,
    /// The handler's index in the current state.
    handler: usize,
    /// The arguments, already checked against the event's parameters.
    args: Vec<Value>,
    /// The detected block, for a detection event.
    detected: Option<Vec<Detected>>,
    /// The tick it was posted in, which decides whether a later one merges.
    posted: Tick,
    /// The first tick its handler may start in; until then the events
    /// behind it that are free to run go first.
    ready: Tick,
    /// How many handlers the script had begun when it was posted: the
    /// events raised while one handler ran are ranked among themselves, and
    /// wait behind every event raised before it began.
    batch: u64,
}

/// What executing one instruction leads to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Flow {
    /// Carry on with the next instruction.
    Next,
    /// The bottom body of the activity returned.
    BodyDone,
    /// Suspend for this many seconds, then carry on.
    Sleep(f64),
    /// Reset the script.
    Reset,
    /// End the script: its object is leaving the region (`llDie`).
    Died,
    /// End the slice so the script started by the call runs first.
    Hand(CallerId),
}

/// One script instance: a shared compiled program and everything that is this
/// copy's own — globals, current state, event queue, run flag, the body in
/// progress with its call and operand stacks, and the tick it sleeps until.
///
/// All of "where the script is" is plain data, so a slice can stop between
/// any two instructions and the next resumes there.
#[derive(Debug, Clone)]
pub struct Instance {
    /// The compiled script, shared by every copy of it.
    program: Arc<Program>,
    /// One value per [`Program::globals`] entry.
    globals: Vec<Value>,
    /// The list objects the script holds, and which each global holds, where
    /// lists are objects ([`crate::vm::objects`]).
    objects: ListObjects,
    /// The current state.
    state: StateId,
    /// The run flag (`SetScriptRunning`, the contents' Running checkbox).
    running: bool,
    /// Held by a run-time error: nothing runs, whatever the run flag says,
    /// until a reset.
    held: bool,
    /// Events waiting for their handlers, oldest first.
    queue: VecDeque<Queued>,
    /// How many event handlers the script has begun, which numbers the
    /// batch an event posted now joins ([`Queued::batch`]).
    begun: u64,
    /// The body in progress, if any.
    activity: Option<Activity>,
    /// A state change requested by the body in progress, carried out when
    /// its event handler ends.
    pending_state: Option<StateId>,
    /// The first tick the script may run again.
    wake: Tick,
    /// Reset once it has run every event it has queued: YEngine's reset of a
    /// running script by another (`llResetOtherScript`), which delivers what
    /// was queued first (`inventory/listing`).
    reset_when_idle: bool,
    /// What the library may read and write: the detected block, the timer
    /// and the start parameter.
    data: ScriptData,
    /// Instructions charged past the share of a tick already served — one
    /// operation that cost more than was left, a join of a long list — and
    /// owed out of the shares of the ticks that follow.
    overdraft: u32,
    /// What the script's memory holds and may hold, as its grid counts it.
    memory: Memory,
    /// Instructions charged in the tick under way, for [`Self::load`].
    charged: u64,
    /// The instructions the script runs a tick, averaged over
    /// [`LOAD_WINDOW_SECONDS`] of ticks: its share of the region's script
    /// time.
    load: f64,
}

/// How many seconds of ticks a script's load and the region's active-script
/// count are averaged over. The reference's top-scripts report averages its
/// script times over a window too; its length is not measured, and thirty
/// seconds is a decision.
pub const LOAD_WINDOW_SECONDS: f64 = 30.0;

/// The body a [`BodyRef`] names.
fn body_of(program: &Program, body: BodyRef) -> Result<&Body, RuntimeError> {
    match body {
        BodyRef::Init => Some(&program.init),
        BodyRef::Function(id) => program.function(id).map(|function| &function.body),
        BodyRef::Handler { state, index } => program
            .state(state)
            .and_then(|state| state.handlers.get(index))
            .map(|handler| &handler.body),
    }
    .ok_or_else(|| internal(format!("no body {body:?}")))
}

/// A broken compiler promise.
fn internal(what: impl Into<String>) -> RuntimeError {
    RuntimeError::Internal(what.into())
}

/// A frame for `body`, its parameters filled from `args` and every other
/// local at its type's default, costed as `flavour` costs a frame.
fn new_frame(
    program: &Program,
    body: BodyRef,
    args: Vec<Value>,
    flavour: EngineFlavour,
) -> Result<Frame, RuntimeError> {
    let code = body_of(program, body)?;
    let params = usize::try_from(code.params).unwrap_or(usize::MAX);
    if args.len() != params {
        return Err(internal(format!(
            "{body:?} takes {params} arguments, got {}",
            args.len()
        )));
    }
    let mut locals = args;
    locals.extend(
        code.locals
            .iter()
            .skip(params)
            .map(|local| Value::default_of(local.ty)),
    );
    Ok(Frame {
        body,
        pc: 0,
        costs: vec![0; locals.len()],
        objects: vec![None; locals.len()],
        locals,
        cost: frame_cost(code, flavour),
    })
}

/// The index of `state`'s handler for `event`, if it has one.
fn handler_index(program: &Program, state: StateId, event: &str) -> Option<usize> {
    program
        .state(state)?
        .handlers
        .iter()
        .position(|handler| handler.event.name == event)
}

/// The top operand as it is.
fn pop_operand(stack: &mut Vec<Operand>) -> Result<Operand, RuntimeError> {
    stack
        .pop()
        .ok_or_else(|| internal("operand stack underflow"))
}

/// The top operand as a value, a double rounded to a float: what a store, an
/// argument and every other consumer that is not an operator takes.
fn pop(stack: &mut Vec<Operand>) -> Result<Value, RuntimeError> {
    pop_operand(stack).map(Operand::round)
}

/// The top operand, which must be a float, rounded.
fn pop_float(stack: &mut Vec<Operand>) -> Result<f32, RuntimeError> {
    match pop(stack)? {
        Value::Float(float) => Ok(float),
        other => Err(internal(format!(
            "expected a float operand, found {:?}",
            other.type_name()
        ))),
    }
}

/// The top `count` operands, deepest first, each rounded.
fn pop_n(stack: &mut Vec<Operand>, count: usize) -> Result<Vec<Value>, RuntimeError> {
    let at = stack
        .len()
        .checked_sub(count)
        .ok_or_else(|| internal("operand stack underflow"))?;
    Ok(stack
        .split_off(at)
        .into_iter()
        .map(Operand::round)
        .collect())
}

/// The top `count` operands, deepest first, each rounded, with the list
/// object each hands on.
fn pop_n_held(
    stack: &mut Vec<Operand>,
    count: usize,
) -> Result<Vec<(Value, Option<ListId>)>, RuntimeError> {
    let at = stack
        .len()
        .checked_sub(count)
        .ok_or_else(|| internal("operand stack underflow"))?;
    Ok(stack
        .split_off(at)
        .into_iter()
        .map(objects::split)
        .collect())
}

/// `slot` as an index.
fn index(slot: u32) -> usize {
    usize::try_from(slot).unwrap_or(usize::MAX)
}

/// One component of a vector or rotation.
fn get_member(aggregate: &Value, component: Component) -> Result<f32, RuntimeError> {
    match (aggregate, component) {
        (Value::Vector(Vector { x, .. }) | Value::Rotation(Rotation { x, .. }), Component::X)
        | (
            Value::Vector(Vector { y: x, .. }) | Value::Rotation(Rotation { y: x, .. }),
            Component::Y,
        )
        | (
            Value::Vector(Vector { z: x, .. }) | Value::Rotation(Rotation { z: x, .. }),
            Component::Z,
        )
        | (Value::Rotation(Rotation { s: x, .. }), Component::S) => Ok(*x),
        (other, _) => Err(internal(format!(
            "no member .{} of {:?}",
            component.name(),
            other.type_name()
        ))),
    }
}

/// A vector or rotation with one component replaced.
fn set_member(aggregate: Value, component: Component, to: f32) -> Result<Value, RuntimeError> {
    let mut aggregate = aggregate;
    let slot = match (&mut aggregate, component) {
        (Value::Vector(Vector { x, .. }) | Value::Rotation(Rotation { x, .. }), Component::X)
        | (
            Value::Vector(Vector { y: x, .. }) | Value::Rotation(Rotation { y: x, .. }),
            Component::Y,
        )
        | (
            Value::Vector(Vector { z: x, .. }) | Value::Rotation(Rotation { z: x, .. }),
            Component::Z,
        )
        | (Value::Rotation(Rotation { s: x, .. }), Component::S) => x,
        (other, _) => {
            return Err(internal(format!(
                "no member .{} of {:?}",
                component.name(),
                other.type_name()
            )));
        }
    };
    *slot = to;
    Ok(aggregate)
}

/// Everything one instruction may touch, borrowed apart from the instance so
/// the program can be read while the rest is written.
struct Machine<'run> {
    /// The compiled script.
    program: &'run Program,
    /// The globals.
    globals: &'run mut [Value],
    /// The script's list objects.
    objects: &'run mut ListObjects,
    /// The body in progress.
    activity: &'run mut Activity,
    /// Where a `state` statement records its target.
    pending_state: &'run mut Option<StateId>,
    /// The calling instance, for the host.
    caller: CallerId,
    /// The current tick.
    now: Tick,
    /// The length of a tick.
    step: Duration,
    /// The world.
    host: &'run mut dyn Host,
    /// The library-visible state.
    data: &'run mut ScriptData,
    /// The region's other scripts.
    peers: Peers<'run>,
    /// The script's memory accounting.
    memory: &'run mut Memory,
}

impl Machine<'_> {
    /// Execute one instruction, adding what it costs to `used`.
    fn step(&mut self, used: &mut u32) -> Result<Flow, RuntimeError> {
        let program = self.program;
        let Activity {
            frames,
            stack,
            held,
            slept_first,
            ..
        } = &mut *self.activity;
        let frame = frames
            .last_mut()
            .ok_or_else(|| internal("no frame to execute"))?;
        let instr = *body_of(program, frame.body)?
            .code
            .get(index(frame.pc))
            .ok_or_else(|| internal("ran past the end of a body"))?;
        frame.pc = frame.pc.saturating_add(1);
        *used = used.saturating_add(1);
        match instr {
            Instr::Const(id) => stack.push(
                program
                    .constant(id)
                    .cloned()
                    .ok_or_else(|| internal(format!("no constant #{}", id.0)))?
                    .into(),
            ),
            Instr::Pop => {
                let _discarded = pop(stack)?;
            }
            Instr::Dup => {
                let top = stack
                    .last_mut()
                    .ok_or_else(|| internal("operand stack underflow"))?;
                self.objects.share(top);
                let top = top.clone();
                stack.push(top);
            }
            Instr::LoadLocal(slot) => {
                let at = index(slot.0);
                let value = frame
                    .locals
                    .get(at)
                    .cloned()
                    .ok_or_else(|| internal(format!("no local {}", slot.0)))?;
                let object = match frame.objects.get_mut(at) {
                    Some(held) => self.objects.of_slot(held, &value),
                    None => None,
                };
                stack.push(objects::operand(value, object));
            }
            Instr::StoreLocal(slot) => {
                let (value, object) = objects::split(pop_operand(stack)?);
                let cost = self.memory.local_cost(&value);
                let at = index(slot.0);
                *frame
                    .locals
                    .get_mut(at)
                    .ok_or_else(|| internal(format!("no local {}", slot.0)))? = value;
                if let Some(held) = frame.objects.get_mut(at) {
                    *held = object;
                }
                let before = held.locals;
                if let Some(old) = frame.costs.get_mut(at) {
                    held.locals = before.saturating_sub(*old).saturating_add(cost);
                    *old = cost;
                }
                self.memory.store_local(before, *held)?;
            }
            Instr::LoadGlobal(slot) => {
                let at = index(slot.0);
                let value = self
                    .globals
                    .get(at)
                    .cloned()
                    .ok_or_else(|| internal(format!("no global {}", slot.0)))?;
                let mut held_object = self.objects.globals.get(at).copied().flatten();
                let object = self.objects.of_slot(&mut held_object, &value);
                if let Some(slot_object) = self.objects.globals.get_mut(at) {
                    *slot_object = object;
                }
                stack.push(objects::operand(value, object));
            }
            Instr::StoreGlobal(slot) => {
                let (value, object) = objects::split(pop_operand(stack)?);
                let at = index(slot.0);
                let global = self
                    .globals
                    .get_mut(at)
                    .ok_or_else(|| internal(format!("no global {}", slot.0)))?;
                self.memory.store_global(at, &value, *held)?;
                *global = value;
                if let Some(slot_object) = self.objects.globals.get_mut(at) {
                    *slot_object = object;
                }
            }
            Instr::GetMember(component) => {
                let aggregate = pop(stack)?;
                stack.push(Value::Float(get_member(&aggregate, component)?).into());
            }
            Instr::SetMember(component) => {
                let aggregate = pop(stack)?;
                let to = pop_float(stack)?;
                stack.push(set_member(aggregate, component, to)?.into());
            }
            Instr::Cast(ty) => {
                let result = match pop_operand(stack)? {
                    // A list cast to a list is the same object on OpenSim
                    // (`vm/list-identity`'s `cast`).
                    shared @ Operand::Shared(..) if ty == TypeName::List => shared,
                    operand => operand::cast(operand, ty, self.memory.flavour().is_opensim())?,
                };
                if let Operand::Value(value) = &result {
                    self.memory.made(value)?;
                }
                stack.push(result);
            }
            Instr::Binary(op) => {
                let left = pop_operand(stack)?;
                let right = pop_operand(stack)?;
                let in_place = objects::appends_in_place(self.memory.flavour(), op, &left, &right);
                let result = match (self.memory.flavour().is_opensim(), op, &left, &right) {
                    // OpenSim's own formula for a vector by a rotation.
                    (
                        true,
                        BinaryOp::Mul | BinaryOp::Div,
                        Operand::Value(Value::Vector(vector)),
                        Operand::Value(Value::Rotation(rotation)),
                    ) => Operand::Value(Value::Vector(crate::ops::opensim_rotate(
                        vector,
                        rotation,
                        op == BinaryOp::Div,
                    ))),
                    _ => operand::binary(op, left, right)?,
                };
                if let Operand::Value(value) = &result {
                    *used = used.saturating_add(cost::operator(op, value));
                    self.memory.made(value)?;
                }
                let result = match (in_place, result) {
                    (Some(id), Operand::Value(Value::List(elements))) => {
                        objects::rewrite(
                            id,
                            &elements,
                            (&mut *self.globals, &self.objects.globals),
                            frames
                                .iter_mut()
                                .map(|frame| (&mut *frame.locals, &*frame.objects)),
                            stack,
                        );
                        Operand::Shared(elements, id)
                    }
                    (_, result) => result,
                };
                stack.push(result);
            }
            Instr::Prefix(op) => {
                let operand = pop_operand(stack)?;
                stack.push(operand::prefix(op, operand)?);
            }
            Instr::BuildVector => {
                let z = pop_float(stack)?;
                let y = pop_float(stack)?;
                let x = pop_float(stack)?;
                stack.push(Value::Vector(Vector { x, y, z }).into());
            }
            Instr::BuildRotation => {
                let s = pop_float(stack)?;
                let z = pop_float(stack)?;
                let y = pop_float(stack)?;
                let x = pop_float(stack)?;
                // OpenSim's `LSL_Types.Quaternion(double, double, double,
                // double)`, which both its engines build a rotation
                // expression with, makes four zeros the identity
                // (`errors/math`); its string constructor, and so a cast,
                // does not.
                let s = if self.memory.flavour().is_opensim()
                    && [x, y, z, s].iter().all(|component| *component == 0.0)
                {
                    1.0
                } else {
                    s
                };
                stack.push(Value::Rotation(Rotation { x, y, z, s }).into());
            }
            Instr::BuildList(count) | Instr::BuildListReversed(count) => {
                let mut popped = pop_n(stack, index(count))?;
                if matches!(instr, Instr::BuildListReversed(_)) {
                    popped.reverse();
                }
                let elements = popped
                    .into_iter()
                    .map(|value| match value {
                        Value::List(_) => Err(internal("a list element that is a list")),
                        other => Ok(other.into_elements()),
                    })
                    .collect::<Result<Vec<Vec<Element>>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                let list = Value::List(elements);
                self.memory.made(&list)?;
                stack.push(list.into());
            }
            Instr::CallBuiltin(id) => {
                let descriptor = id.descriptor();
                let flavour = self.peers.flavour();
                let factor = self.peers.delay_factor();
                let forced = delays::scaled(flavour, delays::forced(flavour, descriptor), factor);
                let sleeps_first = forced > 0.0
                    && flavour == EngineFlavour::XEngine
                    && SLEEPS_FIRST.contains(&descriptor.name);
                let woke = core::mem::take(slept_first);
                if sleeps_first && !woke {
                    // The delay first; the call runs again from the top when
                    // the script wakes, its arguments still on the stack.
                    *slept_first = true;
                    frame.pc = frame.pc.saturating_sub(1);
                    return Ok(Flow::Sleep(forced));
                }
                let after = if sleeps_first { 0.0 } else { forced };
                // A call that may decide to sleep first keeps its arguments,
                // to be made again with them once the script wakes.
                let replay = (flavour == EngineFlavour::XEngine
                    && !woke
                    && DECIDES_SLEEP_FIRST.contains(&descriptor.name))
                .then(|| {
                    stack
                        .len()
                        .checked_sub(descriptor.args.len())
                        .and_then(|at| stack.get(at..))
                        .map(<[Operand]>::to_vec)
                })
                .flatten();
                let (args, arg_objects): (Vec<Value>, Vec<Option<ListId>>) =
                    pop_n_held(stack, descriptor.args.len())?
                        .into_iter()
                        .unzip();
                *used = used
                    .saturating_add(BUILTIN_CALL_COST)
                    .saturating_add(cost::call(id, &args));
                let mut ctx = ScriptCtx::new(
                    self.caller,
                    self.now,
                    self.step,
                    &mut *self.host,
                    &mut *self.data,
                    self.peers,
                    (&mut *self.memory, *held),
                )
                .woken(woke);
                let called = match ossl::admit(descriptor, &mut ctx)? {
                    Admitted::Run => library::call(id, &mut ctx, args),
                    Admitted::Skip(value) => Ok(Called {
                        value,
                        stubbed: false,
                    }),
                };
                let request = ctx.take_request();
                let (delay, pauses, first) = ctx.take_delays();
                let written_back = ctx.take_written_back();
                let returned = ctx.take_returned();
                // A change the function made in place stands even when the
                // call then fails: OpenSim pads the pay buttons before it
                // throws at one that is not an integer.
                if let Some((argument, list)) = written_back
                    && let Some(Some(object)) = arg_objects.get(argument)
                {
                    objects::rewrite(
                        *object,
                        &list,
                        (&mut *self.globals, &self.objects.globals),
                        frames
                            .iter_mut()
                            .map(|frame| (&mut *frame.locals, &*frame.objects)),
                        stack,
                    );
                }
                let called = called?;
                if called.stubbed {
                    self.host.stubbed(self.caller, id);
                }
                if let Some(value) = called.value {
                    self.memory.made(&value)?;
                    let object =
                        returned.and_then(|argument| arg_objects.get(argument).copied().flatten());
                    stack.push(objects::operand(value, object));
                }
                if let (Some(seconds), Some(replay)) = (first, replay) {
                    // XEngine's thread sleeps before the call does what it
                    // decided to: it is made again, from the top, on waking.
                    stack.extend(replay);
                    *slept_first = true;
                    if let Some(frame) = frames.last_mut() {
                        frame.pc = frame.pc.saturating_sub(1);
                    }
                    return Ok(Flow::Sleep(delays::scaled(
                        flavour,
                        f64::from(seconds),
                        factor,
                    )));
                }
                let after = delay.map_or(after, |seconds| {
                    delays::scaled(flavour, f64::from(seconds), factor)
                }) + f64::from(pauses);
                return Ok(match request {
                    Some(Request::Reset) => Flow::Reset,
                    Some(Request::Die) => Flow::Died,
                    Some(Request::Sleep(seconds)) => Flow::Sleep(f64::from(seconds) + after),
                    None | Some(Request::Hand(_)) if after > 0.0 => Flow::Sleep(after),
                    Some(Request::Hand(target)) => Flow::Hand(target),
                    None => Flow::Next,
                });
            }
            Instr::CallFunction(id) => {
                let body = BodyRef::Function(id);
                let params = body_of(program, body)?.params;
                let (args, arg_objects): (Vec<Value>, Vec<Option<ListId>>) =
                    pop_n_held(stack, index(params))?.into_iter().unzip();
                if frames.len() >= MAX_CALL_DEPTH {
                    return Err(RuntimeError::StackHeapCollision);
                }
                let mut callee = new_frame(program, body, args, self.memory.flavour())?;
                for (slot, object) in callee.objects.iter_mut().zip(arg_objects) {
                    *slot = object;
                }
                let deeper = Stack {
                    frames: held.frames.saturating_add(callee.cost),
                    locals: held.locals,
                };
                self.memory.call(deeper)?;
                *held = deeper;
                frames.push(callee);
            }
            Instr::Return => return Ok(Self::leave(frames, stack, held, None)),
            Instr::ReturnValue => {
                let value = pop_operand(stack)?;
                let (value, object) = objects::split(value);
                return Ok(Self::leave(
                    frames,
                    stack,
                    held,
                    Some(objects::operand(value, object)),
                ));
            }
            Instr::Jump(target) => Self::jump(frame, target, used),
            Instr::JumpIfFalse(target) => {
                if !pop_operand(stack)?.is_true(self.memory.flavour().is_opensim()) {
                    Self::jump(frame, target, used);
                }
            }
            Instr::JumpIfTrue(target) => {
                if pop_operand(stack)?.is_true(self.memory.flavour().is_opensim()) {
                    Self::jump(frame, target, used);
                }
            }
            Instr::StateChange(target) => {
                *self.pending_state = Some(target);
                // On OpenSim a `state` throws `ScriptChangeStateException`,
                // which ends the whole handler whatever function it is in
                // (`events/state-self`: no line after the call).
                if self.memory.flavour().is_opensim() {
                    loop {
                        if Self::leave(frames, stack, held, None) == Flow::BodyDone {
                            return Ok(Flow::BodyDone);
                        }
                    }
                }
                // In a function — the reference's "state change hack" — the
                // call yields its return type's default and the caller
                // carries on; the transition waits for the handler's end.
                let yielded = match frame.body {
                    BodyRef::Function(id) => program
                        .function(id)
                        .and_then(|function| function.ret)
                        .map(|ty| Operand::Value(Value::default_of(ty))),
                    BodyRef::Init | BodyRef::Handler { .. } => None,
                };
                return Ok(Self::leave(frames, stack, held, yielded));
            }
            Instr::Print => match pop(stack)? {
                Value::String(text) => self.host.print(self.caller, &text),
                other => {
                    return Err(internal(format!(
                        "print of a {:?}, not a string",
                        other.type_name()
                    )));
                }
            },
            Instr::InvalidProgram => return Err(RuntimeError::InvalidProgram),
        }
        Ok(Flow::Next)
    }

    /// Continue at `target`, charging a loop's back-edge its cost.
    const fn jump(frame: &mut Frame, target: CodeOffset, used: &mut u32) {
        // `pc` already points past the jump, so a target at or before the
        // jump itself goes back.
        if target.0 < frame.pc {
            *used = used.saturating_add(BACKWARD_JUMP_COST);
        }
        frame.pc = target.0;
    }

    /// Leave the executing body, releasing what its frame held and handing
    /// `value` to the caller if there is one.
    fn leave(
        frames: &mut Vec<Frame>,
        stack: &mut Vec<Operand>,
        held: &mut Stack,
        value: Option<Operand>,
    ) -> Flow {
        if let Some(left) = frames.pop() {
            held.frames = held.frames.saturating_sub(left.cost);
            held.locals = left
                .costs
                .iter()
                .fold(held.locals, |locals, cost| locals.saturating_sub(*cost));
        }
        if frames.is_empty() {
            stack.clear();
            return Flow::BodyDone;
        }
        if let Some(value) = value {
            stack.push(value);
        }
        Flow::Next
    }
}

impl Instance {
    /// A fresh instance of `program`, running, about to run its global
    /// initialisers and then `default`'s `state_entry`, its memory counted as
    /// Second Life's Mono counts it.
    #[must_use]
    pub fn new(program: Arc<Program>) -> Self {
        Self::with_flavour(program, EngineFlavour::SecondLife)
    }

    /// [`Self::new`], its memory counted as `flavour`'s engine counts it.
    #[must_use]
    pub fn with_flavour(program: Arc<Program>, flavour: EngineFlavour) -> Self {
        let memory = Memory::new(&program, flavour);
        let objects = ListObjects::new(flavour, program.globals.len());
        let mut instance = Self {
            program,
            globals: Vec::new(),
            objects,
            state: StateId::DEFAULT,
            running: true,
            held: false,
            queue: VecDeque::new(),
            begun: 0,
            activity: None,
            pending_state: None,
            wake: Tick::default(),
            reset_when_idle: false,
            data: ScriptData::default(),
            overdraft: 0,
            memory,
            charged: 0,
            load: 0.0,
        };
        instance.reset();
        instance
    }

    /// Pay what earlier ticks overdrew out of this tick's `share`, returning
    /// what is left of it to run on. A script that overdrew more than a
    /// share sits the tick out, as it would on the grid, where the operation
    /// that overdrew is still running.
    pub(crate) const fn repay(&mut self, share: u32) -> u32 {
        let paid = if self.overdraft < share {
            self.overdraft
        } else {
            share
        };
        self.overdraft = self.overdraft.saturating_sub(paid);
        share.saturating_sub(paid)
    }

    /// Owe `instructions` charged past this tick's share, out of the next.
    pub(crate) const fn overdraw(&mut self, instructions: u32) {
        self.overdraft = self.overdraft.saturating_add(instructions);
    }

    /// Count `instructions` charged to the script in the tick under way.
    pub(crate) fn charge(&mut self, instructions: u32) {
        self.charged = self.charged.saturating_add(u64::from(instructions));
    }

    /// The tick is over: fold what it charged into [`Self::load`], `weight`
    /// being one tick's share of the averaging window.
    pub(crate) fn settle(&mut self, weight: f64) {
        let charged = f64::from(u32::try_from(self.charged).unwrap_or(u32::MAX));
        self.load += (charged - self.load) * weight;
        self.charged = 0;
    }

    /// The instructions the script has been running a tick, averaged over the
    /// last [`LOAD_WINDOW_SECONDS`]: what the region's top-scripts report
    /// ranks it by.
    #[must_use]
    pub const fn load(&self) -> f64 {
        self.load
    }

    /// What `llGetUsedMemory` would answer now, between two slices.
    #[must_use]
    pub fn used_memory(&self) -> u32 {
        self.memory.used(
            self.activity
                .as_ref()
                .map(|activity| activity.held)
                .unwrap_or_default(),
        )
    }

    /// `llGetMemoryLimit`: `65536`, or what `llSetMemoryLimit` set on Mono.
    #[must_use]
    pub const fn memory_limit(&self) -> u32 {
        self.memory.limit()
    }

    /// What the script adds to its object's `OBJECT_SCRIPT_MEMORY` in
    /// `flavour`: on Second Life its limit, running or not; on OpenSim 16 384
    /// while it reads as running, nothing stopped (`memory/object`,
    /// 2026-10-26).
    #[must_use]
    pub const fn script_memory(&self, flavour: EngineFlavour) -> u32 {
        match flavour {
            EngineFlavour::SecondLife => self.memory.limit(),
            EngineFlavour::YEngine | EngineFlavour::XEngine => {
                if self.reads_running(flavour) {
                    OPENSIM_SCRIPT_MEMORY
                } else {
                    0
                }
            }
        }
    }

    /// The compiled script.
    #[must_use]
    pub const fn program(&self) -> &Arc<Program> {
        &self.program
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> StateId {
        self.state
    }

    /// Whether the run flag is set.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// Whether the current state has a handler for `event` — what a host
    /// reads to know whether a prim takes a touch (`FLAGS_HANDLE_TOUCH`).
    #[must_use]
    pub fn handles(&self, event: &str) -> bool {
        handler_index(&self.program, self.state, event).is_some()
    }

    /// Whether a run-time error holds the script: it runs nothing and takes
    /// no events until it is reset, whatever the run flag says.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.held
    }

    /// Whether `llGetScriptState` reads the script as running in `flavour`:
    /// Second Life reads a held script as stopped even after
    /// `llSetScriptState(TRUE)` (aditi, 2026-10-23); YEngine reads only the
    /// run flag, so a held script reads as running though it answers
    /// nothing.
    #[must_use]
    pub const fn reads_running(&self, flavour: EngineFlavour) -> bool {
        match flavour {
            EngineFlavour::SecondLife => self.running && !self.held,
            EngineFlavour::YEngine | EngineFlavour::XEngine => self.running,
        }
    }

    /// The globals' current values, in [`Program::globals`] order.
    #[must_use]
    pub fn globals(&self) -> &[Value] {
        &self.globals
    }

    /// The current value of the global called `name`.
    #[must_use]
    pub fn global(&self, name: &str) -> Option<&Value> {
        let slot = self
            .program
            .globals
            .iter()
            .position(|global| global.name == name)?;
        self.globals.get(slot)
    }

    /// How many events are queued.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Whether a body is in progress — mid-handler, possibly asleep in it.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.activity.is_some()
    }

    /// The first tick the script may run again after a sleep.
    #[must_use]
    pub const fn wakes_at(&self) -> Tick {
        self.wake
    }

    /// Whether a slice at `now` would do anything: running, awake, and with a
    /// body in progress or an event queued. An idle script costs nothing.
    #[must_use]
    pub fn is_runnable(&self, now: Tick) -> bool {
        self.running
            && !self.held
            && self.wake <= now
            && (self.activity.is_some()
                || self.queue.iter().any(|queued| queued.ready <= now)
                || (self.queue.is_empty() && self.reset_when_idle))
    }

    /// Whether starting the script would let it run at `now` at once, in the
    /// middle of the body it was stopped in: it is stopped, not held, has a
    /// body in progress and no sleep left. XEngine drops the body at the
    /// stop ([`Self::set_running_as`]), so it never has one there.
    #[must_use]
    pub const fn resumes_when_started(&self, now: Tick) -> bool {
        !self.running && !self.held && self.activity.is_some() && self.wake.0 <= now.0
    }

    /// Reset the script as YEngine resets a running script another asks it
    /// to: at once in a handler or with nothing queued, otherwise once it has
    /// run what it has queued (`inventory/listing`).
    pub fn reset_when_idle(&mut self) {
        // In a handler — asleep in it, `time/reset-mid-sleep` — YEngine resets
        // at once; idle, it runs what it had queued first.
        if self.activity.is_some() || self.queue.is_empty() {
            self.reset();
        } else {
            self.reset_when_idle = true;
        }
    }

    /// Set or clear the run flag. Stopping is not resetting: the globals, the
    /// state and a handler in progress are kept, and a restarted script
    /// carries on where it stopped. The event queue is emptied, and a stopped
    /// script receives no events. Setting the flag does not release a script
    /// a run-time error holds; a reset does, before or after.
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
        if !running {
            self.queue.clear();
        }
    }

    /// [`Self::set_running`] as `flavour`'s engine stops a script: XEngine
    /// abandons the handler in progress — asleep in it, it never wakes
    /// (`time/reset-mid-sleep`, `time/sleep-while-stopped`) — where Second
    /// Life and YEngine keep it to carry on when the script runs again.
    pub fn set_running_as(&mut self, running: bool, flavour: EngineFlavour) {
        if !running && flavour == EngineFlavour::XEngine && self.running {
            self.activity = None;
            self.pending_state = None;
            self.wake = Tick::default();
        }
        self.set_running(running);
    }

    /// The script's listens: what lines it hears
    /// ([`Engine::hear`](crate::vm::Engine::hear)).
    #[must_use]
    pub const fn listens(&self) -> &Listens {
        &self.data.listens
    }

    /// What the script holds of an avatar's permissions.
    #[must_use]
    pub const fn permissions(&self) -> &Permissions {
        &self.data.permissions
    }

    /// The script's permissions, for an answer or a revocation.
    pub(crate) const fn permissions_mut(&mut self) -> &mut Permissions {
        &mut self.data.permissions
    }

    /// The library-visible state of the script.
    pub(crate) const fn data(&self) -> &ScriptData {
        &self.data
    }

    /// The library-visible state of the script, to change.
    pub(crate) const fn data_mut(&mut self) -> &mut ScriptData {
        &mut self.data
    }

    /// Reset the script: every global back to its type's default for the
    /// initialisers to run again, the call and operand stacks, the event queue
    /// and any requested state change dropped, any sleep cancelled, and the
    /// state back to `default`, whose `state_entry` runs after the
    /// initialisers. The timer is stopped, the detected block cleared, the
    /// listens closed and every permission dropped with the question, and
    /// a script a run-time error held is released; the run flag and the
    /// start parameter are left as they are.
    pub fn reset(&mut self) {
        self.held = false;
        self.reset_when_idle = false;
        self.data = ScriptData {
            start_parameter: self.data.start_parameter,
            start_string: core::mem::take(&mut self.data.start_string),
            say_throttle: self.data.say_throttle,
            ..ScriptData::default()
        };
        self.globals = self
            .program
            .globals
            .iter()
            .map(|global| Value::default_of(global.ty))
            .collect();
        self.objects.globals = vec![None; self.globals.len()];
        self.state = StateId::DEFAULT;
        self.queue.clear();
        self.pending_state = None;
        self.wake = Tick::default();
        self.memory.reset();
        self.activity = new_frame(
            &self.program,
            BodyRef::Init,
            Vec::new(),
            self.memory.flavour(),
        )
        .ok()
        .map(|frame| Activity::new(Phase::Init, frame));
    }

    /// Offer the script an event with no detected block.
    ///
    /// # Errors
    ///
    /// [`PostError`] when `args` does not match the event's parameters, when
    /// the event is a detection event (post those with
    /// [`Self::post_detected`]), or when it is `state_entry` or `state_exit`.
    pub fn post(
        &mut self,
        arrival: Arrival,
        event: &'static Event,
        args: Vec<Value>,
    ) -> Result<Posted, PostError> {
        if is_detection_event(event.name) {
            return Err(PostError::Detection(event.name));
        }
        self.enqueue(arrival, event, args, None, false)
    }

    /// [`Self::post`], the event queued ahead of every event already waiting
    /// rather than behind them: Second Life's `on_rez` after a take, which
    /// runs before the events the script had queued when it was taken
    /// (`persistence/rerez`, aditi 2026-10-26). A body in progress still
    /// finishes first.
    ///
    /// # Errors
    ///
    /// As [`Self::post`].
    pub(crate) fn post_first(
        &mut self,
        arrival: Arrival,
        event: &'static Event,
        args: Vec<Value>,
    ) -> Result<Posted, PostError> {
        if is_detection_event(event.name) {
            return Err(PostError::Detection(event.name));
        }
        self.enqueue(arrival, event, args, None, true)
    }

    /// Offer the script a detection event — a touch, a collision, a sensor
    /// sweep — with who or what was detected. The event's one parameter, how
    /// many were detected, is the block's length.
    ///
    /// # Errors
    ///
    /// [`PostError`] when `event` is not a detection event, or `detected` is
    /// empty or longer than [`MAX_DETECTED`].
    pub fn post_detected(
        &mut self,
        arrival: Arrival,
        event: &'static Event,
        detected: Vec<Detected>,
    ) -> Result<Posted, PostError> {
        if !is_detection_event(event.name) {
            return Err(PostError::Detection(event.name));
        }
        if detected.is_empty() || detected.len() > MAX_DETECTED {
            return Err(PostError::DetectedCount {
                event: event.name,
                found: detected.len(),
            });
        }
        let count = Value::Integer(i32::try_from(detected.len()).unwrap_or(i32::MAX));
        self.enqueue(arrival, event, vec![count], Some(detected), false)
    }

    /// Check an event against its parameters and the queue's rules, and
    /// queue it — behind every event waiting, or with `first` ahead of them.
    fn enqueue(
        &mut self,
        arrival: Arrival,
        event: &'static Event,
        args: Vec<Value>,
        detected: Option<Vec<Detected>>,
        first: bool,
    ) -> Result<Posted, PostError> {
        if matches!(event.name, "state_entry" | "state_exit") {
            return Err(PostError::Transition(event.name));
        }
        if args.len() != event.args.len() {
            return Err(PostError::Arity {
                event: event.name,
                expected: event.args.len(),
                found: args.len(),
            });
        }
        for (position, (value, parameter)) in args.iter().zip(event.args).enumerate() {
            if value.type_name() != parameter.ty {
                return Err(PostError::Argument {
                    event: event.name,
                    index: position,
                    expected: parameter.ty,
                    found: value.type_name(),
                });
            }
        }
        if !self.running || self.held {
            return Ok(Posted::Stopped);
        }
        let Some(handler) = handler_index(&self.program, self.state, event.name) else {
            return Ok(Posted::NoHandler);
        };
        if NON_STACKING.contains(&event.name)
            && self
                .queue
                .iter()
                .any(|queued| queued.event.name == event.name)
        {
            return Ok(Posted::AlreadyQueued);
        }
        if COALESCING.contains(&event.name)
            && let Some(detected) = &detected
            && let Some(queued) = self.queue.iter_mut().find(|queued| {
                queued.event.name == event.name
                    && if TOUCHES.contains(&event.name) {
                        arrival.coalescing.merges_touch()
                    } else {
                        arrival
                            .coalescing
                            .merges(queued.posted, arrival.tick, false)
                    }
            })
            && let Some(block) = queued.detected.as_mut()
        {
            for detection in detected {
                if block.len() < MAX_DETECTED
                    && !block.iter().any(|known| known.key == detection.key)
                {
                    block.push(detection.clone());
                }
            }
            queued.args = vec![Value::Integer(
                i32::try_from(block.len()).unwrap_or(i32::MAX),
            )];
            return Ok(Posted::Merged);
        }
        if event.name == "changed"
            && let [Value::Integer(bits)] = args.as_slice()
            && let Some(newest) = self
                .queue
                .iter()
                .rposition(|queued| queued.event.name == "changed")
            && let Some(queued) = self.queue.get_mut(newest)
            && arrival
                .coalescing
                .merges(queued.posted, arrival.tick, newest > 0)
            && let Some(Value::Integer(queued_bits)) = queued.args.first_mut()
        {
            *queued_bits |= *bits;
            return Ok(Posted::Merged);
        }
        if self.queue.len() >= MAX_QUEUED {
            return Ok(Posted::QueueFull);
        }
        let queued = Queued {
            event,
            handler,
            args,
            detected,
            posted: arrival.tick,
            ready: arrival.ready,
            batch: self.begun,
        };
        if first {
            self.queue.push_front(queued);
        } else {
            // Ranked among the events raised while the same handler ran;
            // behind every one raised before it began (`events/order`: a
            // link message the next handler sends comes after a `changed`
            // raised during the last).
            let rank = arrival.coalescing.rank(event.name);
            let batch = self.begun;
            match self.queue.iter().position(|waiting| {
                waiting.batch == batch && arrival.coalescing.rank(waiting.event.name) > rank
            }) {
                Some(ahead_of) => self.queue.insert(ahead_of, queued),
                None => self.queue.push_back(queued),
            }
        }
        Ok(Posted::Queued)
    }

    /// The start parameter, `llGetStartParameter`.
    #[must_use]
    pub const fn start_parameter(&self) -> i32 {
        self.data.start_parameter
    }

    /// Set the start parameter — what the object was rezzed with, before its
    /// `on_rez` is posted.
    pub const fn set_start_parameter(&mut self, start_parameter: i32) {
        self.data.start_parameter = start_parameter;
    }

    /// The start string, `llGetStartString`.
    #[must_use]
    pub fn start_string(&self) -> &str {
        &self.data.start_string
    }

    /// Set the start string — what the object was rezzed with
    /// (`REZ_PARAM_STRING`), before its `on_rez` is posted. Like the start
    /// parameter it survives a reset; unlike it, a take does not save it.
    pub fn set_start_string(&mut self, start_string: String) {
        self.data.start_string = start_string;
    }

    /// Whether a timer is set.
    #[must_use]
    pub const fn has_timer(&self) -> bool {
        self.data.timer.is_some()
    }

    /// One more tick of the script's time (`llGetTime`), which runs on
    /// whether the script runs or not.
    pub(crate) const fn count_tick(&mut self) {
        self.data.script_ticks = self.data.script_ticks.saturating_add(1);
    }

    /// If the timer is due at `now`, schedule the next one and say so; the
    /// caller posts the `timer` event.
    ///
    /// On Second Life a timer does not fall due while its own handler runs:
    /// one that came due meanwhile is raised in the first tick after the
    /// handler returned, behind whatever the handler raised, and a new
    /// interval set in the handler replaces it. Busy in any other handler,
    /// the script has it queued when it falls due (`events/timer-reset`,
    /// 2026-11-05). OpenSim's timer thread queues it whatever runs.
    ///
    /// When the next falls due is each grid's (`time/timer-phase`): Second
    /// Life keeps the phase of the due times, skipping any a busy handler let
    /// pass; YEngine counts the next interval from this event, so its error
    /// adds up; XEngine counts from its first event and keeps that phase.
    pub(crate) fn timer_due(&mut self, now: Tick, step: Duration, flavour: EngineFlavour) -> bool {
        if flavour == EngineFlavour::SecondLife && self.runs_timer_handler() {
            return false;
        }
        // A microsecond's slack: 45 ticks of 1/45 s, in whole nanoseconds,
        // fall a hair short of the second a timer is due at.
        let time =
            crate::vm::context::region_time(now, step).saturating_add(Duration::from_micros(1));
        match self.data.timer.as_mut() {
            Some(timer) if timer.next <= time => {
                let from_now = time.saturating_add(timer.interval);
                timer.next = match flavour {
                    EngineFlavour::YEngine => from_now,
                    EngineFlavour::XEngine if !timer.fired => from_now,
                    EngineFlavour::SecondLife | EngineFlavour::XEngine => {
                        let mut next = timer.next.saturating_add(timer.interval);
                        if next <= time && !timer.interval.is_zero() {
                            let behind = time.saturating_sub(next).as_nanos();
                            let steps = behind
                                .checked_div(timer.interval.as_nanos())
                                .unwrap_or(0)
                                .saturating_add(1);
                            let skip = timer.interval.as_nanos().saturating_mul(steps);
                            next = next.saturating_add(Duration::from_nanos(
                                u64::try_from(skip).unwrap_or(u64::MAX),
                            ));
                        }
                        next
                    }
                };
                timer.fired = true;
                true
            }
            Some(_) | None => false,
        }
    }

    /// The sweeps due at `now`: those asked for since the last tick, and the
    /// repeating one when it falls due, which then counts on to its next.
    pub(crate) fn sweeps_due(
        &mut self,
        now: Tick,
        step: Duration,
    ) -> Vec<crate::vm::sensing::Query> {
        if !self.running || self.held {
            self.data.sweeps.clear();
            return Vec::new();
        }
        let mut due = core::mem::take(&mut self.data.sweeps);
        let time =
            crate::vm::context::region_time(now, step).saturating_add(Duration::from_micros(1));
        if let Some(repeat) = self.data.repeat.as_mut()
            && repeat.next <= time
        {
            due.push(repeat.query.clone());
            repeat.next = time.saturating_add(repeat.interval);
        }
        due
    }

    /// The script's targets, if it may be checked at `time` and is due: the
    /// next check is then counted from `time`, `interval` on.
    pub(crate) fn targets_due(
        &mut self,
        time: Duration,
        interval: Duration,
    ) -> Option<crate::vm::context::Targets> {
        let targets = &mut self.data.targets;
        if !self.running || self.held || !targets.any() || targets.next_check > time {
            return None;
        }
        targets.next_check = time.saturating_add(interval);
        Some(targets.clone())
    }

    /// Whether an event `name` whose first argument is `first` waits in the
    /// queue: a target's event is not queued twice for one handle.
    pub(crate) fn has_queued(&self, name: &str, first: Option<&Value>) -> bool {
        self.queue
            .iter()
            .any(|queued| queued.event.name == name && queued.args.first() == first)
    }

    /// Whether the body in progress is the current state's `timer` handler.
    fn runs_timer_handler(&self) -> bool {
        self.activity.as_ref().is_some_and(|activity| {
            activity.phase == Phase::Event
                && matches!(
                    activity.frames.first().map(|frame| frame.body),
                    Some(BodyRef::Handler { state, index })
                        if handler_index(&self.program, state, "timer") == Some(index)
                )
        })
    }

    /// Run at most `budget` instructions at tick `now`, stopping early at the
    /// end of a body, a sleep, a state change, a reset or an error. `step` is
    /// the length of a tick, which turns a sleep's seconds into ticks.
    ///
    /// Run this way, outside an [`Engine`](crate::vm::Engine), the script
    /// sees no other scripts: `llGetScriptState` reports any but itself
    /// stopped.
    pub fn run_slice(
        &mut self,
        caller: CallerId,
        now: Tick,
        step: Duration,
        budget: u32,
        host: &mut dyn Host,
    ) -> Slice {
        self.run_slice_among(caller, now, step, budget, host, Peers::NONE)
    }

    /// [`Self::run_slice`], with the region's other scripts in view.
    pub(crate) fn run_slice_among(
        &mut self,
        caller: CallerId,
        now: Tick,
        step: Duration,
        budget: u32,
        host: &mut dyn Host,
        peers: Peers<'_>,
    ) -> Slice {
        let mut used = 0_u32;
        self.data
            .say_throttle
            .start(crate::vm::context::region_time(now, step));
        let outcome = self.run_body(caller, now, step, budget, host, peers, &mut used);
        Slice {
            outcome,
            used,
            refusals: core::mem::take(&mut self.data.refusals),
            raised: core::mem::take(&mut self.data.raised),
        }
    }

    /// The body of [`Self::run_slice_among`]: run, adding to `used`, and say
    /// how it ended.
    #[expect(
        clippy::too_many_arguments,
        reason = "run_slice_among's parameters plus the counter it reports"
    )]
    fn run_body(
        &mut self,
        caller: CallerId,
        now: Tick,
        step: Duration,
        budget: u32,
        host: &mut dyn Host,
        peers: Peers<'_>,
        used: &mut u32,
    ) -> Outcome {
        if !self.is_runnable(now) {
            return Outcome::Idle;
        }
        if self.activity.is_none() && self.queue.is_empty() && self.reset_when_idle {
            self.reset();
        }
        if self.activity.is_none()
            && let Err(error) = self.start_next_event(now)
        {
            return self.fault(error, None, peers.flavour());
        }
        while *used < budget {
            let Some(activity) = self.activity.as_mut() else {
                return Outcome::Idle;
            };
            let at = activity.frames.last().map(|frame| (frame.body, frame.pc));
            let mut machine = Machine {
                program: &self.program,
                globals: &mut self.globals,
                objects: &mut self.objects,
                activity,
                pending_state: &mut self.pending_state,
                caller,
                now,
                step,
                host: &mut *host,
                data: &mut self.data,
                peers,
                memory: &mut self.memory,
            };
            let flow = machine.step(used);
            if core::mem::take(&mut self.data.drop_queued_timer) {
                self.queue.retain(|queued| queued.event.name != "timer");
            }
            if core::mem::take(&mut self.data.drop_queued_sensor) {
                self.queue
                    .retain(|queued| !matches!(queued.event.name, "sensor" | "no_sensor"));
            }
            let removed = core::mem::take(&mut self.data.targets.removed);
            if !removed.is_empty() {
                self.queue.retain(|queued| {
                    !(matches!(queued.event.name, "at_target" | "at_rot_target")
                        && matches!(queued.args.first(),
                            Some(Value::Integer(handle)) if removed.contains(handle)))
                });
            }
            return match flow {
                Ok(Flow::Next) => continue,
                Ok(Flow::BodyDone) => self.finish_body(caller, host),
                Ok(Flow::Sleep(seconds)) => {
                    let ticks = ticks_for(seconds, step);
                    if ticks == 0 {
                        continue;
                    }
                    self.wake = now.after(ticks);
                    Outcome::Sleeping(self.wake)
                }
                Ok(Flow::Hand(target)) => Outcome::Handed(target),
                Ok(Flow::Reset) => {
                    // A reset drops the library's state; what the call
                    // refused before it is still said, and what it changed
                    // in the linkset's store and the link messages it sent
                    // still reach the linkset's other scripts.
                    let refusals = core::mem::take(&mut self.data.refusals);
                    let raised = core::mem::take(&mut self.data.raised);
                    self.reset();
                    self.data.refusals = refusals;
                    self.data.raised = raised;
                    Outcome::Reset
                }
                Ok(Flow::Died) => {
                    self.activity = None;
                    self.pending_state = None;
                    self.set_running(false);
                    Outcome::Died
                }
                Err(error) => self.fault(error, at, peers.flavour()),
            };
        }
        Outcome::Yielded
    }

    /// End the body in progress with `error`, raised by the instruction at
    /// `at`, and do with the script what `flavour`'s engine does: Second
    /// Life stops and holds it, YEngine holds it with its run flag still
    /// set, XEngine lets it run on with its next event. Either way the state
    /// and the globals stay as they were, and a requested state change is
    /// not carried out.
    fn fault(
        &mut self,
        error: RuntimeError,
        at: Option<(BodyRef, u32)>,
        flavour: EngineFlavour,
    ) -> Outcome {
        let position = at.and_then(|(body, pc)| {
            body_of(&self.program, body)
                .ok()
                .and_then(|code| self.program.position(code, CodeOffset(pc)))
        });
        let event = self.handled_event();
        self.activity = None;
        self.pending_state = None;
        match flavour {
            EngineFlavour::SecondLife => {
                self.set_running(false);
                self.held = true;
            }
            EngineFlavour::YEngine => {
                self.queue.clear();
                self.held = true;
            }
            EngineFlavour::XEngine => {}
        }
        Outcome::Faulted(Fault {
            error,
            position,
            event,
        })
    }

    /// The event whose handler is the bottom body in progress, or [`None`]
    /// for the global initialisers (or nothing in progress).
    fn handled_event(&self) -> Option<&'static str> {
        let bottom = self.activity.as_ref()?.frames.first()?;
        match bottom.body {
            BodyRef::Handler { state, index } => self
                .program
                .state(state)?
                .handlers
                .get(index)
                .map(|handler| handler.event.name),
            BodyRef::Init | BodyRef::Function(_) => None,
        }
    }

    /// Start the handler of the first queued event free to run at `now`,
    /// if there is one. One still held back stays where it is, and the next
    /// behind it goes first: on Second Life a `changed` raised in a handler
    /// runs as the handler ends, ahead of the link message the handler sent
    /// itself last, which waits for the next frame (`events/timer-reset`).
    fn start_next_event(&mut self, now: Tick) -> Result<(), RuntimeError> {
        let Some(Queued {
            handler,
            args,
            detected,
            ..
        }) = self
            .queue
            .iter()
            .position(|queued| queued.ready <= now)
            .and_then(|first| self.queue.remove(first))
        else {
            return Ok(());
        };
        self.begun = self.begun.wrapping_add(1);
        // An event with no detected block clears the last one: a `timer`
        // after a touch reads nothing (aditi, 2026-09-28).
        self.data.detected = detected.unwrap_or_default();
        let body = BodyRef::Handler {
            state: self.state,
            index: handler,
        };
        self.activity = Some(Activity::new(
            Phase::Event,
            new_frame(&self.program, body, args, self.memory.flavour())?,
        ));
        Ok(())
    }

    /// Start `state`'s handler for `event` (`state_entry` or `state_exit`) in
    /// `phase`, if the state has one; otherwise leave nothing in progress.
    fn start_transition_handler(&mut self, state: StateId, event: &str, phase: Phase) -> bool {
        let started = handler_index(&self.program, state, event)
            .map(|index| BodyRef::Handler { state, index })
            .and_then(|body| new_frame(&self.program, body, Vec::new(), self.memory.flavour()).ok())
            .map(|frame| Activity::new(phase, frame));
        let is_started = started.is_some();
        if is_started {
            self.data.detected.clear();
        }
        self.activity = started;
        is_started
    }

    /// Move to `target`: the queue and the listens are discarded, the host
    /// drops what else the old state held (taken controls), and `state_entry`
    /// is next.
    fn switch_to(&mut self, target: StateId, caller: CallerId, host: &mut dyn Host) -> Outcome {
        self.state = target;
        self.queue.clear();
        self.data.listens.clear();
        // A repeating sensor ends with the state on every grid, targets on
        // OpenSim's engines only (`events/state-drops`).
        self.data.repeat = None;
        if self.memory.flavour().is_opensim() {
            self.data.targets.positions.clear();
            self.data.targets.rotations.clear();
        }
        host.left_state(caller);
        let _entered = self.start_transition_handler(target, "state_entry", Phase::Entry);
        Outcome::StateChanged(target)
    }

    /// The bottom body returned: decide what runs next.
    fn finish_body(&mut self, caller: CallerId, host: &mut dyn Host) -> Outcome {
        let Some(finished) = self.activity.take() else {
            return Outcome::Finished;
        };
        match finished.phase {
            Phase::Init => {
                let _entered =
                    self.start_transition_handler(self.state, "state_entry", Phase::Entry);
                Outcome::Finished
            }
            Phase::Event | Phase::Entry => match self.pending_state.take() {
                // `state` to the current state is no transition at all.
                Some(target) if target != self.state => {
                    if self.start_transition_handler(self.state, "state_exit", Phase::Exit(target))
                    {
                        Outcome::Finished
                    } else {
                        self.switch_to(target, caller, host)
                    }
                }
                Some(_) | None => Outcome::Finished,
            },
            // A `state` statement inside `state_exit` is not obeyed: the
            // transition already under way wins.
            Phase::Exit(target) => {
                self.pending_state = None;
                self.switch_to(target, caller, host)
            }
        }
    }
}
