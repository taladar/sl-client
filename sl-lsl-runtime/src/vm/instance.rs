//! One running script: its instance state, and the interpreter that advances
//! it a bounded number of instructions at a time.

use std::collections::VecDeque;
use std::sync::Arc;

use core::time::Duration;

use sl_lsl::ast::TypeName;
use sl_types::lsl::{Rotation, Vector};

use crate::bytecode::{Body, CodeOffset, Component, FunctionId, Instr, Program, StateId};
use crate::library::{self, Event};
use crate::num::ticks_for;
use crate::value::{Element, Value};
use crate::vm::context::{Request, ScriptCtx, Tick};
use crate::vm::fault::{Fault, RuntimeError};
use crate::vm::host::{CallerId, Host};

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
/// growing the host's memory without bound. The memory accounting of
/// `server-lsl-memory-and-limits` is what will stop it at the reference's
/// exact depth.
pub const MAX_CALL_DEPTH: usize = 4096;

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
    /// The state changed to this one: `state_exit` of the old state has run,
    /// the event queue is empty, and `state_entry` of the new one is next.
    StateChanged(StateId),
    /// The script reset itself (`llResetScript`): globals, stack and queue
    /// are gone and the initialisers run next, then `default`'s
    /// `state_entry`.
    Reset,
    /// A run-time error stopped the script. It keeps its state and its
    /// globals, and is no longer running.
    Faulted(Fault),
}

/// The result of one [`Instance::run_slice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    /// How it ended.
    pub outcome: Outcome,
    /// Instructions charged, counting each library call and each loop
    /// back-edge at its cost. May exceed the budget by the cost of the one
    /// instruction that crossed it, never by more.
    pub used: u32,
}

/// What became of a posted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    /// Queued behind any earlier events.
    Queued,
    /// Dropped: the current state has no handler for it. An event nobody
    /// handles is never queued, so it cannot fire after a later state change.
    NoHandler,
    /// Dropped: the script is not running. A stopped script receives no
    /// events at all.
    Stopped,
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
}

/// The body in progress: the call stack and the operand stack.
#[derive(Debug, Clone)]
struct Activity {
    /// Why the bottom body runs.
    phase: Phase,
    /// The call stack; the last frame is executing.
    frames: Vec<Frame>,
    /// The operand stack, shared by every frame (a frame's operands sit
    /// above its caller's).
    stack: Vec<Value>,
}

