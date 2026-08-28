---
id: protocol-audit-decoder-reference-fidelity
title: Four decoder divergences from the reference viewer
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

Four places where a decoder is internally consistent but does not match the
reference. None is a crash; each produces visibly different output.

- **`sl-sculpt/src/tessellate.rs:47` — sculpt LOD is hardcoded to the maximum.**
  `WORKING_SUBDIVISIONS: usize = 32`, "matching Firestorm's highest sculpt LOD,
  `SCULPT_REZ_4`", used unconditionally. The reference selects among
  `SCULPT_REZ_1..4` (4/8/16/32) from the object's volume detail, so every sculpt
  in the scene is tessellated at 33x33 = 1089 vertices regardless of distance —
  correct geometry at 16x the triangle budget at range.
- **`sl-material/src/decode.rs:145`, `:160` — `base_color` and `emissive_factor`
  are not clamped** while `metallic_factor`, `roughness_factor` (`:150-151`) and
  `alpha_cutoff` (`:166`) all are. A hostile `AT_MATERIAL` asset setting
  `emissiveFactor: [1e30, 1e30, 1e30]` goes through unclamped into the bloom
  path.
- **`sl-terrain/src/lib.rs:216` — `perlin2` is not the reference's noise.** The
  module docs say the algorithm "mirrors `LLVLComposition::generateHeights`,
  reimplemented idiomatically rather than copied", and the constants are
  faithfully cited — but `perlin2` is a GLSL-style `mod289` hash permutation
  (`HASH_PERIOD: f32 = 289.0`, `:70`), not `LLPerlinNoise`'s gradient table. The
  *values* therefore differ, so splat transition bands will not match SL's
  server-rendered map tiles or other viewers. Looks deliberate but is not called
  out where a reader would see it. **Not numerically diffed against
  `llperlin.cpp` — confirm before changing anything.** The test to add is a
  reference-value one: a handful of `(x, y, elevation)` to expected-weights
  samples taken from Firestorm, which is exactly the case where a synthetic
  fixture replaces a live-grid check.
- **`sl-bake/src/composite.rs:259`, `:293`, `:526`, `:577` — the "linear RGBA"
  comments are wrong.** `composite_region` calls its canvas "linear RGBA in
  `0.0..=1.0`", `luma()` is documented as Rec. 601 luma of a *linear* source,
  and `u8_from_unit_f32` as quantising a *linear* channel — but
  `LayerSampler::texel` reads 8-bit `DecodedImage` bytes and divides by 255, and
  there is no sRGB-to-linear transfer anywhere in the crate. The *behaviour*
  matches the reference's 8-bit `LLTexLayer` compositing, so this is a
  documentation bug — but given the project's explicit linear-vs-sRGB rule it is
  a live trap for the next person porting blending math. `sl-bake` also has 31
  tests and **none** asserts the body-region V flip the project treats as
  load-bearing; a 2x1 asymmetric layer composited and checked for row order
  would pin it.

Adjacent, lower priority: `sl-j2c-encode/src/lib.rs:253` hardcodes
`OPJ_CLRSPC_SRGB`, which is right for the avatar bake it exists for but wrong
for the "canonical RGBA8 images" its own description promises — worth a doc note
or an explicit parameter.

## Fixed (2026-08-28)

All four, plus the adjacent `sl-j2c-encode` note.

**Sculpt LOD is selected, not assumed.** `sl-sculpt` gained `mesh_resolution`,
a faithful port of the reference's `sculpt_calc_mesh_resolution`: the level of
detail caps the vertex budget (`SCULPT_REZ_1..4` = 6/8/16/32 per side), the map
caps it again at a vertex per four pixels, and what survives is split between
the axes in the map's own aspect ratio. The grid is therefore **non-square** —
`WORKING_SUBDIVISIONS` is gone and the builder carries separate row and column
counts — which the old fixed 32x32 could not express at all, so a 64x32 map was
being resampled square as well as at full rez.

`tessellate` / `tessellate_with` now take a `PrimLod`, and the viewer treats a
sculpt as what it is: a client-tessellated object, on the same pixel-area LOD
path as a plain prim. It starts at `INITIAL_MANAGED_PRIM_LOD`,
`drive_render_priority` writes it a `PrimLodTargets` entry (from the branch that
handles objects *with* an asset — a sculpt's asset is its map texture), and
`apply_prim_lod` re-stitches it. `ObjectBuilds` carries the inputs as
`sculpt_rebuild`, the sculpt half of a new `ClientTessellated` pair, and
`GeometryKey::Sculpt` gained the level so a LOD swap is a clean different cache
key. `apply_prim_lod` fetches the decoded map *before* despawning anything: a
map that has since left the store leaves the sculpt at the level it has rather
than despawning into nothing.

That change needed a seventh follow-up slot out of `build_object_geometry`,
which already returned a six-tuple with up to five `None`s per arm. It returns
an `ObjectGeometryBuild` struct instead, holding the `ObjectBuilds` it is
establishing directly — so the arms state what they set and default the rest,
and the two call sites hand the record straight on.

**Colour factors are clamped.** `base_color` and `emissive_factor` go through
`clamp_unit`, as the reference clamps them in `setBaseColorFactor` /
`setEmissiveColorFactor`. A `NaN` component survives, exactly as the reference's
`if (< 0) … else if (> 1) …` leaves it.

**The terrain noise is the reference's noise.** It was diffed, and it did differ
— in two ways, not one: the ease curve was the modern quintic where the
reference uses the cubic `3t² − 2t³`, and the gradients came from a `sin`-hash
where the reference indexes a shuffled 256-entry permutation into 256
precomputed unit vectors. `sl-terrain::noise` is now a verbatim port of
`indra/newview/noise.{h,cpp}`, tables included. The reference builds those
tables at run time from `rand()` under a fixed `srand(42)`, so reproducing them
would mean reproducing a C library's generator; they are baked in instead,
dumped from a verbatim extract of `noise.{h,cpp}` compiled against glibc — which
also makes our noise identical on every platform, as the reference's is not.

`LOW_FREQUENCY_SCALE` was also a digit short of the reference's
`0.2222222222f`. That looked harmless and was not: a region's global coordinates
are tens of thousands of metres, so the rounding shifted the noise lattice by
about a thousandth of a cell and moved the transition bands with it. It is what
the end-to-end test caught after the raw noise already matched.

Two reference-value tests pin it: raw `noise2` / `turbulence2` samples, and
whole `composition_value` results for a four-different-corner region, both taken
from that same extract (with `generateHeights`' per-texel math added for the
second). The second is what would catch a transposed corner or a wrong constant;
the first alone would not.

**The `sl-bake` "linear" comments were the wrong half of the pair.** The
behaviour is right — the reference's `LLTexLayer` composite is an 8-bit blend of
encoded texels — so the fix is documentation, stated once at the module level
rather than three times in passing, and stated as *why*: this crate deliberately
holds the opposite rule to the rest of the workspace, so blending math ported in
from a shader would arrive with the wrong assumption. The V flip lives in the
viewer (`composite_region_from_layers`) and is tested there; what was untested
was the premise it rests on, so `compositing_preserves_source_row_order`
composites an asymmetric 1x2 layer and pins the row order coming out. With two
flips in the chain and neither pinned, nothing downstream could say which one
was wrong.

**`OPJ_CLRSPC_SRGB` is a declaration that never reaches the file.** A raw J2K
codestream has no colour-space signalling — that is a JP2 `colr` box — and
OpenJPEG's `j2k.c` never reads `color_space`. Noted at the call site, with what
would have to change (an argument) if the crate ever grew a JP2 output or a
non-sRGB input.
