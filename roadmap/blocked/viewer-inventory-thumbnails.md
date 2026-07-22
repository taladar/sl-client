---
id: viewer-inventory-thumbnails
title: Inventory thumbnails — view & edit
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-texture-picker]
refs: [viewer-inventory-gallery]
---

Context: [context/viewer.md](../context/viewer.md).

Item and folder **thumbnails**: the gallery view
([[viewer-inventory-gallery]]) already renders thumbnail images where
inventory carries them; this task adds the *editing* side — set / change /
remove a thumbnail on an item or folder (AIS3 `thumbnail` field update),
picking the image from inventory via the texture picker
([[viewer-ui-texture-picker]]) or from a snapshot, plus the bulk helper the
reference ships (apply one image to a selection, copy/paste thumbnails
between items).

Reference (Firestorm, read-only): `llfloaterchangeitemthumbnail`,
`floater_inventory_thumbnails_helper.xml` (bulk helper), AIS3 update
semantics in `llaisapi`.

Builds on: AIS3 inventory mutation (`protocol-30` / `protocol-61`) and the
inventory gallery.

Deps: [[viewer-ui-texture-picker]] (image selection).
