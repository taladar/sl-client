---
id: viewer-edit-gizmo-interaction-tests
title: Gizmo handle drags — press, constrain, stream, release
topic: viewer
status: done
origin: user request (2026-07) — build floater / gizmos named the priority
points: 8
refs: [viewer-build-undo-redo]
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

Done (2026-08-31): every manipulator in the acceptance list answers a real
cursor in the fixture world, and each case asserts both the local
`ObjectSlMotion` and the `SlCommand` a simulator would have seen.

- **translate** — `a_translate_x_drag_moves_only_x_and_sends_one_update`
  (X-only motion, one position-only `UpdateObject` on release);
  `snapping_lands_a_translate_drag_on_the_grid` (the *same* cursor path,
  out past the snap-guide ruler, lands on a half-metre multiple with
  `snap` on and off it with `snap` off — the pair is the teeth);
  `the_grid_frame_decides_the_translate_axis` (the same +X arrow moves
  along world X in `GridFrame::World` and along a side-standing prim's own
  X in `GridFrame::Local`).
- **rotate** — `a_rotate_ring_drag_turns_the_prim_about_that_axis_alone`
  (a sweep along the ring's own circle turns by exactly the angle swept,
  tilts nothing, moves nothing, and sends one rotation-carrying update)
  and `a_rotate_drag_past_the_detents_lands_on_one` (pulled outside the
  tick circle it lands on a 5.625° detent instead).
- **stretch** — `a_stretch_drag_streams_updates_and_a_short_one_does_not`
  (a thirty-frame face drag streams at the reference's 10 Hz and its last
  update carries the size the prim ended at; a two-frame drag sends only
  the release, and no face drag sets `UNIFORM`);
  `stretch_both_sides_doubles_the_size_and_holds_the_centre` (twice the
  size change, and the centre pinned instead of shifting half the growth);
  `a_corner_stretch_scales_every_axis_by_one_factor` (one shared factor
  across all three sizes, and the one drag that does set `UNIFORM`).
- **modifiers and guards** — `a_shift_drag_leaves_exactly_one_copy_behind`
  (one `DuplicateObjects` at zero offset while the original follows the
  cursor; a plain drag copies nothing), `alt_held_yields_the_pointer_to_
  the_camera`, and `a_press_over_blocking_ui_never_begins_a_drag` over a
  panel in `world_app_with_ui_and_edit`. The last two each open with the
  unmodified drag as a control, so "the prim did not move" can never be a
  fixture that was never grabbable.

Shared harness pieces this grew: `handle_toward` (a rig part's address is
its *part*, and a translate arrow is three entities — so a test says which
way it means), `ring_point` (a point on a ring's drawn circle, read
through its own `GlobalTransform`, which is also a point in the drag
plane, so the ray through its projection hits exactly it), `drag_handle`
(along / across the pivot→handle screen direction — the across component
is what crosses into the snap regime), `selected_fixture`, and
`world_app_with_ui_and_edit`.

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
