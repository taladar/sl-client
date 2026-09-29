---
id: viewer-automation-world-aim
title: World actions — pick-verified aiming, reveal, click and pie
topic: viewer
status: done
origin: viewer automation design review (2026-09-28)
points: 8
blocked_by: [viewer-automation-world-model, viewer-automation-synthetic-input]
refs: [viewer-cpu-pick-resolver, viewer-build-tool-row-parity,
  viewer-automation-executor]
---

## Done (2026-09-28)

**Pick probes** (`sl-viewer-world-api/src/pick_probe.rs`): `PickProbes`
asks "what would a click at this pixel hit?" of the viewer's own pick
resolver without clicking — the automation crate must not depend on
`sl-viewer-world-view`, so the two meet in the api crate. The resolver side
(`gpu_pick.rs`, shared by the GPU and the CPU resolver) submits one queued
point a frame under a new `PickPurpose::Probe` and files the answers back as
`ProbeTarget` (avatar / object face / ground / water) plus the world point.
`GpuPicker::take_requests` now takes only the requests at one pixel per
submission and leaves the rest for the next frame: "the last request wins"
was harmless while every consumer asked at the live cursor, and is wrong
once a probe asks about another pixel.

**Framing** (`FrameObject`, `sl-viewer-world-api`; answered by the camera's
`frame_object`): focus an object from the side the camera is on, at the
older reference `handle_zoom_to_object` distance (bounding extent × 2 /
atan of the wider view angle), leaving mouselook / flycam for third person.
Camera collision still applies, which is what lets a reveal put the eye in
front of a wall — but not against the framed object's own prims
(`passes_through`): the focus sits inside it, and a hollow cast from there
stopped at the object's own far side, parking the eye inside the target.

