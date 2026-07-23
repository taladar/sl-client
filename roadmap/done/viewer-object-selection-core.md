---
id: viewer-object-selection-core
title: Object selection core (select set + protocol)
topic: viewer
status: done
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

## Done

`sl-client-bevy-viewer/src/edit_selection.rs`. The [`SelectionSet`] resource
(ordered nodes, the primary last; per-node `ObjectProperties` folded in from
the reply events) is the shared state; while the build tool
([[viewer-object-edit-floater-shell]]) is active, a left click selects
(root-by-default, picked prim in edit-linked-parts mode, Shift/Ctrl toggles,
applied on mouse-up with the reference's 5 px slop) and an empty-world drag
sweeps an inclusive rubber-band rectangle over the projected bounds of
in-world volume objects, tentative-highlighting during the sweep. The wire
side diffs the set into batched `ObjectSelect` / `ObjectDeselect`
(`Command::RequestObjectProperties` / `DeselectObjects`), ingests
`ObjectProperties`, applies simulator-forced `ForceObjectSelect`, and prunes
killed objects. The highlight is a translucent unlit overlay child per face
mesh (committed yellow / tentative blue) — deliberately simpler than the
reference's silhouette edges. `Escape` deselects; the touch pick stands down
via a run condition while the tool is active. A new `ObjectSlMotion`
component on every object entity mirrors the wire-frame position / rotation /
scale for all the editing surfaces.
