---
id: viewer-snapshot-quick-key
title: Quick-snapshot keybind → disk
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-snapshot-tools
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

The **quick-snapshot key**: a keybind that captures straight to disk with the
current settings and no floater — the "just grab it" path a photographer reaches
for mid-shoot. Wired through the input action map
([[viewer-input-action-map]]) as a bindable action. On every disk save, **log
the saved file's path to chat history**, so the local-chat log becomes a running
index of what you shot and where it went (the reference viewer does this and
photographers rely on it).

`screenshot.rs` already captures to disk from a CLI flag; this task is the
interactive keybind and the chat-log-the-path behaviour.

Reference (Firestorm, read-only): `llfloatersnapshot`, `llsnapshotlivepreview`.

Builds on: `screenshot.rs`.
