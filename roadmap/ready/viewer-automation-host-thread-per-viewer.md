---
id: viewer-automation-host-thread-per-viewer
title: In-process host — step each viewer on a thread of its own
topic: viewer
status: ready
origin: test-e2e-sweep-two-avatars aditi run (2026-10-02)
refs: [viewer-automation-inprocess-transport, test-e2e-sweep-two-avatars]
---

Context: [context/automation.md](../context/automation.md).

## Problem

`InProcessHost` (`sl-viewer-automation/src/in_process_host.rs`) builds every
viewer App it hosts on its one thread and steps them in turn, a frame of each,
2 ms apart. The viewers' frames therefore take turns: two viewers each get at
most half the thread. On aditi's busy `Morris` region the in-process pair of
`two_residents_befriend_trade_rights_and_contest_one_prim` ran at about
2.75 fps each (330 frames in 120 s), and a world aim never held still long
enough to pass its stability check. The process backend — one viewer per
process — managed 3.6 fps (437 frames in 120 s) on the same spot and failed
the same check, so the region's load is most of it; the turn-taking costs
the rest, and grows with every viewer a test adds.

Nothing needs the turn-taking: the reason is only that a Bevy `App` is not
`Send`, so it must stay on the thread that built it. Determinism is not a goal
of the in-process backend either; the process backend has none.

## What to do

- Build and step each hosted App on a thread of its own: the build closure
  runs on that thread, and the thread loops `update` (2 ms apart, as now) until
  the viewer exits or the host stops.
- Keep the `ViewerLink` shape (a request channel and a message channel per
  viewer), so `sl-viewer-driver` and `sl-e2e` do not change.
- `with_app` posts its closure to the viewer's thread and waits for the
  answer, between that viewer's frames.
- `stop` joins every viewer's thread after its pipeline has finished, as the
  single thread is joined today; an exited viewer still closes its link.
- Keep the `viewer{name}` span on each thread, so log lines stay attributed.
- Tests: two hosted viewers advance concurrently (one blocked in a slow
  frame does not stall the other's frame count), and the existing host tests
  pass unchanged.
- Re-run the two-viewer live tests in process on aditi and compare their frame
  rate with the process backend.
