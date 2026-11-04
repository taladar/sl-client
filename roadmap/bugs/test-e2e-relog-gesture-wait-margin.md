---
id: test-e2e-relog-gesture-wait-margin
title: e2e_relog's active-gesture test runs at the edge of its 60 s wait
topic: test
status: bugs
origin: full viewer test run while landing viewer-left-click-click-actions (2026-11-03)
refs: [test-e2e-sweep-relog]
---

Context: [context/automation.md](../context/automation.md).

## Observation

`sl-client-bevy-viewer/tests/e2e_relog.rs`
`an_active_gesture_is_active_after_a_relog` failed in a full release
`cargo nextest run` of the viewer and five library crates (1044 tests in
parallel): after 68.9 s, waiting for `menuitem[name_key=menu-inv-deactivate]`
to be enabled, `TimedOut … frames: 2500, millis: 60016` — the file's `WAIT`
of 60 s. Run alone straight after, it **passed in 58.1 s**.

So the test spends nearly all of its 60 s budget even unloaded, and any
contention pushes it over. Either something in the relog path is far slower
than it needs to be (a fixed delay, a poll interval, the relog itself), or the
wait is waiting on the wrong thing.

## To do

Find where the 58 s goes (the per-step timings of the scenario, against the
other `e2e_relog` cases, which finish well inside their waits) and fix that;
do not just raise `WAIT`.
