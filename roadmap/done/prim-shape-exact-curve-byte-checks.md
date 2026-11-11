---
id: prim-shape-exact-curve-byte-checks
title: Tessellation shortcuts compare decoded curve kinds where the reference compares whole bytes
topic: viewer
status: done
origin: prim-path-curve-byte-ignores-reference-mask (2026-11-09)
refs: [prim-path-curve-byte-ignores-reference-mask]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`LLPath::generate` and `LLProfile::generate` decode masked nibbles. Several
shortcuts in `llvolume.cpp` instead compare the **whole** byte with `==`:

- `LLVolume::generate`: the split is `0` only when the path byte is exactly
  `LL_PCODE_PATH_LINE` and the **full profile byte, hole bits included**, is
  exactly square / iso / equal / right triangle (~2064). The LOD scale bias
  makes the same exact checks for a cylinder and a circle path (~2084).
- `LLVolumeFace::createCap`: the uncut-box cap needs the profile byte to be
  exactly `LL_PCODE_PROFILE_SQUARE` and the path byte exactly
  `LL_PCODE_PATH_LINE` (~6164).

`sl-prim`'s `split_for` and `is_uncut_cube` (`volume.rs`) test the decoded
`PathCurve` / `ProfileCurve` instead. `PrimShape` keeps only the decoded kinds,
so the raw bytes are gone by then.

## Effect

Take a hollow box whose profile byte names its hole, such as `0x21` (square,
square hole). The reference splits its edges and we do not, so the vertex
counts differ. A path byte with low bits set (`0x11`) differs the same way. The
cap shortcut needs a solid prim, so a hole byte on an otherwise solid box
changes the cap there as well. Check whether we have the LOD bias at all.

## Fix

Carry the raw curve bytes far enough for these shortcuts to compare them the
way the reference does. That could be the bytes themselves on `PrimShape`, or
a typed "exactly the canonical byte" flag on each curve. Then unit-test the
split and the cap for a hole-carrying profile byte.

## Fixed (2026-11-09)

- `PrimShape::from_params` (`sl-prim/src/shape.rs`) first constrains the curve
  bytes as `LLVolumeParams::setType` does on every unpack: a bad profile
  nibble becomes a square with **no hole** (`0x26` used to decode a square
  hole), a bad hole nibble no hole, and a path nibble outside `1..=8` an exact
  line. The new `path_byte_form: PathByteForm` records whether the kept path
  byte was exactly its curve's (`Exact`) or only decodes to it (`Variant`:
  `0x11`, `0x50`, `0x81`). After constraining, a profile byte is exactly its
  curve's when the hole is `Same`, so no extra state is needed there.
- `PrimShape::path_is_exactly` / `profile_is_exactly` are the reference's `==`
  tests. `split_for`, `is_uncut_cube` and the half-circle pole normals in
  `volume.rs` (`createSide` matches the path byte whole) use them.
- The LOD scale bias was missing altogether. The viewer ranked every volume by
  its unweighted scale length, the reference's fallback for an object with no
  volume, so prims, sculpts and meshes all took a finer level than the
  reference at a given distance. `LodScaleBias` (`sl-prim/src/lod.rs`) carries
  `mLODScaleBias`: `0.5` per axis by default and for every mesh, `(0.6, 0.6, 0)`
  for an exact cylinder, `0.6` per axis for an exact circle path
  (`PrimShape::lod_scale_bias`). `render_priority.rs`'s `lod_radius` feeds it to
  `PrimLod::for_distance` / `MeshLod::for_distance`. Trees keep the scale length
  (`LLVOTree` is not a volume). Rigged attachments still render at the finest
  level, and the reference's own radius for them (the wearer's animation
  extents) is noted on [[viewer-perf-avatar-mesh-lod-screen-size]].
- Pinned by `curve_bytes_are_constrained_like_set_type`,
  `exact_curve_predicates_compare_whole_bytes`,
  `lod_scale_bias_follows_the_whole_curve_bytes`,
  `lod_radius_weights_the_scale_per_axis`,
  `box_cap_quad_needs_exact_curve_bytes`, `split_drop_needs_exact_curve_bytes`
  and `lod_radius_weighs_the_scale_by_the_volume_shape`.
