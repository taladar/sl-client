# LSL engine architecture

This chapter is the design record for running LSL scripts on
`sl-fake-grid` (roadmap task `server-lsl-architecture`, context in
`roadmap/context/lsl.md`). It makes four decisions that every later task
in the programme builds on — where the code lives, how a script executes,
how scripts share a tick, and what a library function looks like — and
gives the reason for each, so that the lowering, the VM and the sixteen
library tranches do not each decide them again.

The crate this chapter describes exists, and its `README.md` points back
here; so far it holds the value model, the library table, the compiler and
the VM with its scheduler (below, each marked **as built**). The `Host`
trait exists with the methods the functions written so far need, and grows
with the library.

## 1. Where the code lives

**A new workspace crate, `sl-lsl-runtime`**, holding the value model, the
lowering, the VM, the event queue and the library with its dispatch table.
Every library function that touches the world does so through a **`Host`
trait** the crate defines and does not implement; `sl-fake-grid`
implements it over its region.

```text
sl-lsl           front end: lexer, parser, AST, semantic pass, types table
  ↑
sl-lsl-runtime   values, lowering, bytecode VM, events, library, Host trait
  ↑
sl-fake-grid     implements Host over the region; owns the heartbeat
```

Why not in `sl-lsl`: `sl-lsl` is a *client* dependency — the script
editor, the highlighter and `sl-lsl-lsp` all link it — and a runtime is
several times its size and useless to all three. Why not in
`sl-fake-grid`: a runtime inside the grid can only be tested through a
grid, and the offline script corpus (`test-lsl-script-corpus`) wants to
run a script against a mock `Host` that records calls, with no region,
sockets or sessions at all.

The runtime is **I/O-free, Bevy-free and synchronous**, like the rest of
the sans-I/O core. It depends on `sl-lsl` (the tree and the type tables)
and may use `sl-types` (`sl_types::lsl::Vector` / `Rotation` already
exist). It depends on neither `sl-wire` nor `sl-proto`: turning a
`Host::say` into a `ChatFromSimulator` is the grid's job.

### The value model is split by what it answers

LSL's rules come in two halves, and they belong in different crates.

- **What type** — which `(operator, left, right)` combinations are legal
  and what they produce, which implicit conversions exist, which casts are
  compile errors (`(key)integer`). This is needed by the semantic pass
  *and* by the lowering, so it lives in **`sl-lsl`** as a pure table over
  `ast::TypeName` — `sl_lsl::types`, transcribed from tailslide's
  `OPERATOR_RESULTS` and `LEGAL_CAST_TABLE`. The semantic pass's
  `expr_type` used to give up on arithmetic for want of it; it now asks
  the table whenever both operand types are known.
- **What value** — `f32` rounding, 32-bit wrapping, the float and vector
  formatting, the cast matrix on actual values, list comparison by length.
  Only a running script needs this, so it lives in **`sl-lsl-runtime`**.

A test in the runtime sweeps every operator and cast over one value of
each type and checks the value half accepts exactly what the table accepts
and produces the type the table states.

The value half's oracle is LSL PyOptimizer — `lslbasefuncs.py` and the
expected outputs of its `unit_tests/expr.suite`, which were measured
against Second Life. Several of its answers correct what one would guess,
and the first draft of the task guessed them:

- **Float division by zero is a `Math Error`**, as integer division is (so
  are `vector / 0.0` and a float quotient that is NaN, `inf / inf`).
- **Every type is usable in a condition**: the zero vector,
  `ZERO_ROTATION`, `""`, the empty list and a key that is `NULL_KEY` *or
  not a UUID at all* are false.
- **`(string)vector` prints five decimals**, `<1.00000, 2.00000,
  3.00000>`; the same vector inside a list cast to a string prints six. A
  float prints Mono's **seven significant digits** padded with zeros
  (`(string)123456789.0` is `123456800.000000`).
- **`list != list` is the length difference**, not a boolean; `string !=
  string` is `0` or `1`.
- **`a <= b` is `!(b < a)`**, so a NaN operand makes `<=` and `>=` true.
- **`!`, `~`, `&&` and `||` take only integers**; `&&` and `||` evaluate
  both operands (right to left, as every operator does).

