---
id: viewer-snapshot-quick-key
title: Quick-snapshot keybind → disk
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-snapshot-tools
blocked_by: [viewer-input-action-map]
refs: [viewer-snapshot-floater, viewer-input-modifier-chords]
---

Context: [context/viewer.md](../context/viewer.md).

The **quick-snapshot key**: ``Ctrl+` `` captures straight to disk with the
last-used snapshot settings and no floater — the "just grab it" path a
photographer reaches for mid-shoot. It just requests the same save the snapshot
floater's Save button does ([[viewer-snapshot-floater]]), so the whole capture
path is shared: it honours the persisted format and include-UI / include-HUD
toggles, saves at the window resolution, and **logs the saved file's path to
nearby chat** — the running local-chat index photographers rely on. The floater
need not be open (its state and handles exist from startup).

Implemented (`snapshot_floater::snapshot_hotkey`) as a direct-keyboard chord
handler gated on the world (not a text field) owning the keyboard — the same
pattern the other viewer chords use (`Ctrl+B` build tools, `Ctrl+L` link), since
the action map still keys on a single `KeyCode`. Making ``Ctrl+` `` a
*rebindable* action through the map (rather than a hardcoded chord) is the
follow-up [[viewer-input-modifier-chords]], which widens the map to modifier
chords and migrates every hardcoded chord onto it.

Reference (Firestorm, read-only): `llfloatersnapshot`, `llsnapshotlivepreview`.

Builds on: [[viewer-snapshot-floater]] (the shared capture path),
`screenshot.rs`.
