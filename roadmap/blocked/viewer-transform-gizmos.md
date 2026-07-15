---
id: viewer-transform-gizmos
title: Position / rotation / scale gizmos
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection-core, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Interactive manipulators for the selected object(s) in the selection set
([[viewer-object-selection-core]]): move (3-axis + planar handles), rotate
(rings), and stretch / scale handles, with grid snapping and local / world /
reference frames. The manipulators consume input **actions**
([[viewer-input-action-map]]) rather than raw mouse buttons. Edits are pushed to
the sim via `ObjectUpdate` / `MultipleObjectUpdate`.

Reference (Firestorm, read-only): `llmaniptranslate`, `llmaniprotate`,
`llmanipscale`, `llmanip`.
