---
id: viewer-snapshot-tools
title: Snapshot / photo tools
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug `screenshot.rs` into a real snapshot floater: framed live
preview, resolution / aspect / format selection, and destinations —
save-to-disk, postcard / email, and profile feed.

Keep two behaviours that don't need the floater at all, because they are what a
photographer actually reaches for mid-shoot:

- A **quick-snapshot key** that captures straight to disk with the current
  settings and no floater — the "just grab it" path. (`screenshot.rs` already
  captures to disk from a CLI flag; this is the interactive keybind, wired
  through [[viewer-input-system]].)
- **Log the saved file's path to chat history** on every disk save (floater or
  key), so the local-chat log is a running index of what you shot and where it
  went — the reference viewer does this and photographers rely on it.

Related tasks split out of the floater: **save-to-inventory as a texture** is
[[viewer-snapshot-to-inventory]] (its resolution rules and L$ cost differ from
disk), **equirectangular 360 capture** is [[viewer-360-snapshot]] (a distinct
capture-renderer), and sharing to external sites is
[[viewer-photo-hosting-upload]]. This task owns the floater, the disk / postcard
/ profile destinations, and the quick-key + chat-log behaviour; the rest plug
into it.

Reference (Firestorm, read-only): `llsnapshotlivepreview`, `llfloatersnapshot`,
`llviewerassetupload`.

Builds on: `screenshot.rs` and the asset-upload caps (`upload.rs`).

Deps: [[viewer-ui-framework]], [[viewer-image-upload]] (shared upload path).
