---
id: viewer-p2-2
title: Height-blended texture splatting
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 2 — Terrain
---

Context: [context/viewer.md](../context/viewer.md).

**P2.2. Height-blended texture splatting.** Replace the flat ground
material with the real Second Life terrain shading: the region's four
`TERRAIN_TEXTURE_*` UUIDs and per-corner low/high elevation ranges (from the
`RegionHandshake` / region info), blended by elevation with a Perlin-noise
transition band (Firestorm `llvosurfacepatch` / terrain shaders,
`llvlcomposition` for the CPU reference). Factor the Bevy-free blend-weight
math into a new **`sl-terrain`** crate (mirroring `sl-prim` / `sl-mesh`), with
the `StandardMaterial`/custom material living in `sl-client-bevy`; fetch the
four textures through the existing texture pipeline. **Done (GPU path):**
`sl-terrain` emits a per-vertex four-component blend weight; a custom
`TerrainMaterial` (`AsBindGroup`, four detail-texture bindings) +
`terrain.wgsl` in `sl-client-bevy` (behind a new `bevy_pbr` feature the viewer
enables) blends the four live textures on the GPU with the interpolated
weights + simple sun lighting. `RegionIdentity` gained a
`terrain: RegionTerrainComposition` field.
