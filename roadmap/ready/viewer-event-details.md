---
id: viewer-event-details
title: Event details floater + reminders
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g5, viewer-search-floater, viewer-ui-notification-host]
---

**Partly covered by [[viewer-search-floater]] (2026-07-28).** The reference
`fsfloatersearch` shows an event's full detail **inline** in the search
floater's shared details pane (name, host, date/time + duration, cover,
description, location) with **Teleport** / **Show on Map** and a local
**Remind me** toggle (`EventNotificationAddRequest` / `RemoveRequest`), all
driven by `EventInfoRequest` → `EventInfoReply`. So the "see an event's details
from search" case is done there.

**Still open here:** a **standalone** event floater (the reference's
`llfloaterevent`, opened from an SLURL / a reminder / the events directory, not
just from search), and the reminder **that arrives** — which the protocol cannot
deliver today (there is no `EventNotification` reply/event and no way to read a
subscription; it also needs a notification host,
[[viewer-ui-notification-host]]). Local-timezone date rendering (vs the
dataserver SLT string) is the other gap.

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
