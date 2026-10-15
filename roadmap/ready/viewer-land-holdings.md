---
id: viewer-land-holdings
title: My land holdings floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-parcel-options-general, viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The land-holdings floater: every parcel the agent owns (and their group-land
contributions), with name, region, area, and the tier summary (m² used vs
allowed). Rows: show-on-map and teleport.

Wire coverage (audit 2026-10-15): the protocol half exists —
`Command::PlacesQuery` (`PlacesQuery`, an agent's or group's land holdings,
`sl-proto/src/command.rs`) answered by `Event::PlacesReply` with its
`PlacesResult` rows (`sl-proto/src/types/event.rs`). Nothing in the viewer
issues it yet; the tier allowance comes with the money/economy data.

Reference (Firestorm, read-only): `llfloaterlandholdings`,
`floater_land_holdings.xml`.

Builds on: parcel/money protocol (`protocol-11`, `protocol-13`) and the
`PlacesQuery` / `PlacesReply` pair above.
