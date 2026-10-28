---
id: server-lsl-lib-time-timers
title: Library tranche — time, timers and sleeping, on a tick clock
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-determinism-contract,
  gridspec-lsl-time-timers]
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
  time, counted in ticks. `llSleep` itself is done
  ([[server-lsl-vm-execution]], with the table's forced delays), and so is
  the finding that pins this one: on aditi `llGetTime` moves only in **whole
  simulator frames** (1/45 s) — two scripts timing the same 50 000-iteration
  loop both read exactly four frames, and one loop read four or five from run
  to run — so a tick-counted `llGetTime` is the reference's shape, not an
  approximation of it.
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
across two runs of one seed; and a state change **keeps** the timer at its
interval (measured on every grid, `events/state-change`).

Already done by [[server-lsl-state-and-events]]: `llSetTimerEvent` and the
`timer` event — tick-counted, never stacking, surviving a state change and
stopped by a reset, each measured on aditi.

## Measured (gridspec-lsl-time-timers, 2026-10-28)

`book/src/gridspec/lsl.md` § Time, timers and sleep. What the tick clock
has to reproduce per flavour, beyond what the VM does today:

- **The floor.** Second Life runs no timer more often than every second
  frame (0.044 s); OpenSim clamps to `MinTimerInterval` (YEngine 0.1 s,
  XEngine 0.5 s). Today any interval rounds up to one tick.
- **A negative interval** stops the timer on Second Life (the VM's
  behaviour today) but runs it at the floor on OpenSim.
- **Phase.** Second Life keeps the phase of the due times (ten 1.0 s events
  end at 10.009 s), and a handler longer than the interval puts the next
  event two or three frames after it ends; YEngine counts each interval
  from when the last event ran, so its error adds up; XEngine keeps the
  phase of its first event.
- **Stopping the timer** drops an event already due on Second Life and
  YEngine; XEngine still runs it. Setting the timer again restarts the
  count on every grid, and keeps an event already due.
- **A sleep** rounds up to whole frames on Second Life (`llSleep(0.01)` is
  one frame); OpenSim sleeps the time asked. A negative sleep returns at
  once, but on XEngine throws and ends the event.
- **A stop or reset mid-sleep**: a reset is at once everywhere; a stopped
  sleep counts on while stopped, except on XEngine, where the sleeping event
  is abandoned. A stopped script's `llGetTime` runs on.
- **The clocks' shapes**: `llGetTimestamp` has six fraction digits on Second
  Life and seven on OpenSim; `llGetGMTclock` and `llGetWallclock` are whole
  seconds on Second Life; `llGetWallclock` is Pacific time on Second Life
  and the host's zone on OpenSim; `llGetTimeOfDay` is the (UTC, or OpenSim's
  local) second of the day modulo four hours.
