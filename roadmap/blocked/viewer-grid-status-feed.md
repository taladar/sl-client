---
id: viewer-grid-status-feed
title: Grid status feed notifications
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

Surface the Second Life grid-status feed in the viewer: poll the public
status feed (status.secondlifegrid.net RSS/Atom, as the reference's grid
status floater does), show new incidents as notifications
([[viewer-ui-notification-host]]) with severity, and keep a small floater
listing current/recent incidents with links out. Poll interval and
enable/disable are settings; SL-only (hide on grids without a feed URL —
grid-info driven).

Reference (Firestorm, read-only): `llfloatergridstatus`,
`floater_grid_status.xml`.

Builds on: the notification host; plain HTTP fetch (no CEF needed for the
feed itself).
