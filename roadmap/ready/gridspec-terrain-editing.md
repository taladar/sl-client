---
id: gridspec-terrain-editing
title: Terraforming, raw terrain transfer and terrain textures on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-region-options-terrain, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim `UndoLand` is a stub, raise / lower limits ±100 m (SL 4 m); the fake
grid handles no `ModifyLand`.

## Discover

`modify-land`, `terrain-raw-*` on OpenSim; aditi on a sandbox that allows
terraforming.

## Document

`book/src/gridspec/terrain.md` § Editing.

## Fake grid

Large — [[server-fake-grid-terraform]].

## Viewer

Per-grid limits from `RegionInfo`; no reliance on undo on OpenSim.
