---
id: viewer-inventory-worn-markers-show-early
title: Show (worn) / bold inventory markers early, without opening Current Outfit
topic: viewer
status: bugs
origin: user report (2026-07-31, aditi live testing)
refs: [viewer-ais3-inventory-mutations-and-cof-reconverge]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

The `(worn)` / bold markers in the inventory tree (and the Worn tab) are derived
from the **Current Outfit Folder (COF)** link items
(`InventoryModel::worn_item_keys`, keyed on `items_of(cof)`). The COF's contents
are only fetched lazily — by the background inventory crawl (so the markers
appear *late* in the rez) or when the user first opens **Current Outfit** / the
**Worn** tab. Until then, worn items show unmarked even though the avatar wears
them.

## Where to look / what was tried

A first attempt eagerly called `request_folder(cof)` in the `InventoryFolders`
ingest arm (`inventory.rs`) the moment `model.cof` was known. It **regressed**:
`appearance.rs` re-queries `QueryInventoryFolders` on every re-bake, so the
eager fetch fired repeatedly and, interacting with the COF invalidation
([[viewer-ais3-inventory-mutations-and-cof-reconverge]]), left
`model.items[cof]` set to an empty cache read that stuck — Current Outfit and
the Worn tab showed **empty**. Reverted.

A correct fix needs to fetch the COF's contents **once**, early, from the server
(`QueryInventoryFolder` on an `Unknown` folder auto-schedules
`fetch_folder_contents`), **without** re-setting `model.items[cof]` to an empty
page on the repeated `InventoryFolders` events the bake handshake produces, and
without fighting the COF re-fetch churn. Consider: gate the eager fetch to a
true one-shot; or only (re)populate `model.items[cof]` from an authoritative
`InventoryDescendents`, never from a cache-empty `InventoryFolderPage`; or prime
`model.requested` for the COF without issuing the paged cache read.

## Verify

Live on aditi: log in, open the inventory floater (not Current Outfit / Worn
first), and confirm worn items show `(worn)` / bold promptly, and that Current
Outfit + the Worn tab list the worn items — while take-off / detach still
reconverge correctly.
