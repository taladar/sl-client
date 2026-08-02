---
id: viewer-terrain-region-edge-seam-slope
title: Terrain seam / misalignment at region edges on slopes
topic: viewer
status: ready
origin: user observation (2026-08-02), while reviewing viewer-parcel-borders-render
---

Context: [context/viewer.md](../context/viewer.md).

On a slope, terrain patches at the **edge of a region** don't line up
properly with the neighbouring region's terrain — the two regions' terrain
meshes meet with a visible step / gap / misalignment along the shared border
where the ground is sloped (flat ground looks fine). Spotted while reviewing
the in-world parcel borders ([[viewer-parcel-borders-render]]), which drape
over the same heightfield and made the mismatch visible along the region rim.

Likely area: terrain patch meshing / seam stitching in `terrain.rs`. Each land
patch owns `size`×`size` samples but spans `size` metres and borrows one shared
boundary sample from its north / east neighbour to close the seam
(`build_patch_mesh` / `sample_height`). At the **region** boundary the shared
sample comes from *another region's* patches, which may not be loaded, may use
a different origin offset, or the cross-region neighbour lookup may be missing
— so the edge row/column falls back to this region's own clamped edge (a flat
strip) instead of the neighbour's real height, stepping on a slope. Confirm
whether `sample_height` reaches across the region boundary and whether the two
regions' vertical placement agrees at the seam.

Investigate:

- Does `sample_height` fetch the neighbour **region's** boundary sample at a
  region edge, or only same-region patches? (The doc note says a patch at the
  region's own far edge keeps a flat 1 m strip because "its neighbour is in
  another region".)
- Are the two regions' terrain meshes at exactly the same world height along
  the shared edge, given each region's south-west-corner offset?
- Reference: `LLSurfacePatch` / `LLSurface` edge stitching
  (`updateNorthEdge` / `updateEastEdge`, `calcDrawInfo`).

Repro: the OpenSim slope test scene ([[sl-client-opensim-slope-test-scene]])
has a hill near spawn; view a region border that crosses the slope (a 2×2
local region layout), or any sloped sim crossing on aditi.
