# LSL engine architecture

This chapter is the design record for running LSL scripts on
`sl-fake-grid` (roadmap task `server-lsl-architecture`, context in
`roadmap/context/lsl.md`). It makes four decisions that every later task
in the programme builds on — where the code lives, how a script executes,
how scripts share a tick, and what a library function looks like — and
gives the reason for each, so that the lowering, the VM and the sixteen
library tranches do not each decide them again.

The crate this chapter describes exists, and its `README.md` points back
here; so far it holds the value model (below). The rest of the chapter is
still design.

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

The methods the first tranches need — chat and listens, identity,
position, hover text, and the seeded randomness and clock the determinism
rule requires — are enough to fix its shape:

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
