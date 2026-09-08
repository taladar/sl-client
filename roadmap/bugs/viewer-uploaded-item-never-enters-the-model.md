---
id: viewer-uploaded-item-never-enters-the-model
title: An item the viewer just uploaded is not in its own inventory
topic: viewer
status: bugs
origin: found measuring the grids for test-fake-grid-imitates-upload-announcements (2026-09-08)
points: 2
refs: [test-fake-grid-imitates-upload-announcements]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-inventory` learns about a newly uploaded item only if the grid
**pushes** it. `ingest_inventory`'s `SlSessionEvent::AssetUploaded` arm calls
`rebind_saved_asset`, and that resolves through `InventoryModel::rebind_asset`,
which walks the folders it already holds and returns `None` for an item it does
not know — so a `NewFileAgentInventory` upload, whose whole point is that the
item is new, changes nothing in the model. The item is in the tree on the grid
and absent from the window until something re-fetches its folder.

Invisible until now because the fake grid announced every upload with the
legacy UDP message, and against a grid that announces, the item arrives through
the `InventoryItemCreated` path instead. That is no longer what every grid
does: OpenSim was measured (2026-09-08) sending **nothing** after either
capability upload, and `sl_fake_grid::UploadAnnouncement::Silent` now
reproduces it, so the gap is reachable offline as well as against the local
grid.

The reference viewer does not have the problem and shows the fix:
`LLBufferedAssetUploadInfo::finishUpload` builds an `LLViewerInventoryItem` out
of the capability's own response body — the folder, permissions, asset id and
item id are all in it — hands it to `gInventory.updateItem` and calls
`notifyObservers()`. It never waits for a push, which is exactly why a grid
that sends none is fine for it.

- Build the item from `Event::AssetUploaded` (and `ScriptUploaded`) where
  `new_inventory_item` names an item the model does not hold, rather than
  returning early: the upload command knows the folder, name, description and
  permission masks it asked for, and the completion names the ids.
- Keep the announcement path working: a grid that *does* push (Second Life,
  measured) then announces an item the model already has, which must not
  duplicate the row.
- Assert it offline against `Grid::FakeOpensim`, where the grid is silent by
  policy — an upload followed by "the item is in the model" with no folder
  re-fetch in between.

Acceptance: an upload against a silent grid puts the new item in the inventory
model without a folder re-fetch, and an upload against an announcing grid still
leaves exactly one row.
