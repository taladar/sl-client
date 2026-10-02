---
id: viewer-camera-framing-ends-inside-the-framed-object
title: A framing glide can carry the camera inside the object it frames
topic: viewer
status: bugs
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

## What to do

- On aditi, log the camera's target eye, eased eye, focus and mode every
  frame from the `Escape` reset to the end of the framing glide. Find which
  of them enters the cube's box.
- Fix that cause, and add a test with the same reset-then-frame sequence that
  asserts the eased eye never enters the framed box.
