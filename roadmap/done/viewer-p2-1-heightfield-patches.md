---
id: viewer-p2-1
title: Heightfield patches
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 2 — Terrain
---

Context: [context/viewer.md](../context/viewer.md).

**P2.1. Heightfield patches.** On `TerrainPatch`, build a mesh for the
patch (grid of cells at `values[..]`, computed normals, whole-region UVs)
placed at its `patch_x * size, patch_y * size` origin (`sl_to_bevy`); keep a
`HashMap<(patch_x, patch_y), Entity>` and replace on update. One flat
`StandardMaterial` (no splatting). Verify terrain renders on OpenSim.
