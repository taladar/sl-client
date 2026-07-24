---
id: viewer-build-undo-redo
title: Object-edit undo/redo stack
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-transform-gizmos, viewer-prim-parameter-editing]
refs: [viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Build ▸ Undo (Ctrl+Z) / Redo (Ctrl+Y): multi-level undo of object edit
operations. The reference keeps a bounded per-session action stack of
edits (move/rotate/scale, parameter and texture changes) and undoes by
issuing the inverse object-update messages — there is no server-side
history, so this is entirely a client-side ledger.

Scope:

- Record each outgoing edit (transform deltas, parameter before/after)
  as an undoable action on a bounded stack.
- Undo/redo issue the inverse (respectively re-applied) updates for the
  affected objects; selection changes prune actions whose objects are
  gone.
- Ctrl+Z / Ctrl+Y bindings active in edit mode; menu entries reflect
  availability.

Reference (Firestorm, read-only): `Edit.Undo`/`Edit.Redo`
(`menu_viewer.xml` Build, ~L2481-2504), `LLManip`/selection undo hooks.

Builds on: the transform gizmos and prim parameter editing (both
blocked) — the edits those tasks send are exactly what gets recorded.
