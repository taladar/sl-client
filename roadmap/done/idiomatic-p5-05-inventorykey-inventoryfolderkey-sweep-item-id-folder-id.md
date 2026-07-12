---
id: idiomatic-p5-05
title: InventoryKey / InventoryFolderKey sweep (item_id, folder_id)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`InventoryKey` / `InventoryFolderKey` sweep (`item_id`, `folder_id`).
Replaced every unambiguous inventory **item** id (LL `mItemID`/`InventoryID`)
with `sl_types::key::InventoryKey` and every inventory **folder**/category id
(`mFolderID`, incl. the nil-parent root) with `InventoryFolderKey`,
wrapping at the codec boundary only (wire bytes byte-identical, incl. the
`inventory_item_crc` checksum, which unwraps `.uuid()` before `uuid_crc`). NO
sl-types change — both keys already carry `Copy`/`Hash`/`uuid()`/`From<Uuid>`/
`Display` from the AgentKey-sweep 0.4.0, so sl-types stayed clean (no version
bump). Maximal scope across `sl-proto` + `sl-wire`: the type structs
(`InventoryItem`, `InventoryFolder`, `NewInventoryItem` — which lost its
`Default` derive → equivalent manual impl, `GestureActivation`,
`ObjectProperties`, `RestoreItem`, `NotecardRez`, `ScriptPermissionRequest`,
`GroupNoticeAttachment.item_id` [the Inventory half deferred here by the
GroupKey sweep], `Wearable`, `RezAttachment`); the id-bearing `Event`
variants (`InventoryDescendents.folder_id`, `ScriptRunning.item_id`, the
`InventoryBulkUpdate` `item_callbacks` `Vec<(InventoryKey, _)>`); ~35
`Command` variants (folder/item CRUD, the `Ais3*` REST ops,
`BuyObjectInventory`, the three script-running variants, `RemoveAttachment`,
`UpdateInventoryAsset`, `GiveInventory`/`GiveInventoryFolder`,
`Accept`/`DeclineInventoryOffer` folders,
`AcceptFriendship.calling_card_folder`, and the plural `Vec` lists); the
**inventory/library root folder** chain (`LoginSuccess.inventory_root`/
`library_root`, `LoginAccount.library_root`, the `Session` state +
`inventory_root()` accessor → `Option<InventoryFolderKey>`); and the `sl-wire`
helper signatures (`login.rs` `SkeletonFolder`; `inventory.rs` every AIS3
URL/body builder+parser, `CreateInventoryCategoryRequest`, and `AisUpdate`'s
eight folder/item id-lists; `llsd.rs` `build_fetch_inventory_request`/
`build_group_notice_bucket`/`build_update_item_asset_request`/
`build_new_file_agent_inventory_request`). Builders stay wire-identical via
the keys' `Display`; parsers wrap `Key::from`. **Left raw (deliberately):**
`InventoryOffer.item_id` (an item-*or*-folder union discriminated by
`asset_type == Folder` → deferred to the union-key item, the same precedent as
`ChatMessage.source_id` / the dialog-discriminated IM field); every `asset_id`
(TextureKey/asset family); `transaction_id` (TransferId); the Owner-family
owner ids (`GroupNoticeAttachment.owner_id`, `RezAttachment.owner_id`, no
discriminator); and `DerezObjects.destination_id` (a folder-or-task union,
left raw). Re-exported `InventoryKey`/`InventoryFolderKey` through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the raw
`Uuid` then wraps, both runtimes mirrored, examples typed their folder
trackers.
+2 focused unit tests (the keys round-trip bit-identically and are distinct
types over the same uuid; an `InventoryFolder`'s ids survive a round trip
incl. the nil-parent root). NO sl-types touched.
