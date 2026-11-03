---
id: viewer-build-phantom-flag-wrong-bit
title: Build floater's Phantom checkbox reads and writes the any-owner bit
topic: viewer
status: done
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

## Done (2026-11-03)

Every `PrimFlags` bit the viewer reads now comes from
`sl-viewer-world-api::object_flags`, which gained `FLAGS_CREATE_SELECTED`,
`FLAGS_OBJECT_TRANSFER` and `FLAGS_TEMPORARY_ON_REZ`. The five local copies
are gone: the two wrong ones in `edit_params.rs` (phantom at bit 4, cast
shadows at bit 1) and the three right-valued duplicates in `edit_params.rs`
(transfer), `edit_create.rs` (create-selected), `object_menu.rs` (copy,
you-owner) and `media_controls.rs` (you-owner). Removing the `sl-proto` and
`sl-object-asset` copies is still [[idiomatic-prim-flags-and-object-codes]].

The `ObjectFlagUpdate` comes from one pure `object_flag_settings(flags)`,
built the way `LLViewerObject::updateFlags` builds it: physics, temporary and
phantom from their own bits, and `CastsShadows` always `false`.
`a_flag_toggle_changes_only_its_own_field` drives it with an owned box flagged
as both grids flag one (any-owner, you-owner, the grants, create-selected):
nothing reads as set, Physical sets only physics, Phantom sets only phantom,
and Temporary on a phantom box keeps it phantom.

The automation can now see these flags: `WorldNode::behaviour` (an
`ObjectBehaviour` of `physical` / `phantom` / `temporary`, left out of the
JSON when none is set) is filled from the object's flag word, so a stage
reads what the grid kept rather than what the window shows.

## Verified

- `cargo nextest run --release -p sl-viewer-edit -p sl-viewer-ui-context-menus
  -p sl-viewer-world-view -p sl-viewer-world-api` (477 green) and
  `-p sl-automation-proto -p sl-viewer-automation` (161 green, including
  `a_behaviour_names_only_what_is_set`).
- `e2e_objects` `each_behaviour_checkbox_sets_its_own_flag` rezzes a prim,
  checks all three boxes read unticked, then ticks and unticks Phantom,
  Temporary on top of it, and Physical. After each one it waits for the
  grid's answer and reads the prim's behaviour, plus the grid's own record on
  the fake grid. It passes on both fake flavours, on the local OpenSim and
  on aditi, in-process and as a separate process. With the old bit 4 put
  back it fails at once ("a fresh prim shows Phantom ticked").
- The live runs turned up two faults in the file's shared helpers, both
  fixed. `arrived()` waited for the region only, so a fast process viewer
  reached a ground rez before it knew its own position ("Alpha has no
  position" on aditi). It now also waits for the position and the camera
  eye. And `sweep_leftovers` gave up the whole stage when its delete
  right-click could not reach a leftover prim that a select had reached. It
  now leaves that prim, as it already did for one it could not select.
