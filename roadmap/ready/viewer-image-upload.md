---
id: viewer-image-upload
title: Image / texture (and sound / animation) upload
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A creator-facing asset-upload wizard: pick a file from disk, preview it, show
the L$ upload cost, choose name / description / folder, and upload to inventory.
Covers textures / images (encode to J2C) and the sibling simple uploads — sound
(`.wav`) and animation (`.bvh` / `.anim`) — plus bulk upload. The wizard is a
floater ([[viewer-ui-widget-scaffold]]).

The `NewFileAgentInventory` upload path and J2C encoding already exist
(`sl-j2c-encode`, `upload.rs`, and the `asset-upload` / `baked-texture-upload`
test cases); this task is the wizard UI + cost / preview around them.

Reference (Firestorm, read-only): `llfloaterimagepreview`, `llfloaternamedesc`,
`llviewerassetupload`, `llfloaterbulkupload`.

Builds on: `sl-j2c-encode` + the `NewFileAgentInventory` upload path
(`upload.rs`).

## Parity-audit addendum (2026-08-19)

The parity audit adds Build ▸ Upload ▸ **Material…**
(`File.UploadMaterial`): import a `.gltf` file from disk into a
material inventory item. The material asset encoder already exists
(viewer-pbr-material-editor, done), so this is a file-picker + parse +
create-inventory-item flow over the existing encoder.

Scope addition from the parity audit: the reference's per-folder default
upload destinations — the inventory folder context menu's "Use as
default for ▸ Image / Sound / Animation / Model / PBR-material uploads"
entries (menu_inventory.xml, `Inventory.FileUploadLocation`), which set
where each upload type files its results. Ours has no equivalent; add
the context entries and the per-type destination settings alongside the
upload flows.
