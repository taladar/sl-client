---
id: prim-path-curve-byte-ignores-reference-mask
title: PathCurve::from_byte matches the exact byte where the reference masks 0xf0
topic: viewer
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-prim/src/shape.rs` `PathCurve::from_byte` matches the whole byte
(`0x20 => Circle`, `0x30 => Circle2`, `0x80 => Flexible`, else `Line`). The
reference switches on `getCurveType() & 0xf0` (`llvolume.cpp` ~1433, ~1493)
and has an `LL_PCODE_PATH_TEST` (0x40) case.

## Effect

A path byte with low bits set (e.g. 0x21) renders as a line instead of a
circle; 0x40 renders as a line instead of the five-point test path. Rare in
practice, silent when it happens.

## Fix

Mask with 0xf0 in `from_byte`, add the test path, and keep the raw byte where
round-tripping must stay lossless (`sl-object-asset`'s bridge — see
[[idiomatic-prim-flags-and-object-codes]]). Unit-test the masked decode.
