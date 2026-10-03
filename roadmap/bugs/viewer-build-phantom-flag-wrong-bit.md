---
id: viewer-build-phantom-flag-wrong-bit
title: Build floater's Phantom checkbox reads and writes the any-owner bit
topic: viewer
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns, viewer-prim-parameter-editing]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-viewer-edit/src/edit_params.rs` declares its own `PrimFlags` bits:
`FLAGS_PHANTOM = 1 << 4` and `FLAGS_CAST_SHADOWS = 1 << 1`. The reference
`object_flags.h` has `FLAGS_PHANTOM = 1 << 10`; bit 4 is
`FLAGS_OBJECT_ANY_OWNER` and bit 1 is `FLAGS_CREATE_SELECTED` (the old
cast-shadows bit was 23, now unused, and the reference always sends
`CastsShadows = false`). `sl-proto` (`PrimFlags::PHANTOM`) and
`sl-viewer-world-api/src/object_flags.rs` both have the right value.

## Effect

Both grids set the any-owner bit on owned objects, so the Phantom checkbox
shows ticked for every owned object, and the `ObjectFlagUpdate` built from
the checkbox states (`edit_params.rs` ~3201-3214) sends `IsPhantom` from the
wrong bit: toggling Physical or Temporary makes an owned object phantom,
Phantom itself can never be set, and a really phantom object is un-phantomed
by any other toggle. `CastsShadows` goes out from the create-selected bit.

## Fix

Use the one `PrimFlags` definition (see
[[idiomatic-prim-flags-and-object-codes]] for removing the four copies
entirely); send `CastsShadows = false` as the reference does. Add a unit test
over the flag-update builder with an owned (any-owner set) non-phantom object.
Live-check on the local grid: toggle Physical on an owned box, confirm it stays
solid; toggle Phantom, confirm it becomes phantom.
