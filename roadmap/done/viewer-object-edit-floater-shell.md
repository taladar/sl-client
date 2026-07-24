---
id: viewer-object-edit-floater-shell
title: Object edit-floater / tool shell
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-object-selection-core, viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The edit floater / tool shell that hosts the per-aspect editing tabs. It opens
against the current selection set ([[viewer-object-selection-core]]), presents
the build-tool mode switch, and provides the tabbed container the individual
editors dock into — the object / features tab
([[viewer-prim-parameter-editing]]), the texture / material tab
([[viewer-prim-texture-editing]]), and the contents tab
([[viewer-prim-inventory-editing]]). This task is only the shell and its
tab-hosting / selection-binding plumbing; each tab's contents ship in their own
task.

Reference (Firestorm, read-only): `llfloatertools`.

Builds on: the basic floater ([[viewer-ui-floater-basic]]) and the selection set
from [[viewer-object-selection-core]].

## Done

`sl-client-bevy-viewer/src/edit_tool.rs`. The Build Tools floater (Build ▸
Build Tools, `Ctrl+B`, the bottom-toolbar Build button, or an object pie
menu's Edit — which also selects the picked object; an open floater *is*
edit mode via [`EditToolState`]): the tool-mode switch (Move / Rotate /
Stretch — with the reference's held chords, `Ctrl` = rotate and
`Ctrl+Shift` = stretch, overriding while held), the
snap / local-axes / edit-linked-parts / stretch-both-sides toggles, the grid
unit field, a selection summary line (count, primary name, no-modify
warning), and live numeric Position / Rotation (XYZ Euler degrees) / Size
fields that mirror the primary selection — including mid-gizmo-drag — and
commit on Enter / focus loss by sending the same `MultipleObjectUpdate` the
gizmos send. The per-aspect tab strip (General / Object / Features /
Texture / Content, the reference's order) hosts the transform fields on the
**Object** tab (the reference's `llpanelobject` placement); the General /
Object / Features contents are [[viewer-prim-parameter-editing]], the
Texture and Content placeholders are [[viewer-prim-texture-editing]] and
[[viewer-prim-inventory-editing]]. Text roles are skinnable
(`.sk-build-label` / `-value` / `-placeholder` in `common.css`); a gallery
specimen covers the shape in the `ui_test` matrix.
