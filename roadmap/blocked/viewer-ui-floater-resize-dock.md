---
id: viewer-ui-floater-resize-dock
title: Floater window manager (resize / minimize / dock / tear-off)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The second tier of the floater manager on top of [[viewer-ui-floater-basic]]:
manual **resize**, **minimize**, **dock** (snap/host a floater into a
container), and **tear-off** (detach a docked panel back into a free floater).
Manual resize composes with the content-driven auto-sizing from the basic tier —
the user's size is a constraint layered over the min-content floor, not a
replacement for it.

Reference (Firestorm, read-only): `indra/llui/llfloater` (resize handles,
`llmultifloater` docking), `lllayoutstack`.
