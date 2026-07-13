---
id: viewer-snapshot-tools
title: Snapshot / photo tools
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug `screenshot.rs` into a real snapshot floater: framed live
preview, resolution / aspect / format selection, and destinations —
save-to-disk, upload-to-inventory (as a texture), postcard / email, and profile
feed — plus optional 360-degree capture.

Reference (Firestorm, read-only): `llsnapshotlivepreview`, `llfloatersnapshot`,
`llfloater360capture`, `llviewerassetupload`.

Builds on: `screenshot.rs` and the asset-upload caps (`upload.rs`).

Deps: [[viewer-ui-framework]], [[viewer-image-upload]] (shared upload path).
