---
id: viewer-saved-asset-reopens-stale
title: A saved notecard or script re-opened as it was before the save
topic: viewer
status: done
origin: found live-checking the keyed notecard editor on the local grid
  (2026-09-06)
refs: [viewer-notecard-editor, viewer-keyed-floater-audit,
  viewer-new-notecard-unreadable-on-opensim]
---

Context: [context/viewer.md](../context/viewer.md).

Editing a notecard and pressing **Save** reported "Saved" — truthfully — and
then re-opening it showed the text gone. The same held for a script.

## The save was never the problem

The grid had everything right. Its asset store held the saved text, in a valid
container, and its inventory item pointed at it:

```text
$ sqlite3 Asset.db "select cast(Data as text) from assets where UUID='e948759b-…'"
Linden text version 2
{ LLEmbeddedItems version 1 { count 0 } Text length 11
Hello World}
$ sqlite3 inventory.db "select inventoryName, assetID from inventoryitems …"
New Note|e948759b-…
```

The **viewer's** `InventoryModel` was the stale one. A save writes a *new*
asset and the grid rebinds the item to it; the model kept the item's old asset
id, and the editor opens what the model says — so every re-open fetched the
pre-save asset. On OpenSim that is the one-byte placeholder a new notecard is
created with ([[viewer-new-notecard-unreadable-on-opensim]]), which is why the
text looked lost rather than merely one revision old.

## The fix, and the false start worth remembering

`InventoryModel::rebind_asset` points a loaded item at a new asset (and names
its folder, so the folder is re-queried for whatever else the save changed).

The first attempt drove it from the **upload reply's** `new_inventory_item`,
and did nothing: OpenSim's `UpdateNotecardAgentInventory` answers with the new
asset **alone**. (The editor's own save-report already hinted at this — it
treats a `None` item id as "ours".) So the rebinding is driven from the
**editor**, which knows the item it saved without being told, and the
reply-driven path is kept for the savers that do name an item. Task-inventory
items are deliberately excluded: those live in an object's contents, not the
inventory model.

## How to verify

Headless: `a_saved_item_is_rebound_to_its_new_asset` (the model follows a save
and leaves an unknown item alone). Live (local grid, 2026-09-07): type, Save,
close, re-open — the text comes back, and the grid shows one fresh asset per
save.
