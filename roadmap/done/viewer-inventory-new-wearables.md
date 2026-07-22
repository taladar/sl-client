---
id: viewer-inventory-new-wearables
title: New Clothes / New Body Parts creation
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The folder context menu's **"New Clothes ▸"** and **"New Body Parts ▸"**
submenus: create a fresh wearable of a chosen slot (shirt, pants, …, shape,
skin, hair, eyes). New Folder / Script / Notecard / Gesture are live; the
wearable creators are greyed because a wearable item needs a **default
wearable asset** behind it — the reference builds one from the slot's default
parameters (`LLWearable::createNewWearable`: the `.wearable` text format with
every visual param at default) and uploads it, then creates the item against
the uploaded asset (`NewInventoryItem` carries the `wearable_type`).

So this task is: emit the default `.wearable` asset per slot (the asset
format is already parsed for bakes — `sl-avatar`), upload it, then
`CreateInventoryItem` with the right `AssetType`
(`Clothing` / `Bodypart`) and slot flags.

Reference (Firestorm, read-only): `llwearable.cpp` / `llwearabletype.cpp`,
`llagentwearables.cpp` (`createWearable`).

Shipped 2026-07-22: every New Clothes / New Body Parts creator (context
menu + the toolbar's + menu) authors the slot's default `.wearable`
(LLWearable version 22, permissions/sale blocks, the slot's avatar_lad
params at their defaults — empty without `--viewer-assets` — and no
layer textures; round-trip pinned against the shared parser), uploads
it via `UploadAsset` (NewFileAgentInventory; OpenSim accepts
wearable/clothing+bodypart), then stamps the created item's wearable
flags via `ChangeInventoryItemFlags` when the reply lands (the uploader
path leaves flags empty, which would read as a Shape) — matched FIFO,
since the reply carries no correlation id. SL-grid behaviour of
NewFileAgentInventory for wearables still wants a live aditi check.
