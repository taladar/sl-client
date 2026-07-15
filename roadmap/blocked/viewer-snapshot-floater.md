---
id: viewer-snapshot-floater
title: Snapshot floater — preview, format, destinations
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-snapshot-tools
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-snapshot-quick-key, viewer-snapshot-to-inventory, viewer-360-snapshot, viewer-photo-hosting-upload]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug `screenshot.rs` into a real **snapshot floater**
([[viewer-ui-widget-scaffold]]): a framed **live preview**, **resolution /
aspect / format** selection, and destinations — **save-to-disk**, **postcard /
email**, and **profile feed**. Like the quick key
([[viewer-snapshot-quick-key]]), log the saved file's path to chat history on
every disk save.

Related tasks plug into this floater: **save-to-inventory as a texture** is
[[viewer-snapshot-to-inventory]] (its resolution rules and L$ cost differ from
disk), **equirectangular 360 capture** is [[viewer-360-snapshot]] (a distinct
capture-renderer), and sharing to external sites is
[[viewer-photo-hosting-upload]]. This task owns the floater, the disk / postcard
/ profile destinations, and the framed preview; the rest plug into it.

Reference (Firestorm, read-only): `llsnapshotlivepreview`, `llfloatersnapshot`,
`llviewerassetupload`.

Builds on: `screenshot.rs` and the asset-upload caps (`upload.rs`).
