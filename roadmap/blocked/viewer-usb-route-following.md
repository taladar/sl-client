---
id: viewer-usb-route-following
title: USB route following — advancing waypoint beacons
topic: viewer
status: blocked
origin: user request (2026-07-23), follow-up to viewer-world-map-floater
blocked_by: [viewer-usb-route-map-display, viewer-beacons-beam-render]
refs: [viewer-usb-route-comments]
---

Context: [context/viewer.md](../context/viewer.md).

Follow a loaded USB route in-world: the viewer tracks the **current**
waypoint of the active route ([[viewer-usb-route-map-display]]) and drives
the shared tracking beacon ([[viewer-beacons-beam-render]], via the
`MapTracking` resource both map surfaces already share) at it. When the
avatar gets **close to the current waypoint**, the route advances and the
beacon jumps to the **next** one — so cruising a route is beacon-to-beacon
without touching the map:

- an arrival radius (a setting; routes are typically sailed/flown, so
  generously sized, ~10–30 m) using 2-D distance in global metres, robust
  across region crossings;
- manual next / previous / restart controls beside the route display's
  load/clear UI, for when the auto-advance misjudges;
- both map surfaces highlight the current waypoint distinctly from the
  rest of the route;
- route state survives teleports within the session (the route is grid
  coordinates, not scene state).

The waypoint's comment surfacing on advance is
[[viewer-usb-route-comments]]; this task owns the advancing/tracking state
machine.

Deps: [[viewer-usb-route-map-display]], [[viewer-beacons-beam-render]].
