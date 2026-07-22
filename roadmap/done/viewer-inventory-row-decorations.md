---
id: viewer-inventory-row-decorations
title: Inventory row decorations (permissions, worn-bold)
topic: viewer
status: done
origin: reference-viewer parity notes on viewer-inventory-folder-tree (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The per-row detail the reference viewer shows that the first folder-tree cut
([[viewer-inventory-folder-tree]], done) does not:

- **Permission flags after the item name** — the copy / modify / transfer (and
  next-owner) state, drawn trailing the label. `ItemInfo.permissions`
  (`Permissions5`) already carries this; this is the display.
- **Worn items in bold** — an item currently worn (in the COF / worn set) is
  drawn bold, so the outfit stands out in the tree. The worn set is already
  tracked (`InventoryModel`, COF contents + `AgentWearables`).

Both are additions to the `bind_rows` row rendering in `inventory.rs`.

Reference (Firestorm, read-only): `llinventoryitemslist`, `llinventorybridge`.
