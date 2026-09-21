---
id: server-lsl-lib-time-timers
title: Library tranche — time, timers and sleeping, on a tick clock
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-determinism-contract]
refs: [server-lsl-state-and-events, server-world-heartbeat]
---

Context: [context/lsl.md](../context/lsl.md).

Small, and the one tranche where the fake grid's determinism rule makes
the implementation *differ* from the reference rather than merely being
careful.

- `llSetTimerEvent(float sec)` and the `timer()` event, with `0.0`
  cancelling it, one timer per script, the timer cleared by a state
  change, and — the rule content trips over — a `timer` event that does
  not stack: if the handler takes longer than the interval, ticks are
  dropped, not queued.
- `llSleep(float sec)` as a VM suspension
  ([[server-lsl-vm-execution]]), plus the **implicit** sleeps a third of
  the library carries (`llGiveInventory` 2 s, `llEmail` 20 s,
  `llRequestAgentData` 0.1 s, …). Those are in the descriptor table
  ([[server-lsl-library-surface-table]]), which is why they belong to
  the dispatch and not to each function.
- `llGetTime`, `llResetTime`, `llGetAndResetTime` — script-local elapsed
  time, counted in ticks.
- `llGetUnixTime`, `llGetTimestamp`, `llGetDate`, `llGetWallclock`,
  `llGetGMTclock`, `llGetTimeOfDay` — absolute time. **This is the
  determinism problem**: a real grid reads the wall clock, and a
  reproducible one cannot. The resolution: the region's clock starts at
  a **stated epoch** (the crate already freezes a fixture creation date
  at a constant for exactly this reason) and advances by the tick, so
  `llGetUnixTime` is a pure function of the seed and the tick count. A
  scenario may set the starting epoch; nothing reads the host clock.
- `llGetRegionTimeDilation` and `llGetRegionFPS`, answered from the
  heartbeat's own measurements rather than invented.

Acceptance: a timer at 0.5 s fires the expected number of times over a
known number of ticks; a handler longer than the interval drops ticks
rather than accumulating them; `llGetUnixTime` returns the same sequence
across two runs of one seed; and a state change cancels the timer.
