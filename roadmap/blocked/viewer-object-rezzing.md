---
id: viewer-object-rezzing
title: Object rezzing from inventory
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection-core, viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

Drag / "rez" an object item from inventory into the world (`RezObject` /
`RezRestoreToWorld`), with a drop-point ray-cast and permission / region checks
(is rezzing allowed on this parcel?). The rezzed object joins the selection set
([[viewer-object-selection-core]]), and the drag originates from the inventory
context actions ([[viewer-inventory-context-actions]]).

The `object-rez-derez` test case already exercises the RezObject path on the
local grid; this task is the interactive drag-from-inventory rez.

Reference (Firestorm, read-only): `lltooldraganddrop`, `llviewerinventory` rez
paths.

Builds on: the existing `object-rez-derez` test case and `inventory.rs`.
