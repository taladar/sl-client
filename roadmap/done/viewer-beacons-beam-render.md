---
id: viewer-beacons-beam-render
title: Tracking beacon — beam + off-screen direction arrow
topic: viewer
status: done
origin: user request (2026-07); split from viewer-beacons
refs: [viewer-beacons-control]
---

## Done (2026-08-08)

New viewer module `beacons.rs` + `beacon_beam.wgsl` (`BeaconPlugin`), driven by
the shared `MapTracking` resource. Live-verified on OpenSim.

- **Beam** — a custom unlit, alpha-blended, glow-mask-preserving
  `BeaconBeamMaterial` (the `parcel_borders` material template) on two stacked,
  CPU-billboarded blade meshes: a blue lower shaft (ground → target) and a red
  upper shaft (target → the reference's 5020 m sky ceiling), the tracked
  position at the red/blue seam. Depth-tested, no depth-write (the reference's
  `LLGLDepthTest(GL_TRUE, GL_FALSE)`). Colours: `MapTrackColor`/`…Under`
  (red/blue) for a location, distinct green / gold for a tracked avatar /
  friend. Blade has a solid opaque core band + soft edges, and the shader's
  distance-alpha is nudged to `[0.35, 0.7]` (vs the reference's `[0.2, 0.5]`) so
  it reads as solid as SL's.
- **Label + arrow** — a screen-space overlay (not the name-tag billboard
  machinery, to dodge its fade-distance cutoff). The label (name + `"%.0f m"`
  agent-distance) pins to the projected seam when on-screen, clamped into the
  viewport. The **arrow** is the reference `LLTracker::drawMarker`: a generated
  arrow sprite (shaft + head) on an ellipse around the projected seam, pointing
  back out at the beacon — up when the camera is below the seam altitude, down
  when above, sideways when level; pinned to the viewport edge when off-screen.
  Clickable to dismiss (stop tracking). Overlay z-order is above the toolbars
  (z 9500 vs 9000) so the arrow is not hidden behind them.
- **Sources** — reads `MapTracking` (the shared beacon resource): a tracked
  location (map / world-map click, cleared on arrival) or avatar (minimap radar
  "Start Tracking"), avatar vs. friend distinguished for colour. Landmark and
  teleport-in-progress render through the same global-position `Location` path,
  exactly as the reference's single `getTrackedPositionGlobal` does; there is no
  distinct source for them yet (the set/clear UI is `viewer-beacons-control`),
  so no dead enum variants were added.

Also implemented here (overlaps `viewer-beacons-control`, at user request while
live-testing, since it is how a beacon is naturally set / cleared): a
**world-map single-click sets the beacon** (`LLTracker::trackLocation`) at the
clicked spot, the world-map **Z field live-drives the seam altitude**, and the
**arrow click dismisses** the beacon.

Env `SL_VIEWER_LOG_BEACON=1` logs the overlay's label/arrow/hidden decisions.

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
