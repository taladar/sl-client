---
id: viewer-settings-save-as-create-then-put
title: A settings Save As uploads through a cap that does not take settings
topic: viewer
status: bugs
origin: Live aditi / OpenSim run of the My Environments window (2026-09-09)
refs: [viewer-environment-fixed-editor, viewer-environment-my-environments]
---

Context: [context/viewer.md](../context/viewer.md).

The settings editors' **Save As** mints its copy with
`Command::UploadAsset` — `NewFileAgentInventory` — carrying
`asset_type: "settings"`. **No grid takes that**, and both fail silently:

- OpenSim's `UploadCompleteHandler`
  (`OpenSim/Region/ClientStack/Linden/Caps/BunchOfCaps/BunchOfCaps.cs`) declares
  `sbyte assType = 0; sbyte inType = 0;` and matches the type string against
  *sound, snapshot, animation, animset, wearable, object* only. Nothing sets
  them for `settings`, so the item is created and filed as a **Texture** —
  invisible to every settings surface, since they all filter on
  `AssetType::Settings`.
- Second Life creates nothing at all.

Neither reports an error, which is why this survived: the viewer's own
`AssetUploaded` handling sees a plausible reply.

The **creation** half of this was fixed with
[[viewer-environment-my-environments]]: `new_settings_item` now sends
`CreateInventoryItem` and lets the simulator author the default asset and stamp
the subtype byte, which is what `LLSettingsVOBase::createNewInventoryItem` does.
Save As is the other half, and it needs the *second* branch of the reference's
flow, because it has a body to store rather than a default to accept:

1. `create_inventory_settings` — the same `CreateInventoryItem` (nil transaction
   id, `AT_SETTINGS` / `IT_SETTINGS`, subtype byte in the wearable-type field);
2. then, once `UpdateCreateInventoryItem` names the fresh item,
   `LLSettingsVOBase::updateInventoryItem` PUTs the encoded frame to
   `UpdateSettingsAgentInventory` — which is `Command::UpdateInventoryAsset`
   with `AssetUpdateLocation::AgentInventory`, exactly what an **in-place** Save
   already uses.

So the pieces all exist; what is missing is the correlation between step 1's
reply and the body waiting to be written. An in-place Save needs none of it (the
reply names the item it wrote), and `PendingItemCreations` is the wrong queue
here — it exists to stamp flags after an *upload* mints an item, and this path
does not upload and does not need a stamp.

Reference (Firestorm, read-only): `llsettingsvo.cpp`
(`createNewInventoryItem`, `createInventoryItem`, `onInventoryItemCreated`,
`updateInventoryItem`), `llviewerinventory.cpp` (`create_inventory_settings`).

## Worth fixing together

`LLSettingsVOBase::onInventoryItemCreated` also widens the fresh item's
permissions — `if (perm.getMaskEveryone() != PERM_COPY)` then sets it and
`updateServer`s — which neither half does yet. Small, and the same callback.
