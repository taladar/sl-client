---
id: viewer-beacons-control
title: Tracking beacon — set / clear from the UI
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-beacons
blocked_by: [viewer-beacons-beam-render, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

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
