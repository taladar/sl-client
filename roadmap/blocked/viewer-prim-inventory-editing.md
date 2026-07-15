---
id: viewer-prim-inventory-editing
title: Prim inventory (contents) editing
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-edit-floater-shell, viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The object **contents** tab of the edit floater
([[viewer-object-edit-floater-shell]]): list the items inside a prim (a
virtualized list, [[viewer-ui-virtualized-list]]), add (drag-in), remove, and
rename them, and drop scripts / notecards into an object.

Reference (Firestorm, read-only): `llpanelcontents`, `llsidepaneltaskinfo`;
messages `RequestTaskInventory`, `UpdateTaskInventory`, `RemoveTaskInventory`.
