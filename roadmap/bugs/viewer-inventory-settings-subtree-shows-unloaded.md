---
id: viewer-inventory-settings-subtree-shows-unloaded
title: The Library root and its Environments subtree never read as loaded in the viewer model
topic: viewer
status: bugs
origin: protocol-ais3-library-cap verification on aditi (2026-10-04)
refs: [protocol-ais3-library-cap]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

With every folder paged (a search query sweeping the inventory), the viewer's
`InventoryModel` holds all 490 Library folders and their items, but 32 of
them — the `Library` root and the whole `Library/Environments` subtree — keep
`FolderState` not loaded, although 29 of them carry their items (the 475
environment settings). The same 32 on both fetch roads (AIS3 and the
descendents capabilities), so it is the viewer side, not the fetch.

## Suspect

`settings_index` eagerly fetches the Library's Environments subtree for the
settings combos on a path of its own; the pages it folds in may set the items
without the folder state, and the root is only ever known from the skeleton.

## Fix

Find what marks a folder loaded in the projection and make the settings
index's path (and the root's) do it; read back with `sl-viewer-ctl inventory
--library --wait-loaded 900` after a search sweep: no unloaded folders.
