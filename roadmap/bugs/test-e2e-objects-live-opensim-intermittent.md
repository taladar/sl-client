---
id: test-e2e-objects-live-opensim-intermittent
title: e2e_objects on the live OpenSim fails now and then, two ways
topic: test
status: bugs
origin: gridspec-object-rez-derez (2026-10-10)
refs: [gridspec-object-rez-derez, test-unbounded-waits-in-test-harnesses]
---

`a_prim_is_taken_and_rezzed_again_on_a_live_grid` (`e2e_objects`,
`SL_E2E_GRID=opensim`, `--profile live`, release) was run some twenty times
on 2026-10-10 while a third failure of its own was found and fixed (it
aimed at an inventory row before the search had narrowed the list). Two
others were left, neither understood.

## The first ground placement never settles

Three of eleven runs. `rez_a_prim`'s place on the ground at about
`<131.2, 121.1>` in Default Region fails after its 120 s with
`GroundTimedOut { failed_check: Stable, frames: ~5200 }`, in the
in-process backend, some two seconds after login; the viewer logs
`camera: framing … from 7.42 m` once and nothing else. `Stable` is
`AimStage::Settling` (`sl-viewer-automation/src/ground_aim.rs`, `step`):
the camera never holds still for two polls, or `view` or the viewport is
never there — the run had `sl_viewer_automation=debug` asked for and
logged no "camera still: a point moved", which either rules the first out
or means the filter was not in force; that was not checked. The eight runs
that passed took about 30 s each.

## A right click on a just-rezzed object opens no pie

Three runs in a row (two on OpenSim, one on aditi in the process backend),
then none in eleven. The object the drag put back is found by name, its
`open_pie` reports success, and no pie is in the tree ten seconds later —
only the hover tip of that object, so the pointer is on it. `open_pie`
plays a right click and does not look for a pie
(`sl-viewer-automation/src/executor/world.rs`, `WorldAct`). The viewer's
right-click path has two exits that said nothing and now say so at debug
(`sl-viewer-ui-context-menus/src/avatar_menu.rs`,
`resolve_right_click_pick`): a pick that struck nothing, and a face the
surface ray finds nothing of — which a prim whose mesh is a frame old may
well be. Not caught with the log on.

## To do

- Reproduce each with `RUST_LOG` at `info` and at `debug` for
  `sl_viewer_automation`, `sl_viewer_ui_context_menus` and
  `sl_viewer_inventory`, and read which exit it took; make sure the filter
  reaches an in-process viewer first.
- `open_pie` should end when a pie is open, not when the click is played.

## Seen again (2026-10-11)

`a_link_too_far_apart_on_a_live_grid` (added by
[[gridspec-object-link-delink]]) met the first failure once in four runs
with `SL_E2E_GRID=opensim`: its first ground placement, at about
`<131.2, 121.0>`, timed out after 120 s with
`GroundTimedOut { failed_check: InViewport, frames: 5545 }` — the same
spot and the same stage, reporting the other check. The three other runs
passed in 34 s each. (Two earlier failures of that test were its own: a
shift-select left with an action's ten seconds, where the camera needs
longer to come round to a prim on a live grid.) On a live grid the test's
two prims land some twelve metres apart, because each placement derives
its spot from the clock; untidy, and not what fails.
