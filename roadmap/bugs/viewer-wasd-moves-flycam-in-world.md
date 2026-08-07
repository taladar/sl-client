---
id: viewer-wasd-moves-flycam-in-world
title: WASD appears to drive the flycam during normal play (debug-camera leftover?)
topic: viewer
status: bugs
origin: user report (2026-08-07), noticed during double-click-teleport live testing
refs: [viewer-camera-flycam, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

During ordinary (third-person) play the **WASD** keys seem to move the
**flycam** rather than (or in addition to) walking the avatar — the user
suspects a **leftover from the early debug fly-camera**.

By design the per-[`InputMode`] binding profiles
(`input_action.rs` `InputBindings::default`) bind WASD to the *avatar* in
`ThirdPerson`/`Mouselook` and to the *camera* only in `Flycam`
([[viewer-camera-flycam]]). So one of these is happening and needs pinning down:

- the viewer is (unexpectedly) in `CameraMode::Flycam` during normal play — e.g.
  it starts there, or something flips it — so WASD correctly drives the flycam
  but the mode is wrong; or
- a debug fly-camera / free-camera system still reads the raw WASD keys directly
  (bypassing the action-map profiles) and moves a camera regardless of
  `CameraMode` — the "early debug camera" the user remembers.

Investigate: log the live `CameraMode` and grep for any camera system reading
`KeyCode::KeyW`/`KeyA`/`KeyS`/`KeyD` (or `Action::Move*`) outside the
mode-gated flycam driver. Expected: in third person, WASD walks the avatar and
never translates the camera.

Reference (Firestorm, read-only): the avatar-driving vs. flycam key split in
`keys.xml` / the movement controller.
