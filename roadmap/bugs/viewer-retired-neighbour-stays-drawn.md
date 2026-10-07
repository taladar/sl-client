---
id: viewer-retired-neighbour-stays-drawn
title: A neighbour the simulator retired stays drawn — its ground, water and map tile outlive its circuit
topic: viewer
status: bugs
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
