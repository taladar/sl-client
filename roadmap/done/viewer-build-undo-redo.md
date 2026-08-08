---
id: viewer-build-undo-redo
title: Object-edit undo/redo stack
topic: viewer
status: done
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-transform-gizmos, viewer-prim-parameter-editing]
refs: [viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Build ▸ Undo (Ctrl+Z) / Redo (Ctrl+Y): multi-level undo of object edit
operations (move/rotate/scale, parameter and texture changes).

**Correction to the original premise:** the task was filed assuming
"there is no server-side history, so this is entirely a client-side
ledger" issuing inverse updates. That is **wrong** about the reference.
SL object undo is **server-side**: the reference viewer's `Edit.Undo` /
`Edit.Redo` (`LLSelectMgr::undo` / `redo`) simply send the `Undo` /
`Redo` messages (Low 75/76) carrying the selected objects' ids, and the
simulator keeps a bounded per-object edit history and reverts. There is
no client-side action stack, no inverse `ObjectUpdate` synthesis (the
`llundo.h` include in `llselectmgr.cpp` is unused — the only client
`LLUndoBuffer` uses are text fields and the appearance morph sliders).
OpenSim implements the same server side (`SceneObjectPart.Undo()` /
`Redo()`). The user confirmed: match SL, so this is implemented
server-side.

Reference (Firestorm, read-only): `Edit.Undo` / `Edit.Redo`
(`menu_viewer.xml` Build, ~L2481-2504), `LLSelectMgr::undo` / `redo` /
`canUndo` / `canRedo` / `packObjectID`; messages `Undo` / `Redo`.

Builds on: the transform gizmos and prim parameter editing — the edits
those tasks send are what the simulator records and this reverts.

## Done

Server-side, reference-faithful.

- **Wire / proto** — `sl-wire` already generated the `Undo` / `Redo`
  message structs from the vendored template; net-new is the sl-proto
  plumbing: `Command::UndoObjects` / `Command::RedoObjects`
  (region-scoped id lists, like every other object command),
  `Session::undo_objects` / `redo_objects` resolving each scoped id to
  its full id from the object cache (`resolve_full_ids` — the wire
  addresses objects by full id, mirroring `packObjectID`'s `mID`), and
  `Circuit::send_undo` / `send_redo`. The message's `GroupID` (the
  reference fills it with the active group) is sent **nil** — both SL and
  OpenSim's `HandleUndo` read only the object ids for undo. Uncached ids
  resolve to nothing and send nothing. Dispatched in both runtimes
  (bevy + tokio) and exposed as `undo_objects` / `redo_objects` REPL
  commands.
- **Viewer** — `sl-client-bevy-viewer/src/edit_undo.rs` (`EditUndoPlugin`)
  drives Ctrl+Z / Ctrl+Y (world-owns-keyboard, build-tool-active) and the
  Build-menu Undo / Redo entries, sending `UndoObjects` / `RedoObjects`
  for the current selection. The send set is the selection as-is —
  linkset roots in whole-linkset mode (the reference's `SEND_ONLY_ROOTS`)
  or individual prims in edit-linked-parts mode (`SEND_CHILDREN_FIRST`);
  order is immaterial since the sim reverts each object independently.
  `can_undo` / `can_redo` mirror `canUndo` / `canRedo`
  (`getFirstUndoEnabledObject` = modify-or-move, `getFirstEditableObject`
  = modify; not-yet-known nodes optimistic), additionally gated on the
  build tool being active so the Build-menu entries grey out outside edit
  mode.
- **Deviations** (documented in `edit_undo.rs`): key-repeat on a held
  chord is not reproduced (the reference's `allow_key_repeat` would undo
  at frame rate); `isPermanentEnforced` objects are not excluded from the
  undo-enable gate (the viewer does not track that flag — a rare
  permanent object would send an undo the sim ignores).
- Unit tests: sl-proto lifecycle (undo/redo resolve full ids, nil group,
  uncached-skip), sl-repl registry parse, viewer `edit_undo`
  (enable-gate + send-set) tests.
