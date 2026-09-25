---
id: viewer-day-cycle-strip-click-mirrored-rtl
title: In a right-to-left layout a click on the day-cycle timeline lands mirrored from the markers
topic: viewer
status: bugs
origin: viewer-gallery-floaters-are-mostly-stubs — noticed while fixing the timeline layout (2026-09-25)
refs: [viewer-gallery-floaters-are-mostly-stubs]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`day_cycle_editor::strip_fraction` (`sl-viewer-environment`) computes a
click's day position from the strip's **left** edge, while the keyframe
markers and the cursor are placed from the **leading** edge. Under a
right-to-left UI the two disagree, so clicking a marker selects the position
mirrored across the strip.

## What to do

Decide whether a day timeline mirrors under RTL at all (a clock face and a
time axis often do not), then make placement and hit-testing agree on it, and
cover both directions in a test.

## Done when

A click on a marker selects that marker in both directions.
