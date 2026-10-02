---
id: viewer-live-opensim-world-renders-white
title: The world renders all white at times on the live OpenSim grid
topic: viewer
status: bugs
origin: test-e2e-sweep-two-avatars live runs (2026-10-02)
refs: [test-e2e-sweep-two-avatars]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

During the live OpenSim runs of `tests/e2e_two_avatars.rs` (2026-10-02,
`Default Region`, two headless stage viewers), many failure screenshots show
the world **entirely white** — sky, ground and prims alike — with the UI drawn
normally over it. Both backends showed it (in-process and process viewers),
and both viewers of one run at the same moment (Alpha and Beta), while other
runs an hour apart rendered the same spot normally (a dark region with its
prims). The hover tip and the build manipulator still drew over the white, so
the scene was there and picked.

World actions aimed while it lasted failed far more often (`Stable` or
`ReceivesEvents` for the full two minutes); whether that is the same cause or
the aim being judged on a frame that shows nothing is open.

## To find out

- Whether the region's environment at those times (its day cycle, a sky the
  sim pushed) is what draws white — read `Probe::Environment` and the scene
  dump's `environment.sky_params` when it happens.
- Whether Firestorm draws the same region white at the same moment.
- Whether it is the capture (`CaptureTarget::Window` of the off-screen window)
  rather than the frame.
