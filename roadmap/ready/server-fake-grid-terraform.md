---
id: server-fake-grid-terraform
title: Fake grid — terraforming, undo and bake
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-terrain-editing]
---

Context: [context/server.md](../context/server.md).

## What

`ModifyLand` / `UndoLand` are unhandled and a raw upload re-broadcasts nothing.
Brushes, patch re-broadcast, undo honoured per flavour, bake baseline.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured by gridspec-terrain-editing (2026-10-18)

`book/src/gridspec/terrain.md` § Editing has the answers. Per flavour: a
stroke sends the one moved patch to every avatar (OpenSim), a refused stroke
nothing (both); OpenSim's brushes are its `PaintBrushes` (cosine raise/lower,
flatten reaching `√2` × radius, revert reaching `√radius`), the area form its
`FloodBrushes`; undo does nothing on OpenSim; limits are counted from the
baked map and skipped for a god (OpenSim's region owner by default); a RAW
load re-bakes; bake is silent. When done, drop the brush rows from
`NOT_IMITATED_AS_OPENSIM` in `sl-conformance/src/cases/terrain_editing.rs`.
