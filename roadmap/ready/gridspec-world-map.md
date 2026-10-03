---
id: gridspec-world-map
title: World map blocks, items, layers and tiles on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-world-map-tracking-teleport]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim: a placeholder green dot, one whole-grid layer. SL: one global
layer, tiles from a CDN. The aditi run of `map-blocks-items` was deferred.

## Discover

Run `map-blocks-items` on aditi; add per-item-type probes (telehub, land for
sale, events, agent counts), `MapNameRequest`, the tile host.

## Document

`book/src/gridspec/world-map.md`.

## Fake grid

Small — per-flavour item / layer answers in this task.

## Viewer

Tile URL per grid, placeholder items; the world map floater via automation on
both flavours.
