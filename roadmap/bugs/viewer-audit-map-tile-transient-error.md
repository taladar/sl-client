---
id: viewer-audit-map-tile-transient-error
title: A transient map-tile fetch error is cached as permanently missing
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-map/src/world_map_tiles.rs:231` — `fetch_tile` returns `None` for
both `Ok(None)` (the server genuinely has no tile) and `Err(_)` (timeout, 500,
DNS). `drain` (`:158`) folds both into `TileState::Missing`, and `request`
(`:134`) early-returns on `self.tiles.contains_key(&key)`.

So one transient failure blanks that region on the map for the **rest of the
session**. The module doc admits it — "a later session retries through the
cache's freshness rules" — which is to say this session never does. That is
directly against the project's never-drop-a-failed-fetch rule.

Fix: distinguish the two in the worker's reply and let an errored slot be
re-requested with a retry budget.

Second defect in the same file, `:170` — LRU eviction sorts purely on
`last_used`, and a pending tile's stamp is its request frame. `state()` (`:186`)
touches only tiles the compositor still asks for, so a tile requested and then
panned off-screen has the oldest stamp, gets evicted, and is re-sent by
`request()` when panned back — while the first fetch is still in flight.
`MAX_RESIDENT_TILES = 384` is reachable at low zoom on a large window.

`world_map_tiles.rs` is 263 lines with zero tests and is pure apart from the
worker: push `(key, Some(raster))` / `(key, None)` / an error through the
channel and assert the eviction, the touch, and the re-requestability of an
error.
