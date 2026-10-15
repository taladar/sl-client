---
id: viewer-world-map-tracking-teleport
title: World-map floater — tracking & teleport hand-off
topic: viewer
status: ready
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

## Partial (2026-08-07, updated by the 2026-10-15 audit)

The **teleport + map-marker** half landed as
[[viewer-world-map-double-click-teleport]] (done): a single click on the
map sets the shared `MapTracking` target and feeds the X/Y/Z fields and the
Teleport button; a double-click teleports through the shared backend
(`sl-viewer-map/src/world_map.rs`). The **in-world tracking beam** this
item used to wait for exists too: [[viewer-beacons-beam-render]] draws the
`MapTracking` target (`sl-viewer-world-scene/src/beacons.rs`).

What remains is the map floater's own track selectors — track a **friend**
(the reference's online-friends combo, a `TrackTarget::Avatar`), a
**landmark** (the landmarks combo) and an **event** from its marker — and
the option below.

## Parity-audit addendum (2026-08-19)

Parity-audit extension: a keep-map-open-on-teleport option
(`FSDoNotHideMapOnTeleport`) — do not auto-close the world-map floater
when a map-initiated teleport starts.
