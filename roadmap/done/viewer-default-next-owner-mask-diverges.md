---
id: viewer-default-next-owner-mask-diverges
title: Three different default next-owner masks, two without PERM_MOVE
topic: viewer
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

New items get their next-owner mask from three independent raw `u32`s:
`sl-viewer-inventory/src/inventory_actions.rs` `0x0000_E000` (new notecards,
scripts, gestures, wearables, settings), `sl-viewer-asset-editors/src/
edit_wearable.rs` `0x0008_e000` (whose doc claims to match the inventory
creators — it does not), and `sl-viewer-edit/src/edit_material.rs`
`0x8000 | 0x4000 | 0x2000`. The reference `LLFloaterPerms::getNextOwnerPerms`
always starts from `PERM_MOVE` and adds copy / modify / transfer from the
per-type `NextOwner*` settings.

## Effect

Items created from inventory and materials go out without `PERM_MOVE` in the
next-owner mask, differently from wearables saved from the editor and from
the reference. Small (move mostly matters for objects) but silent.

## Fix

One `Permissions::default_next_owner(...)` on the typed `Permissions`
(`sl-wire/src/permissions.rs`), honouring the reference's per-type
`NextOwnerCopy/Modify/Transfer` settings if we have them; retype the
`Command::*::next_owner_mask: u32` fields (see
[[idiomatic-permissions-type-everywhere]]).

## Fixed (2026-11-09)

- `Permissions::DEFAULT_NEXT_OWNER` (`sl-wire/src/permissions.rs`) is the one
  default: move | copy | modify | transfer (`0x0008_e000`), what
  `getNextOwnerPerms` returns for every category under the stock settings. The
  inventory creators (scripts, notecards, gestures, settings, new wearables and
  the default wearable's own permission block), the wearable editor's Save As
  and the PBR material save all use it; the three hand-written constants are
  gone.
- The masks a client *asks for* are typed: `Command::CreateScript`,
  `Command::UploadAsset` (next-owner, group and everyone), `NewInventoryItem`,
  `NotecardRez` and `RezObjectParams` carry `Permissions`, turned into bits only
  where the wire block is built (and back on the simulator side). The
  `NewFileAgentInventoryRequest` LLSD body stays raw: it is the wire form.
- Pinned by `every_creator_grants_the_default_next_owner_mask`
  (`sl-viewer-inventory`), which runs each creator and reads the mask it sends.

The per-category `*NextOwnerCopy/Modify/Transfer` settings (and their floater)
remain [[viewer-default-creation-permissions]]; when they land, the default
becomes a function of the category and these call sites take it from there.
