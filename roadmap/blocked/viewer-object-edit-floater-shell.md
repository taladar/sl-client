---
id: viewer-object-edit-floater-shell
title: Object edit-floater / tool shell
topic: viewer
status: blocked
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
