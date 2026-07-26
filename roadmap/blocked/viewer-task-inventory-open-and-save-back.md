---
id: viewer-task-inventory-open-and-save-back
title: Open a task-inventory item into its editor + Save Back to Object
topic: viewer
status: blocked
origin: split from viewer-prim-inventory-editing (2026-07)
blocked_by: [viewer-notecard-editor, viewer-lsl-editor-save-compile]
---

Context: [context/viewer.md](../context/viewer.md).

The two contents actions the Content-tab task
([[viewer-prim-inventory-editing]]) could not ship because they need the asset
editors:

- **Open an item from a prim's contents** into its editor / preview
  (double-click a notecard / script / gesture in the Content tab or the Object
  Contents floater), the reference's `LLTaskInvFVBridge::openItem`. The editor
  must remember the item's holding object (its task id) as the source.
- **Save Back to Object Contents** (`Tools.SaveToObjectInventory`, Build ▸
  Object): write an item opened from an object's contents straight back into
  that object rather than into agent inventory — the reference's task-asset
  upload back to `UpdateTaskInventory`.

Both depend on the notecard editor ([[viewer-notecard-editor]]) and the script
editor / compile-save ([[viewer-lsl-editor-save-compile]]) existing first, and
on an "opened-from-task" provenance the editor carries.

Reference (Firestorm, read-only): `llpanelobjectinventory` (`openItem`),
`llpreviewscript` / `llpreviewnotecard` (the task-asset save path),
`llviewermenu` (`Tools.SaveToObjectInventory`).
