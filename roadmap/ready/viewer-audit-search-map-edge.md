---
id: viewer-audit-search-map-edge
title: sl-viewer-search depends on sl-viewer-map for one two-field struct
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-search/src/lib.rs:20` re-aliases `sl_viewer_map::world_map` solely to
reach `OpenWorldMap` (used at `search.rs:74`, `:2903`, `:2965`) — a two-field
`Message` defined at `sl-viewer-map/src/world_map.rs:188`.

That drags a 7k-line crate, plus its `tokio` / `arboard` / `sl-map-apis` tree,
into search's build graph for one struct.

`sl-viewer-world-api` is precisely where "a beacon on what the map is tracking"
belongs — its module doc says so, and it already hosts `MapTracking` at `:1414`.
Move `OpenWorldMap` there and the edge disappears.
