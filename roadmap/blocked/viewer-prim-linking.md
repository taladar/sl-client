---
id: viewer-prim-linking
title: Prim linking & unlinking
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection-core, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Link a selection set ([[viewer-object-selection-core]]) into a linkset (root =
last-selected), unlink, and reorder; enforce link limits and permissions. The
link / unlink commands are driven from input **actions**
([[viewer-input-action-map]]).

Also the **Edit Linked Parts** mode (Build menu checkbox,
`Tools.EditLinkedParts`): select and manipulate individual child prims
inside a linkset without unlinking (main-menu survey 2026-07-23).

Reference (Firestorm, read-only): `llselectmgr` link / delink; messages
`ObjectLink`, `ObjectDelink`.
