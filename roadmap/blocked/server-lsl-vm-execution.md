---
id: server-lsl-vm-execution
title: The script VM — suspendable execution in per-tick slices
topic: server
status: blocked
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
