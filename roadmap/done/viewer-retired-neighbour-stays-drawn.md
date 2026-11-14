---
id: viewer-retired-neighbour-stays-drawn
title: A neighbour the simulator retired stays drawn — its ground, water and map tile outlive its circuit
topic: viewer
status: done
origin: gridspec-neighbours-crossing (2026-10-07)
refs: [gridspec-neighbours-crossing]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

A simulator takes a neighbouring region away with a `DisableSimulator` down
its child circuit when the agent's draw distance stops reaching it: within a
second on OpenSim, fifty seconds later on Second Life (which holds a
neighbour sharing an edge only from 128 m up —
`book/src/gridspec/teleport.md`, *Neighbours and crossings*). The session
then reports every object of that region removed, empties its coarse
locations, and raises `Event::NeighborRetired`; `sl-client-bevy` drops the
region's entity and parcel overlay.

Everything the viewer keeps per region outside those is only ever purged on
a **world reset** (a distant teleport): the land patches and their materials
(`TerrainState`), the sea cells, the parcel-border bands, the minimap's
terrain backdrop, the region's time dilation. So a user who lowers the draw
distance below 128 m on Second Life keeps looking at the ground of three
regions the grid has stopped sending — frozen, since nothing updates it —
and the reference viewer shows water there (`LLWorld::removeRegion` on
`DisableSimulator`).

## Wanted

Each per-region store drops a region on `NeighborRetired`, as it drops
everything on a world reset, and takes it back when the region is announced
again. The current region is never retired this way. A full-stack test:
the border scene, the draw distance stepped down, the neighbour's marker and
ground gone from the picture, then back.

## Resolution

A store declares that it holds something per region the way it already
declared that it belongs to one world: `sl_viewer_world_api::region_scoped`
adds a `RegionScoped` trait (`forget_region`) and
`register_region_scoped`, beside the world-reset `WorldScoped`, and runs
each registered purge in the world reset's own `Detect` / `Purge` phases on
the frame a `NeighborRetired` names a region the agent does not stand in.
Registered:

- `TerrainState` — the region's drawn patches despawned, its raw patches,
  composition and per-region revision dropped, the global revision bumped
  (so the minimap's backdrop, which composes from the regions still held,
  rebuilds without it). The retained `land_cache` keeps its heights, as it
  does through a world reset.
- `TerrainTextures` — its splat material; `PendingPatchRebuilds` — its
  queued rebuilds. A new `rebuild_retired_seams` (first in the terrain fold,
  after the purge) queues the surviving patches whose far edge was stitched
  to the region, so they fall back to the loaded world's edge lip — the seam
  half of `LLWorld::removeRegion`.
- `WaterState` — its sea level; its square of the sea grid stays and is
  void water at the level around it from the next frame.
- `RegionTimeDilation` — so `neighbours_known` stops counting it.
- `ParcelOwnerOverlay` — its Land Owners map (now also world-scoped; it
  was purged on neither before).

The parcel-border bands already reconcile against the live `SlRegion` set,
but only for regions that drew bands: a stamp for one that drew nothing
outlived it and could match the region's state when it came back. They now
forget every region that left.

Unit tests pin the trait (the current region is never forgotten, a burst
forgets each once, only in its frame), each store's forget, the seam
selection and the queue purge, and the bands' stamp. A full-stack test
(`a_retired_neighbour_leaves_the_picture_and_comes_back`) logs into the
border grid on the OpenSim flavour, steps the draw distance down to 32 m
and back, and asserts the stores, the marker pillar and a disc of the
neighbour's ground each time. The gridspec teleport chapter documents it.
