---
id: viewer-automation-host-thread-per-viewer
title: In-process host — step each viewer on a thread of its own
topic: viewer
status: done
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

## Outcome (2026-10-02)

Each hosted viewer now has a thread of its own (named `sl-viewer-<label>`,
started in the hosting code's tracing context) with an `InProcessTransport`
hosting just that viewer; the host keeps one command channel and join handle
per viewer. `ViewerLink`, `with_app`, `exited` and `stop` keep their shape;
`InProcessHost::start()` became the infallible `new()`, since there is no
host thread left to fail to spawn. New unit test
`a_viewer_stuck_in_a_frame_does_not_stall_another`; the fake-grid
`e2e_two_avatars`, `e2e_pilot`, `e2e_arrival` and the driver acceptance pass
in process.

aditi, Morris, `two_residents_befriend_trade_rights_and_contest_one_prim`:

- In process: about 26 fps per viewer (3141 and 3163 frames in 120 s), against
  2.75 before. Process backend: about the same scene, passed.
- The failures that remained were not the threads. The ground aim's camera
  rule compared the eye to 0.1 mm, while an AO swayed Alpha's head 0.5–2 mm a
  frame, so it passed only by luck. `CameraStill` now judges the points an
  action uses on screen (1 px), for the ground aim, the sweep and the handle
  drag.
- A failure screenshot drew the world blank. The PNG kept the world pass's
  glow mask as alpha; frames are now written opaque.
- At the live test's widest offset (8 m to the side) the rez point needs a
  reveal. Some in-process reveals after an `Escape` camera reset took an odd
  path and failed. Watched on the process backend (`SL_E2E_WATCH=1`, new),
  a re-select's framing carried the camera inside the fresh cube for a while.
  The object aim now waits out an eye inside its target after a reveal; the
  camera's part is
  [[viewer-camera-framing-ends-inside-the-framed-object]].
- Rezzing is allowed there: one sandbox parcel with `CREATE_OBJECTS`.
