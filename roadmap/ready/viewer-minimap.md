---
id: viewer-minimap
title: Minimap (net map)
topic: viewer
status: ready
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-beacons-control, viewer-minimap-parcel-overlay, viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

The net map: the small top-down view of the region around you, with dots for
nearby avatars and a marker for yourself.

Most of the data is already tracked. `avatars.rs` consumes
`CoarseLocationUpdate` (`update_coarse_avatars`, `coarse_translation`) and keeps
a per-region coarse map — `viewer-r24` fixed exactly the case that matters here,
neighbour-region avatars arriving over the child circuit, so the minimap must
show avatars from the surrounding regions and not just the one you stand in.
Object and terrain data for the backdrop is already in the scene mirror.

Scope: the map panel itself (its backdrop — terrain colour / object silhouettes
and how it is rasterised or rendered), avatar dots with above/below-you height
cues, your own marker with a heading cone, zoom levels, north-up vs camera-up
rotation, hover tips, click-to-target (which sets a tracking beam — see
[[viewer-beacons-control]]), and the right-click menu.

Deliberately **not** here: the parcel colour / border layer
([[viewer-minimap-parcel-overlay]]) and the Firestorm-style nearby-avatar list
([[viewer-avatar-radar]]) — both build on this one.

Reference (Firestorm, read-only): `llnetmap`, `llfloatermap`.

Builds on: `CoarseLocationUpdate` handling in `avatars.rs` (incl. the
`viewer-r24` per-region fix) and the existing scene mirror.

Deps: [[viewer-ui-widget-scaffold]] (the panel / floater).
