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
allowed). Driven by the classic `AgentData`-family wire pair
(`RequestLandHoldings`? — verify: the reference uses the money-server-backed
`LandHoldings` reply via `AgentDataUpdate`/dir queries; confirm which of our
implemented surfaces carries it, and add the small missing decode if the
protocol batches did not cover it). Rows: show-on-map and teleport.

Reference (Firestorm, read-only): `llfloaterlandholdings`,
`floater_land_holdings.xml`.

Builds on: parcel/money protocol (`protocol-11`, `protocol-13`); first step
is the wire-coverage check above.
