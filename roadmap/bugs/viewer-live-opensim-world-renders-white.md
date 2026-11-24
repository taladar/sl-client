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

## Investigation (2026-11-23): not reproduced

Nothing below showed a white world, so the cause is still unknown.

- **Not the time of day.** Headless captures on the live `Default Region`, with
  fresh settings, at day positions 0 to 0.875 in steps of 0.125 and once on the
  wall clock, all rendered normally, with no pixel at 250 or above.
- **Not seen in a full live suite.** `e2e_two_avatars` on OpenSim, both
  backends: 27 of 28 passed. A passing test keeps few screenshots, so this run
  left only three frames to inspect. None is white, which is weak evidence.
- **No stored environment.** `regionenvironment` is empty today. The October
  OpenSim journal has been rotated away, so what the region held on 2026-10-02
  cannot be read back.

## The failed aims have a separate cause

The `ReceivesEvents` timeouts happen on a correctly drawn world.
`contest_one_prim` rezzes at one of six spots chosen by `stamp % 6`, and the
spots repeat between runs because the avatar lands in the same place. Prims
left by failed runs in October stand on them:

| Failed aim | Leftover prim at |
| --- | --- |
| 130.85, 119.91 | 130.86, 119.89 (`Link Root`) |
| 130.96, 122.92 | 130.96, 122.92 (`Taken By The Viewer`) |

A 256 m long `Edited By The Viewer` prim also crosses the region at y 123.92.
In the logged failure every probe hit one object 1.72 m from the point. Its
local id could not be mapped to a stored prim, so which of these it was is not
established. These prims date from 2026-10-10 and later, so they cannot explain
the aim failures of the original 2026-10-02 runs.
