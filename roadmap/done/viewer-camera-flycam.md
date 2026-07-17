---
id: viewer-camera-flycam
title: Flycam mode (first-class free camera)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the existing debug fly-camera (`camera.rs`) into a **first-class flycam
mode** in the camera-mode state machine: a free 6-DOF spectator cam driven by
input **actions** ([[viewer-input-action-map]]), participating in mode
transitions ([[viewer-camera-transitions]]). This is the mode the SpaceNavigator
([[viewer-input-spacenav-camera-mapping]]) drives, and the
[[viewer-camera-flycam-floater]] controls.

**Region-crossing correctness (do not skip):** the flycam holds its own absolute
position decoupled from the avatar, so when the coordinate origin shifts on a
region crossing it must be **rebased along with the rest of the scene** — hook
the same origin-shift the scene mirror uses. Otherwise the flycam jumps by one
region (256 m) at the crossing.

Reference (Firestorm, read-only): `llviewerjoystick` flycam, `llagentcamera`
free camera.

Builds on: the existing `camera.rs` fly-camera.

## Done

`src/camera.rs` `drive_flycam` + `src/flycam_ui.rs`. Flycam is a first-class
`CameraMode`: a free 6-DOF spectator camera whose eye is the entity transform.
Orientation composes **local-frame** quaternion deltas (gimbal-free, roll works)
with the reference's **AutoLeveling** easing the horizon level each frame
(levels the right axis, so it is stable and can loop over the top). Entering
flycam keeps the current pose; leaving it **warps** (does not glide) to third
person, matching the reference. A "**Stop flycam**" button shows in flycam and
leaves it (the requested wording). Region-crossing rebase is handled by
`recenter_terrain` translating the flycam transform (never rotating it) and
resnapping the rig, so a crossing cannot yaw the view.
