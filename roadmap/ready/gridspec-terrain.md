---
id: gridspec-terrain
title: Terrain, wind and cloud layers as each grid sends them
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-pbr-terrain]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
