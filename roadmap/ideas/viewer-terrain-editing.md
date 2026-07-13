---
id: viewer-terrain-editing
title: Terrain editing
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-input-system, viewer-region-options]
---

Context: [context/viewer.md](../context/viewer.md).

In-world terrain sculpting brushes: raise / lower / flatten / smooth / roughen /
revert over a selected land area, plus bake / revert. Terrain textures and
elevation ranges overlap with the region-options floater.

Reference (Firestorm, read-only): `lltoolbrushland` (`LLToolBrushLand`); the
`ModifyLand` message.

Builds on: `terrain.rs` and `sl-terrain`.

Deps: [[viewer-ui-framework]], [[viewer-input-system]] (brush drag),
[[viewer-region-options]] (terrain textures / heights overlap).
