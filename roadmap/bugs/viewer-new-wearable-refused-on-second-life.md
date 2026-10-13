---
id: viewer-new-wearable-refused-on-second-life
title: New wearables and a wearable's Save As are refused on Second Life
topic: viewer
status: bugs
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
