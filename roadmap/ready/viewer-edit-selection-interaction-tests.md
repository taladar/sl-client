---
id: viewer-edit-selection-interaction-tests
title: Selecting for edit — click, shift-click, rubber band, faces
topic: viewer
status: ready
origin: user request (2026-07) — build floater / gizmos named the priority
points: 5
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

Drive `edit_selection.rs::handle_select_pointer` headlessly against the
fixture world:

- click selects (`SelectionSet` primary), shift-click accumulates,
  click-empty clears;
- rubber-band drag sweeps `sweep_candidates`;
- `edit_linked` toggles child-vs-root selection (`promote_to_roots`);
- face-select mode toggles `PrimFaceId`s
  (`select_only_face`/`toggle_face`);
- selection changes emit the object-select/deselect `SlCommand`s the
  server expects.

The gizmo systems order before selection exactly as live
(`drive_gizmo_interaction` claims the pointer first) — assert that
ordering holds: a press on a gizmo handle never mutates the selection.
