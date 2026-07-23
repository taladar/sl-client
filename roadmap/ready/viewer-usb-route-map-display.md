---
id: viewer-usb-route-map-display
title: USB route display on the minimap & world map
topic: viewer
status: ready
origin: user request (2026-07-23), follow-up to viewer-world-map-floater
blocked_by: [viewer-minimap, viewer-world-map-floater]
refs: [viewer-usb-route-following, viewer-usb-route-comments]
---

Context: [context/viewer.md](../context/viewer.md).

Load a **USB route notecard** (the community route format `sl-types` /
`sl-map-apis` already parse: `USBNotecard` = a list of `USBWaypoint`s, each a
`Location` — region name + integer region-local coordinates — plus an
optional comment) and draw the route on **both** map surfaces:

- a polyline through the waypoints plus per-waypoint markers, composited
  into the minimap and world-map surfaces like the existing layers (the
  world map draws the whole route; the minimap the nearby segment);
- waypoint hover tooltips (name/number; the comment text itself is
  [[viewer-usb-route-comments]]);
- loading UI: open a route file from disk (the format is a plain text
  notecard export; `sl-map-web` / `sl-map-cli` interoperate with the same
  files), and clearing the active route again.

Waypoint positions need region-name → grid-coordinate resolution: reuse the
world-map model (a `MapNameRequest` per distinct region name works on every
grid) rather than `sl-map-apis`' cap-based
`RegionNameToGridCoordinatesCache` (SL-only); consider the latter as a
fallback for huge routes on agni.

Builds on: the world-map floater's surface + tile service
([[viewer-world-map-floater]]), the minimap compositor, and the
`sl-map-tools` route types (`USBNotecard`, used by `Map::draw_route`).

Deps: [[viewer-minimap]], [[viewer-world-map-floater]].