**`WorldAim`** (`sl-viewer-automation/src/world_aim.rs`), polled once a
frame: resolve with `WorldQuery` / `WorldWant::One` (waits for names,
fails on ambiguity) → wait for the camera and the target to hold still →
project the target's box (its geometry holder's transform, which carries
the object scale) and pick candidates: the projected centre, then the
centre and a 3×3 lattice of each face towards the camera, in the viewport,
not under a UI node that would take the click (`UiModel::takes_click_at`,
the world's own `pointer_over_blocking_ui` rule) → probe them one a frame
and take the first that lands on the target (its link children count; an
attachment's rigged submeshes count). With no point on screen
(`in_viewport`) or none landing (`receives_events`), the camera frames the
target once and the aim starts over; then it fails with
`WorldNotActionable`, naming what a click at the centre hit instead
(`covered_by`). A pose change mid-probe restarts the settle.
`screen_projection` gives any node's projected bounds and whether it is on
screen. Intents: click (touch), right-click (pie), hover, select (waits for
the build tool — `ActionabilityCheck::BuildMode`), and drop-from (a drag
from a UI point — an inventory row — onto the target, resting on it while
the drag's world pick catches up). `WorldTarget::input()` is the gesture for
the synthetic input. A **select is judged by the build tool's own resolver**,
not the world pick: a build-mode press is classified by the selection
gesture's `ObjectPicker` (and a press the transform rig's hit test takes is a
handle drag, not a select), so the aim asks that through `SelectionProbes`.

**Build-mode drags**, the scope widened on review (2026-09-29):

- `ManipulatorDrag` drags a transform handle (`ManipulatorHandle`: move
  arrow / pad, rotate ring, stretch face / corner) by a stated amount
  (`ManipulatorAmount`: metres, a pad offset, radians, a factor), on a stated
  side of the snap guide (`SnapRegime`: free — on the axis, the line, or
  inside the ring's tick circle — or grid, well past it), holding the
  modifier keys that pick the rig (`HeldKeys`: `Ctrl` rotate, `Ctrl+Shift`
  stretch, `Shift` a copy on a move). It drives the input itself because the
  keys must be down before the rig they select exists to be planned against.
- The plan is the build tool's own (`ManipulatorProbes`, answered by
  `sl-viewer-edit/src/gizmos/plan.rs`): press points are ones the rig's own
  hit test (`struck_handle`, now shared with the hover) puts on the handle;
  the drag state is `begin_drag`'s for that press; the path is its math run
  backwards, projected to the screen; and the plan predicts the result,
  grid mark or detent included. It refuses a drag whose release would leave
  the window (the last movement would never be seen).
- `WorldSweep` draws the rubber band over the things a locator names: the
  padded union of their projected boxes, started at a corner the selection
  gesture calls empty world, and ready only if the gesture's own rectangle
  test (`sweep_candidates`, via `SelectionProbes`) selects exactly them —
  otherwise `SweepInexact` names what it would miss and what it would catch.
- `ProbeQueue` (world-api) is the one request/answer queue behind
  `ManipulatorProbes` and `SelectionProbes`; the press classification
  (`pressed_object`) is factored out of the live gesture so both use it.

**Protocol**: `AutomationError::WorldNotActionable` (node boxed, so the
error enum stays small), `failed_check` on `WorldTimedOut`, the `build_mode`
check, `ManipulatorRefused`, `ManipulatorTimedOut` and `SweepInexact`.

**Tests**: the fixture world (`automation_world_aim.rs`, CPU resolver):
a prim behind a wall reveals then takes the click (a `TouchObject` for it,
not the wall), a right-click then opens its pie with no second reveal;
without reveal it fails naming the wall; a prim behind the camera reveals
as `in_viewport`; a hover leaves the pointer where a probe finds the prim;
a select waits for build mode and then selects through the real gesture
(no camera group there, so no reveal to lean on); an aimed drop puts a
notecard into a prim whose centre is under the inventory window. In the
build-tools world (the Build window parked over half the screen): a move by
exactly 1 m with snapping on, 0.7 m past the guide landing on the 129.5 m
grid mark, and a Shift move leaving one copy; under `Ctrl` a ring turning
30° free, then 20° past the tick circle landing on the 50.625° detent; under
`Ctrl+Shift` a face stretch by 0.5 m free, 0.3 m more landing the size on
3.0 m, and a corner ×1.2; a band over two of three prims selecting exactly
those two, and one over the outer two refused with the middle one named.
The full-stack tier (`a_world_aim_is_verified_by_the_gpu_pick`) aims a
right-click at the stock box past the own avatar through the **GPU**
ID-buffer pick of an off-screen window and opens its pie. Unit tests:
candidate geometry, the probe queues, the framing distance, the drag path.

A trap paid for on the way: a camera rotation read back from
`GlobalTransform` is unit length only to `f32` precision, so `1 − |q·q|`
is already ~1.2e-7 for a camera that has not moved — a dot-product
stability tolerance below that never settles. The aim compares rotations
component-wise.

**Not done — grab-drag of the object itself.** Outside build mode the
viewer has no press-drag-release grab of an object (the Move tool of
[[viewer-build-tool-row-parity]]; a touch is still an instantaneous grab +
degrab), so there is nothing for an aimed grab-drag to drive. It lands with
that tool. Every drag the viewer has — the transform handles, the rubber
band, an inventory drop — is covered above.

Context: [context/automation.md](../context/automation.md).

A click on an object must land on *that* object, not on whatever happens to
be in front of it, and a test must not need to know where the camera is.

## Wanted

- Screen projection for a `WorldNode`: on screen or not, projected bounds.
- An **aim point verified by the pick resolver** — the GPU pick
  (`gpu_pick.rs`, `GpuPickResolved`) live, the CPU resolver in headless
  tiers — to hit this object; `covered` when no candidate point does.
- **Reveal**: when no verified point is on screen, frame the object with
  the camera (the viewer's focus-on-object path) and re-aim; restore is not
  implied — the test sees the camera where reveal left it.
- Actions over the synthetic input: click / touch, right-click (the object
  pie opens with this object as its target), hover (hover text shows),
  select in build mode, drag.

The target is resolved with `WorldQuery` and `WorldWant::One`
(`sl-viewer-automation`, [[viewer-automation-world-model]]). It waits for
names and fails on ambiguity. Its `WorldNode` has the region-local position;
the entity to aim at is the `ObjectState` entry for its local id.

Acceptance: in a `WorldTest` fixture a prim hidden behind another is
reported covered and a click on it reveals, then hits it; a right-click
opens the object pie whose stashed target is that prim.
