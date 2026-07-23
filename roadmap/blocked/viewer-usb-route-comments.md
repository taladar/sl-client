---
id: viewer-usb-route-comments
title: USB waypoint comments in the viewer
topic: viewer
status: blocked
origin: user request (2026-07-23), follow-up to viewer-world-map-floater
blocked_by: [viewer-usb-route-map-display]
refs: [viewer-usb-route-following]
---

Context: [context/viewer.md](../context/viewer.md).

Surface the **comments** a USB route's waypoints carry (`USBWaypoint`'s
optional comment — sailing instructions like "keep the island to port",
rez-zone notes, hazards) in the viewer:

- on the map surfaces ([[viewer-usb-route-map-display]]): the waypoint
  hover tooltip shows its comment text;
- while following ([[viewer-usb-route-following]]): the current waypoint's
  comment is shown on screen (and refreshed as the route advances), so the
  instruction is readable without opening the map — plus a peek at the
  next waypoint's comment for anticipation;
- comments render as plain text (they are free-form notecard lines; no
  markup).

Deps: [[viewer-usb-route-map-display]].
