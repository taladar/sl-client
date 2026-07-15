---
id: viewer-terrain-edit-bake-revert
title: Terrain editing — bake / revert
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-terrain-editing
blocked_by: [viewer-terrain-edit-brushes]
---

Context: [context/viewer.md](../context/viewer.md).

Bake the current terrain as the new revert baseline, and revert the terrain back
to the last bake, over a selected land area. Extends the brush tooling from
[[viewer-terrain-edit-brushes]] with the bake / revert `ModifyLand` actions.

Reference (Firestorm, read-only): `lltoolbrushland` (`LLToolBrushLand`); the
`ModifyLand` message (bake / revert brush kinds).

Builds on: `terrain.rs` and `sl-terrain`.

Deps: [[viewer-terrain-edit-brushes]].
