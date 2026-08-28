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

The Perlin noise is the one part that is **not** idiomatic but a verbatim port,
tables and all: it is Firestorm's `indra/newview/noise.{h,cpp}` — the classic
256-entry permutation and gradient tables under a cubic ease — because its exact
values decide where one ground texture gives way to the next. Anything else,
however well-shaped, would put the transition bands somewhere the region's map
tile and every other viewer do not. The reference builds those tables at run
time from a fixed-seed `rand()`, so they are baked in here; the unit tests check
both the raw noise and whole composition values against samples taken from the
reference's own code.
