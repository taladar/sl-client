---
id: viewer-task-inventory-open-and-save-back
title: Open a task-inventory item into its editor + Save Back to Object
topic: viewer
status: ready
origin: split from viewer-prim-inventory-editing (2026-07)
blocked_by: [viewer-notecard-editor]
refs: [viewer-lsl-editor-save-compile, viewer-prim-inventory-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The two contents actions the Content-tab task
([[viewer-prim-inventory-editing]]) could not ship because they needed the
asset editors.

## Already done (audit 2026-10-15)

**Opening a notecard or a script from a prim's contents** is in:
`ContentsAction::Open` in `sl-viewer-edit/src/edit_contents.rs` opens the
notecard editor or the script editor with a `NotecardSource::Task` /
`ScriptSource::Task { task_id, item_id }` provenance, read-only unless both
the object and the item are modifiable, and the editors save back to the
object over `UpdateNotecardTaskInventory` / `UpdateScriptTask`
(`sl-viewer-asset-editors/src/edit_notecard.rs`, `edit_script.rs`). The
script editor's richer compile/error UI is still
[[viewer-lsl-editor-save-compile]], but it no longer gates this task, so the
edge was dropped and the item moved out of blocked/.

## What remains

- **Open the other types** from the contents (the reference's
  `LLTaskInvFVBridge::openItem` per type): today every type but notecard and
  script is a no-op there — textures / snapshots into the texture preview,
  sounds and animations into their previews, gestures, landmarks.
- **Save Back to Object Contents** (`Tools.SaveToObjectInventory`, Build ▸
  Object). This is not an asset save: the reference derezzes the selected
  root with `DRD_SAVE_INTO_TASK_INVENTORY` into the object it was rezzed
  from (`node->mFromTaskID`, `LLToolsSaveToObjectInventory` in
  `llviewermenu.cpp`), enabled only when that id is set. The pieces exist —
  `DeRezDestination::SaveIntoTaskInventory` in
  `sl-proto/src/types/editing.rs` and the `from_task_id` the object
  properties carry — but no menu entry uses them.

Reference (Firestorm, read-only): `llpanelobjectinventory` (`openItem`),
`llviewermenu.cpp` (`LLToolsSaveToObjectInventory`,
`enable_save_into_task_inventory`).
