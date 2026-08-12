---
id: viewer-terrain-land-cache-on-disk
title: Persist the land-height cache to disk (ground floor available at login)
topic: viewer
status: ready
origin: fall-through fix — the in-memory land cache is empty at login (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

`TerrainState` now keeps an **in-memory** `land_cache` (the last-known land
patch per key), so `land_height` keeps answering across a mid-session region
teardown/rebuild — which stops the avatar ground floor (`physics.rs`) from
dropping to `None` and letting the avatar fall through the terrain while its
patches are momentarily absent.

But that cache is **empty at login**, and that is exactly the window the
observer hit ("initially still fell through the floor, possibly from the
position saved at last logout; after a teleport it was better" — the teleport
gave the terrain time to stream in and populate the floor). Persisting the cache
to disk closes it: on the next login the land floor is available immediately,
before terrain streams.

## Plan

- **Serialize** the land patches (`TerrainPatch` is already `serde`) to a
  **grid-scoped** cache file — region handles are grid-global coordinates and
  collide across grids (aditi vs OpenSim both start near the grid origin), so
  the path must include the grid (e.g.
  `asset_cache_dir("terraincache")/<grid>/…`, keyed off the login grid; see
  `paths.rs`).
- **Load** at startup / on first region entry into `land_cache`, before the
  avatar needs a floor.
- **Save** incrementally (on patch ingestion, debounced) or on a timer / at
  exit; do the I/O **off the main thread** (a task, like the other asset caches)
  so a large region write never hitches a frame.
- **Bound** the size (LRU by region, a cap on cached regions) so it does not
  grow without limit across many regions visited.
- **Staleness**: land is effectively static (only terraforming changes it,
  rarely and slowly); a slightly stale cached height is a fine *floor* (far
  better than a fall-through), and live terrain overrides the cache the instant
  it streams. A terraform (`LayerData` for an already-cached patch) refreshes
  the entry. No explicit invalidation beyond "newest patch wins".

## Verify

Log in over a fresh session (cold cache), walk immediately — no fall-through
during the terrain-load window. Repeat after a prior session populated the
cache; confirm the floor is present from the first frame
(`SL_VIEWER_LOG_AVATAR_GROUND` shows `land=Some(...)` immediately, not `None`).
Both aditi and OpenSim.
