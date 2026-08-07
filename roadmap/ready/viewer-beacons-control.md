---
id: viewer-beacons-control
title: Tracking beacon — set / clear from the UI
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-beacons
blocked_by: [viewer-beacons-beam-render, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Partly done already (implemented alongside [[viewer-beacons-beam-render]]): a
**world-map single-click sets** the location beacon (and the world-map Z field
drives its altitude), the minimap radar's **Start / Stop Tracking** sets /
clears an avatar beacon, the **dismiss arrow** on the beam is clickable to stop
tracking, and a location beacon **auto-clears on arrival**
(`clear_reached_location_track`, minimap.rs — within 3 m horizontally, the
reference `LLTracker` behaviour, runs ungated so it applies to a world-map-set
beacon too). Remaining: a landmark-tracking source, and a general "clear current
track" UI control outside the arrow / radar menu.

The control side of the tracking beacon: **set** a beacon from the UI and
**clear** it. This is the map / radar hand-off — a map double-click, a tracked
landmark, or a tracked avatar / friend from the radar sets the target position
that the beam renderer ([[viewer-beacons-beam-render]]) draws, and the user can
stop tracking it again.

Two clearing paths, mirroring the reference `LLTracker`:

- the small **dismiss arrow** — a camera-facing chevron on the beacon the user
  clicks to stop tracking that beacon. It needs a clickable world-space widget —
  reuse the existing object-pick path or the UI framework's interaction layer —
  so a click on the arrow clears the corresponding track without disturbing
  world picking;
- a UI control to clear the current track.

Reference (Firestorm, read-only): `LLTracker` (track / untrack), `llfloatermap`
and the map / radar track menus.

Deps: [[viewer-beacons-beam-render]] (the beam this sets a target for) and
[[viewer-ui-widget-scaffold]] (the control surface).
