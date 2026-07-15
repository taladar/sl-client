---
id: viewer-beacons-beam-render
title: Tracking beacon — beam + off-screen direction arrow
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-beacons
refs: [viewer-beacons-control]
---

Context: [context/viewer.md](../context/viewer.md).

The in-world **beacon**: a tall vertical beam of light the reference viewer
renders at a tracked position so you can walk / fly toward it — the destination
of a map double-click or teleport, a tracked landmark, or a tracked avatar /
friend. In the reference this is the `LLTracker` system (`renderBeacon` draws
the beam plus a floating label with the name / distance).

Given a **target position** (region-local), render:

- a world-space **vertical beam** and its label at the tracked position,
  colour-coded by what is tracked, drawn so it reads through geometry as a
  waypoint;
- an **off-screen direction arrow** — a small camera-facing chevron pointing
  toward the beacon when the target is outside the view, so you can turn to face
  it.

This is purely client-side rendering: the tracked position is already known
(from a map click, a teleport target, or a tracked avatar's coarse location).
Setting and clearing a beacon from the UI — the map / radar hand-off and the
clickable dismiss on the beacon — is [[viewer-beacons-control]]; this task takes
a position and draws the beam. Cover every beacon source the reference viewer
has (map location, landmark, avatar / friend tracking, teleport-in-progress)
rather than just one.

Note the distinction from the separate debug **render beacons** (physics /
scripted / sound / particle-source markers toggled from the dev menu) — those
are a different feature ([[viewer-debug-render-beacons]]); this task is the
user-facing tracking beacon.

Reference (Firestorm, read-only): `LLTracker::renderBeacon`.
