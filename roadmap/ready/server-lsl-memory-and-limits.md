---
id: server-lsl-memory-and-limits
title: Script memory and limits, because scripts observe them
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-lsl-vm-execution, server-lsl-memory-sizes,
  gridspec-lsl-memory-limits]
refs: [server-lsl-runtime-errors, server-fake-grid-script-engine-wiring]
---

Context: [context/lsl.md](../context/lsl.md).

A Rust VM has no natural 16 KB limit, and that is a problem rather than a
relief: scripts *read* their memory. `llGetFreeMemory`,
`llGetUsedMemory`, `llSetMemoryLimit` and `llGetMemoryLimit` are library
functions content branches on — a notecard reader that stops at
`llGetFreeMemory() < 2000`, a list that is trimmed when it gets close —
and a stack-heap collision is a *behaviour*, not a crash: the script
stops, shouts on `DEBUG_CHANNEL`, and the object turns up in the region's
top-scripts report.

So the VM has to account memory even though it does not need to.

- **The budget.** LSO scripts get 16 KB; Mono scripts get 64 KB and
  `llSetMemoryLimit` may lower it. The fake grid compiles from source
  and has no LSO, so the sane default is the Mono budget with the
  language flag (`ScriptLanguage`, which `sl-proto` already models
  including Luau) deciding.
- **What counts.** Globals, the call stack, and the heap — a string's
  bytes, a list's cells and its elements' bytes. The reference's exact
  per-value overheads are what make `llGetFreeMemory` return the number
  content compares against, so they must be **exact, not approximated**:
  charge less than Second Life and a script hits its stack-heap collision
  later than it would there; charge more and it collides where it would
  not. The
  costs are measured on aditi by [[server-lsl-memory-sizes]], which also
  provides the cost function this task accounts with.
- **Stack-heap collision** stops the script with the reference's
  message, and `llGetScriptState` then reports it.
- **The region-level view.** `llGetObjectDetails`'s
  `OBJECT_SCRIPT_MEMORY` / `OBJECT_SCRIPT_TIME`,
  `llGetParcelPrimCount`'s script counts, and the `LandStatRequest`
  top-scripts report — which `sl-fake-grid` currently answers from
  **stated** `ObjectCost` fixtures, precisely because "a fake region
  cannot measure, because it runs no scripts and simulates no physics"
  (the comment on `SceneFixtures::object_costs`). Once scripts run, the
  measurement is real and the fixture becomes a fall-back for objects
  with no scripts. Keeping both is the point: a scenario may still state
  a cost for a prim it wants in the report.

Acceptance: `llGetFreeMemory` falls as a list grows, by exactly the
aditi-measured amounts, and rises when it is cleared; `llSetMemoryLimit` below
current usage fails as the reference's does; a deliberate runaway allocation
stops the script with a stack-heap collision rather than exhausting the host;
and the top-scripts report lists a genuinely busy script above an idle one.

## Measured (gridspec-lsl-memory-limits, 2026-10-26)

`book/src/gridspec/lsl.md` § Memory and limits has the table per grid; what
it changes here:

- **Second Life's `llGetFreeMemory` is a low-water mark**: the limit less the
  most the script has used since the limit was last set — it never rises
  when a value is cleared, while `llGetUsedMemory` does. `llSetMemoryLimit`
  resets it to the new limit less the current use. LSO reports
  `llGetUsedMemory` as the whole 16 384 always, and takes only
  `llSetMemoryLimit(16384)`.
- **`llSetMemoryLimit`** is `TRUE` from the current use up to 65 536 on
  Mono, `FALSE` above or below; `FALSE` always on both OpenSim engines,
  whose limit stays 65 536.
- **The stack counts** against the limit (a lowered limit cuts the
  recursion depth), and Mono's depth at the collision differs from run to
  run (1 039–1 116 levels for one integer argument), so a test can only
  hold the fake grid to a range there; LSO's is fixed (763).
- **Per flavour**: `FakeSl` is Mono's model (the budget, the low-water
  mark, the collision message pair on `DEBUG_CHANNEL`, the script stopped);
  `FakeOpensim` is YEngine's (a 1 MiB heap of the values the script holds,
  globals 4 bytes an integer and 2 a character, `OutOfHeapException` told to
  the owner alone, the script still reading as running) — XEngine counts
  nothing and answers 65 536 to every call.
- **The region-level view**: `OBJECT_SCRIPT_MEMORY` is the sum of every
  script's limit, running or not, on Second Life, and 16 384 per *running*
  script on OpenSim; the region's `ActiveScripts` stat is a fractional
  average of the running scripts on Second Life and the count of scripts
  present, stopped or not, on OpenSim. The fake grid sends `0` today.

## From server-lsl-memory-sizes (2026-10-30)

The cost function is `sl-lsl-runtime`'s `value::memory`, per flavour
(`book/src/gridspec/lsl.md` § What a value costs, exactly). Three things to
decide here that it leaves open:

- **Code.** Mono's `llGetUsedMemory` includes the script's code: a script
  that only reads its memory and says it uses 4 456 bytes, each character of
  a string literal costs 2, a declared global 20 to 64. The fake grid
  compiles to its own bytecode, so the base has to be a model (a constant
  plus per-literal and per-declaration terms fitted to the corpus' scripts),
  stated as one rather than passed off as exact.
- **Shared literals.** Mono counts one string literal put in every element
  of a list once, then `MONO_REFERENCE` (4) an element; the value model
  copies. Exact accounting needs the literal's identity (an `Rc<str>` for
  constants, or a literal id on `Element::String`).
- **The stack.** No local, argument or call frame shows in Mono's
  `llGetUsedMemory`, yet recursion collides and a lowered limit cuts the
  depth (§ Running out): the collision check counts something the reading
  does not. YEngine counts a local string or list in its heap; XEngine
  counts nothing.
