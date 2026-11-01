---
id: server-world-heartbeat
title: A region heartbeat — the fake grid has no simulation loop at all
topic: server
status: done
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

## As built (2026-11-01)

`sl_fake_grid::heartbeat`: one task per region, spawned when the grid starts
and ticking with nobody in it. A tick serialises on the heartbeat's own pulse
lock (no session path takes it), advances the region clock by exactly one
step, and, every statistics interval of the imitated grid rounded up to whole
steps, sends `SimStats` to every root agent in the region in session order —
the session table read and released first, then each session locked in turn,
never the region's world. The per-session `SimStats` timer is gone from the
fake grid (`RegionTelemetry::stats_interval: Option<Duration>`, `None` there);
`SimSession::send_region_stats(TimeDilation, now)` states the dilation and
scales the simulator and physics frame rates by it, root agents only.

- The step: `DEFAULT_STEP` = 100 ms, on `RegionConfig::heartbeat`
  (`HeartbeatConfig::running(step)` / `stepped(step)`); a zero step is
  `Error::ZeroHeartbeatStep` at start.
- Dilation (`sl_proto::TimeDilation`, held to `0..=1`): ticks × step over the
  grid clock's elapsed time since the last statistics; a tick due while the
  last still runs is skipped (`MissedTickBehavior::Skip`), so falling behind
  shows. A stepped region reports 1.0.
- `FakeGrid::advance_one_tick(region) -> Tick` (number, region time, the
  statistics sent) and `FakeGrid::ticks(region)` (a `watch` of the latest).
- Tests: `sl-fake-grid/tests/heartbeat.rs` (an empty region ticks; a stepped
  region sends nothing until ticked, then one `SimStats` per tick with
  dilation 1; zero step and unknown region refused), the sl-proto
  `region_stats_sent_by_the_loop_carry_its_dilation`, and the existing
  offline conformance cases and fake-grid suites with the heartbeat running.

Steps 2–4 of the tick (the script slice, movement and physics, collecting the
tick's changes for an update scheduler) slot in between the clock and the
statistics as [[server-fake-grid-script-engine-wiring]],
[[server-world-agent-movement]], [[server-world-collision-and-physics]] and
[[server-world-update-scheduling]] build them; each already lists this task
as its blocker. `SimulatorViewerTimeMessage` and `CoarseLocationUpdate` stay
on the session's timer until a region clock has something of its own to say
in them (the sun still does not move).
