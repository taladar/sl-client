---
id: viewer-region-tracker
title: Region tracker — watch regions, notify on status
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's region tracker: a small floater holding a user-maintained list
of region names, periodically polled via the map-block protocol
(`protocol-12` `MapBlockRequest` by name — the `access` field distinguishes
up / down / not found), showing each region's status and agent count, and
raising a notification ([[viewer-ui-notification-host]]) when a watched
region comes back online — the "waiting for my home region to restart"
tool. Rows teleport / show-on-map. Persist the watch list per account.

Reference (Firestorm, read-only): `fsfloaterregiontracker`,
`floater_region_tracker.xml`.

Builds on: `protocol-12` map blocks and the notification host.
