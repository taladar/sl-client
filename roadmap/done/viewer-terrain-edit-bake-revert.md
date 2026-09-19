---
id: viewer-terrain-edit-bake-revert
title: Terrain editing — bake / revert
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-terrain-editing
blocked_by: [viewer-terrain-edit-brushes]
---

Done (2026-09-19). The task's premise was half wrong: `ModifyLand` has no bake
action. **Revert** is one of the six brushes and came with
[[viewer-terrain-edit-brushes]] (both under the cursor and over a selection,
the latter at the reference's fixed 0.5 s). **Bake** is region-wide and rides
`EstateOwnerMessage`/`terrain` `["bake"]`, which had a `SimSession` decode but
no client method: `Session::bake_region_terrain` and
`Command::BakeRegionTerrain` are new, and the button sits in the Region /
Estate floater's Terrain tab where the reference keeps it (which closes that
half of [[viewer-region-options-terrain]]'s parity addendum).

**Not verified live**: bake is estate-owner gated and the simulator answers a
successful one with silence, so proving it takes a bake, a raise and a revert
on an owned region.

Context: [context/viewer.md](../context/viewer.md).

Bake the current terrain as the new revert baseline, and revert the terrain back
to the last bake, over a selected land area. Extends the brush tooling from
[[viewer-terrain-edit-brushes]] with the bake / revert `ModifyLand` actions.

Reference (Firestorm, read-only): `lltoolbrushland` (`LLToolBrushLand`); the
`ModifyLand` message (bake / revert brush kinds).

Builds on: `terrain.rs` and `sl-terrain`.

Deps: [[viewer-terrain-edit-brushes]].
