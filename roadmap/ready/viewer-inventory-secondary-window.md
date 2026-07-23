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
