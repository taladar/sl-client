---
id: viewer-world-map-markers
title: World-map floater — MapItem marker layers
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The `MapItem` marker layers over the map surface: avatar dots, telehubs,
land-for-sale, and the event classes, with their per-layer filters. The protocol
side is done — `request_map_items` returns `MapItem` (with `region_handle()` /
`region_position()`) and `MapItemType` already distinguishes telehubs, agent
locations, land-for-sale and the event classes.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmap`,
`llworldmapmessage`.

Builds on: `protocol-12` map items and the map floater surface
([[viewer-world-map-floater]]).

Deps: [[viewer-world-map-floater]].
