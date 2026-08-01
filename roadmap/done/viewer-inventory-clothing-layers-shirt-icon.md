---
id: viewer-inventory-clothing-layers-shirt-icon
title: All clothing layers show the shirt icon instead of per-type icons
topic: viewer
status: done
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

## Resolution

`inventory.rs` now routes every item icon through a new
`item_glyph(inv_type, flags)`: for an `InventoryType::Wearable` it reads the
wearable sub-type from the low byte of the item flags (LL's
`II_FLAGS_SUBTYPE_MASK`) and returns a per-`WearableType` glyph from a new
`wearable_icon()` table — a distinct emoji for each body part
(shape/skin/hair/eyes) and every clothing layer
(shirt/pants/shoes/socks/jacket/gloves/undershirt/underpants/skirt/alpha/tattoo/
physics/universal), mirroring `LLInventoryIcon`'s clothing / body-part
sub-tables. Wired through the main tree (`decorated_item_row` reads
`item.flags`), the Worn tab (COF-link target flags, and the legacy
`AgentWearables` fallback which carries the `WearableType` directly), and the
Recent tab (`RecentItem` gained a `flags` field). Body parts already had their
own path and are unaffected. Guarded by the `wearable_icons_are_per_type` unit
test. Live-verify on aditi against a mixed outfit still pending a login session.
