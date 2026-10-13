---
id: viewer-new-material-refused-on-second-life
title: Saving a PBR material to inventory is refused on Second Life
topic: viewer
status: bugs
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
