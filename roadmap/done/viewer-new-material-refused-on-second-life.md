---
id: viewer-new-material-refused-on-second-life
title: Saving a PBR material to inventory is refused on Second Life
topic: viewer
status: done
origin: gridspec-asset-upload (2026-10-13)
refs: [gridspec-asset-upload, gridspec-materials]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The build floater's PBR Save (`sl-viewer-edit/src/edit_material.rs`,
`handle_pbr_save_press`) uploads the material with `Command::UploadAsset` —
`NewFileAgentInventory`, class `material`.

Second Life's uploader does not take that class: the first request is
answered `Upload_InvalidAssetType`, *Invalid asset type: material*
(`upload-http`, 2026-10-13, `book/src/gridspec/assets.md` § Uploads).
OpenSim takes the request and files the item as a **texture** — and grants
no PBR capability, so the tab is greyed there anyway.

## Effect

On Second Life, the one grid with PBR materials, Save makes nothing.

## Fix

Do what the reference viewer does (`LLMaterialEditor::saveIfNeeded`): create
the item with `CreateInventoryItem` (class material) and, when it is
announced, save the bytes into it over `UpdateMaterialAgentInventory`
(`Command::UpdateInventoryAsset`). The settings editors already do exactly
this for a new sky (`PendingSettingsCreations`). An `e2e` test on the
Second-Life-flavoured fake grid, which now refuses the upload as the grid
does.

## Done (2026-11-12)

`handle_pbr_save_press` (`sl-viewer-edit/src/edit_material.rs`) now mints an
empty material item with `CreateInventoryItem` (`new_material_item`: class
material, no transaction, no subtype) and holds the encoded material on
`PendingMaterialCreations`; `finish_material_creations` matches each announced
material item to the oldest Save in flight and writes the bytes into it with
`Command::UpdateInventoryAsset` (`UpdateMaterialAgentInventory`). The queue
reads only material items, so it never takes a settings creation's reply, and
it skips an item it has already filled or that the inventory already holds,
because Second Life announces the item again after the fill rewrites it.

Unit tests pin the create command, the in-order fill, the ignored rewrite and
the unasked item. `e2e_objects`' `a_material_is_saved_from_the_build_window`
runs on the Second-Life-flavoured fake grid (which refuses the old upload)
against a polished-metal PBR box, and on aditi against a box it rezzes and
polishes: Save on the Texture tab's Material channel, a new "New Material"
item in Materials, the fill in the viewer's log with the grid's answer, the
material editor opened on the item reading metallic 1 / roughness 0, then the
item purged and the box deleted. The fake run also reads the grid's stored
asset. A live run first clears any "New Material" item a stopped run left,
since every save is filed under that one name. The gridspec assets chapter
describes the new road.
