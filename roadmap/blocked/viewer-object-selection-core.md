---
id: viewer-object-selection-core
title: Object selection core (select set + protocol)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The foundation that every object-editing operation plugs into: **click** and
**drag-rectangle** selection with a highlight and a maintained selection set,
plus the object-select / deselect and object-permissions protocol behind it.
Clicking an object (ray-cast pick) selects it; a drag-rectangle selects all
objects whose screen bounds fall inside the rubber-band; selected objects render
a highlight, and the selection set is the shared state the edit floater, gizmos,
linking, and per-aspect tabs all read.

This owns the wire side of selection: request select / deselect on the objects
in the set and track the returned object-permissions so downstream editors know
what the agent may change. The edit-floater shell
([[viewer-object-edit-floater-shell]]), the hover / context menu
([[viewer-object-context-menu]]), and the concrete editing operations —
[[viewer-prim-creation]], [[viewer-object-rezzing]],
[[viewer-transform-gizmos]], [[viewer-prim-parameter-editing]],
[[viewer-prim-texture-editing]], [[viewer-prim-inventory-editing]],
[[viewer-prim-linking]] — all build on the selection set this task establishes.

Reference (Firestorm, read-only): `llselectmgr`, `lltoolmgr`, `lltoolselect`,
`lltoolselectrect`.

Builds on: the `objects.rs` lifecycle. Supersedes the MVP "object selection /
interaction" non-goal.
