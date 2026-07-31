---
id: viewer-inventory-show-in-main-from-worn-recent
title: '"Show in Main view" inventory menu action from the Worn / Recent tabs'
topic: viewer
status: ready
origin: user request (2026-07-31, aditi live testing)
---

Context: [context/viewer.md](../context/viewer.md).

The inventory floater's **Worn** and **Recent** tabs are filtered/flat views
of items that live elsewhere in the folder tree. Add a **"Show in Main view"**
(reference viewer: *Show in Main Panel* / *Find Original*) context-menu action
on an item in those tabs that switches back to the main (folder-tree) inventory
view and reveals + selects the item in its real parent folder — expanding the
folder path to it and scrolling it into view.

## Where to look

- The inventory context-menu action table (`inventory_actions.rs` — where the
  per-item menu entries are built and dispatched).
- The Worn / Recent tab views and how they enumerate items
  (`inventory.rs` / `inventory_gallery.rs`).
- The main folder-tree view's selection + reveal/scroll-to-item plumbing (what
  a normal folder-tree selection does), so the action can drive it to the item's
  parent folder.

## Verify

In the viewer, open the Worn (and Recent) inventory tab, right-click an item,
choose "Show in Main view", and confirm the panel switches to the folder-tree
view with the item's parent folder expanded and the item selected / scrolled
into view.
