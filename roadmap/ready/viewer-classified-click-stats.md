---
id: viewer-classified-click-stats
title: Show click statistics on one's own classifieds
topic: viewer
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, protocol-29]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life grants `SearchStatRequest`. POST `{classified_id}` →
`{teleport_clicks, map_clicks, profile_clicks, search_teleport_clicks,
search_map_clicks, search_profile_clicks}`, shown only on the agent's own
classifieds. The legacy path is the inbound `GenericMessage`
`classifiedclickthrough` (strings: classified id, teleport, map, profile),
which our client does not handle either.

Fetch the counts over the capability where granted, handle the generic message
otherwise, and show them in the classified panel.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
