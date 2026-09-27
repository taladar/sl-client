---
id: server-lsl-vm-execution
title: The script VM — suspendable execution in per-tick slices
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 13
blocked_by: [server-lsl-compiler-ir]
refs: [server-lsl-state-and-events, server-lsl-memory-and-limits,
  server-world-heartbeat, server-world-determinism-contract]
---

Context: [context/lsl.md](../context/lsl.md).

The machine that runs the IR. One region, one thread, N script
instances, one heartbeat — so the defining property is not speed, it is
that **execution can stop anywhere and resume later**.

A script instance holds: its compiled program (shared, refcounted — a
hundred copies of one script share one program), its globals, its call
stack, its program counter, its current state, its event queue, its
run flag, and its accounting (instructions used this tick, memory used,
sleep-until tick).

What the VM owes:

- **Slicing.** `run_slice(budget) -> Outcome`, where the budget is an
  instruction count ([[server-world-determinism-contract]] forbids a
  wall-clock one) and the outcome is one of: finished this event, still
  running (resume next tick), sleeping until tick N, changed state,
  reset, or errored. The heartbeat round-robins over runnable instances.
- **`llSleep`** as a suspension, not a thread block — the whole reason a
  program counter beats a Rust call stack.
- **`llResetScript`** dropping globals, the stack and the event queue and
  restarting at `default`'s `state_entry`, from inside the running
  script.
- **The run flag** (`SetScriptRunning`, `llSetScriptState`): a stopped
  script keeps its state and its globals and receives no events; a
  started one resumes. Note the asymmetry content relies on — stopping is
  not resetting.
- **The host boundary.** Every library call goes through the `Host` trait
  from [[server-lsl-architecture]]; the VM itself knows nothing about
  prims, chat or agents. That is what lets [[test-lsl-script-corpus]] run
  a script with a mock host and no grid.
- **Errors.** A run-time error (integer division by zero, a stack-heap
  collision, a bad list index in the cases where LSL errors rather than
  returning a default) stops the script and reports
  ([[server-lsl-runtime-errors]]); it never panics the region.

Reference: `YEngine/XMRInstRun.cs` for the slice/resume shape and
`Shared/Instance/ScriptInstance.cs` for the surrounding bookkeeping. Note
that OpenSim runs each script on a pooled thread and uses a
continuation-capturing IL rewrite to suspend it; a bytecode VM does not
need any of that machinery, which is most of the argument for it.

Acceptance: a script that loops forever does not stall the region — it
uses its budget each tick and the tick still completes; `llSleep(2.0)`
resumes at the right tick and not before; a stopped script resumes with
its globals intact; and a run-time error stops one script and leaves its
neighbours running.

## Done (2026-09-28)

Module `sl_lsl_runtime::vm`; the book chapter `simulator/lsl-engine.md`
("As built: the VM and the scheduler") is the full description.

- **`Instance`**: an `Arc<Program>` plus its own globals, state, queue, run
  flag, wake tick and the body in progress (frames over one operand stack).
  `run_slice(caller, now, step, budget, host)` stops at every boundary and
  says which (`Outcome`): finished, yielded mid-body, sleeping until tick N,
  state changed, reset, faulted.
- **`Engine`**: a region's instances by `CallerId`, served round-robin per
  `tick(host)` from just after the one served last, under a per-script and a
  region-wide budget; the `TickReport` carries the runnable/served counts
  behind the scripts-run percentage.
- **`Host`** (`print`, `stubbed`) and **`ScriptCtx`** (caller, tick, host,
  sleep and reset requests); the registry now dispatches with a concrete
  `ScriptCtx`. `llSleep` and `llResetScript` are implemented; a table forced
  delay suspends like a sleep.
- **Errors never panic the region**: `Math Error`, `InvalidProgram`,
  `Unimplemented`, `Internal` (a lowering bug, named), and a
  `MAX_CALL_DEPTH` stack-heap collision until the memory task accounts
  exactly. A fault stops the one script with its source position.

**Measured on aditi** (2026-09-28, stock Firestorm, probe scripts):

- **Throughput and costs.** A one-million-iteration empty loop runs at
  500 000 it/s, one with three more operations in its body at 494 300, one
  calling `llAbs` at 226 400. Mono's time is the loop back-edge, not the
  arithmetic, so a back-edge is charged 513, a library call 626, and a script
  gets 260 million charged instructions a second — which reproduces all three
  rates. Two busy scripts in one prim each kept the full rate, so a script is
  held by its own share; the region's sixteen shares are a decision.
- **`llGetTime` moves in whole frames**, which made the first 50 000-iteration
  runs read four or five frames; noted on [[server-lsl-lib-time-timers]].
- **Stopping**: a link message sent to a stopped script is never delivered,
  and a stopped script's `llSleep` counts on while it is stopped: stopped one
  second into a five-second sleep and restarted at eight, it woke within the
  restart's frame; restarted at three, it woke at five.
- **States**: `state default;` in `default` runs neither `state_exit` nor
  `state_entry`; a function's `state` returns the default to a caller that
  carries on, and the transition happens when the handler ends.

Each acceptance point has a test in `src/vm/tests.rs`: a runaway loop spends
its budget every tick while a neighbour keeps its pace, `llSleep(2.0)` wakes on
tick 21 of a 100 ms step and not before, a stopped script restarts mid-handler
with its globals, and a `Math Error` stops one script with its line while the
other runs on.
