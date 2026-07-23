---
id: viewer-world-map-search
title: World-map floater — region-name search
topic: viewer
status: done
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Region-name search on the world-map floater: type a region name, issue
`request_map_by_name`, list the matching results, and recentre the map surface
([[viewer-world-map-floater]]) on a selected result.

Done (2026-07-23), inside `world_map.rs`: the floater's side panel hosts the
shared search-field widget (`ui_search`); typing debounces 0.6 s (min two
characters) into a `Command::RequestMapByName`, whose `MapBlock` replies fold
into the shared region model. The result list (wheel-scrollable, capped at
30 rows) shows the known regions matching the query — prefix matches first,
then alphabetical (a committed unit test pins the ordering) — and clicking a
row recentres the map on that region, zooming in to the detail regime when
the map was zoomed far out.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmapmessage`.

Builds on: `protocol-12` map data and the map floater surface.

Deps: [[viewer-world-map-floater]].
