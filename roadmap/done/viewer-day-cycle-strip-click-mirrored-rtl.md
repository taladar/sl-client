---
id: viewer-day-cycle-strip-click-mirrored-rtl
title: In a right-to-left layout a click on the day-cycle timeline lands mirrored from the markers
topic: viewer
status: done
origin: viewer-gallery-floaters-are-mostly-stubs — noticed while fixing the timeline layout (2026-09-25)
refs: [viewer-gallery-floaters-are-mostly-stubs, viewer-slider-click-mirrored-rtl]
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

## Fixed (2026-11-09)

- **The timeline mirrors.** A day timeline is a value running from one end
  to the other, like a slider or a progress bar, which mirror under
  right-to-left; it is not a picture of a clock face, which would not. The
  layout already mirrored — the markers sit at `inline_start` on a rail inset
  at `inline_end`, and the tick row is a `FlexDirection::Row` — so only the
  hit-test disagreed.
- `strip_fraction` (`sl-viewer-environment/src/day_cycle_editor.rs`) takes the
  live `UiDirection` and measures the pointer from the leading edge: the
  strip's right edge under `Rtl`. Both strip observers (press and drag) pass
  it.
- `node_rect` now returns the box inside the strip's border, the box the
  rail's percentages resolve against; measuring against the outer box read a
  click up to a border's width off its marker, in either direction.
- Pinned by `a_click_on_a_marker_reads_its_own_position_in_either_direction`
  (a marker's drawn centre reads its own fraction in both directions, and the
  ends clamp with the day starting on the right under `Rtl`),
  `the_timeline_maps_a_pointer_to_the_span_a_marker_is_placed_in` and
  `the_strip_rect_is_the_box_inside_its_border`.
- The shared slider has the same disagreement — its thumb mirrors, its track
  click and drag do not — filed as [[viewer-slider-click-mirrored-rtl]].
