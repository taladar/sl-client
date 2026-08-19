---
id: viewer-grid-status-feed
title: Grid status feed notifications
topic: viewer
status: ready
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

## Parity-audit addendum (2026-08-19)

Addition from the audit: honour the OpenSimExtras `GridStatusRSS` /
`GridStatus` URL overrides from SimulatorFeatures
(`lfsimfeaturehandler.cpp`) — the body is grid-info driven for hiding
the feature but does not name the per-region SimulatorFeatures override
of the feed/floater URLs. Wire decode already exists in
`sl-wire/src/sim_features.rs`.
