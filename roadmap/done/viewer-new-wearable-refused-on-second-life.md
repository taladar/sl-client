---
id: viewer-new-wearable-refused-on-second-life
title: New wearables and a wearable's Save As are refused on Second Life
topic: viewer
status: done
origin: gridspec-asset-upload (2026-10-13)
refs: [gridspec-asset-upload]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The inventory's New Clothes / New Body Parts entries
(`sl-viewer-inventory/src/inventory_actions.rs`, `dispatch_create`) and the
appearance editor's Save As (`sl-viewer-asset-editors/src/edit_wearable.rs`)
make the item with `Command::UploadAsset` — `NewFileAgentInventory` — stating
an `expected_upload_cost` of `0`.

Second Life's uploader takes `bodypart` and `clothing` only at a price: the
first request is answered with an uploader and `upload_price` 10, and the
bytes with `Upload_UploadPriceDiffers`, *The server expects a different
upload fee* (`upload-http`, 2026-10-13, `book/src/gridspec/assets.md`
§ Uploads). OpenSim takes it, free.

## Effect

On Second Life no new wearable can be made and no copy saved. Stating the
price would make it work and cost L$ 10 a shirt, which no viewer charges.

## Fix

Do what the reference viewer does (`LLWearableList::createNewWearable`,
`LLWearable::saveNewAsset`): upload the asset over the legacy transaction
road (`AssetUploadRequest`) and create the item with `CreateInventoryItem`
naming that transaction — the road `Command::SaveInventoryAsset` already
takes for a wearable that exists. The fake grid serves both halves
(`sl-fake-grid/src/uploads.rs`, `CreateInventoryItem` with a bound asset),
and its Second-Life flavour now refuses the upload as the grid does, so an
`e2e` test on both flavours can hold the fix.

## Done (2026-11-12)

`Session::create_inventory_item_with_asset`
(`Command::CreateInventoryItemWithAsset`, wired through both runtimes and the
repl's `create_inventory_item_with_asset`) sends the bytes with
`AssetUploadRequest` under the item's transaction — inline when small, by Xfer
otherwise — and then the `CreateInventoryItem` naming that transaction; it
refuses an item with no transaction. `Session::save_inventory_asset` shares the
upload half.

New Clothes / New Body Parts and the appearance editor's Save As build the
command with `new_wearable_command` (class clothing or bodypart, the slot in
the subtype byte, which is where the grid stamps the wearable's flags from, so
the old `ChangeInventoryItemFlags` follow-up is gone). `PendingItemCreations`
now matches each `UpdateCreateInventoryItem` by the transaction it echoes
rather than by arrival order, and a refused upload (`AssetUploadComplete` with
success false) ends the creation with the reference's `CannotSaveToAssetStore`
notice.

Loopback tests in `sl-proto/tests/sim_session.rs` pin the upload-then-create
order and the refused nil transaction; unit tests pin the commands for a shirt
and a shape, matching by transaction and the refused-upload notice.
`e2e_windows`' `a_new_shirt_and_its_copy_are_made` stage runs on the
Second-Life-flavoured fake grid (which refuses the old upload) and on aditi: a
New Shirt from the Clothing folder's menu, its icon (and on the fake grid the
grid's item flags and stored asset), worn, edited, saved as "New Shirt (copy)"
with the editor's status confirming it, taken off and both items purged. A
live run first clears either name a stopped run left behind. The gridspec
assets and xfer chapters describe the road.
