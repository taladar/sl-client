---
id: viewer-automation-ground-reveal-eye-under-terrain
title: A ground reveal can put the camera under higher ground beside the point
topic: viewer
status: bugs
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

## What to do

Have the reveal lift the eye clear of the terrain under it — the height of
the ground at the eye plus a margin — before judging the frame, and prefer
a framing from the point's own side of a rise when the first one is
blocked. [[viewer-camera-framing-ends-inside-the-framed-object]] is the
same failure with an object in place of the hill.

Reproduce with a temporary test beside `e2e_terrain`'s: its terraced stage,
then `alpha.world().ground(stage.home_region(), 100.0, 120.0).hover()`.
