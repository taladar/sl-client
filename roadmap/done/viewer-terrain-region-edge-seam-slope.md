---
id: viewer-terrain-region-edge-seam-slope
title: Terrain seam / misalignment at region edges on slopes
topic: viewer
status: done
origin: user observation (2026-08-02), while reviewing viewer-parcel-borders-render
---

**Done (2026-08-02).** Fixed in `sl-client-bevy-viewer/src/terrain.rs`. Two
distinct bugs in `sample_height` produced the sloped-border seam:

1. The shared far-edge sample only ever looked up patches within the **same
   region**, so at a region border the north / east edge could never reach the
   neighbouring region's real edge height — it fell back to this region's own
   clamped edge (a flat strip), stepping on a slope.
2. The missing-neighbour fallback clamped **both** axes to `(size-1, size-1)`.
   This is what produced the visible per-chunk corner **fold**: an interior
   north-edge patch's *east* far-edge column correctly follows the in-region
   east neighbour's rising slope, but its north-east corner collapsed to the
   patch's own low corner instead of extending that height northward — so the
   corner folded down even on a genuinely void top edge (confirmed in-world on
   the local 2×2 grid's NE region north edge, a slope rising east).

Fix: `resolve_axis` computes whether a far-edge sample belongs to the next
patch in-region or the adjacent region's patch 0 (region handle shifted by
256 m); `resolved_height` looks it up in the right region. When no neighbour is
loaded, `sample_height` now **flat-extends along only the missing axis**,
keeping whichever far edge does resolve, so the corner tracks the sloped edge.
`rebuild_neighbours` crosses region boundaries too (`step_back`), so a border
seam closes as the neighbouring region streams in. Vertical placement already
agreed (every patch transform sits at `z = 0` with absolute heights baked into
the vertices), so no datum change was needed. Three unit tests pin the
before/after (`far_edge_stitches_across_region_border`,
`void_north_edge_corner_does_not_fold`, `step_back_crosses_region_boundary`).

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

Repro: the OpenSim slope test scene (see the
`sl-client-opensim-slope-test-scene` progress memory) has a hill near spawn;
view a region border that crosses the slope (a 2×2
local region layout), or any sloped sim crossing on aditi.
