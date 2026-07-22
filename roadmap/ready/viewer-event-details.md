---
id: viewer-event-details
title: Event details floater + reminders
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g5, viewer-search-floater, viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The event-info floater the search results open ([[viewer-search-floater]]
lists events; its rows land here): full event details (name, host,
category, date/time in SLT *and* local time, duration, cover charge,
description, location) with **teleport** / **show on map** actions, and the
**notify-me** toggle — `EventNotificationAddRequest` / `RemoveRequest` plus
the reminder that then arrives (`EventNotification` → a notification via
[[viewer-ui-notification-host]]). The whole wire surface is [[api-g5]],
already done and conformance-tested (`test-event-info`).

Reference (Firestorm, read-only): `llfloaterevent`, `panel_event_info.xml`,
`lleventnotifier`.

Builds on: [[api-g5]] events protocol.
