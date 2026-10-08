---
id: gridspec-terrain
title: Terrain, wind and cloud layers as each grid sends them
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-pbr-terrain, gridspec-terrain-editing,
  viewer-automation-ground-reveal-eye-under-terrain]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-08)

Measured and written up in `book/src/gridspec/terrain.md`.

- **Discover.** `terrain-layerdata`, until now a fake-grid check of the
  fixture's heights, takes a census of every `LayerData` of an arrival on
  all four grids: 100 s with the circuits probed from the first datagram,
  each message's time, length and reliability off the wire and its headers
  off a new event. Three runs on aditi (Ahern), three on OpenSim;
  `terrain-composition` once on each for the textures.
- **Findings.** Both grids send all 256 patches of the agent's region and
  of every neighbour, reliably, once, starting with the patch the agent
  stands in — and nothing but the ground and the wind: no cloud and no
  water layer. Second Life cuts a message before the patch that would take
  it past about 1,200 bytes and sends nearest first only roughly, never in
  the same order twice; OpenSim closes a message with the patch that takes
  it past 890 bytes, sends strictly nearest first counted in whole patches,
  and writes a patch with no relief as a header alone (`QuantWBits` 0, a
  `range` of one). The wind is two patches at (0, 0) on both: every second,
  unreliably, under a stride of 18 with six bits of prequantization and
  right behind the ground on Second Life; every 13.63 s, reliably, in the
  ground's encoding and on the region's own clock on OpenSim. Both send a
  neighbour's wind down its child circuit.
- **Known-already, corrected.** "Aditi mainland sends nil detail ids" does
  not hold for Ahern, which is mainland and names four textures that fetch
  and decode. The ids, the blend heights and the water height are a
  region's settings, not a grid's, so no flavour differs in them. "One
  patch per `LayerData`" on OpenSim is 44 to 60.
- **Client.** `Event::TerrainLayerBatch` carries each `LayerData` whole:
  both headers and the patches in order. The encoder takes a
  `LayerEncoding` (stride, prequantization, how a flat patch is written)
  and cuts a ground into messages by a `LayerPacking`;
  `SimSession::send_terrain` sends nearest first from a point instead of in
  a spiral four at a time, which neither grid does, and `set_wind_feed`
  sends the wind on a timer down a root or a child circuit.
- **Fake grid.** `ImitatedGrid::terrain_policy`: where "nearest" is counted
  from, where a message is cut, the flat-patch form, and the wind's start,
  interval, reliability and encoding. `terrain-layerdata` holds both
  flavours and both live grids to every row the chapter marks **held**.
- **Viewer.** A new `e2e_terrain`: on each fake flavour, in a region of
  terraces, the height the viewer's ground pick gives is the region's on a
  flat patch, either side of a terrace edge inside one patch, and beyond
  it; and on each live grid (`SL_E2E_GRID=opensim` and `=aditi`, both run)
  the viewer has ground the pointer rests on beside its avatar. The viewer
  makes nothing of the wind.
- **Not done here.** Whether Second Life counts "nearest" from the agent or
  from its camera; either grid at a small draw distance; a region larger
  than 256 m (the extended layers); a region whose detail ids are nil or
  name PBR materials — none was found, and rendering them is
  [[viewer-pbr-terrain]]. What a terraform sends is
  [[gridspec-terrain-editing]].
- **Filed.** [[viewer-automation-ground-reveal-eye-under-terrain]], met
  while writing the viewer test.

## Known already

Aditi mainland sends nil detail ids (PBR terrain); OpenSim real J2C ids,
one patch per `LayerData`, wind as two patches, clouds one.

## Discover

A live census mode for `terrain-layerdata` / `terrain-composition`: patch count,
order, layer types, wind / cloud presence and cadence; both grids.

## Document

`book/src/gridspec/terrain.md`.

## Fake grid

Small — per-flavour detail ids, layer types and cadence.

## Viewer

Nil detail ids, PBR terrain on SL.
