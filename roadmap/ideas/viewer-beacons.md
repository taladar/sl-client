---
id: viewer-beacons
title: Tracking beacons (map-position beams) & their dismiss arrows
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The in-world **beacon**: a tall vertical beam of light the reference viewer
renders at a tracked position so you can walk / fly toward it — the destination
of a map double-click or teleport, a tracked landmark, or a tracked avatar /
friend. In the reference this is the `LLTracker` system (`renderBeacon` draws
the beam plus a floating label with the name / distance).

Two parts:

- **The beam.** A world-space vertical beam (and its label) at the tracked
  region-local position, colour-coded by what is tracked, visible through
  geometry so it reads as a waypoint. Purely client-side rendering — the tracked
  position is already known (from the map click, teleport target, or the tracked
  avatar's coarse location).
- **The dismiss arrow.** The small camera-facing chevron / arrow on the beacon
  that the user can click to stop tracking that beacon (clear it). It needs a
  clickable world-space widget — reuse the existing object-pick path or the
  UI-framework's interaction layer — so a click on the arrow clears the
  corresponding `LLTracker`-style track without disturbing world picking.

Cover every beacon source the reference viewer has (map location, landmark,
avatar / friend tracking, teleport-in-progress) rather than just one. Note the
distinction from the separate debug **render beacons** (physics / scripted /
sound / particle-source markers toggled from the dev menu) — those are a
different feature; this task is the user-facing tracking beacon.
