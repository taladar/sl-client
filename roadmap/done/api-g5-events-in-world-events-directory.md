---
id: api-g5
title: Events (in-world events directory)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G5 — Events (in-world events directory)

`EventInfoRequest`/`EventInfoReply`, `EventNotificationAddRequest`/
`RemoveRequest`, `EventLocationRequest`/`Reply`, plus `DirEventsReply` from G4.
SL-only in practice.

- [x] G5 events directory. New `EventInfo` type in
  `sl-proto/src/types/directory.rs`:
  the full event listing (creator, name, category, description, human-readable
  `date` + Unix `date_utc`, duration, cover/amount, region name, position,
  `EVENT_FLAG_*`). Commands `EventInfoRequest`, `EventNotificationAddRequest`,
  `EventNotificationRemoveRequest`; event `EventInfoReply` decoded in the
  dispatch path (the `Creator` variable field is parsed as a UUID, matching the
  viewer). Server: each request surfaces as a matching `ServerEvent` and
  `SimSession` gains `send_event_info_reply`. Both runtimes + REPL
  (`event_info_request`, `event_notification_add_request`,
  `event_notification_remove_request`) + tests (2 lifecycle encode/decode, 1
  loopback round-trip) + book (`search.md` "Events directory"). SL-only in
  practice (OpenSim serves the events directory only with a Search module
  present). **Scope note:** `EventLocationRequest`/`EventLocationReply` (Low
  307/308) are **not** wrapped — they are `Trusted` simulator↔dataserver
  messages the viewer never sends or receives (present only in the viewer's
  message prehash, unhandled by OpenSim's `LLClientView`), and the event's
  location already arrives in `EventInfoReply`'s global position. This follows
  the G4 precedent for trusted backend-only messages; they remain reachable as
  raw `AnyMessage` if ever needed.
