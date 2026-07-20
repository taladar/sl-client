---
id: viewer-inventory-floater-menus
title: Inventory floater menus (gear options + create/New menu)
topic: viewer
status: ready
origin: follow-up noted while shipping viewer-ui-menu-search (2026-07-20)
blocked_by: [viewer-ui-menu-bar]
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory floater's own **menus** — the drop-downs in its toolbar, on
[`crate::menu`]'s reusable widget (the same one the top bar and the gear button
already use). The gear menu shipped as a stub with the menu-bar work
(`INVENTORY_GEAR_MENU` in `inventory.rs`: expand / collapse plus a
`(more options soon)` placeholder); this task fleshes the floater's menus out to
the reference set:

- **Gear / options menu** (the ⚙ button, `menu_inventory_gear_default`) — the
  window options: sort by name / by date, sort folders always to the top,
  show filters, show the item count / empty system folders, and "new window".
  Several of these are **view/sort state** on the floater, so this pairs with a
  small sort/filter read-model the entries toggle (some already exist as
  `InventoryUiAction`s; the rest are new).
- **Create / "New" menu** (the reference's **+** button, `menu_inventory_add`) —
  create an inventory item in the selected folder: New Folder, New Script, New
  Notecard, New Gesture, and the New Clothes / New Body Parts submenus (shirt,
  pants, … / shape, skin, hair, eyes). Each is a create action against the
  inventory model; wiring the ones the model can already create and leaving the
  rest as declared-but-unhandled entries (the bar's own pattern) is fine.

Builds directly on the line-menu widget and the gear-button placement that
already exist, so the work is authoring the `MenuDef`s and routing their
`UiAction`s to inventory operations — not new UI mechanism. The per-**item**
right-click actions (wear / rez / delete / rename, drag-and-drop) are a separate
concern, already covered by [[viewer-inventory-context-actions]]; this task is
the floater-level toolbar menus, not the item context menu.

Reference (Firestorm, read-only): `menu_inventory_gear_default.xml`,
`menu_inventory_add.xml`, `indra/newview/llpanelmaininventory.{h,cpp}`.
