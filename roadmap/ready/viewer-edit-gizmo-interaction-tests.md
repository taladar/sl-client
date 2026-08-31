---
id: viewer-edit-gizmo-interaction-tests
title: Gizmo handle drags — press, constrain, stream, release
topic: viewer
status: ready
origin: user request (2026-07) — build floater / gizmos named the priority
points: 8
refs: [viewer-build-undo-redo]
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

First slice landed (2026-08-31): the first drag —
`a_translate_x_drag_moves_only_x_and_sends_one_update` selects the
fixture prim, enters build mode, finds the +X cone by its new
`edit-gizmo:translate-x` `Name` (every rig handle is now named
`edit-gizmo:<part-slug>`), drags along the pivot→cone screen direction,
and asserts X-only motion plus exactly one position-only `UpdateObject`
on release. Remaining: snap / grid frame, rotate ring, stretch streaming
and `stretch_both`, shift-drag copy, Alt yield, and the blocking-UI
guard.

The densest manual-retest burden in the viewer, and the user's named
priority. Drive `gizmos.rs::drive_gizmo_interaction` headlessly: project a
handle's position via `world_to_viewport`, place the cursor
(`set_physical_cursor_position`), press left, move across frames, release.

Assert per tool:

- translate drags move `ObjectSlMotion` along the constrained axis only,
  honouring `EditToolState.snap`/`grid_unit`/`GridFrame`;
- rotate ring drags produce the constrained rotation;
- stretch face/corner drags stream `SlCommand` updates at the reference
  10 Hz throttle and respect `stretch_both`;
- release sends the final update;
- shift-drag queues the duplicate copy exactly once;
- Alt yields the pointer to the camera;
- a press over blocking UI never begins a drag
  (`pointer_over_blocking_ui`).

Assert both the local `Transform`/`ObjectSlMotion` **and** the recorded
`SlCommand` contents — the reaction the server would see.
