---
id: viewer-audit-map-tile-transient-error
title: A transient map-tile fetch error is cached as permanently missing
topic: viewer
status: done
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

## Resolved (2026-09-11)

The worker's reply is now a `TileAnswer`, and the two cases it used to fold
together are apart: `Ok(None)` is `Missing` — settled, never asked for again —
while an error, a bad zoom level or a runtime that cannot be built is `Failed`,
which is retryable. A `TileSlot` carries a `failures` count that survives the
`Pending` state a retry passes through, and `TileState::Failed` carries the
frame stamp a retry may be sent at. `request` re-sends a failed slot once that
stamp has passed, backing off 60 frames and doubling, up to
`MAX_TILE_FETCH_ATTEMPTS = 4` fetches; the last failure logs that the tile is
being given up on rather than dropping the error silently. A given-up slot is
still evictable, so panning away and back is still a way to ask again.

The eviction pass moved into `evict_settled_tiles`, and `Pending` slots are no
longer candidates. They hold no raster, so they were never what the residency
cap is about, and evicting one let `request` start a second fetch for a tile
whose first was still in flight. The cap now counts settled slots only.

The compositor needed no change: `collect_tile_blits` matches `Ready` and falls
through to a coarser resident level for everything else, so a failed tile shows
the same fallback it always did — it simply stops being permanent.

Tests (the module had none): a failed tile is re-requested only after its
backoff; a retry that arrives is `Ready` and forgets its failures; a missing
tile is never asked for again however long the session runs; the budget stops
at exactly `MAX_TILE_FETCH_ATTEMPTS` sends and leaves `retry_at: None`; an
in-flight tile survives an eviction pass that drops 8 settled ones; eviction
takes the least-recently-used settled tile and spares one touched by `state`;
and a request with no worker records nothing.