One known divergence remains, recorded in the test: Second Life composes
`<3,5,7,17> * <.22,.26,.38,.86>` with a `y` of exactly `8.32`, while the
formula PyOptimizer uses — each product rounded to `f32`, summed in
double — gives `8.320001`, and no summation order tried reproduces
Second Life's value.

A rule is written once, in the half it belongs to. Operators on the
`sl-types` vector and rotation are free functions in the runtime, never
`impl Mul` on the foreign type — the orphan rule forbids it anyway, and
Linden's rotation composition is the reverse of glam's, which should be
spelled out at the call rather than hidden in an operator.

## 2. The execution model: a stack bytecode

**The lowering compiles each script to a stack bytecode, and a VM runs
it.** The alternative considered seriously was a tree-walking interpreter
over the resolved syntax tree.

The deciding constraint is not speed, it is **suspension**. `llSleep`, a
forced library delay, a state change, an exhausted execution budget and
`llResetScript` all have to stop a script *between two operations* and
resume it on a later tick, while the fake grid runs a whole region on one
thread. With a bytecode, the whole of "where this script is" is plain
data: a program counter, an operand stack of values and a stack of call
frames. Suspending is returning from `run_slice`; resuming is calling it
again.

A tree walker keeps that position on the Rust call stack, so suspending
it needs either a Rust `async` state machine or a hand-written
continuation. The first cannot be inspected, cloned or serialised — and
script state has to survive a region save (`server-lsl-script-persistence`)
— and the second is a bytecode interpreter reinvented badly.

