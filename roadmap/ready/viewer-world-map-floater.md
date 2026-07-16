---
id: viewer-world-map-floater
title: World-map floater — surface, zoom & region tiles
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The world-map floater surface: pan and zoom across the grid with its two zoom
regimes (grid-wide vs. region-detail), region tiles as the backdrop. This is the
root of the world-map cluster — search, markers and tracking / teleport all
extend the surface it introduces.

The **protocol is already done** (`protocol-12` full world map, `protocol-65`
map-service pairing): `Session::request_map_blocks`, `request_map_by_name`,
`request_map_items` and `request_map_layer` in
`sl-proto/src/session/methods.rs`, returning `MapRegionInfo`, `MapItem` and
`MapLayer`. None of it is wired to anything visual.

**Region tile imagery** — fetch and cache the map tiles; the sibling
`sl-map-tools` workspace already exists (we depend on it for `sl-types`) and
does tile work, so reuse its fetch / cache rather than writing a third one.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmap`,
`llworldmapmessage`, `llworldmipmap`.

Builds on: `protocol-12` map data, `protocol-65` map-service pairing, and
`sl-map-tools` for tiles.

Deps: [[viewer-ui-widget-scaffold]] (the floater).
