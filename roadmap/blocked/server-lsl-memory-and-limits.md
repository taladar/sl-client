---
id: server-lsl-memory-and-limits
title: Script memory and limits, because scripts observe them
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-lsl-vm-execution]
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
  content compares against; approximate them deliberately, document the
  approximation, and pin it with a test rather than leaving it
  accidental.
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

Acceptance: `llGetFreeMemory` falls as a list grows and rises when it is
cleared; `llSetMemoryLimit` below current usage fails as the reference's
does; a deliberate runaway allocation stops the script with a stack-heap
collision rather than exhausting the host; and the top-scripts report
lists a genuinely busy script above an idle one.
