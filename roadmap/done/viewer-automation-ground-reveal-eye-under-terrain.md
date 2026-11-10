---
id: viewer-automation-ground-reveal-eye-under-terrain
title: A ground reveal can put the camera under higher ground beside the point
topic: viewer
status: done
origin: gridspec-terrain (2026-10-08)
refs: [gridspec-terrain, viewer-automation-ground-aim,
  viewer-camera-framing-ends-inside-the-framed-object]
---

Context: [context/automation.md](../context/automation.md).

## Observation

A ground action on a point of low ground close to a rise never finds a
click that lands on it. On the fake grid, in a region of terraces stepping
up two metres every 51.2 m (`Heightfield::Steps { base: 22, rise: 2, count:
5 }`), with the agent at the centre on the 26 m terrace and its camera
behind it:

| point asked for | its ground | where the reveal left the eye | outcome |
| --- | --- | --- | --- |
| 120, 120 | 26 m | not moved | found |
| 108, 120 | 26 m | not moved | found |
| 100, 120 | 24 m, 2.4 m west of the terrace wall at 102.4 | 107.1, 121.8, **25.16** — over ground that is at 26 m | `GroundTimedOut`, `ReceivesEvents` after 25 s; `Stable` after 60 s in an earlier run |
| 90, 128 | 24 m, 12 m from the wall | 97.0, 125.5, 24.48 — over ground at 24 m | found |

The reveal frames the point from where it would put the eye for flat
ground: a little above the point's own height, back towards where the
camera was. With higher ground between, that is inside the hill, and no
ray from there reaches the point. The same four points on the stock flat
ground are all found.

Both runs of the terraced case gave the same eye to a centimetre, on both
automation backends.

## Why it matters

A test that aims at ground has to know the terrain round the point to pick
one that works, which is what the ground action exists to spare it, and
live ground is seldom flat.

## Related, fixed (2026-10-11)

A framing no longer looks from below its target's level
(`camera::framing_side`, from [[test-e2e-objects-live-opensim-intermittent]]),
which is one way an eye got under the ground. It is not this one: here the
eye is above the point and under a rise beside it.

## What to do

Have the reveal lift the eye clear of the terrain under it — the height of
the ground at the eye plus a margin — before judging the frame, and prefer
a framing from the point's own side of a rise when the first one is
blocked. [[viewer-camera-framing-ends-inside-the-framed-object]] is the
same failure with an object in place of the hill.

Reproduce with a temporary test beside `e2e_terrain`'s: its terraced stage,
then `alpha.world().ground(stage.home_region(), 100.0, 120.0).hover()`.

## Resolution (2026-11-10)

The camera never looked at the ground at all: its collision tests prims
only, and a framing looked from the side the eye was on whatever lay between.
Two changes in `sl-viewer-world-view/src/camera.rs`, both reading the ground
through the new `TerrainState::ground_at` (a Bevy-space point → the height of
the origin region or the grid neighbour under it):

- **The third-person eye stays out of the land**, as the reference's
  `calcCameraPositionTargetGlobal` keeps it: lifted to 0.5 m above the ground
  under it (`getCameraMinOffGround` outside mouselook), the scripted sit camera
  included. The reference's `DisableCameraConstraints` escape is not ported.
- **A framing picks a side whose line of sight clears the ground**
  (`clear_framing_side`): the side the eye was on when its line to the centre
  stays above the ground from 1 m out and its eye stands 0.5 m clear; else
  the same rise turned round in 45° steps, the far side last, then 30°, 45°,
  60° and 75° rises in turn; the original side when nothing clears. Objects
  framed above the ground are unaffected.

`land_height` now looks a standard 16 m patch up by its key instead of
scanning every patch, since the camera asks it every frame. The grid-cell
helpers (`region_handle_at`) moved from the minimap to `sl_viewer_kit::coords`
so the terrain store can share them.

Unit tests pin the terrace case (the side the reveal used is hidden; the
chosen one is clear at the same rise), level and unknown ground keeping the
side, the lift, and `ground_at` across a neighbour. The e2e
`low_ground_under_a_terrace_wall_is_reached` hovers 100, 120 and 90, 128 on
the terraced fake-grid region; it timed out with the camera changes disabled.
