---
id: viewer-object-selection
title: Object selection & edit-floater shell
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The foundation that the individual editing operations plug into: click and
drag-rectangle **selection** with highlight and a selection set, the edit
floater / tool shell that hosts the per-aspect tabs, the hover / context (pie)
menu, and the object-select / deselect + object-permissions protocol.

The concrete editing operations — create, rez, transform, parameters, contents,
texture, link — live in their own stubs: [[viewer-prim-creation]],
[[viewer-object-rezzing]], [[viewer-transform-gizmos]],
[[viewer-prim-parameter-editing]], [[viewer-prim-inventory-editing]],
[[viewer-prim-texture-editing]], [[viewer-prim-linking]].

Reference (Firestorm, read-only): `llselectmgr`, `lltoolmgr`, `lltoolselect`,
`lltoolselectrect`, `llfloatertools`.

Builds on: the `objects.rs` lifecycle. Supersedes the MVP "object selection /
interaction" non-goal.

Deps: [[viewer-ui-widget-scaffold]].