OpenSim shows what the other road costs. YEngine does not interpret at
all: it emits CIL per script (`MMRScriptCodeGen.cs`) and runs it on a
pooled thread, and to suspend it the code generator plants a numbered
**call label before every call that might reach `CheckRun()`**, with
emitted code that saves every live local on the way out and, in restore
mode, reloads them and jumps back to the label on the way in (the
`CallLabel` comment: "our restore code will restore our args, locals &
temps, then jump to `__call_5`"). That machinery is most of the
engine's complexity, and it exists only because the program counter of
compiled code is not a number. A bytecode's is.

The rest follows cheaply from the same choice:

- **Instruction counting** is one increment per dispatched instruction,
  which is what the scheduler (below) needs.
- **A source map** is one span per instruction, emitted by the lowering;
  run-time errors report a line and a debugger can step.
- **Sharing**: a compiled `Program` is immutable and reference-counted, so
  a hundred copies of one script in a region share one program and differ
  only in their instance state.

Stack rather than register: LSL values are owned and heterogeneous
(strings and lists allocate), performance is not the constraint, and a
stack machine keeps the lowering a straightforward post-order walk. The
operand stack holds the runtime's `Value` enum; the lowering has already
made every implicit conversion an explicit instruction, so the VM carries
no coercion logic of its own.

### As built: the compiler

`sl_lsl_runtime::compile(source)` returns a `Program` or the compile errors
a grid would answer the upload with (`server-lsl-compiler-ir`). It runs two
stages and stops at the first that fails:

1. **Parse.** Only the first syntax error is reported, as the grid's parser
   stops there.
2. **The lowering**, which cannot be conservative: to emit an instruction it
   must know every expression's type and every name's meaning. So it enforces
   the grid's rules itself — tailslide's, which reproduce Linden's compiler:
   names and scopes, call arity and argument types, `return`s, operator,
   assignment, cast and condition typing; void values; lists in lists;
   members; declarations that need a `{ }`; constant global initialisers;
   event signatures and the state layout; `state` in a function outside an
   `if`; and "not all code paths return a value", which is an **error** there
   (the last statement of a value function must be a `return` or an
   `if`/`else` whose branches both end in one — a loop does not count).

The editor's semantic pass (`sl_lsl::analyze`) is not a stage. It is
conservative by design, and everything it reports the lowering reports too.

The `Program` (module `bytecode`) holds the globals, a literal pool, one
`Body` for the global initialisers, one per user function and one per event
handler. A body lists its locals — parameters first, one slot per
declaration, each starting at its type's default — and its instructions,
with a parallel source map of byte spans; `Program`'s `Display` is a
disassembler that the tests pin. What the lowering guarantees the VM:

- **Every name is resolved** to a local, global, function, library
  (`BuiltinId`) or state index.
- **Every implicit conversion is a `Cast`**: an `integer` argument where a
  `float` is wanted, `string` ↔ `key` on assignment, and the `integer` side
  of a mixed `integer`/`float` (or `integer`/`vector`) operation. The last is
  exactly what the value model does anyway (`int_to_float` first), so the
  explicit cast changes no result.
- **Operands of a binary operator are evaluated right to left**, so the
  left operand is on top when `Binary` runs; call arguments, list elements
  and vector components left to right. Both are what tailslide's Mono
  back end emits and PyOptimizer measured. `&&` and `||` evaluate both
  sides.
- **The stack is empty between statements**, so any `jump` is sound. A
  `jump` goes to the **last** label of its name in the function — though
  the label must be in scope at the jump — which is the reference's
  behaviour when a name repeats in nested blocks.
- **Every body ends in an instruction that leaves it**, so no jump targets
  the end.

The order was confirmed on Second Life itself (aditi, 2026-09-27): a script
printing from each operand saw `f(1) - f(2) + f(3)` evaluate 3, 2, 1; a
list, a vector and a call's arguments left to right; `f(0) && f(11)` both
sides, 11 first; `c + 5 + e *= 4` as `c + 5 + (e *= 4)`; and a nested
`return` of a void call run.

Three instructions carry reference quirks. `integer *= float` stores
`(integer)((float)target * value)` and yields that integer as a `float`.
`StateChange` requests the transition and leaves the body at once: in a
function — legal only inside an `if`, the reference's "state change hack" —
it returns the default of the return type, and the transition happens when
the event handler ends, as Mono does. And `InvalidProgram`, only ever a
body's first instruction, raises the run-time error Second Life raises when
Mono refuses a whole method: using the *value* of `integer *= float` compiles
there, but the event then fails with `System.InvalidProgramException` before
its first line runs (measured on aditi), because the IL leaves an `int32`
where it declares a `float`.

**A compile error happens exactly where Second Life's does, and says more.**
Each carries the grid's kind — Linden's own message, as PyOptimizer quotes it
(`Syntax error`, `Name not defined within scope`, `Type mismatch`, …) — and a
message that names the specifics (`server-lsl-compile-error-detail`): the
undefined name and a near name it may be a typo of, the types expected and
found with a cast hint, the operator and its operand types, the called
function's signature. Mistakes Linden's *grammar* catches — an unknown event or
a wrong event signature, a missing or misplaced `default`, a state with no
handler, a constant or event name used as a variable, an assignment to a
non-variable, a non-constant global initialiser — are of the syntax kind,
because there they are. "Not all code paths return a value" points at the
function's closing brace, as Second Life's does. A `CompileError`'s `Display`
has the shape a grid sends and the viewer parses,
`(line, column): ERROR : message`, with **zero-based** line and column, as
Second Life's are — measured on aditi, a function whose closing brace is on
line 5, column 1 answers `(4, 0): ERROR : Not all code paths return a value`
(the viewer passes the numbers to a zero-based `setCursor`, and OpenSim
subtracts one from its own for the same reason).

The proof is `tests/compile_corpus.rs`, with tailslide as the oracle
(`SL_LSL_TAILSLIDE_BIN`): over `sl-lsl`'s corpus, and over tailslide's own
187 test scripts (`SL_LSL_DIFFTEST_CORPUS`), every script tailslide accepts
compiles and every one it rejects fails with an error on a line tailslide
names. Two known divergences are set apart: the parser's 128-level nesting
ceiling, and a few functions tailslide knows that the vendored library
document lacks.

## 3. Scheduling: an instruction budget, round-robin, per tick

One region, one heartbeat, N script instances.

- **Each tick, each runnable instance gets at most a fixed number of
  instructions** (a per-script budget), and the region as a whole a fixed
  total. Both are counted in instructions, never in elapsed time: a
  wall-clock slice would make how far a script gets depend on the machine
  and its load, and with it the *order* of observable effects. This is the
  determinism rule of `roadmap/context/lsl.md`, and the one decision most
  likely to be made wrongly by reflex.
- **Runnable** means: running (the run flag is set), not sleeping past
  this tick, and either mid-handler or with an event queued. An idle
  script costs nothing.
- **Round-robin in a stated order** (entity index, then script item id),
  starting each tick just after the instance the previous tick served
  last. A script that exhausts its budget mid-handler resumes exactly
  there next tick.
- **Sleeping is a tick number.** `llSleep(t)` and a library function's
  forced delay set `wake = now + ceil(t / step)`; the instance is simply
  not runnable until then.
- **Library calls cost more than one instruction**: each descriptor
  states a base cost, so a loop of `llListSort` is not as cheap as a loop
  of additions. Size-proportional charges for list and string work are the
  VM task's to add — still counted, never timed.

**When the region is over budget, every script runs slower**, as on a
real simulator: the region total runs out, the rest wait for the next
tick, and the rotating start keeps that fair. The grid reports it the way
Second Life does — as the *percentage of scripts run* statistic
(`LL_SIM_STAT_PCTSCRIPTSRUN`, id 35 in the viewer's `SimStats` table) —
and per instance as instructions used, which becomes `OBJECT_SCRIPT_TIME`
and the top-scripts report (`server-lsl-memory-and-limits`) through a
stated nanoseconds-per-instruction constant. **Time dilation is not
bent to report script load**: in Second Life it measures the physics
frame, and the fake grid's tick is simulated rather than timed, so
`llGetRegionTimeDilation` stays 1.0 unless a later physics task gives it
something real to report.

The two budget constants, and the tick step itself
(`server-world-heartbeat`), are named constants in their crates, tuned by
the VM task against the reference's observed throughput. What is decided
here is their unit and their shape.

### As built: the VM and the scheduler

Module `vm` (`server-lsl-vm-execution`). An `Instance` is one script: an
`Arc<Program>` shared by every copy, and its own globals, current state,
event queue, run flag, the tick it sleeps until, and the body in progress
— a call stack of frames (body, program counter, locals) over one operand
stack. `Instance::run_slice(caller, now, step, budget, host)` runs at most
`budget` instructions and stops early at every boundary, saying which
(`Outcome`): a body finished, the budget ran out mid-body (`Yielded` — the
next slice resumes at that instruction), a sleep until tick N, a state
change, a reset, or a `Fault`. `Engine` holds a region's instances by
`CallerId` and, per `tick(host)`, advances the tick by one and serves the
runnable ones round-robin from just after the one served last, each up to
its share while the region's lasts; its `TickReport` carries every slice,
the instructions used, and the runnable and served counts behind the
scripts-run percentage.

- **Costs.** Every instruction is one; a library call adds `BUILTIN_CALL_COST`,
  and a jump taken backwards — a loop going round — adds `BACKWARD_JUMP_COST`,
  because on aditi Mono's time goes into the scheduler check at each loop
  back-edge, not into the arithmetic between them — six more instructions in a
  loop body cost it one per cent. So a back-edge is charged 513, a call 626, and
  a script runs 260 million a second (`SCRIPT_INSTRUCTIONS_PER_SECOND`): an
  empty loop at the 500 000 iterations a second aditi runs it at. The doc
  comments carry the measurements. A slice may overshoot its budget by the one
  instruction that crosses it and never more. Size-proportional charges wait for
  the functions that need them and a measurement of each (roadmap
  `server-lsl-call-cost-sizes`).
- **Budgets.** `EngineConfig::for_step(step)` derives the per-script share
  from `SCRIPT_INSTRUCTIONS_PER_SECOND` and the region's from
  `REGION_SCRIPT_SHARES` shares, so the numbers hold whatever step the
  heartbeat picks.
- **Sleeping** (`llSleep`, and the table's forced delay after any call that
  has one) is `wake = now + ceil(seconds / step)`, the seconds rounded to
  whole microseconds first so `0.2` as an `f32` — a hair above a fifth — is
  not taken for one more tick. Not positive is no sleep.
- **Stopping is not resetting.** `set_running(false)` keeps the globals, the
  state and a handler in progress, and empties the queue; a stopped script is
  offered no events (`Posted::Stopped`), and a restarted one carries on
  mid-handler. A sleep counts on while the script is stopped: restarted after
  its wake tick it resumes at once, restarted before it waits out the rest. All
  of it measured on aditi (2026-09-28), where it differs from OpenSim's YEngine
  only in the last point.
- **Reset** (`llResetScript`, or `Instance::reset` from outside) puts every
  global back to its default for the initialisers to run again, drops the
  stacks, the queue, a pending state change and any sleep, and returns to
  `default`, whose `state_entry` follows the initialisers. The run flag is
  left alone.
- **State changes.** `state x;` records the target; when the event handler
  ends, `state_exit` of the old state runs, the queue is discarded, and
  `state_entry` of the new one runs. `state` to the current state is no
  transition at all, and a `state` inside `state_exit` is not obeyed.
- **Events.** `post(event, args)` checks the arguments against the event's
  parameters (`PostError` is a host bug) and queues the event only if the
  current state handles it (`Posted::NoHandler` otherwise). The queue's
  bound and coalescing rules are `server-lsl-state-and-events`'.
- **Errors never panic the region.** A `Math Error`, a body Mono refuses
  (`InvalidProgram`), a call to a function nobody wrote
  (`RuntimeError::Unimplemented`), or a broken compiler promise
  (`RuntimeError::Internal` — a lowering bug, named) stops the one script:
  it keeps its state and globals and is no longer running, and the `Fault`
  carries the source position of the instruction that failed. Recursion
  deeper than `MAX_CALL_DEPTH` is a stack-heap collision until the memory
  accounting of `server-lsl-memory-and-limits` stops it at the reference's
  exact depth; that task also bounds the heap, which nothing does yet.
- **`print`** goes to `Host::print`: Second Life writes it to the
  simulator's log, where no resident sees it, so a host logs it and never
  turns it into chat. It is also what the VM's own tests observe.

### As built: events and states

`server-lsl-state-and-events`. The rules around the queue are as
observable as the handlers, so each is a named rule with a test.

- **Only the current state's handlers are offered events.** `post` drops an
  event the current state does not handle (`Posted::NoHandler`) instead of
  queueing it, so it cannot fire after a later state change. `state_entry`
  and `state_exit` are the VM's own and cannot be posted.
- **The queue holds `MAX_QUEUED` (64) events**; one more is dropped
  (`Posted::QueueFull`).
- **A timer never stacks.** `llSetTimerEvent` is counted in ticks, rounded
  up and at least one; `Engine::tick` posts `timer` first when it falls due,
  and a `timer` already waiting in the queue absorbs the next
  (`Posted::AlreadyQueued`). A reset stops it; a state change does not.
- **Touches and collisions coalesce per tick** (under the `Reference`
  policy below). A detection event posted in the same tick as a queued one of
  the same kind merges into it
  (`Posted::Merged`): the detected lists join, without repeating a key, up to
  `MAX_DETECTED` (16), and the count parameter follows. So everyone who
  touched in one frame arrives as one `touch_start` with a count, as on the
  grid; a touch in the next tick is an event of its own.
- **The detected block travels with the event.** A host posts a detection
  event (`touch*`, `collision*`, `sensor`) with `post_detected` and a list of
  `Detected` records, and the count parameter is the list's length. The block
  becomes current when the handler starts, and any other event clears it: a
  `timer` right after a touch reads nothing (aditi, 2026-09-28 — the quirk
  the roadmap once expected, the old block staying visible, is not Second
  Life's). Past the end of the block every `llDetected*` reads its type's
  zero — `NULL_KEY`, `0`, `ZERO_VECTOR`, `ZERO_ROTATION` — except
  `llDetectedName`, which reads the `NULL_KEY` *string*; the touch face and
  coordinates read zero too, not the `TOUCH_INVALID_*` markers a real touch
  without surface information carries.
- **A state change** runs the old state's `state_exit`, discards the queue,
  calls `Host::left_state` — where the host drops the script's listens and
  taken controls — and runs the new state's `state_entry`.
- **`changed`** is raised through one function, `Engine::changed(id,
  bits)`, which every raiser calls with the `CHANGED_*` bits of what
  happened.
- **Merging is a policy**, `EngineConfig::coalescing`, because on the grid
  whether two changes or two touches arrive as one event is frame timing no
  script controls, so a script must cope with both shapes and a test must be
  able to force each. `Reference`, the default, is Second Life's rule as
  measured on aditi: everything raised in one tick merges, and a `changed`
  raised later joins the newest `changed` still queued **unless that one is
  next in line** — four changes a fifth of a second apart, made while the
  script was busy, arrived as the first alone and the rest as one, four runs
  out of four, while changes made with the script idle arrive one event
  each. `Never` makes every raise its own event and `WhileQueued` merges into
  anything still queued: the most split and the most merged arrivals, for a
  content test to run a scenario under both.
- **`on_rez`**: `Engine::rez(id, start_parameter)` sets what
  `llGetStartParameter` answers and raises `on_rez`. The start parameter
  survives a reset.

A table test posts every event of the table but the two transitions, with a
sample value per parameter, to a handler compiled for it, and checks the
handler printed exactly those values.

## 4. The library: one table, typed functions, an erased dispatch

About 425 `ll*` functions, written over many tasks. The failure mode is
drift — an implementation at the wrong arity, a tranche believed complete
that is four functions short, a served `LSLSyntax` document that
disagrees with what the engine runs.

**One table, generated from one vendored source**
(`server-lsl-library-surface-table` picks which), yields per function a
static descriptor:

```rust,ignore
pub struct Builtin {
    pub name: &'static str,
    pub args: &'static [TypeName],
    pub ret: Option<TypeName>,
    /// The reference's forced delay after the call, in seconds.
    pub sleep: f32,
    /// Instructions charged against the budget (section 3).
    pub cost: u32,
    /// Kept for the `LSLSyntax` document only; nothing enforces energy.
    pub energy: f32,
}
```

plus a `BuiltinId` enum indexing it. Every consumer reads that table: the
lowering resolves a call to a `BuiltinId`, the coverage harness counts
implementations against it, and the grid renders its `LSLSyntax` document
from it (`protocol-sim-lsl-syntax-document`) — so the editor highlights
exactly what the engine can run, and the semantic pass on the fake grid
checks against the same list the lowering resolves against.

**People write typed functions; the table holds an erased entry.** The
roadmap's first sketch was one signature for every function,
`fn(&mut Vm, &mut dyn Host, &[Value]) -> Result<Value, RuntimeError>`.
That shape is kept — as the *generated* entry the dispatch table points
at — but no one writes against it. The generator emits, per function, a
shim that takes the arguments off the stack as the types the table
states and calls a hand-written function with a real signature:

```rust,ignore
// written by hand, in the tranche's module
pub fn ll_say(
    ctx: &mut ScriptCtx<'_>,
    host: &mut dyn Host,
    channel: i32,
    text: LslString,
) -> Result<(), RuntimeError>;
```

So a function cannot be implemented at an arity or type the table does
not state (the shim would not compile), 425 functions do not each unpack
a `&[Value]` by hand, and an unimplemented function is a compile error
until it is either written or declared a stub — which is the
implemented / stubbed / missing split the coverage harness reports.

**As built** (`server-lsl-library-surface-table`): the source is Linden's
own `LSLSyntax` document, `keywords_lsl_default.xml`, vendored in
`sl-lsl-runtime` (provenance in its `README.md`), and `build.rs` turns it
into `BuiltinId`, the `BUILTINS` / `CONSTANTS` / `EVENTS` descriptors and
one generated `Signature` per function (its argument tuple and return
type as Rust types). The per-function shim became one generic
`Handler` over argument tuples, written once: `registry!` registers
`LlAbs => math::ll_abs` by passing the function through
`invoke::<signatures::LlAbs, _, _>`, so the compile-time check is the
same — a mismatched implementation does not compile. Two differences
from the sketch above:

- **A function nobody has registered is `Missing` at run time**
  (`CallError::Missing`), not a compile error. Making it one would mean
  declaring 425 stubs before the first is written, and the coverage
  count's third column would be empty by construction. The coverage test
  fails instead when a function falls back from the committed baseline.
- **The pure functions are generic over the context**
  (`fn ll_abs<C>(_ctx: &mut C, …)`) and ignore it, so their tests pass
  `&mut ()`; `call` itself takes the concrete `ScriptCtx`, and a function
  that needs it (`llSleep`, `llResetScript`) names it.

**`ScriptCtx`, not `&mut Vm`.** A library function sees the calling
instance through a narrow context — which script is calling, the current
tick, the current event's detected block, the instance's accounted
memory — and asks the VM for control-flow effects through it: sleep,
reset, die. It never reaches the operand stack or the program counter.
`llDie` in particular *requests* the object's removal and ends the slice;
the host removes the object once control is back with the grid, which is
the awkward case `server-fake-grid-script-engine-wiring` names.

## The `Host` trait

The host is a thin translation onto the region: a `Host` method with logic
in it is a library function in the wrong crate. A caller is identified by
an opaque `CallerId` the host mints when it creates the instance, so the
runtime never learns what an entity is. Events travel the other way
through the runtime's own API (`post(instance, event)`), not through this
trait.

**As built**, the trait holds what the VM itself needs — `print`, and `stubbed`,
the notice that a stubbed function answered with its type's default — and each
library tranche adds its own methods. The methods the first tranches need — chat
and listens, identity, position, hover text, and the seeded randomness and clock
the determinism rule requires — are enough to fix its shape:

```rust,ignore
pub trait Host {
    /// llWhisper / llSay / llShout / llRegionSay / llOwnerSay.
    fn say(
        &mut self,
        caller: CallerId,
        volume: ChatVolume,
        channel: i32,
        text: &str,
    );

    /// llListen: register a filter; the host delivers matching chat
    /// back as `listen` events, in registration order.
    fn listen(
        &mut self,
        caller: CallerId,
        filter: ListenFilter,
    ) -> ListenHandle;
    /// llListenControl / llListenRemove.
    fn listen_set(
        &mut self,
        caller: CallerId,
        handle: ListenHandle,
        state: ListenState,
    );

    /// llGetKey / llGetOwner / llGetCreator …
    fn identity(&self, caller: CallerId) -> ObjectIdentity;

    /// llGetPos / llSetPos (the host applies the reference's clamps).
    fn position(&self, caller: CallerId) -> Vector;
    fn set_position(&mut self, caller: CallerId, to: Vector);

    /// llSetText.
    fn set_text(
        &mut self,
        caller: CallerId,
        text: &str,
        color: Vector,
        alpha: f32,
    );

    /// 32 raw bits from the grid's seeded minter; llFrand, llGenerateKey
    /// and llListRandomize shape them in the runtime.
    fn random_bits(&mut self) -> u32;
    /// Seconds since the epoch, from the grid's injected clock
    /// (llGetUnixTime), never the machine's.
    fn unix_time(&self) -> i64;
}
```

`ChatVolume`, `ListenFilter`, `ListenHandle`, `ListenState` and
`ObjectIdentity` are the runtime's own types. Listens live on the host
side because which listener hears a message is a question of positions
and ranges, which is the world's (`server-world-chat-routing`). Time
*within* a script — `llGetTime`, timers, sleeps — is counted in ticks by
the runtime and never asks the host.

## Out of scope

- **Luau / SLua.** `sl-proto` models `ScriptLanguage::Luau`, but this
  programme is LSL. The `Host` trait and the library table are
  language-neutral on purpose, so a second front end could target the
  same VM-independent host later without either being redesigned.
- **LSO and Mono bytecode.** Scripts are compiled from source every time;
  nothing reads or writes the reference's compiled forms.
- **A sandbox for hostile content.** Budgets and memory limits exist
  because scripts observe them, not as a security boundary.
