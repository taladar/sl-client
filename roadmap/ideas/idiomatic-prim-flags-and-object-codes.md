---
id: idiomatic-prim-flags-and-object-codes
title: One PrimFlags type and typed object codes past decode
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  viewer-build-phantom-flag-wrong-bit,
  prim-path-curve-byte-ignores-reference-mask]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`PrimFlags` bits are defined four or five times (`sl-proto`
`prim_flags`, `sl-viewer-world-api/src/object_flags.rs`,
`sl-viewer-kit/src/minimap_math.rs`, `edit_params.rs`, `edit_create.rs`,
a copy in `media_controls.rs`, bare `1 << n` in `hover_tooltip.rs`, one in
ui-context-menus) and applied as raw `u32` masks on `TrackedObject.
update_flags`, `ObjectPickSummary.flags`, snapshot flags and the minimap — the
duplication that produced [[viewer-build-phantom-flag-wrong-bit]].

Object wire codes stay `u8` past ingest though sl-proto has the enums:
click action (`double_click_teleport.rs` re-declares `CLICK_ACTION_SIT` /
`_DISABLED`), material (`world_sounds.rs` matches raw `0..=6`), attachment
point (`Option<u8>`, `HashMap<u8, Entity>`), `pcode` plus a `state` byte whose
meaning depends on it (species vs nibble-swapped attachment point), sound
flags, sculpt type (`LL_SCULPT_TYPE_*` re-declared in four crates), profile /
path / hole curve bytes (hand-written in `edit_create.rs` and
`sl-object-asset`'s opensim encoder). Texture-entry bytes are read through
sl-proto getters but written with raw masks (`edit_texture.rs` fullbright /
bump / shiny / texgen, `edit_media.rs` media flag).

## How

One `bitflags! PrimFlags` in sl-proto as the field type everywhere; ingest
into `ClickAction`, `Material`, `AttachmentPoint`, an `ObjectKind { Prim {
attachment }, Avatar, Tree { species }, Grass { species }, … }`, a shared
`SculptType` + flags; enums with an `Unknown(u8)` variant where a lossless
round-trip matters (`sl-object-asset`). `TextureFace` setters with `Bump` /
`Shininess` / `TexGen` enums.
