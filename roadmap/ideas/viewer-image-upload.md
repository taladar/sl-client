---
id: viewer-image-upload
title: Image / texture (and sound / animation) upload
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A creator-facing asset-upload wizard: pick a file from disk, preview it, show
the L$ upload cost, choose name / description / folder, and upload to inventory.
Covers textures / images (encode to J2C) and the sibling simple uploads — sound
(`.wav`) and animation (`.bvh` / `.anim`) — plus bulk upload.

The `NewFileAgentInventory` upload path and J2C encoding already exist
(`sl-j2c-encode`, `upload.rs`, and the `asset-upload` / `baked-texture-upload`
test cases); this stub is the wizard UI + cost/preview around them.

Reference (Firestorm, read-only): `llfloaterimagepreview`, `llfloaternamedesc`,
`llviewerassetupload`, `llfloaterbulkupload`.

Builds on: `sl-j2c-encode` + the `NewFileAgentInventory` upload path
(`upload.rs`).

Deps: [[viewer-ui-widget-scaffold]].
