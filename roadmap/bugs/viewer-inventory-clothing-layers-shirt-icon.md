---
id: viewer-inventory-clothing-layers-shirt-icon
title: All clothing layers show the shirt icon instead of per-type icons
topic: viewer
status: bugs
origin: user report (2026-07-31, aditi live testing)
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

In the inventory panel every **clothing** wearable renders the **shirt** icon
regardless of its actual wearable type — pants, shoes, socks, gloves, jacket,
skirt, undershirt, underpants, alpha, tattoo, universal, etc. all show the same
shirt glyph instead of their distinct per-type icons (the reference viewer has a
separate icon per `WearableType`).

## Where to look

- `inventory.rs` — the item-icon selection (the function mapping an item's
  inventory / wearable type to its display glyph; the folder-type icon table is
  near `FolderType::CurrentOutfit | FolderType::Outfit => "\u{1f454}"`). The
  clothing branch likely returns a single shirt glyph for all
  `InventoryType::Wearable` clothing rather than switching on the item's
  `WearableType` (via `wearable_type_of` / the flags subtype).
- Body parts (Shape / Skin / Hair / Eyes) vs clothing: confirm body parts get
  their own icons and only the clothing sub-types collapse to shirt.
- Provide a per-`WearableType` icon mapping (emoji glyph or a skin icon asset)
  mirroring the reference's `LLInventoryIcon` clothing sub-type table.

## Verify

In the viewer, view a set of mixed clothing layers (shirt, pants, shoes, alpha,
tattoo, …) and confirm each shows a distinct, type-appropriate icon.