/// An event waiting for its handler.
#[derive(Debug, Clone)]
struct Queued {
    /// The handler's index in the current state.
    handler: usize,
    /// The arguments, already checked against the event's parameters.
    args: Vec<Value>,
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
    /// The current state.
    state: StateId,
    /// The run flag (`SetScriptRunning`, the contents' Running checkbox).
    running: bool,
    /// Events waiting for their handlers, oldest first.
    queue: VecDeque<Queued>,
    /// The body in progress, if any.
    activity: Option<Activity>,
    /// A state change requested by the body in progress, carried out when
    /// its event handler ends.
    pending_state: Option<StateId>,
    /// The first tick the script may run again.
    wake: Tick,
}

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
/// local at its type's default.
fn new_frame(program: &Program, body: BodyRef, args: Vec<Value>) -> Result<Frame, RuntimeError> {
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
        locals,
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

/// The top operand.
fn pop(stack: &mut Vec<Value>) -> Result<Value, RuntimeError> {
    stack
        .pop()
        .ok_or_else(|| internal("operand stack underflow"))
}

/// The top operand, which must be a float.
fn pop_float(stack: &mut Vec<Value>) -> Result<f32, RuntimeError> {
    match pop(stack)? {
        Value::Float(float) => Ok(float),
        other => Err(internal(format!(
            "expected a float operand, found {:?}",
            other.type_name()
        ))),
    }
}

/// The top `count` operands, deepest first.
fn pop_n(stack: &mut Vec<Value>, count: usize) -> Result<Vec<Value>, RuntimeError> {
    let at = stack
        .len()
        .checked_sub(count)
        .ok_or_else(|| internal("operand stack underflow"))?;
    Ok(stack.split_off(at))
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
    /// The body in progress.
    activity: &'run mut Activity,
    /// Where a `state` statement records its target.
    pending_state: &'run mut Option<StateId>,
    /// The calling instance, for the host.
    caller: CallerId,
    /// The current tick.
    now: Tick,
    /// The world.
    host: &'run mut dyn Host,
}

impl Machine<'_> {
    /// Execute one instruction, adding what it costs to `used`.
    fn step(&mut self, used: &mut u32) -> Result<Flow, RuntimeError> {
        let program = self.program;
        let Activity { frames, stack, .. } = &mut *self.activity;
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
                    .ok_or_else(|| internal(format!("no constant #{}", id.0)))?,
            ),
            Instr::Pop => {
                let _discarded = pop(stack)?;
            }
            Instr::Dup => {
                let top = stack
                    .last()
                    .cloned()
                    .ok_or_else(|| internal("operand stack underflow"))?;
                stack.push(top);
            }
            Instr::LoadLocal(slot) => stack.push(
                frame
                    .locals
                    .get(index(slot.0))
                    .cloned()
                    .ok_or_else(|| internal(format!("no local {}", slot.0)))?,
            ),
            Instr::StoreLocal(slot) => {
                let value = pop(stack)?;
                *frame
                    .locals
                    .get_mut(index(slot.0))
                    .ok_or_else(|| internal(format!("no local {}", slot.0)))? = value;
            }
            Instr::LoadGlobal(slot) => stack.push(
                self.globals
                    .get(index(slot.0))
                    .cloned()
                    .ok_or_else(|| internal(format!("no global {}", slot.0)))?,
            ),
            Instr::StoreGlobal(slot) => {
                let value = pop(stack)?;
                *self
                    .globals
                    .get_mut(index(slot.0))
                    .ok_or_else(|| internal(format!("no global {}", slot.0)))? = value;
            }
            Instr::GetMember(component) => {
                let aggregate = pop(stack)?;
                stack.push(Value::Float(get_member(&aggregate, component)?));
            }
            Instr::SetMember(component) => {
                let aggregate = pop(stack)?;
                let to = pop_float(stack)?;
                stack.push(set_member(aggregate, component, to)?);
            }
            Instr::Cast(ty) => {
                let value = pop(stack)?;
                stack.push(crate::cast(value, ty)?);
            }
            Instr::Binary(op) => {
                let left = pop(stack)?;
                let right = pop(stack)?;
                stack.push(crate::binary(op, left, right)?);
            }
            Instr::Prefix(op) => {
                let operand = pop(stack)?;
                stack.push(crate::prefix(op, operand)?);
            }
            Instr::BuildVector => {
                let z = pop_float(stack)?;
                let y = pop_float(stack)?;
                let x = pop_float(stack)?;
                stack.push(Value::Vector(Vector { x, y, z }));
            }
            Instr::BuildRotation => {
                let s = pop_float(stack)?;
                let z = pop_float(stack)?;
                let y = pop_float(stack)?;
                let x = pop_float(stack)?;
                stack.push(Value::Rotation(Rotation { x, y, z, s }));
            }
            Instr::BuildList(count) => {
                let elements = pop_n(stack, index(count))?
                    .into_iter()
                    .map(|value| match value {
                        Value::List(_) => Err(internal("a list element that is a list")),
                        other => Ok(other.into_elements()),
                    })
                    .collect::<Result<Vec<Vec<Element>>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                stack.push(Value::List(elements));
            }
            Instr::CallBuiltin(id) => {
                let descriptor = id.descriptor();
                let args = pop_n(stack, descriptor.args.len())?;
                *used = used.saturating_add(BUILTIN_CALL_COST);
                let mut ctx = ScriptCtx::new(self.caller, self.now, &mut *self.host);
                let called = library::call(id, &mut ctx, args)?;
                let request = ctx.take_request();
                if called.stubbed {
                    self.host.stubbed(self.caller, id);
                }
                if let Some(value) = called.value {
                    stack.push(value);
                }
                let forced = f64::from(descriptor.sleep);
                return Ok(match request {
                    Some(Request::Reset) => Flow::Reset,
                    Some(Request::Sleep(seconds)) => Flow::Sleep(f64::from(seconds) + forced),
                    None if forced > 0.0 => Flow::Sleep(forced),
                    None => Flow::Next,
                });
            }
            Instr::CallFunction(id) => {
                let body = BodyRef::Function(id);
                let params = body_of(program, body)?.params;
                let args = pop_n(stack, index(params))?;
                if frames.len() >= MAX_CALL_DEPTH {
                    return Err(RuntimeError::StackHeapCollision);
                }
                frames.push(new_frame(program, body, args)?);
            }
            Instr::Return => return Ok(Self::leave(frames, stack, None)),
            Instr::ReturnValue => {
                let value = pop(stack)?;
                return Ok(Self::leave(frames, stack, Some(value)));
            }
            Instr::Jump(target) => Self::jump(frame, target, used),
            Instr::JumpIfFalse(target) => {
                if !pop(stack)?.is_true() {
                    Self::jump(frame, target, used);
                }
            }
            Instr::JumpIfTrue(target) => {
                if pop(stack)?.is_true() {
                    Self::jump(frame, target, used);
                }
            }
            Instr::StateChange(target) => {
                *self.pending_state = Some(target);
                // In a function — the reference's "state change hack" — the
                // call yields its return type's default and the caller
                // carries on; the transition waits for the handler's end.
                let yielded = match frame.body {
                    BodyRef::Function(id) => program
                        .function(id)
                        .and_then(|function| function.ret)
                        .map(Value::default_of),
                    BodyRef::Init | BodyRef::Handler { .. } => None,
                };
                return Ok(Self::leave(frames, stack, yielded));
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

    /// Leave the executing body, handing `value` to the caller if there is
    /// one.
    fn leave(frames: &mut Vec<Frame>, stack: &mut Vec<Value>, value: Option<Value>) -> Flow {
        let _left = frames.pop();
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
    /// initialisers and then `default`'s `state_entry`.
    #[must_use]
    pub fn new(program: Arc<Program>) -> Self {
        let mut instance = Self {
            program,
            globals: Vec::new(),
            state: StateId::DEFAULT,
            running: true,
            queue: VecDeque::new(),
            activity: None,
            pending_state: None,
            wake: Tick::default(),
        };
        instance.reset();
        instance
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
        self.running && self.wake <= now && (self.activity.is_some() || !self.queue.is_empty())
    }

    /// Set or clear the run flag. Stopping is not resetting: the globals, the
    /// state and a handler in progress are kept, and a restarted script
    /// carries on where it stopped. The event queue is emptied, and a stopped
    /// script receives no events.
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
        if !running {
            self.queue.clear();
        }
    }

    /// Reset the script: every global back to its type's default for the
    /// initialisers to run again, the call and operand stacks, the event queue
    /// and any requested state change dropped, any sleep cancelled, and the
    /// state back to `default`, whose `state_entry` runs after the
    /// initialisers. The run flag is left as it is.
    pub fn reset(&mut self) {
        self.globals = self
            .program
            .globals
            .iter()
            .map(|global| Value::default_of(global.ty))
            .collect();
        self.state = StateId::DEFAULT;
        self.queue.clear();
        self.pending_state = None;
        self.wake = Tick::default();
        self.activity = Some(Activity {
            phase: Phase::Init,
            frames: vec![Frame {
                body: BodyRef::Init,
                pc: 0,
                locals: self
                    .program
                    .init
                    .locals
                    .iter()
                    .map(|local| Value::default_of(local.ty))
                    .collect(),
            }],
            stack: Vec::new(),
        });
    }

    /// Offer the script an event.
    ///
    /// # Errors
    ///
    /// [`PostError`] when `args` does not match the event's parameters.
    pub fn post(&mut self, event: &'static Event, args: Vec<Value>) -> Result<Posted, PostError> {
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
        if !self.running {
            return Ok(Posted::Stopped);
        }
        let Some(handler) = handler_index(&self.program, self.state, event.name) else {
            return Ok(Posted::NoHandler);
        };
        self.queue.push_back(Queued { handler, args });
        Ok(Posted::Queued)
    }

    /// Run at most `budget` instructions at tick `now`, stopping early at the
    /// end of a body, a sleep, a state change, a reset or an error. `step` is
    /// the length of a tick, which turns a sleep's seconds into ticks.
    pub fn run_slice(
        &mut self,
        caller: CallerId,
        now: Tick,
        step: Duration,
        budget: u32,
        host: &mut dyn Host,
    ) -> Slice {
        let mut used = 0_u32;
        if !self.is_runnable(now) {
            return Slice {
                outcome: Outcome::Idle,
                used,
            };
        }
        if self.activity.is_none()
            && let Err(error) = self.start_next_event()
        {
            return self.fault(error, None, used);
        }
        while used < budget {
            let Some(activity) = self.activity.as_mut() else {
                return Slice {
                    outcome: Outcome::Idle,
                    used,
                };
            };
            let at = activity.frames.last().map(|frame| (frame.body, frame.pc));
            let mut machine = Machine {
                program: &self.program,
                globals: &mut self.globals,
                activity,
                pending_state: &mut self.pending_state,
                caller,
                now,
                host: &mut *host,
            };
            let outcome = match machine.step(&mut used) {
                Ok(Flow::Next) => continue,
                Ok(Flow::BodyDone) => self.finish_body(),
                Ok(Flow::Sleep(seconds)) => {
                    let ticks = ticks_for(seconds, step);
                    if ticks == 0 {
                        continue;
                    }
                    self.wake = now.after(ticks);
                    Outcome::Sleeping(self.wake)
                }
                Ok(Flow::Reset) => {
                    self.reset();
                    Outcome::Reset
                }
                Err(error) => return self.fault(error, at, used),
            };
            return Slice { outcome, used };
        }
        Slice {
            outcome: Outcome::Yielded,
            used,
        }
    }

    /// Stop the script with `error`, raised by the instruction at `at`.
    fn fault(&mut self, error: RuntimeError, at: Option<(BodyRef, u32)>, used: u32) -> Slice {
        let position = at.and_then(|(body, pc)| {
            body_of(&self.program, body)
                .ok()
                .and_then(|code| self.program.position(code, CodeOffset(pc)))
        });
        self.activity = None;
        self.pending_state = None;
        self.set_running(false);
        Slice {
            outcome: Outcome::Faulted(Fault { error, position }),
            used,
        }
    }

    /// Start the handler of the oldest queued event, if there is one.
    fn start_next_event(&mut self) -> Result<(), RuntimeError> {
        let Some(Queued { handler, args }) = self.queue.pop_front() else {
            return Ok(());
        };
        let body = BodyRef::Handler {
            state: self.state,
            index: handler,
        };
        self.activity = Some(Activity {
            phase: Phase::Event,
            frames: vec![new_frame(&self.program, body, args)?],
            stack: Vec::new(),
        });
        Ok(())
    }

    /// Start `state`'s handler for `event` (`state_entry` or `state_exit`) in
    /// `phase`, if the state has one; otherwise leave nothing in progress.
    fn start_transition_handler(&mut self, state: StateId, event: &str, phase: Phase) -> bool {
        let started = handler_index(&self.program, state, event)
            .map(|index| BodyRef::Handler { state, index })
            .and_then(|body| new_frame(&self.program, body, Vec::new()).ok())
            .map(|frame| Activity {
                phase,
                frames: vec![frame],
                stack: Vec::new(),
            });
        let is_started = started.is_some();
        self.activity = started;
        is_started
    }

    /// Move to `target`: the queue is discarded and `state_entry` is next.
    fn switch_to(&mut self, target: StateId) -> Outcome {
        self.state = target;
        self.queue.clear();
        let _entered = self.start_transition_handler(target, "state_entry", Phase::Entry);
        Outcome::StateChanged(target)
    }

    /// The bottom body returned: decide what runs next.
    fn finish_body(&mut self) -> Outcome {
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
                        self.switch_to(target)
                    }
                }
                Some(_) | None => Outcome::Finished,
            },
            // A `state` statement inside `state_exit` is not obeyed: the
            // transition already under way wins.
            Phase::Exit(target) => {
                self.pending_state = None;
                self.switch_to(target)
            }
        }
    }
}
