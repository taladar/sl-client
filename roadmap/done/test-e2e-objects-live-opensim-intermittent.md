---
id: test-e2e-objects-live-opensim-intermittent
title: e2e_objects on the live OpenSim fails now and then, two ways
topic: test
status: done
origin: gridspec-object-rez-derez (2026-10-10)
refs: [gridspec-object-rez-derez, test-unbounded-waits-in-test-harnesses,
  gridspec-object-link-delink,
  viewer-camera-framing-ends-inside-the-framed-object,
  viewer-automation-ground-reveal-eye-under-terrain]
---

## Done (2026-10-11)

Neither failure was the grid's, and neither was one thing. Reproduced in
soaks of the live tests with `RUST_LOG` at debug for
`sl_viewer_automation`, `sl_viewer_world_view::camera` and
`sl_viewer_ui_context_menus` — the automation's own lines land in the
stage's `grid.log`, not in a viewer's `viewer.log`, which is why the first
attempt at this "logged nothing". Five causes, in the order they were
found:

- **A run that fails leaves its prim where it aimed, and the next run aims
  there.** A live placement picked one of six spots from the clock, so a
  cube left on one of them stood on the ground the next run to draw that
  spot wanted, for its whole two minutes (`ReceivesEvents`, the probe
  finding an object). A placement now tries twelve spots, twenty seconds
  each, starting from the clock's; and a live test first **sweeps** what
  earlier runs left: it selects the avatar's own half-metre root prims
  within 12 m, reads each one's name off the Build window, and deletes the
  ones whose name ends in the marker every prim these tests name on a live
  grid now carries — or that are still called "Object". Nothing is deleted
  for being owned, near or small alone: the avatar may be standing among
  prims of its own.
- **A prim rezzed on a framed point of the ground brought the camera down
  onto the point.** The camera keeps colliding while it frames, and the new
  prim's surface was at the focus. Camera collision now leaves the framed
  sphere clear and never steps back into it.
- **The framing that followed looked up from under the ground.** A framing
  looks "from the side the camera is on", and the camera was now a quarter
  of a metre under the new prim's centre: the eye went 1.6 m straight
  down, under the ground and the water, which is the frame of water the
  failures' screenshots showed. A framing never looks from below its
  target's level now (`framing_side`).
- **A prim rezzed a hand's width from the last framed point stopped the
  camera inside itself**, at the framed sphere's surface. From in there the
  GPU pick still names the prim and the ray that refines it finds only the
  backs of its faces — "the ray found no surface", thirty-six right clicks
  in a row, and no pie. This is the second failure below. What obstructs
  a framed view right at the edge of the clear sphere is now taken for
  touching what is framed, and the cast starts over from further out, as
  far as twice the sphere; a wall that stands clear of the sphere stops
  the camera as it did (the unit tests with a prim behind a wall hold it
  to that).
- **An aim asked for a framing once.** Whatever lost it — the collisions
  above, in these runs — left the aim waiting out its deadline. Both aims
  ask again once the camera has settled for 150 polls with no click
  reaching the target, and an object aim whose eye stays inside its target
  does too.

Also: `open_pie` ends when a pie is open and plays the click again while
none is (it ended when the click was played); the link test's second prim
stands on its first on a live grid rather than on a second point of ground
that may have grass on it; the camera says at debug what it orbits and in
which mode whenever either changes. And the link test and the sweep delete
a prim **by its pie**: the Delete key deletes the selection only while the
world has the keyboard and the inventory has shown the viewer a Trash, and
does nothing without a word otherwise — on aditi it left the link test's
prim standing, and every later test's sweep failing to remove it the same
way.

With everything above in: the whole of `e2e_objects` on the fake grid, two
rounds of its four live tests on OpenSim and one on aditi all passed, each
live test on two backends, with no right click played twice. Seven more
OpenSim rounds and one more on aditi passed on the way, before the
collision rule was narrowed from "twice the sphere" to "what touches it".

Left open: [[viewer-camera-framing-ends-inside-the-framed-object]]'s own
observation — a glide *through* a cube on the way to a framing, on aditi —
is not this and was not looked at; a right click whose refining ray
misses still opens nothing in the viewer, where it could open the object's
pie without a surface; and the Delete key's two silent conditions are the
viewer's, not only a test's, to say something about.

## As filed

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
