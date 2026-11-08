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

The vector and rotation operators are Second Life's to the bit, held to
`math/rotation-bits`' aditi transcript (seven pairs, every component):
`rotation * rotation`, `vector * rotation` and the dot product are
evaluated in double from the float components and rounded once, as Mono
evaluates a float expression; `rotation / rotation`, `vector / rotation`
(Linden's two-step formula) and the cross product round every operation to
a float. PyOptimizer's open case, `<3,5,7,17> * <.22,.26,.38,.86>`, has a
`y` of the float nearest `8.32` that way, where its products-rounded-first
formula gives `8.320001`.

A float expression of several operators is Mono's too: evaluated in double
and rounded to a float only where the value leaves the expression
(`math/float-rounding`, every row on aditi). The VM's operand stack holds a
float either as a stored `f32` or as an unrounded `f64` (the runtime's
private `Operand`): `+ - * /` of floats, unary `-`, the dot product, `++`,
`--`, an integer converted to `float` and a `(float)` cast produce the
double; comparisons, a condition and the value of an assignment read it as
it is; and every other consumer — a store, an argument to a user or library
function, a return value, a list element, a vector or rotation component,
the factor of `vector * float`, an `(integer)` or `(string)` cast — rounds
it on the way. `Value::Float` stays an `f32`, since every stored float is
one; `ops::binary`, which takes values, keeps rounding each operation and is
what a single operator on stored floats gives anyway.

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

`compile_for(source, Dialect)` compiles as another grid's compiler where
the two were measured to disagree (the `script-compile` census,
[Grid Behaviour → LSL](../gridspec/lsl.md#compiling)): `Dialect::OpenSim`
is YEngine, which compiles a
function that can end without a `return` (the lowering's fall-off `return`
of the type's default then runs), and knows neither the linkset data store
nor experiences: none of the `llLinksetData*` functions, `LINKSETDATA_*`
constants or the `linkset_data` event, and none of the fourteen experience
functions (`llClearExperiencePermissions` and `llGetExperienceList` among
them), the `XP_ERROR_*` constants, `llSitOnLink`'s `SIT_*` results or the two
`experience_permissions*` events, nor the six task-inventory functions Second
Life added after OpenSim's library was written (`llGetStartString`,
`llGetInventoryDesc`, `llGetInventoryAcquireTime`, `llGetNotecardLineSync`,
`llRezObjectWithParams`, `llDerezObject`) with their `REZ_*`, `DEREZ_*` and
`NAK` constants. Which of the rest of the library OpenSim lacks is
unmeasured, so the rest compiles. OpenSim's own families — OSSL,
LightShare and MOD — and the constants only its document lists compile in
that dialect alone; Second Life knows none of them (`Name not defined within
scope`, [§ OSSL](../gridspec/lsl.md#ossl)). Some of OpenSim's functions are
**overloaded** (`osApproxEquals` six ways, `osTeleportAgent` three), and the
lowering picks the overload as YEngine does: the call's arity first, then
argument by argument the overloads whose parameter is the argument's type, or
failing that one it converts to. The fake
grid compiles every script upload this way (`sl_fake_grid::script_compile`).

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
  comments carry the measurements.
- **Size-proportional charges** (`vm/cost.rs`). Joining lists or strings with
  `+` costs what aditi measured for the length of the result, through the
  measured points joined by straight lines and on along the last one's slope:
  a new list 186 µs before it holds anything and some 16 µs an element after,
  a string 11 µs and some 0.19 µs a character (`LIST_JOIN`, `STRING_JOIN`).
  `llGetListLength` is flat there, as the flat call cost charges it. A test
  runs the probes' own loops in the VM and holds every charge to its aditi
  transcript within 5 %. The library's size-dependent functions get their
  charges from the same table as they are written (book: gridspec `lsl.md`,
  § *What a call costs*).
- **Overdraft.** A slice stops at the instruction that crosses its budget, and
  that one may cost more than was left — a join of a long list costs millions.
  What it overdrew comes out of the instance's next shares, and the region's
  budget alike, so a script sits out the ticks its operation would still be
  running for, and doing its work in large pieces never makes it faster.
- **Budgets.** `EngineConfig::for_step(step)` derives the per-script share
  from `SCRIPT_INSTRUCTIONS_PER_SECOND` and the region's from
  `REGION_SCRIPT_SHARES` shares, so the numbers hold whatever step the
  heartbeat picks. 130 shares is aditi's: 128 busy scripts each kept the rate
  of one, 256 ran at half (book: gridspec `lsl.md`, § *How many scripts a
  region runs*).
- **Sleeping** (`llSleep`, and a call's forced delay) is
  `wake = now + ceil(seconds / step)`, the seconds rounded to whole
  microseconds first so `0.2` as an `f32` — a hair above a fifth — is not
  taken for one more tick. Not positive is no sleep.
- **Forced delays are each flavour's** (`vm::delays`): the keyword file's
  `sleep` where the grid agrees, the grid's own where it does not (Second
  Life's `llTextBox` takes none; OpenSim's `llModPow` a second, `llLoadURL`
  ten, the link-media calls one, `llGetPrimitiveParams` none — its
  `ScriptSleep`s as its source states them). A call whose delay depends on how
  it went says so (`ScriptCtx::delay`): OpenSim's calls that return before
  their `ScriptSleep` take none, `llGiveInventory` to an avatar takes 2 s or
  3 s. Every refusal on OpenSim is `LSL_Api.Error`, which costs a second of its
  own (`ScriptCtx::refuse` charges it; `error_pause` for the few Second Life
  complaints that cost one too). OpenSim scales the forced delay — not
  `llSleep`, not the error's second — by the region's `ScriptDelayFactor`
  (`EngineConfig::script_delay_factor`) and drops one under 10 ms. Its
  channel-0 chat throttle sleeps an `llSay` or `llShout` two seconds past
  eleven lines in a second (`delays::SayThrottle`); XEngine, whose thread
  blocks there, sleeps before the line is said — the call returns having done
  nothing and is made again on waking (`ScriptCtx::delay_first`) — and YEngine
  after.
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
  (`RuntimeError::Internal` — a lowering bug, named) ends the body in progress
  of the one script, and the `Fault` carries the source position of the
  instruction that failed and the event being handled. Running out of memory
  is a fault too — a stack-heap collision on Mono, an `OutOfHeapException` on
  YEngine — and so is a runaway allocation where the grid sets no limit
  (`RuntimeError::HostMemory`): see *As built: memory* below.
  `MAX_CALL_DEPTH` is only the backstop for OpenSim's engines, whose
  recursion takes the simulator down at no depth anyone measured.
- **What becomes of a faulted script is the flavour's**
  (`EngineConfig::flavour`, an `EngineFlavour`; `server-lsl-runtime-errors`,
  measured in `gridspec/lsl.md` § Run-time errors). On `SecondLife` it is
  stopped and *held*: it keeps its state and globals, takes no events, and
  setting the run flag again does not release it — a reset does, before or
  after. On `YEngine` it is held with its run flag still set, so it reads as
  running and answers nothing until a reset. On `XEngine` the event ends and
  the script runs on with the next one.
- **The engine says the fault on `DEBUG_CHANNEL`** through `Host::say`, in the
  flavour's words: Second Life's two lines (`<prim name> [script:<script>]
  Script run-time error`, then `Math Error` / `Stack-Heap Collision`), which a
  script within say range hears; YEngine's and XEngine's one line naming the
  script, the event, the prim's key and its position, which reaches avatars
  and no script (and nothing at all for a runaway recursion). No grid names
  the line, so neither does the message; the position stays in the `Fault`
  for the host's log. The words need what only the host knows — the prim's
  name and key, the script's name and item key, the region, the position, the
  object to count a flood against — and `Host::whereabouts` answers it.
  YEngine's exhausted heap is the exception: it is told to the owner alone
  (`Host::owner_say`), four lines naming the item, the exception with its
  three numbers, the prim, script and place, and that a reset is needed.
- **A library call refusing its arguments** (`llGetScriptState` of a missing
  script) words the refusal for the flavour through `ScriptCtx::refuse`
  (`Could not find script 'Nope'.` against OpenSim's `llGetScriptState: Can't
  find script 'Nope'`), and the engine says it after the slice; on OpenSim it
  reaches scripts as a shout. It costs the script a second on every grid.
- **Second Life limits an object's error lines**: eight within sixteen seconds
  pass, the next is
  `Too many errors... dropping further messages until the flood stops`, and
  every line of the object is dropped for 34 seconds from it and until two
  seconds passed with no line, after which the object starts afresh. Aditi's own
  limit turns about a second either way from one run to the next, so the figures
  are the choices that get the fewest measured lines wrong, and the replays name
  the rest. Every line counts — a refusal of any function, a fault's two lines.
  A rule list's complaints (`llSetPrimitiveParams`, `llGetPrimitiveParams`,
  `llParticleSystem`) are told the owner alone (`ScriptCtx::refuse_to_owner`,
  delivered through `Host::say_to` on `DEBUG_CHANNEL`), let one line further and
  dropped silently. Another object's lines are not affected, and OpenSim has no
  limit and says every complaint in the open.
- **One run flag, read where it is kept.** `llGetScriptState(name)` asks the
  host which instance of the caller's prim is called `name`
  (`Host::script_named`) and reads that instance's run flag — the one the
  engine schedules by, a fault clears and the Running checkbox shows — never a
  copy, as the flavour reads it (`Instance::reads_running`: on Second Life a
  held script is stopped whatever its flag). The engine lends a script the
  region's other instances while it runs (`ScriptCtx` sees their flags and
  nothing else); the caller itself reads as running, a script with no instance
  as stopped, and a name that is no script as stopped, refused as above.
- **`print`** goes to `Host::print`: Second Life writes it to the
  simulator's log, where no resident sees it, so a host logs it and never
  turns it into chat. It is also what the VM's own tests observe.

### As built: memory

A Rust VM has no natural 16 KB limit, and that is a problem rather than a
relief: content reads its memory (`llGetFreeMemory() < 2000`) and runs out
where the grid it was written for runs out. So every instance accounts its
memory as its flavour's engine does (`vm::memory`, on top of the per-value
costs of `value::memory`; the measurements are `gridspec/lsl.md` § Memory and
limits):

- **What a script reads.** On Mono `llGetUsedMemory` is the script's code and
  what its globals reach — never a local, an argument or a frame —
  `llGetFreeMemory` the limit less the most used since the limit was set (a
  low-water mark), and `llSetMemoryLimit` takes anything from the current use
  to 65 536 and restarts the mark. On YEngine the reading is the heap
  tracker's — globals and the strings and lists in locals — against a 1 MiB
  heap, and `llSetMemoryLimit` is always `FALSE`. XEngine answers 65 536 to
  every read. `llScriptProfiler(PROFILE_SCRIPT_MEMORY)` and `llGetSPMaxMemory`
  record the most used while profiling, on Mono.
- **Two parts are models, and say so.** Mono's code is not ours to measure —
  the runtime runs its own bytecode — so it is costed as a constant, a cost
  per function or handler and a cost per instruction, fitted to eight probe
  scripts' first readings on aditi (all within 300 bytes, about the grid's own
  wander). Mono's stack never shows in a reading yet collides, so a frame
  costs a fixed amount plus its slots toward the collision only, fitted to the
  depths aditi's recursions reached at the default limit.
- **Where a script runs out** is checked where memory grows — a global or a
  local stored, a call made, a value made that alone could never fit —
  which is what makes aditi's list collide at its measured length although
  appending copies the list. Mono stops and holds the script with its two
  `Stack-Heap Collision` lines; YEngine follows its own source
  (`XMRInstArrays.PopString`'s check of a global's growth, and
  `UpdateLocalsHeapUse`'s of the locals) and leaves the script reading as
  running; and wherever a grid sets no limit, a script past 64 MiB stops with
  the runtime's own `HostMemory` rather than exhaust the machine.
- **The region's view.** `Engine::usage` and `Engine::top_scripts` give each
  script's `OBJECT_SCRIPT_MEMORY` figure (its limit, running or not, on Second
  Life; a flat 16 384 a running script on OpenSim) and its script time — the
  instructions it was charged, averaged over thirty seconds of ticks and
  converted at the scheduler's rate — busiest first; `Engine::active_scripts`
  is `SimStats`' `ActiveScripts` (an average of the running scripts on Second
  Life, the scripts present on OpenSim). The fake grid reports them in
  `SimStats` (with the scripts-run percentage of its last tick) and in a
  `LandStatRequest`'s top scripts, summed per linkset on its root, for an
  object the scene states no cost for. `llGetObjectDetails` does not read
  them yet: its host answers while the engine is lent to the tick, so
  `OBJECT_SCRIPT_MEMORY` is 64 KiB a script, `OBJECT_SCRIPT_TIME` 0 and every
  script with an instance counts as running.
- **Not modelled:** Second Life's LSO and its 16 KB, since the fake grid
  compiles every script from source; and the garbage Mono's readings carry,
  which is why a reading here sits a couple of hundred bytes under aditi's and
  a recursion under a lowered limit stops shallower than aditi's.

### As built: events and states

`server-lsl-state-and-events`. The rules around the queue are as
observable as the handlers, so each is a named rule with a test.

- **Only the current state's handlers are offered events.** `post` drops an
  event the current state does not handle (`Posted::NoHandler`) instead of
  queueing it, so it cannot fire after a later state change. `state_entry`
  and `state_exit` are the VM's own and cannot be posted.
- **The queue holds `MAX_QUEUED` (64) events**; one more is dropped
  (`Posted::QueueFull`).
- **A timer never stacks.** `llSetTimerEvent` keeps its interval and its
  next due time in region time (the ticks times the step), the interval no
  shorter than the flavour's floor — two of Second Life's frames, OpenSim's
  `MinTimerInterval` — and a negative one stopping the timer on Second Life
  and running it at the floor on OpenSim; it falls due at the first tick at or
  after its due time. `Engine::tick` posts `timer` first when it falls due,
  and a `timer` already waiting in the queue absorbs the next
  (`Posted::AlreadyQueued`). The next due time is each grid's: Second Life
  keeps the phase, YEngine counts from the event, XEngine keeps the phase of
  its first (`time/timer-phase`). A reset stops it; a state change does not.
- **A sensor sweeps at the next tick.** `llSensor` asks for a sweep (Second
  Life keeps the last one asked for, OpenSim every one) and `llSensorRepeat`
  sets one going; `Engine::tick` sweeps, after the timers, for every script
  whose sweep is due, and posts `sensor` with the detected block or
  `no_sensor`. A state change ends a repeating sweep.
- **Touches and collisions coalesce** (under the `Reference` policy
  below). A detection event posted in the same tick as a queued one of the
  same kind merges into it (`Posted::Merged`): the detected lists join,
  without repeating a key, up to `MAX_DETECTED` (16), and the count
  parameter follows. So everyone who touched in one frame arrives as one
  `touch_start` with a count, as on the grid. A touch merges into the same
  touch event still queued whatever tick it was raised in — three touches a
  second apart, made while the script slept, were one `touch_start`, one
  `touch` and one `touch_end` on aditi (`events/merging`); a collision in the
  next tick is an event of its own.
- **The reference's queue is ranked** (`events/order`): an event waits
  behind every queued event of a lower rank raised while the same handler
  ran — a touch behind the rest, a `changed` behind the touches — and
  behind every event raised before that handler began (`Queued::batch`
  counts the handlers begun) — a sweep's answer behind the rest, a touch
  behind the answers, a `changed` behind the touches (`events/latency`).
  `Never` and `WhileQueued` keep one rank, first in, first out, as OpenSim
  does.
- **An event can be queued before it may run** (`Arrival::ready`). Second
  Life queues a link message at the call yet runs it the next frame, and a
  setter's `changed` and a sweep's answer two frames after the call
  (`events/latency`); queued at once, they go with the queue at a state
  change or a reset. A script starts the first queued event free to run,
  passing over one still held back (`events/timer-reset`).
- **Each flavour delivers chat when its grid does.** `Host::say` and
  `Host::say_to` answer the scripts in earshot, and the engine offers them
  the line (`Engine::overhear`): at once on Second Life, at the next of
  OpenSim's async command passes, every 100 ms of region time. On OpenSim a
  setter's `changed` and `llSensor`'s answer are queued in the call, and a
  script an event reaches after its turn in the tick runs it on what is
  left of its share, as an engine thread would.
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

### As built: list objects

`server-lsl-opensim-shared-lists`. On Second Life a list is a value. On
OpenSim it is a .NET object both engines hand on by reference, and some of
the library and XEngine's `+` change it in place, so the change shows through
every variable holding it (`gridspec/lsl.md` § A list: a value or an object).
The value model copies a list wherever it goes, so on the OpenSim flavours
the VM keeps an **identity** beside each list it holds (`vm::objects`):

- **Where it lives.** An operand-stack entry is an `Operand::Shared` (the
  elements and a `ListId`); a frame's locals and the instance's globals each
  have a slot of `Option<ListId>` beside them (`Frame::objects`,
  `ListObjects::globals`). A list nothing else holds yet — a literal, a
  library result, a concatenation — has none. A load from a variable mints
  one for the variable if it has none, and `Dup` mints one for a list about to
  be held twice; a store, an argument, a return and a `(list)` cast hand the
  identity on. On Second Life nothing is ever minted, and every list is a
  plain value as before.
- **A change in place** is written to every global, every frame's local and
  every operand holding the identity (`objects::rewrite`). A library function
  reports one through its context — `write_back(argument, list)` for the
  argument it changed (`osListSortInPlace`, `llSetPayPrice`, `llDialog`,
  `llJsonSetValue`, `llJsonGetValue`, `llJsonValueType`; it stands even when
  the call then fails, as OpenSim pads before it throws) and
  `return_argument(argument)` when what it returns is the argument's own
  object (`llList2List`, `llListInsertList`, `llListReplaceList` in their
  edge cases). XEngine's `list + string`, `+ integer`, `+ float` and `+ key`
  change the left operand and hand it back (`objects::appends_in_place`).
- **Not a store.** A change in place charges no memory: YEngine counts a list
  against the variable it is stored in, when it is stored, and is not told of
  a change made in place.
- **Saved with the script.** `SavedScript` keeps the identities (format
  version 3). A take and a rez or a restart under YEngine keeps them; an
  XEngine shutdown gives every global its own list again
  (`SavedScript::separate_lists`), as `ScriptBase.GetVars` copies each list
  global when it saves a script.

### As built: persistence

`server-lsl-script-persistence`. A script's running state belongs to its
object, not to the region: taken into inventory and rezzed again, or caught
by a restart, a script carries on where it was (`gridspec/lsl.md` §
Persistence has the measurements). `vm::persistence::SavedScript` is all of
it as plain data — the globals, the state, the run flag and a run-time
error's hold, the queued events with their detected blocks, the body in
progress down to its program counter, call frames and operand stack, a
requested state change, the timer, the listens, the permissions and the
questions waiting, the start parameter and what of the memory accounting is
the script's own (its limit, Mono's low-water mark, the profile). It holds
no program and no tick:

- **No program.** The compiled script is the inventory item's, handed back
  on the restore, which checks the saved state against it. A global of
  another type, a state, function or handler the program lacks, a program
  counter past its body or a queued event its state cannot handle is a
  `RestoreError`, never a script running off its code. A script is not
  brought back under another engine either: its memory accounting is in
  that engine's measure.
- **No tick.** A sleep and a timer are saved as the time left of them, so a
  saved script comes back in a region whose clock reads anything, and the
  time it was away is the host's to say.

`SavedScript::encode` writes it in a compact form of this crate's own — a
mark, a version, then the fields in order, floats by their bits so a NaN a
script holds comes back as it was — and `decode` reads it back, refusing
another form, another version, a short read or bytes past the end. It is
**not** OpenSim's `ScriptState` XML: that form is OpenSim's engines'
internals, and the recommendation of the task was a form of our own, with
OpenSim's readable later, lossily, if a cross-grid object ever needs it.

What a grid does with the saved state is the engine's, per flavour:

| entry point | Second Life | YEngine | XEngine |
| --- | --- | --- | --- |
| `Engine::take(id)` | the instance leaves, saved, a sleep and all | the same, the sleep **ended** | the same (the rez resets it) |
| `Engine::rez_saved(id, program, saved, away, start)` | everything kept; `away` counts against the sleep and the timer; the body in progress, **`on_rez` ahead of the queue**, then the queue | everything kept; `away` counts against nothing (OpenSim keeps a timer as the time left); the body in progress, the queue, **then `on_rez`** | **reset**, a stopped script running again: initialisers, `state_entry`, `on_rez` |
| `Engine::shut_down()` | every instance leaves, saved | the same, sleeps ended | the same, **bodies in progress dropped** |
| `Engine::start_up(scripts, down)` | everything kept, `down` counting as `away` does; `changed(CHANGED_REGION_START)` | everything kept; `changed(32768)`, then `changed(CHANGED_REGION_START)` | the same, a stopped script running again |

A stopped script stays stopped through a take and a restart on Second Life
and YEngine, so the `on_rez` or `changed` raised in it is dropped
(`Posted::Stopped`); a deliberate `llResetScript` is how a script loses its
state, and is the only way. No grid raises a `changed` for a rez, and none
runs `state_entry` or `on_rez` after a restart. Second Life's restart is not
measured — neither test avatar holds estate rights on aditi — so its row is
the take's rule and the one constant Linden Lab documents. A
script's time (`llGetTime`) is saved with it (saved form 4): it counts on
through a take and a restart on Second Life and YEngine, the time away not
counted, and starts again on XEngine.

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
  that needs it (`llSleep`, `llResetScript`) names it. A pure function whose
  answer differs between the grids is generic over `PureCtx` instead, which
  `ScriptCtx` implements: the flavour the region imitates, and a refusal on
  `DEBUG_CHANNEL` (with the second's sleep OpenSim's `Error` costs). Its tests
  pass a bare context of either flavour.
- **OpenSim's library throws where Second Life's answers.** A function
  ported from `LSL_Api.cs` that throws there (`llInsertString` past the end,
  `llList2ListStrided` of a one-element range past the list, the median of a
  list with no number, `llList2Integer` of a float past the integer's range)
  returns `CallError::Exception`, which faults the script as
  `RuntimeError::Exception`: YEngine tells the owner, as it does an exhausted
  heap; XEngine says the .NET exception on `DEBUG_CHANNEL`.
- **A call costs its argument's size.** On top of the flat call, a string or
  list function is charged what aditi measured it to cost at its argument's
  length (`vm::cost`, from `vm/call-cost-*`): on Second Life every string
  function grows with the string, `llStringLength` included.
- **Second Life's rotation functions are Linden's `llmath` in float.**
  `library::rotations` ports the published C++ (`normVec`, `normalize`,
  `getAngleAxis`, `getMatrix3`, `orthogonalize`, `getEulerAngles`,
  `shortestArc`) with every operation rounded to a float, fitted bit for bit
  to `math/rotation-bits` on aditi; the scalar functions (`llSqrt`, `llSin`,
  …) are the double result rounded once, with PyOptimizer's corners and the
  x87's reduction of an integral angle. OpenSim's are `LSL_Api.cs`'s formulas
  in double, rounded to the value model's float as they are stored.
- **The two grids' JSON share nothing but the names.** `library::json_text`
  dispatches each `llJson*` call to `second_life` — PyOptimizer's
  `lsljson.py` scanner, which keeps the text a script gave it and rewrites
  only the level it writes to, adjusted where aditi answered otherwise
  (where a string ends, which numbers have a needless zero) and with an
  `llJsonSetValue` PyOptimizer never wrote, held to LL's own test script —
  or to `opensim`, a port of the LitJson reader and writer OpenSim builds a
  tree with and writes anew. OpenSim's `JSON_APPEND` is a string, which is the
  compiler's to know (`Dialect::constant_override`), not the library's. The
  port's corners, held to OpenSim's own `LSL_Api` run under Mono with the
  shipped DLLs: a number is written in .NET Framework's 15-digit `G` form with
  `.0` appended when it has no point (`-100.0`, `Infinity.0`), but read back
  by `llJsonGetValue` as `0.0#####`; a number that overflows a double reads as
  0 and one past a `long` fails the whole read; only one token after a
  document is looked at (`[1] x` is refused, `[1] 2` reads `[1]`); single
  quotes and comments are accepted; an empty path in `llJsonSetValue` sets
  index 0 and cuts the first and last characters (`[1,2]` becomes `5,2`); and
  a value is a number when `float.TryParse` says so (`1,000` is 1000).
- **The linkset data store is the host's, its rules the runtime's.** A
  `vm::LinksetData` is a plain value — budget, passwords, return codes, key
  order, the regular-expression search — that the host keeps one of per
  linkset and lends to a call (`Host::linkset_data`); `Host::linkset` says
  which linkset a script is in. A call that changes the store records the
  change on the instance, and the engine raises `linkset_data` for it in
  every script of the linkset once the slice is over — as Second Life queues
  it at the call, except that a reset later in the same event drops the
  writer's own. OpenSim has no store, and its dialect compiles none of the
  functions, constants or the event.
- **Talking is checked and cut by the runtime, routed by the host.**
  `library::comms` holds every say function, `llInstantMessage`, `llDialog`,
  `llTextBox`, `llLoadURL`, `llMapDestination` and `llMessageLinked`, each
  refusing and cutting as its flavour measured (`comms/*`): a line to 1024
  bytes on Second Life and 1023 on OpenSim, a dialog's limits and words, the
  empty list's `OK` or `Ok`. What it hands the host is finished — a `Speech`,
  a `Dialog`, an `InstantMessage` — and the host only says who is in reach.
  A link message is resolved from the linkset the host describes
  (`Host::links`, every prim with its link number and scripts) by the
  flavour's rule (OpenSim takes `LINK_ROOT` for a lone prim, Second Life does
  not), and posted, like a store change, once the slice is over but in the
  order of the calls — on Second Life both are queued at the call, so a link
  message sent before a burst of writes is ahead of their events in every
  queue, the sender's own included.
- **Permissions are the script's, the question and the money the host's.**
  `vm::Permissions` lives in the instance: the bits held, the granter and the
  question waiting, kept by a state change and cleared by a reset, as each
  grid keeps them on the script's inventory item. `llRequestPermissions`
  grants the avatar-control bits of a wearer or a sitter unasked and hands
  anything else to the host as a `Question`; the viewer's answer comes back
  through `Engine::answer` (held as it came, more than was asked included;
  an answer nobody asked for is nothing), a viewer's `RevokePermissions`
  through `Engine::revoke` (no event, on any grid). Second Life's refusals —
  `PERMISSION_DEBIT` of anyone but the owner, `PERMISSION_CONTROL_CAMERA` of
  an avatar that neither wears nor sits on the object — are the runtime's.
  `llGiveMoney` and `llTransferLindenDollars` check the grant, the amount
  and the payee in each flavour's order and words and hand the host a
  `Payment`; the economy answers a transfer with a `MoneyRefusal` or not,
  and the runtime raises `transaction_result` once the slice is over.
  `llSetPayPrice` hands the host the prim's `PayPrice`, which answers a
  viewer's `RequestPayPrice` (`PayPrice::reply`: Second Life's default
  buttons 1, 5, 10, 20; OpenSim's silence for a layout of zeros) and says
  which payments Second Life takes (`PayPrice::takes`); the host raises
  `money` in every script of the prim with `Engine::paid`. OpenSim pads the
  button list a script passes *in place*, which every variable holding that
  list reads afterwards (the list objects, below).
- **An experience is the grid's; the script holds only its share.** Which
  experience a script was compiled into and whether it may run here
  (`Host::experience`, a `ScriptExperience`), what each avatar has answered it
  (`Host::admission`: allowed, blocked or neither — one record for every script
  of the experience), the experience catalogue (`Host::experience_details`,
  `None` while the region has yet to look an id up, as aditi answered an unseen
  id empty and then invalid) and its key-value store (`Host::key_value_store`, a
  `vm::KeyValueStore` the grid keeps per experience) are all the host's.
  `library::experiences` holds the fourteen functions. A request of an avatar
  that allowed the experience is granted at once (`experience_permissions`, the
  grant holding the six experience permissions for that avatar), of one that
  blocked it denied, and of anyone else put as an `ExperienceQuestion` through
  `Host::ask_experience`; the host records the answer for the experience and
  reports it to the script that asked through `Engine::answer_experience`. A
  key-value call returns a fresh handle and raises `dataserver` with the store's
  answer once the slice is over. A script in **no** experience — the only kind
  aditi was measured with (`experience/*`) — is denied with reason 5, answered
  `"0,5"` by every key-value call and `SIT_NOT_EXPERIENCE` by `llSitOnLink`
  before anything else is checked; `llClearExperiencePermissions` and
  `llGetExperienceList` do nothing and list nothing. What a script *in* an
  experience is answered is Linden Lab's documentation, unmeasured, and the
  module says so rule by rule (`gridspec-lsl-in-experience`).

### OpenSim's own functions

OpenSim's engines compile three families Second Life has not got — OSSL
(`os*`), LightShare (`ls*`) and MOD (`mod*`) — 236 functions, and the
scripted content of an OpenSim region leans on them. They are in the one
table: generated from OpenSim's own served document
(`sl-lsl-runtime/opensim/ScriptSyntax.xml`, every overload an entry of its
own: `BuiltinId::OsApproxEquals` … `OsApproxEquals6`) and its supplement of the
functions it compiles and leaves out (`ossl-unserved.xml`: `osListenRegex`,
`modSendCommand`, five more), each descriptor marked with its `Family` and the
`Gate` OpenSim checks before it runs (`ossl-checks.txt`, derived from
`OSSL_Api.cs` by `extract_ossl_checks.py`).

**The gate is OpenSim's permission model, flattened nowhere.** Before an
OpenSim flavour runs one of these, `vm::ossl::admit` does what
`OSSL_Api.CheckThreatLevel` does, against the region's `[OSSL]` settings
(`vm::ossl::Settings`, the stock `osslDefaultEnable.ini` the local grid runs
unless the host sets another with `Engine::set_ossl`) and what the script's
owner is in the region (`Host::standing`: estate owner or manager, parcel
owner or group, a god; the owner and creator keys). An `Allow_` line wins over
the threat level both ways, `false` disables, a list admits those on it
(`ESTATE_MANAGER` being a manager who is not the owner), `Creators_` admits a
script's creator, and a function with no line is held to
`OSFunctionThreatLevel`. A refusal is thrown in OpenSim's words —
`OSSL Permission Error: <fn> permission denied.  Allowed threat level is …`,
`<fn> permission denied.`, `<fn> disabled in region configuration` — and
reported as each engine reports a script exception (YEngine holds the script
until a reset, XEngine runs on; to the owner alone under
`PermissionErrorToOwner`). It is checked *before* the function is looked for,
so a denied function the grid has not written is denied all the same, as the
probes need. A deprecated function, and the few that refuse an argument
rather than throw, shout on `DEBUG_CHANNEL` (`ScriptCtx::shout_error`, heard by
avatars and scripts alike) and sleep the script a second, as `OSSLShoutError`
does; LightShare and MOD switched off shout and answer nothing.

**The scope** is a line per function in `library/opensim-scope.txt`, which the
coverage test holds the registry to (`60 implemented / 166 planned / 10 out of
scope of 236` as this was written):

- **In, now** — everything that touches nothing the fake grid lacks: the
  string, regular-expression, hash and number functions (`osFormatString`,
  `osMatchString`, `osReplaceString`, `osRegexIsMatch`, `osStringSubString`,
  `osSHA256`, `osRound`, `osApproxEquals`, `osSlerp`, …); the drawing-list
  builders (`osDrawText`, `osMovePen`, `osSetPenColor`, …), which only write
  commands down; the engine's and the simulator's identity
  (`osGetScriptEngineName`, `osGetSimulatorVersion`, `osGetGridName`, the
  physics engine's — `Host::simulator`); the region's size and ground
  (`Host::region_size`, `Host::terrain_height`) and the day's length
  (`Host::day_length`) the probes read; `osGetAgentIP` (empty for all but a
  god's script, `Host::agent_ip`); `osListenRegex` (a listen whose name or
  message is a regular expression, `ListenFilter::regex`); `osMessageObject`
  (`dataserver` in another prim's scripts, `Host::prim_scripts`); and
  `modSendCommand` (a fresh key no module answers, the fake grid running none).
  The patterns are the `regex` crate's, which .NET's agree with but for its
  backreferences and lookaround: a pattern using those is refused as one that
  does not parse. The culture-sensitive searches `osStringIndexOf`,
  `osStringLastIndexOf`, `osStringStartsWith` and `osStringEndsWith` are in
  too, on a port of the collator OpenSim's Mono runs (below), and
  `osListSortInPlace`, which sorts the list object it is handed — and so
  every variable holding it (the list objects, below). With the prim tranche
  came the 23 that shadow its functions: the rule lists of any prim by key
  (`osGetPrimitiveParams`, `osSetPrimitiveParams` of a prim its owner owns)
  and of a link (`osGetLinkPrimitiveParams`), `osGetLinkNumber`, the links
  forced without `PERMISSION_CHANGE_LINKS` (`osForceCreateLink`,
  `osForceBreakLink`, `osForceBreakAllLinks`), the three overloads of
  `osSetProjectionParams` (a projector, the light's image on the wire),
  `osParticleSystem` and `osLinkParticleSystem`, the `os*Sound*` family by
  link number, `osCollisionSound`, and `osClearObjectAnimations` (0: nothing
  on the fake grid starts an object animation yet). With the task-inventory
  tranche came its 15: the inventory readers by name or item key
  (`osGetInventoryDesc`, `osGetInventoryItemKey`, `osGetInventoryLastOwner`,
  `osGetInventoryName`), the synchronous notecard readers (`osGetNotecard`,
  `osGetNotecardLine`, `osGetNumberOfNotecardLines`), `osMakeNotecard`,
  `osGetRezzingObject`, `osDie` (of the caller's own object, or one it
  rezzed), `osResetAllScripts`, `osGetLastChangedEventKey` and the loaded
  OAR's data (`osLoadedCreation*`, empty: the fake grid loads no OAR).
- **Later, with the tranche of the `ll*` functions they shadow** — the NPC
  family, avatar animation, attachment, teleport and sitting functions
  (`server-lsl-lib-avatar-control`, which has the NPC fixtures to answer
  them); the parcel, estate, environment, wind, sun and terrain writers,
  the region statistics, notice and restart and LightShare
  (`server-lsl-lib-ossl-land-writers`); inertia and object teleports
  (`server-lsl-lib-physics-vehicles`); URLs (`server-lsl-lib-http-url`). A
  task of its own takes the dynamic textures every OpenSim sign is drawn with
  (`server-fake-grid-dynamic-textures`).
- **Out** — `osConsoleCommand` (the region console, which the fake grid has
  not got: `console`); `osGetSimulatorMemory*` (the host process's memory,
  meaningless for an in-process grid: `host-process`); and the readers of the
  grid services the fake grid does not run — the grid's and an avatar's home,
  gatekeeper and login URIs, the custom grid info, an agent's country
  (`grid-services`). Each is still compiled and gated, so a script calling
  one is denied, or fails, exactly where OpenSim's would.

**.NET's culture-sensitive search, ported.** OpenSim runs on Mono, and every
`String.IndexOf(string)` with no comparison named — `llSubStringIndex`, the
delimiters of the string parsers — and every `CurrentCulture` search of the
OSSL functions goes through Mono's managed collator under en-US, not a
search unit by unit: it reads `æ` as `ae` and `ﬁ` as `fi`, ignores what its
table weighs nothing (U+0000, the noncharacters that hold the `JSON_*`
sentinels, unassigned code points), folds a combining accent into its letter,
lowers both sides by en-US's own map when case is ignored — and has bugs of
its own a script can see (`"xæ"` does not contain `"ae"`, though `"æon"`
starts with `"aeon"`: an ASCII-only first pass rules the match out by length
before the collator sees the `æ`; and a split whose delimiter matched an
expansion at the end of the string throws on its next search).
`library::collation` is a port of the collator's search paths, line for line,
reading Mono's own table
(`sl-lsl-runtime/mono/`, copied out of `mscorlib.dll`) and en-US's lower-case
map, unassigned code points and contractions as `dump_collation.cs` read them
off the running Mono; the 3000 searches that program recorded, with every
answer .NET gave, are replayed by the tests, and a 200 000-case set matched
when it was written. Only what en-US reaches is ported: its tailoring is the
invariant one — 35 one-unit expansions, no contraction of several units or to
a sort key, no diacritic remapping, no French accent order — so the collator's
paths for those are left out (the dumper refuses to write tables for a culture
that has them).

The served `LSLSyntax` document stays OpenSim's own for the OpenSim flavour
(`sl_fake_grid::script_syntax`), every function listed whatever the region
allows, as a viewer sees on the local grid.

**`ScriptCtx`, not `&mut Vm`.** A library function sees the calling
instance through a narrow context — which script is calling, the current
tick, the current event's detected block, the instance's accounted
memory — and asks the VM for control-flow effects through it: sleep,
reset, die. It never reaches the operand stack or the program counter.
`llDie` in particular *requests* the object's removal and ends the slice;
the host removes the object once control is back with the grid, which is
the awkward case `server-fake-grid-script-engine-wiring` names. **As
built**: `Host::die` answers whether the object goes (an attachment stays,
and its script carries on past the call); when it goes the call asks the VM
to end the script (`Outcome::Died`: the body, the queue and the run flag
gone), and the fake grid takes the linkset out after the tick.

## The `Host` trait

The host is a thin translation onto the region: a `Host` method with logic
in it is a library function in the wrong crate. A caller is identified by
an opaque `CallerId` the host mints when it creates the instance, so the
runtime never learns what an entity is. Events travel the other way
through the runtime's own API (`post(instance, event)`), not through this
trait.

**As built**, the trait holds what the VM itself needs — `print`; `stubbed`, the
notice that a stubbed function answered with its type's default; `say`, through
which the engine speaks a script's error lines (a `Speech`: channel, text, and
how far avatars and scripts hear it); `owner_say`, which tells the owner alone
what YEngine reports that way; `whereabouts`, which names the prim and
script those lines name; `random_bits`, the region's seeded draw
`llListRandomize` shuffles by and `llFrand` scales; and `linkset` and
`linkset_data`, the linkset a script is in and that linkset's data store —
and each library tranche adds its own methods. The comms tranche added
`say_to` (`llRegionSayTo`), `instant_message`, `dialog` (a text box too),
`load_url`, `map_destination`, `worn_by` (the avatar an attachment is on,
whom `llMapDestination` opens a map for outside a touch) and `links`. The
permissions and money tranche added `owner`, `seated_on`, `ask_permissions`
(`false` for an avatar not in the region, which OpenSim answers with
`run_time_permissions(0)`), `give_money`, `transfer_money` and
`set_pay_price`. The experiences tranche added `experience`,
`experience_details`, `admission`, `ask_experience`, `key_value_store` and
`sit_on_link` (the sit target search and parcel access, answered with a
`SitRefusal`). OpenSim's own functions added `standing` (what the script's
owner is, for the `[OSSL]` permission check), `simulator`, `region_size`,
`terrain_height`, `day_length`, `prim_scripts` and `agent_ip`. The prim
tranche (`server-lsl-lib-prim-state`) added `prim` and `set_prim` — a whole
prim lent as the runtime's `vm::prim::Prim` and handed back with the
`changed` it raises — `set_region_position`, `linkset_of`, `object_details`
(any object or avatar by key), `inventory_key` (a texture or sound named
rather than keyed) and `sound` (a sound played once, triggered or preloaded).
The task-inventory tranche (`server-lsl-lib-task-inventory`) added
`contents` (the prim's items, in any order — the listing's order is the
library's), `remove_item`, `give_to_prim` and `offer_items`,
`set_remote_pin`, `remote_pin` and `load_script`, `rez` and `derez`,
`notecard_text` and `notecard_read` (the region's notecard cache, which
`llGetNotecardLineSync` reads) and `make_notecard` (`osMakeNotecard`). The
time tranche (`server-lsl-lib-time-timers`) added
`epoch_micros` — the region's calendar time at its tick 0, which every date a
script reads counts on from by the ticks — `local_offset` (the zone OpenSim's
host keeps) and `day_offset` (OpenSim's apparent time). The detection tranche
(`server-lsl-lib-detection-sensors`) added `sense` (every object and avatar a
sweep weighs) and `cast_ray` (every surface a segment crosses), whose filtering
by each grid's rules is `vm::sensing`'s. The land tranche
(`server-lsl-lib-land-region-env`) added `parcel_at`, `region`, `sim_data`,
`ground`, `wind`, `environment`, `parcel_media`, `sim_stats`,
`environment_rule` and `map_texture` — plain `vm::land` records of what is
there, which code of `llGetParcelDetails` or key of `llGetEnv` reads which
field being the library's — and `change_land`, which applies a `LandChange`
the library has checked the script's owner may make (`llScriptDanger` reads
the parcel's flags through `parcel_at` now).
The HTTP tranche (`server-lsl-lib-http-url`) added `send_http` (a checked
`vm::http::Outbound`, every header it goes out with; the answer comes back
through `Engine::http_response`), `request_url`, `release_url`, `free_urls`,
`answer_http`, `http_throttle` (the prim's, kept by the host) and
`url_authority` (the one address on the simulator's network a script may
reach); a request arriving at a URL reaches the script through
`Engine::http_request`, after `vm::http::arrive` has judged it as the grid's
URL surface does.
The avatar tranche (`server-lsl-lib-avatar-control`) added `agent` (a
`vm::avatars::Agent` of an avatar in the region), `agent_record` and
`agent_named` (any account), `animate`, `set_animation_override` and
`animation_override`, `object_animate` and `object_animations`,
`control_changes` (the `ScriptControlChange` blocks `vm::avatars::take` and
`release` shaped, and the controls to route through `Engine::controls`),
`set_camera`, `clear_camera` and `camera`, `teleport_agent` and
`attached_list`.
The methods the first tranches need — chat and listens, identity, position,
hover text, and the seeded randomness and clock the determinism rule requires —
are enough to fix its shape:

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
    fn random_bits(&mut self, caller: CallerId) -> u32;
    /// Seconds since the epoch, from the grid's injected clock
    /// (llGetUnixTime), never the machine's.
    fn unix_time(&self) -> i64;
}
```

`ChatVolume`, `ListenFilter`, `ListenHandle`, `ListenState` and
`ObjectIdentity` are the runtime's own types. Time *within* a script —
`llGetTime`, timers, sleeps — is counted in ticks by the runtime and never
asks the host.

**As built, listens are the instance's, not the host's.** The sketch put
them on the host because which listener hears a line is a question of
positions and ranges, which is the world's (`server-world-chat-routing`).
That question stays the host's, but it is only half of hearing: whether a
script's filters admit the line, the handles, the per-script limit (65 and
an error on Second Life, 64 and a silent `-1` on OpenSim) and the drop at a
state change and a reset are the script's own, and the same on every host.
So `vm::Listens` lives in the instance; the host decides who is in earshot —
by range, the whole region, one `llRegionSayTo` target, never the speaking
prim itself — and offers each such script the line with
`Engine::hear(id, &Heard)`, which raises `listen` once for every active
listen that admits it. The fake grid's own listen registry
(`chat::Listens`) stays the consumer-side registry it is for agents and
tests; the engine wiring offers its region's lines to scripts through
`Engine::hear`.

### As built: the fake grid's host

`sl_fake_grid::scripts` (`server-fake-grid-script-engine-wiring`). Each
region's world holds a `RegionScripts`: the region's `Engine`, configured
from the heartbeat's step and the imitated grid's engine (Second Life's, or
YEngine for the OpenSim flavour), and which prim and item each `CallerId`
is.

- **The world says which scripts exist.** Nothing creates an instance by
  hand. Each tick the region asks its object store which prims' contents
  were written since it last asked (`bevy_ecs`'s change ticks on the
  `Contents` component) and brings those prims' instances in line: a script
  item (LSL, not marked Luau, with an asset) gets an instance of what its
  source compiles to, a removed item loses its own, a saved one (a new
  asset) gets a fresh instance under the same id, so it keeps its place in
  the serving order, and one that no longer compiles has none. A prim that
  left the region, however it left, takes its scripts with it. So a
  fixture's script, a `RezScript`, a drop, a save and a delete reach the
  engine by one path. A save hands the engine the program its upload
  compiled, so it is compiled once.
- **The run flag.** A new instance runs if the prim's contents list it as
  running (what the Running checkbox, a `RezScript`, an upload and a fixture
  write); from then on the instance's flag is the truth. `SetScriptRunning`
  sets both, `GetScriptRunning` reads the instance as its engine reads it,
  and `ScriptReset` resets it.
- **A tick** runs under the region's lock: reconcile, then one
  `Engine::tick` against a `RegionHost` over the world, the engine lent out
  of the world for the tick so the host can hold the rest of it. What the
  scripts said and did is applied after the tick — a line said to the
  region's listens, avatars and the scripts in earshot, a line offered to
  one prim's scripts, a linkset removed — and what goes to one viewer (a
  dialog, a permission question, a page, a map destination, a directed
  line, an instant message) is sent once the lock is released, the crate's
  session-then-region lock order. The region's own changes go out on its
  change stream with the source `REGION_SOURCE`, which no session's watcher
  takes for its own.
- **Lines.** Every line said in the region — an avatar's, a dialog reply (a
  region-wide line from the replying avatar, as OpenSim says it), a test's —
  is offered to every script in earshot of it but the speaking prim's own,
  by the line's own reach; a script's line reaches scripts as far as its
  `Speech` says and avatars as far as its type carries.
- **Taken, rezzed, restarted.** A take files each prim's contents and its
  scripts' `SavedScript`s with the taken linkset (`Engine::take`, or
  `Engine::save` for a take of a copy, which leaves the object running). A
  rez puts the contents back under the new prims' keys and the scripts come
  back with them at the next reconcile through `Engine::rez_saved`, the time
  away measured on the grid's injected clock, with `on_rez(0)`; a script that
  saved nothing, or no longer fits its program, starts afresh with `on_rez`,
  and the misfit is logged. `FakeGrid::restart_region` takes the region's
  scripts through `Engine::shut_down` and `Engine::start_up` with the time
  down; the countdown and the agents a restart sends away are
  `server-fake-grid-estate-actions`'.
- **Experiences.** A script compiled into an experience (the experience
  its upload named, kept on the item) stands where its prim is as the
  experience documentation says, which no live measurement has held yet:
  an id the catalogue does not know is `XP_ERROR_INVALID_EXPERIENCE`; a
  suspended or disabled record says so; one the estate blocks, or one a
  restricted parcel does not admit, is `XP_ERROR_NOT_PERMITTED_LAND`; one
  rated above the region is `XP_ERROR_MATURITY_EXCEEDED`; and a land-scoped
  (not grid-wide) experience runs only where the estate allowed, trusted or
  defaulted it, or the parcel admits it by name. The lists are the
  region's (`ExperienceLand`), seeded from the fixture every session's
  store is seeded from and rewritten by a `RegionExperiences` POST. A
  question put under an experience is remembered: a *Yes* records the
  experience allowed for the agent (the reference viewer sends nothing
  else — `script_question_cb` only posts a local event), so every later
  script of it is granted unasked; a *No* records nothing and the script
  hears `XP_ERROR_NOT_PERMITTED`; a block arrives over
  `ExperiencePreferences` on its own; and a question still unanswered
  after five minutes of region ticks is `XP_ERROR_REQUEST_PERM_TIMEOUT`.
- **OSSL.** `RegionConfig::ossl` hands the engine the region's `[OSSL]`
  settings (`Settings::from_ini` of a scenario's own, every value OpenSim
  would ignore logged as a warning); unset, the stock
  `osslDefaultEnable.ini`. The owner's standing names the gods as OpenSim's
  `PermissionsModule` does: `GRID_GOD` an account of god level 200 or more,
  `GOD` that or the estate owner (`region_owner_is_god`), `ACTIVE_GOD` an
  owner in the region with god powers on.
- **Sitting and wearing** are the region's (`sl_fake_grid::seating`): a
  prim's seat settings (`Host::seat` — the sit target, the sitter's camera,
  mouselook), its sitters in the order they sat and a worn root's wearer and
  point are components of the prim in the region's store, with the store's
  own indices from an avatar to its seat and to what it wears. A sit request
  is answered, and the avatar seated, at once: on a free sit target raised
  0.35 m, otherwise where the flavour's `PlainSeat` puts it (Second Life
  stacking a second sitter 0.54 m higher, and answering a seat without a
  free target only within 8.4 m); `changed(CHANGED_LINK)` follows in every
  script of the object, and a stand, an `llUnSit`, a teleport, a logout or
  the seat's leaving the region stands the avatar up, lapsing
  `PERMISSION_TAKE_CONTROLS` and `PERMISSION_CONTROL_CAMERA`. The sitters
  take the link numbers after the prims (`Links::sitters`, read by
  `llGetNumberOfPrims` and `llGetLinkKey`). A wear from the world or by
  `llAttachToAvatar` and a drop keep the object's ids and raise `attach`, on
  OpenSim after its own `changed(32768)`; a drop lapses
  `PERMISSION_TAKE_CONTROLS`. A worn object stands, hears and speaks where
  its wearer does, and is sent to every viewer its wearer is shown to — a
  HUD to its wearer's alone. Each avatar has a region-local id of its own in
  each region, a new one when it comes back.
- **Touches** are the region's too (`sl_fake_grid::touches`,
  `server-world-touch-and-grab`): `ObjectGrab`, `ObjectGrabUpdate` and
  `ObjectDeGrab` become `touch_start`, `touch` and `touch_end` as the
  flavour's `TouchPolicy` says — Second Life holding a touch open and raising
  `touch` on the region's clock every 1/22.5 s (and once for a click too
  short for it), OpenSim answering each message — with the detected block
  filled from the toucher and the message's surface block, as sent. A touch
  on a child reaches the child's scripts that take touches and the root's as
  `llPassTouches` (`Host::pass_touches`, the prim's setting) and the flavour
  say. After every tick the region keeps `FLAGS_SCRIPTED` and
  `FLAGS_HANDLE_TOUCH` true of its scripted prims — a running script with a
  touch handler in its current state — with each grid's rule for a child of
  such a root: both flags, quietly, on Second Life; `FLAGS_SCRIPTED` and the
  child sent again in full as the root's scripts change, on OpenSim.
- **Prims** (`sl_fake_grid::prim_state`, `server-lsl-lib-prim-state`). A
  prim is lent to the library read out of its `Object` — the volume
  parameters, the texture entry, the extra parameters, the hover text, the
  flags, the angular velocity, the attached sound, the particle system and
  texture animation — and its properties record (name, description,
  creator), and written back into them; what it changed goes out in the
  tick's burst like any other write. The wire holds less than a grid keeps
  (a colour as a byte, a texture offset as 16 bits, a spin as an angular
  velocity, a normal map or a sit flag not at all), so the last prim a
  script wrote is kept with the record it wrote (a `ScriptedPrim` component
  in the store), and a read takes each part from there — the faces, the
  text, the shape, the light, the flexible path, the spin, the particles,
  the projector — while that part of the record is still what was written; a
  viewer's edit since is what the next read decodes. The hover text's alpha
  travels flipped (`255 − opacity`): Second Life reads the opacity, OpenSim
  the flipped byte, which is why its `llSetText(…, 1.0)` reads 0. A setter's
  `changed` is raised after the tick, in the written prim's scripts; a sound
  played once, triggered or preloaded goes to every avatar in the region
  (`llTriggerSoundLimited`'s to those inside its box) as `AttachedSound`,
  `SoundTrigger` or `PreloadSound`.
- **Randomness and calendar time.** `Host::random_bits` draws from a
  xorshift64 stream per region seeded with the region's id, and the region's
  world holds the grid's wall clock (`SceneFixtures::wall`), where a script's
  date is to start before its ticks move it, so what a script draws and reads
  follows from the seed and the ticks ([determinism](determinism.md)).
- **Clocks and sensing** (`server-lsl-lib-time-timers`,
  `server-lsl-lib-detection-sensors`). The region's calendar time at tick 0
  is read off its wall clock at its first tick; its host keeps UTC. A sweep
  weighs every unworn object by its root and every avatar present; a ray meets
  a prim as its box (its scale, turned as it is turned), an avatar as an
  average avatar's box, and the ground by its heightfield, cut at the region's
  edge (`sl_fake_grid::scripts::sensing`).
- **Avatars** (`sl_fake_grid::scripts::avatars`,
  `server-lsl-lib-avatar-control`). A script reads the avatars present at the
  tick's start — their account names, a default shape's size, the posture the
  movement controller holds them in (or `Sitting` on a seat), the animations
  their set plays and their viewer's camera from its last `AgentUpdate`. An
  animation a script starts or stops joins the avatar's animation set and is
  stated to its viewer after the tick; overriding `Standing` changes what the
  set stands as. A script that took controls hears, before each tick, the keys
  its avatar's viewer holds (`Engine::controls`), and its viewer is sent the
  flavour's `ScriptControlChange`s; the follow camera goes out as
  `SetFollowCamProperties` naming the script's prim. Scripted teleports and
  object animations are not modelled yet: the call does nothing and the fake
  grid's log says so.
- **HTTP** (`sl_fake_grid::script_http`, `sl_fake_grid::scripts::http`,
  `server-lsl-lib-http-url`). A URL is granted on the grid's own HTTP service
  in its flavour's form; a request to it — from outside, or from any script
  of the grid — reaches the holding script at its region's next tick, and the
  answer goes back at the sender's next tick, or after the flavour's wait in
  ticks as its timeout. A request to any other address never leaves the
  process: the grid's `HttpResponder` answers it, by default `NoResponder`'s
  499 and a line saying so; a scenario mounts a `TableResponder` of canned
  answers (`FakeGridBuilder::http_responder`). The URLs a script stops holding
  — released, or dropped by a reset — are released after the tick.
- **Land, region and environment** (`sl_fake_grid::scripts::land`,
  `server-lsl-lib-land-region-env`). A script reads the parcel records the
  region keeps (their directory listing's id and dwell beside them), the
  region's limits and estate as an estate manager last left them, the
  neighbours the grid runs beside it, the heightfield its viewers are sent, the
  terrain fixture's uniform wind and the region's own day cycle (both grids'
  default day without one). A change is made at once: a stream set, a pass or
  ban written into the parcel's access list (what the access-list reply then
  answers), the estate's lists, and the parcel's media — the avatars on the
  parcel sent its record again, or the media update and command. The fake grid
  sends nobody off a parcel yet, so `llEjectFromLand` changes nothing, and it
  keeps no prim counts by owner, so `llGetParcelPrimOwners` is `[]` on both
  flavours.
- **Task inventory** (`sl_fake_grid::scripts::task_inventory`,
  `server-lsl-lib-task-inventory`). A script's write into a prim's contents
  happens at once, so the same event reads it back, and advances the serial;
  the region's viewers are told after the tick as each grid tells them of a
  write (the prim again, the record to those who hold it selected), and the
  `changed` the library chose follows. A give into a prim copies under a free
  name, a script stopped; a give to an avatar is the `TaskInventoryOffered`
  instant message both grids send. `llRezObject` and its family put back the
  linkset the object item was taken as — an object dropped into a prim carries
  its taken linkset with it — its scripts arriving at the next reconcile as
  they were saved, with the start parameter and string, and the rezzer is told
  (`Engine::object_rezzed`) after the tick in which they ran `on_rez`, before
  what they said is heard. A script loaded with `llRemoteLoadScriptPin` starts
  stopped and runs once the tick after the one it landed in is over, so the
  prim's scripts read it stopped when they hear of it. Notecards are read out
  of the asset store with `sl-notecard`, and the region remembers which it
  has read.
- **Money** goes through the grid's ledger (`sl_fake_grid::money`):
  `llGiveMoney` and `llTransferLindenDollars` pay the payee out of the owner's
  balance, both told by balance replies of type `5009` sent after the tick, a
  refusal told to the owner by a failed one; a viewer's payment to a prim is
  judged by its pay layout and raises `money` in its scripts; a bought
  original raises `changed(CHANGED_OWNER)` as its grid does. On the OpenSim
  flavour nothing moves, as the stock money module moves nothing.
- **Worn objects** (`sl_fake_grid::attachments`). A wear by a script files a
  new item for its object — its id minted in the region, the item filed by its
  wearer's session — and a worn object's scripts travel with it from region to
  region, hearing `changed(CHANGED_REGION | CHANGED_TELEPORT)` as their engine
  raises it.
- **Held by** `sl-fake-grid/tests/scripts.rs` (the life of a script, its
  state through a restart and a take, the experience questions, the OSSL
  gate, a scripted seat, a taken target, Second Life's reach, a wear and a
  drop, a held touch and a click, a touch on a child, a prim a script dresses
  and a viewer retextures), the `sit-stand` and
  `object-touch-grab` censuses offline on both flavours,
  `sl-client-bevy-viewer/tests/e2e_running_scripts.rs` (a running
  script's dialog and experience card answered in the real viewer and heard
  back by the script) and the `script-compile` census, which runs offline
  on both fake flavours.

### As built: the transcript corpus

The other side of the trait is a host that is no world at all.
`sl-lsl-runtime/tests/script_corpus/` runs every script under
`sl-lsl-runtime/corpus/<area>/<name>.lsl` in the VM against a recording host
— one prim alone in a region, its owner present — for a fixed number of
ticks, and sets the transcript of what it did against `<name>.expected`
beside it. It is the cheapest test of the runtime: no grid, no network, no
async runtime, the whole corpus in milliseconds.

- **A transcript is behaviour, not internals.** One line per stimulus
  (`@3 > touch by …`), one per host call that *does* something (`print`,
  `say` with its channel and volume, `owner-say`, `set-position`,
  `set-prim` with the `changed` bits it raised, `die`, …), each stamped with
  its tick; then the script's state, run flag and queue length, and every
  global's final value, floats in the shortest form that reads back as the
  same `f32`. A query leaves no line: what the script does with its answer
  does. So a case survives a rewrite of the VM.
- **The header is the script's leading comments**: `// corpus:` says what
  the case pins (required), `// flavours:` limits the engines
  (`secondlife yengine xengine`, all three by default), `// ticks:` the run
  (20), and `// at N: touch | chat <channel> <text> | link <num> <text>
  <key> | changed <bits> | rez <param>` posts a stimulus before tick `N`. A
  lowercase key it does not know fails the case; any other comment is the
  case's notes, its `Oracle:` line among them.
- **One expectation, or one per flavour.** A case whose engines agree keeps
  one `<name>.expected`; one whose engines differ (a run-time error is
  worded and handled three ways) keeps `<name>.<flavour>.expected` instead.
- **`UPDATE_EXPECT=1 cargo test -p sl-lsl-runtime --test script_corpus`**
  writes them, arranging the files by whether the flavours agreed and
  removing what the other arrangement left. An expectation is a claim about
  LSL: read what it wrote against the case's oracle before committing it.
  An expectation with no script is a failure, so a renamed case cannot
  leave its old transcript looking like coverage.

The value model's rules each have a case — the casts out of and into a
string, a key that is not a string, `f32` drift, boolean context, `list ==
list` and `!=`, string comparison, the `+` overloads, the vector and
rotation operators, integer wrap-around and the Math Errors — beside
evaluation order as aditi showed it (operands right to left, arguments,
list elements and vector components left to right, `&&` and `||` both
sides), `llGetSubString`'s wrapping indices, and the event machinery the
stimuli reach: timers, sleeps, state changes, touches, listens, link
messages, `on_rez` and a setter's `changed`. Each names its oracle —
PyOptimizer's `expr.suite`, the SL wiki, the aditi measurements above, or
OpenSim's own `LSL_TypesTestLSLInteger` and `LSL_ApiListTests`.

## Out of scope

- **Running Luau / SLua.** `sl-proto` models `ScriptLanguage::Luau`, but
  this programme is LSL. The fake grid *compiles* an upload under the
  `luau` target, with the reference Luau compiler through `mlua`
  (`sl_fake_grid::script_compile`), so its answer is Second Life's word
  for word; it runs nothing it compiled. The `Host` trait and the library
  table are language-neutral on purpose, so a second front end could
  target the same VM-independent host later without either being
  redesigned.
- **LSO and Mono bytecode.** Scripts are compiled from source every time;
  nothing reads or writes the reference's compiled forms.
- **A sandbox for hostile content.** Budgets and memory limits exist
  because scripts observe them, not as a security boundary.
