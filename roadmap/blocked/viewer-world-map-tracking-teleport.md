---
id: viewer-world-map-tracking-teleport
title: World-map floater — tracking & teleport hand-off
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-world-map-floater, viewer-beacons-beam-render, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

Landmark / friend / event tracking and double-click-to-teleport on the map
surface. Tracking a location hands off to the in-world beam
([[viewer-beacons-beam-render]]) pointed at the tracked point; double-clicking a
region hands off to the teleport progress flow
([[viewer-teleport-flow-progress]]). This task owns the map-side selection and
hand-off, not the beam or teleport flows themselves.

Reference (Firestorm, read-only): `llfloaterworldmap` (tracking + map teleport),
`llworldmap`.

Builds on: the map floater surface ([[viewer-world-map-floater]]), the beam
render, and the teleport progress flow.

Deps: [[viewer-world-map-floater]], [[viewer-beacons-beam-render]],
[[viewer-teleport-flow-progress]].
