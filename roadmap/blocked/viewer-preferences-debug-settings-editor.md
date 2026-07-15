---
id: viewer-preferences-debug-settings-editor
title: Raw debug-settings editor
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The raw **debug-settings editor** (`llfloatersettingsdebug`): a searchable,
type-aware editor over every entry in the typed settings store — pick a setting
by name, see its type / default / current value, and edit it directly. The
escape hatch for settings that have no dedicated preferences control. Built over
the same store binding the preferences floater ([[viewer-preferences-floater]])
uses.

Reference (Firestorm, read-only): `llfloatersettingsdebug`, `llviewercontrol`.

Builds on: [[viewer-preferences-floater]] and the typed settings store.
