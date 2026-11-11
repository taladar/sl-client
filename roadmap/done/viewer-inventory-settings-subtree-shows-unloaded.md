---
id: viewer-inventory-settings-subtree-shows-unloaded
title: The Library root and its Environments subtree never read as loaded in the viewer model
topic: viewer
status: done
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

## Cause (2026-11-11)

The viewer's `InventoryModel` only ever learnt a folder's `FolderState` as a
*child*: from the login skeleton or from its parent's
`InventoryFolderPage`, carrying the state the session held at that moment. A
page reported nothing about the paged folder itself. So the Library root,
which has no parent page and comes from the `LibraryInventory` skeleton as
`Unknown`, never read as loaded; and the settings index's eager fetch pages
`Environments` and then each of its sub-folders while they are still being
fetched, after which the parent is never paged again, so every sub-folder kept
the `Unknown` / `Fetching` its parent's first page reported, items and all.
Both fetch roads end in the same page event, which is why AIS3 and the
descendents capabilities agreed.

## Fixed (2026-11-11)

`InventoryFolderPage` gains `state: Option<FolderState>`, the paged folder's
own state, read after the on-demand fetch the query schedules (both runtimes).
The viewer folds it in through `InventoryModel::set_folder_state`, a `Loaded`
state bringing its version along. A folder's own page always follows its fetch
(the `InventoryDescendents` re-query), so every folder the viewer pages ends
up reading as the session holds it.

Verified on aditi (2026-11-11, fresh run directory, so nothing came from a
cache): after a search sweep, `sl-viewer-ctl inventory --library
--wait-loaded 900` read 490 Library folders and 5581 items with **no**
unloaded folder (32 before), and the agent tree's 41 folders likewise. The
unit test `a_page_reports_its_own_folders_state` fails with the fold-in
removed; `b8_inventory_pull_bridge_synthesizes_page_and_roots` pins the
session's side.
