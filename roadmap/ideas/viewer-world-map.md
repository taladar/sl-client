---
id: viewer-world-map
title: World-map floater
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The full world map: pan and zoom across the grid, region tiles as the backdrop,
markers for what is out there, search, tracking, and teleport-by-map.

The **protocol is already done** (`protocol-12` full world map, `protocol-65`
map-service pairing): `Session::request_map_blocks`, `request_map_by_name`,
`request_map_items` and `request_map_layer` in
`sl-proto/src/session/methods.rs`, returning `MapRegionInfo`, `MapItem` (with
`region_handle()` / `region_position()`) and `MapLayer`, and `MapItemType`
already distinguishes telehubs, agent locations, land-for-sale and the event
classes. There is a conformance case for map blocks and items. None of it is
wired to anything visual.

Scope: the floater and its two zoom regimes (grid-wide vs. region-detail),
**region tile imagery** — fetch and cache the map tiles; the sibling
`sl-map-tools` workspace already exists (we depend on it for `sl-types`) and
does tile work, so evaluate reusing its fetch/cache rather than writing a third
one — region-name search and results, the `MapItem` marker layers (avatar dots,
telehubs, land for sale, events) with their filters, landmark / friend / event
tracking, and double-click-to-teleport.

Tracking a location hands off to [[viewer-beacons]] (the in-world beam to the
tracked point) and teleporting hands off to [[viewer-teleport-flow]]; this task
owns the map surface, not those flows.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmap`,
`llworldmapmessage`, `llworldmipmap`.

Builds on: `protocol-12` map data, `protocol-65` map-service pairing, and
`sl-map-tools` for tiles.

Deps: [[viewer-ui-widget-scaffold]] (the floater).
