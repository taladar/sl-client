---
id: viewer-transform-gizmos
title: Position / rotation / scale gizmos
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Interactive manipulators for the selected object(s): move (3-axis + planar
handles), rotate (rings), and stretch / scale handles, with grid snapping and
local / world / reference frames. Edits are pushed to the sim via `ObjectUpdate`
/ `MultipleObjectUpdate`.

Reference (Firestorm, read-only): `llmaniptranslate`, `llmaniprotate`,
`llmanipscale`, `llmanip`.

Deps: [[viewer-object-selection]], [[viewer-input-system]],
[[viewer-camera-system]].
