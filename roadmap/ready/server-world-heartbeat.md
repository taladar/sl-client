---
id: server-world-heartbeat
title: A region heartbeat — the fake grid has no simulation loop at all
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
refs: [server-world-determinism-contract, server-world-update-scheduling,
  server-lsl-vm-execution]
---

Context: [context/lsl.md](../context/lsl.md).

Nothing in `sl-fake-grid` ticks. Every world change is made by the
session that received the client message that caused it, published as a
`RegionChange` on the region's broadcast, and streamed to the region's
other sessions by their watchers. The only thing resembling a clock is
`crate::timeline`, which is a *script for one avatar* — a list of steps
with deadlines, handed over on teleport — not a region loop. A script
engine, physics, `llSetTimerEvent`, `at_target`, `moving_end` and
interest management all need a loop; none of them fit the timeline,
because they belong to the region and outlive any one session.

Wanted: a per-region heartbeat task, one per `RegionEntry`, that runs a
fixed step and does, in order:

1. advance the region clock by exactly one step (never by measured
   elapsed time — see [[server-world-determinism-contract]]);
2. run due timers and the script slice
   ([[server-lsl-vm-execution]]);
3. run movement and physics
   ([[server-world-agent-movement]],
   [[server-world-collision-and-physics]]);
4. collect the tick's changes and hand them to the update scheduler
   ([[server-world-update-scheduling]]);
5. emit `SimStats` at the reference's cadence, with a real
   `time_dilation` derived from whether the step kept up.

Three things to get right, because they are what a naive loop gets
wrong:

- **The step is a decision, not a copy of the reference.** A real
  simulator runs physics at 45 fps and sends updates at a lower rate.
  The fake grid's rate should be chosen for reproducibility and
  test-suite cost, stated once, and exposed on `RegionConfig` so a test
  can pick a coarse tick and a cross-check run a fine one.
- **A paused or stepped clock.** The grid already takes an injected
  `Now` (`crate::time::Now`, with a `tokio_clock` that tokio's test
  timer can pause). The heartbeat must be drivable *step by step* from a
  test — `advance_one_tick()` — so a case can say "run three ticks" and
  assert, rather than sleeping.
- **The lock order.** The crate's rule is session lock, then region
  lock, never the reverse (`SharedSim::with_region`). The heartbeat
  starts from the region and must reach the sessions to send; it
  therefore needs to collect under the region lock and send after
  releasing it, exactly as `flush_locked` / `finish_flush` already do on
  the session side. Getting this wrong deadlocks the whole grid under
  load, silently and intermittently.

Acceptance: a region with no client connected still ticks; a test can
advance the tick deterministically and observe a `SimStats` per tick with
a plausible dilation; and the existing offline conformance cases pass
with the heartbeat running.
