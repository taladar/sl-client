---
id: viewer-object-rezzing
title: Object rezzing from inventory
topic: viewer
status: done
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

## Status (2026-09-27): closed

- **Restore to Last Position** is in the object item menu (the reference's
  `restoreToWorld`). It is offered for an object that is not worn, not in the
  Trash, not a link and not in the Library. On Second Life it refuses a
  no-copy item with `CantRestoreToWorldNoCopy`, the reference's guard
  against losing it. On OpenSim, detected by the region's `OpenSimExtras`
  simulator features, the item is sent, as the reference allows there.
  Stock OpenSim parses `RezRestoreToWorld` but no module acts on it, so the
  restore only does something on Second Life.
- **The rezzed object joins the selection** as the reference does it. The
  drag-rez sends `RezSelected` while the build tools are open
  (`LLToolMgr::inEdit`). The build tools then select any root object that
  arrives flagged `FLAGS_CREATE_SELECTED` while they are open
  (`LLViewerObjectList`'s rule, `sl-viewer-edit`'s `select_create_selected`).
- **A worn attachment is not rezzed** by a drop on the world, the
  reference's `isWearingAttachment` refusal.
- **No parcel pre-check.** The reference's `dad3dRezObjectOnLand` does not
  ask whether the parcel allows building. It sends the rez and lets the
  simulator refuse with its own alert, which this viewer already shows. A
  viewer-side guess from the agent parcel's flags would also be wrong for a
  drop on a different parcel, or for an agent the parcel's group or estate
  lets build, so none was added.
