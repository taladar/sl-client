---
id: viewer-terrain-edit-brushes
title: Terrain editing — sculpt brushes
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-terrain-editing
blocked_by: [viewer-input-action-map, viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

In-world terrain sculpting brushes: raise / lower / flatten / smooth / roughen /
revert over a selected land area. The brush drag uses input **actions**
([[viewer-input-action-map]]) and sends the `ModifyLand` message; brush size /
strength selection lives next to the region floater
([[viewer-region-options-debug]]), which owns the terrain-limit and
terrain-texture controls the editing overlaps with.

Reference (Firestorm, read-only): `lltoolbrushland` (`LLToolBrushLand`); the
`ModifyLand` message.

Builds on: `terrain.rs` and `sl-terrain`.

Deps: [[viewer-input-action-map]] (brush drag),
[[viewer-region-options-debug]] (terrain textures / heights overlap).
