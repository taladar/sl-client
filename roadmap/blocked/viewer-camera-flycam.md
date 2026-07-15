---
id: viewer-camera-flycam
title: Flycam mode (first-class free camera)
topic: viewer
status: blocked
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
