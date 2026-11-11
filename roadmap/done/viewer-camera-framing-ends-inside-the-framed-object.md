---
id: viewer-camera-framing-ends-inside-the-framed-object
title: A framing glide can carry the camera inside the object it frames
topic: viewer
status: done
origin: viewer-automation-host-thread-per-viewer aditi runs (2026-10-02)
refs: [viewer-automation-host-thread-per-viewer, test-e2e-sweep-live-grid]
---

Context: [context/automation.md](../context/automation.md).

## What was seen

aditi, Morris, `two_residents_befriend_trade_rights_and_contest_one_prim`
watched on the process backend (`SL_E2E_WATCH=1`). Alpha stood at about
(187, 149) with Beta 2 m ahead of it, and had just rezzed a 0.5 m cube at
about (190.5, 140.8). After the rename, the test presses `Escape` twice (the
second resets the camera to the rear view, gliding) and re-selects the cube.
The cube is under the selection rig, so the aim reveals it:
`camera: framing … from 1.61 m`, 0.55 s after the reset.

The person watching saw Alpha's camera move **smoothly into the cube**, and
out of it again later. From inside, with back faces culled, the GPU pick sees
through the cube, so no point on it takes a click. The run still passed.

## What is known

- 1.61 m is right for the cube: `framing_distance` of its 0.43 m bounding
  radius at 60° and 16:9. The framed pose is outside the cube.
- Collision acts on the target eye, not the eased one, and its exemption for
  the framed object's own prims is keyed on the target focus, which equals
  the framed centre for the whole glide. Nothing but alt-drag, the wheel or a
  new framing writes `CameraRig::point_offset`.
- The same sequence on the fake grid
  (`a_ground_point_at_the_edge_of_the_view_is_revealed_and_rezzed_on`) glides
  in along a straight line and stops outside. So something specific to aditi
  (content, timing, the reset glide still running) is involved.
- In process, some reveals after a reset show a different on-screen motion,
  ramping up from 5 to 54 px a poll before settling instead of decaying from
  the first frame. Those runs went on to fail `ReceivesEvents` or
  `InViewport`.

The aim now waits out an eye inside its target after a reveal
(`world_aim.rs`, `eye_inside`), so this costs time rather than a failure. The
camera should not do it at all.

## Related, fixed (2026-10-11)

[[test-e2e-objects-live-opensim-intermittent]] found two ways the camera *ended*
inside a prim, neither of them a glide: collision with a prim rezzed on or
beside a framed point stopped the eye at that point, and the framing after it
looked from wherever that had left the eye. Collision now leaves the framed
sphere clear, passes what touches it, and a framing never looks from below its
target. The glide seen here, into a cube and out again, is still to be
explained.

## What to do

- On aditi, log the camera's target eye, eased eye, focus and mode every
  frame from the `Escape` reset to the end of the framing glide. Find which
  of them enters the cube's box.
- Fix that cause, and add a test with the same reset-then-frame sequence that
  asserts the eased eye never enters the framed box.

## What was found (2026-11-11)

The third-person pose now has a per-frame trace (target `camera_glide`, at
`trace`: focus, target and eased eye and focus, framed sphere), on every
frame the pose moves. Three aditi runs of the test (process backend,
`RUST_LOG=info,camera_glide=trace`) all passed; one went through the exact
sequence, a rez framing at 7.42 m, `Escape`, and the cube revealed at
1.61 m 0.8 s later. Its framing glide ran radially in from the rear view and
stopped at 1.61 m. **The eye entered no box on any frame.**

The likeliest cause of what was seen on 2026-10-02 is the one
[[test-e2e-objects-live-opensim-intermittent]] fixed nine days later. The
live tests then left their prims behind on the spot the next run aimed at
(that fix added the sweep of leftovers). A leftover touching the fresh cube
was not among the framed prims. Collision from the cube's centre stopped at
it, inside the cube, and the eased eye glided smoothly there. The reveal
framing again later took it back out. Collision now leaves the framed
sphere clear and passes what touches it.

The trace did show one defect in every run, the fake grid's too: **the
`Escape` reset moved the eye 6–7 m in one frame** before it glided. The
avatar follow eased only the eye's offset from its focus and held the focus
rigidly on the avatar's head, so the eye's offset from the *framed point*
was put onto the head. That was the "different on-screen motion" after a
reset. The follow now carries the look-at point's lag behind the avatar
(`CameraRig::follow_focus`), decaying and riding with the avatar. A settled
follow stays rigid, and a follow that has just begun glides on a straight
line from the eye it had, as the reference's camera animation does.

Tests: `a_reset_into_the_follow_glides_from_the_eye_it_had` (camera unit:
no frame's step over 15% of the glide, a straight path, a rigid settled
follow) and `a_reset_then_a_framing_glides_and_never_enters_the_framed_box`
(world fixture: a rezzed point framed, `Escape`, then a half-metre cube
beside it framed; no frame of the reset jumps and no frame of the framing
puts the eye in the cube).
