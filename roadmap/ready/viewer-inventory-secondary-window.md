---
id: viewer-inventory-secondary-window
title: Secondary inventory windows
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-inventory-floater-menus, viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

Avatar ▸ New Inventory Window: open additional independent inventory
floaters, each with its own filter/scroll state — the standard way to
drag items between two folders far apart in the tree. Explicitly listed
as unimplemented in [[viewer-inventory-floater-menus]] ("New Inventory
Window (no multi-window)").

Scope:

- Allow N concurrent inventory floaters over the shared inventory model;
  each window owns its filter text, expansion, and scroll state.
- Drag-and-drop between two inventory windows.
- Window lifecycle: menu entry + gear-menu entry; closing a secondary
  window never disturbs the primary.

Reference (Firestorm, read-only): `Inventory.NewWindow`
(`menu_viewer.xml` Avatar section), `llfloaterinventory` multi-instance
support.

Builds on: the inventory folder tree and floater menus (done); mainly a
UI-instancing refactor (per-window view state instead of a singleton).

## Parity-audit addendum (2026-08-19)

Include the FS **inventory settings** folder-open-mode toggles
(`floater_inventory_settings.xml`): single- vs multi-folder double-click
open modes for inventory windows, and the "find original opens a new
window" toggle. (The same floater's favorites-star display options are
separate scope — the new item-favorites idea task.)

Firestorm's secondary windows are **folder-rooted**
(`fsfloaterpartialinventory.cpp`): the folder context menu's "open
folder in new window" spawns an inventory floater rooted at that
starting folder, showing only that subtree — not just a second
full-tree window. Add the rooted variant and the folder context-menu
entry to this task's scope.
