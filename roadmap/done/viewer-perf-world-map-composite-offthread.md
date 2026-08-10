---
id: viewer-perf-world-map-composite-offthread
title: Composite the world-map surface on the compute pool
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: [viewer-perf-minimap-layer-raster-offthread]
---

Context: [context/viewer.md](../context/viewer.md).

`composite_world_map` rendered the whole map surface on the frame thread:
a full-surface RGBA fill (up to ~16 MB at the 2048² cap), a per-pixel blit
of every resident tile, the region grid, and all markers — re-run every
time the change stamp moved, which while the map is open and panning (or
tiles are streaming in, since tile arrivals bump the stamp) meant repeated
multi-millisecond frame-thread composites.

Fix, mirroring the minimap's coalesced background-render pattern
([[viewer-perf-minimap-layer-raster-offthread]]):

- `spawn_world_map_compose` (frame thread, cheap): gathers markers every
  frame (hover hit-test data), builds the stamp, and on a change snapshots
  all drawing inputs into an owned `WorldMapComposeJob` — including the
  per-tile best-resident-source probe, kept on the frame thread because
  `WorldMapTiles::state` bumps the store's residency LRU; the job only
  carries `Arc<TileRaster>` clones.
- `run_world_map_compose` (AsyncComputeTaskPool): the void fill, tile
  blit, grid, markers, beacon, selection, and own-avatar marker — pure
  pixel work over owned data.
- `apply_world_map_surface`: polls the single in-flight task, installs the
  buffer into the map `Image` behind the minimap's size guard (the surface
  can resize mid-flight), and promotes the stamp. At most one compose in
  flight; changes during a run coalesce into the next spawn.
