---
id: viewer-slider-click-mirrored-rtl
title: In a right-to-left layout a slider's track click and drag run mirrored from its thumb
topic: viewer
status: bugs
origin: viewer-day-cycle-strip-click-mirrored-rtl (2026-11-09)
refs: [viewer-day-cycle-strip-click-mirrored-rtl, viewer-slider-editable-value-field]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The shared slider widget (`sl-viewer-ui-widgets/src/ui_slider.rs`) places
its thumb by a `LogicalInset` `inline_start`, so under `UiDirection::Rtl`
the thumb mirrors: the minimum sits at the track's right end. But the value
a pointer sets comes from `bevy_ui_widgets`' core `Slider` (our bevy fork,
`crates/bevy_ui_widgets/src/slider.rs`), which knows nothing of the UI
direction:

- a **track click** converts the pointer with `x_from_left`, so it selects
  the value mirrored across the track from where the thumb would be drawn;
- a **drag** adds the physical `distance.x`, so dragging the thumb toward the
  maximum (leftward under `Rtl`) lowers the value and the thumb runs away
  from the hand.

Every slider in the viewer is built on this widget, so all of them behave
this way under right-to-left. The day-cycle timeline had the same bug in its
own hit-test and was fixed in [[viewer-day-cycle-strip-click-mirrored-rtl]].

## What to do

Make the value follow the thumb under `Rtl`, for both the click and the
drag, and decide what the arrow keys do (`Left` / `Right` on a mirrored
slider usually follow the visual direction). Either the core slider learns
a direction (bevy fork, and an upstream fix — `taffy` already lays out off
`Node::direction`, which `apply_ui_direction` keeps current), or the widget
mirrors the value on the way in. Cover both directions in a test.

## Done when

A click on a slider's track sets the value under the pointer and a drag
moves the thumb with the hand, in both directions.
