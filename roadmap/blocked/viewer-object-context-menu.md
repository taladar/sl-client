---
id: viewer-object-context-menu
title: Object hover / context (pie) menu
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

The hover / context (pie) menu for the object under the cursor or the current
selection: the radial / context menu that offers touch, sit, edit, open, buy,
take, and the other per-object actions. It reads the selection set and pick
target from [[viewer-object-selection-core]] and dispatches each entry to the
relevant operation (edit opens [[viewer-object-edit-floater-shell]], and so on).

Reference (Firestorm, read-only): `llselectmgr`, `lltoolpie`.

Builds on: the `objects.rs` lifecycle and the pick / selection set from
[[viewer-object-selection-core]].
