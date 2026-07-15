---
id: viewer-pbr-terrain
title: PBR terrain
topic: viewer
status: ideas
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The newer, material-based terrain: instead of the classic four **diffuse**
textures splatted by elevation, a region can assign four **PBR materials**
(albedo / normal / metallic-roughness / emissive) to the terrain corners, giving
regions proper lit, normal-mapped ground. We render the classic 4-texture splat
(P2); this is the PBR path alongside it, and it is increasingly what modern
regions use.

Firestorm gates it on `RenderTerrainPBREnabled`, with triplanar / planar
sampling (`RenderTerrainPBRScale`, `RenderTerrainPBRPlanarSampleCount`,
`RenderTerrainPBRTriplanarBlendFactor`), normal-map
(`RenderTerrainPBRNormalsEnabled`) and per-material transform
(`RenderTerrainPBRTransformsEnabled`) controls.

Scope: ingest the region's terrain **material** assignment (the PBR-terrain
region info, distinct from the legacy texture-corner assignment), fetch the four
material sets, and blend them per the existing elevation/noise weights but
through the PBR lighting model — with triplanar sampling so steep slopes do not
stretch. Fall back to the classic splat when a region provides only textures.

Reference (Firestorm, read-only): the PBR-terrain shader path, the
`RenderTerrainPBR*` settings; the region terrain-material composition.

Builds on: P2 terrain (the heightfield, patches and blend weights) and P27 GLTF
PBR materials (the lighting model and material fetch).
