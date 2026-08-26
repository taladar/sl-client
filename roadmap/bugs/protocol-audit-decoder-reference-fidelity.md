---
id: protocol-audit-decoder-reference-fidelity
title: Four decoder divergences from the reference viewer
topic: protocol
status: bugs
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
