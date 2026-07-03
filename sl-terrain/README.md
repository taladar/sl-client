# sl-terrain

Pure **terrain texture-splat blend-weight math** for Second Life / OpenSim
clients — the height-blended shading counterpart of `sl-prim` (prim
tessellation) and `sl-mesh` (LLMesh decode). Given a region's four
`TERRAIN_TEXTURE`/detail corner elevation parameters and a ground point's
elevation, it computes the four-component **blend weight** (one weight per
detail texture) that selects and blends the region's four ground textures by
altitude, with a Perlin-noise transition band so the boundaries between the
four textures wobble naturally rather than following flat contour lines.

This crate is deliberately **Bevy-free and I/O-free**, mirroring `sl-prim` /
`sl-mesh` / `sl-texture`: it consumes plain numbers and produces a plain
`[f32; 4]` weight. The GPU side — a `TerrainMaterial` that samples the four
detail textures and blends them by these per-vertex weights — lives in
`sl-client-bevy`, at the rendering boundary.

The blend follows Firestorm's `indra/newview/llvlcomposition.cpp`
(`LLVLComposition::generateHeights`) and the terrain shaders
(`llvosurfacepatch` / `pbrterrainUtilF`), reimplemented idiomatically rather
than copied: the four per-corner start-height and height-range values are
bilinearly interpolated across the region, an elevation-plus-noise value is
scaled into the `[0, 3]` detail-texture index range, and that scalar is
resolved into a normalised four-weight linear blend between the two adjacent
detail textures.

The Perlin noise is a self-contained gradient noise in `f32` (no permutation
table indexing), matching the low-frequency plus turbulence structure of the
viewer's terrain noise in character rather than bit-for-bit.
