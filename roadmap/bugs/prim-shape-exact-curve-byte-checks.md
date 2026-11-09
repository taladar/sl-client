---
id: prim-shape-exact-curve-byte-checks
title: Tessellation shortcuts compare decoded curve kinds where the reference compares whole bytes
topic: viewer
status: bugs
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
