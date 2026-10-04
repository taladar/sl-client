---
id: viewer-gesture-management-ui
title: Gesture management & editor UI
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-gestures-ui
blocked_by: [viewer-ui-widget-scaffold, viewer-gesture-runtime]
---

Context: [context/viewer.md](../context/viewer.md).

The gesture management surface: the **active-gestures** list (activate /
deactivate, see the trigger and key), and the **gesture editor / preview** —
build and reorder the animation / sound / chat / wait steps and preview the
result. Hosted in a floater ([[viewer-ui-widget-scaffold]]) and driving the
gesture runtime ([[viewer-gesture-runtime]]) for the preview and playback.

Builds on the runtime's step model; this task is the list and editor UI over it.

Reference (Firestorm, read-only): `llfloatergesture`, `llpreviewgesture`,
`llgesturemgr`.

Builds on: [[viewer-gesture-runtime]].

## Capability (triage 2026-10-04)

Saving a gesture held in an object uses `UpdateGestureTaskInventory` (both
grids): the two-step upload with `{task_id, item_id}`, used only when
`UpdateGestureAgentInventory` is granted too; otherwise the legacy asset store
plus `UpdateTaskInventory`. Our client already dispatches it — only the name is
missing from `REQUESTED_CAPABILITIES`.

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.
