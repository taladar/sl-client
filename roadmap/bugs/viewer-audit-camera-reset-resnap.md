---
id: viewer-audit-camera-reset-resnap
title: Escape out of flycam interpolates between two unrelated poses
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-view/src/camera.rs:539` — `reset_camera_view` (Escape) calls
`rig.reset_orbit()` but never `rig.resnap()`, so leaving flycam via Escape
interpolates the camera between two unrelated poses.

`toggle_flycam` (`:522`) and `clear_sit_camera_on_stand`
(`sit_camera.rs:166`) both call `resnap()` for exactly this reason, with a
comment saying an interpolation there "just flies through the scene".

A test asserting `rig.seeded == false` after Escape in flycam fails today.

Related, same file: `apply_pose` documents the invariant that an unguarded
camera write "defeats every change-driven consumer that gates on camera
movement" (`:1294`) and guards at `:1305` — but the mouselook branch bypasses it
entirely (`*transform = posed;`, `:1144`) and flycam auto-level slerps every
frame regardless of whether the horizon is already level (`:948`). Two of three
camera modes break the stated invariant.
