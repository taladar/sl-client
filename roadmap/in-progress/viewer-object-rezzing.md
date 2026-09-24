---
id: viewer-object-rezzing
title: Object rezzing from inventory
topic: viewer
status: in-progress
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

## Status (2026-09-24): partly done

The drag-rez works: a drop from the inventory onto the world builds
`rez_object_command` (`sl-viewer-inventory/src/inventory_drag.rs`), a no-copy
item is moved rather than copied (`rez_respects_the_copy_permission`), and the
link case was fixed separately
(`viewer-inventory-link-drop-to-world-rezzes-nothing`).

**Left:**

- **Restore to Last Position.** `Command::RezRestoreToWorld` is wired through
  `sl-client-bevy` and `sl-repl`, but no inventory menu offers it.
- **The parcel pre-check.** Nothing asks whether this parcel lets the agent
  rez before the drop is sent; a refusal only arrives as the grid's alert.
- **The rezzed object joining the selection.** The drop sends
  `rez_selected: false`, so a fresh rez is not selected
  ([[viewer-object-selection-core]]).
