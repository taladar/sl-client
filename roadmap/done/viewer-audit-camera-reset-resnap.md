---
id: viewer-audit-camera-reset-resnap
title: Escape out of flycam interpolates between two unrelated poses
topic: viewer
status: done
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

## Fixed

`reset_camera_view` now resnaps when — and only when — it is leaving flycam, so
Escape warps exactly as the flycam toggle does while Escape out of mouselook (or
a plain orbit reset) keeps its glide.

The `apply_pose` invariant turned out to hold in **none** of the three modes,
for two reasons beyond the two the audit named:

- the guard sat *inside* `apply_pose`, which the caller reached as
  `apply_pose(&mut transform, …)` — and `Mut<Transform>` marks the component
  changed on its first mutable deref, so the decision not to write came a deref
  too late. `apply_pose` now takes `&Transform` and returns the pose, and each
  mode writes inside the `if`;
- the guard's rotation half compared with `Quat::angle_between`, which is
  `acos(dot)`. Near `dot == 1` that amplifies the ~6e-8 of `f32` resolution into
  **~8e-4 rad** for a quaternion against *itself* — above the 3e-4 settle
  epsilon, so the test could never say "settled" even when both halves were
  reached. `rotation_moved` compares components instead, where a `θ` rotation
  moves them by `≈ θ/2`.

Mouselook and the flycam's AutoLeveling slerp now route through the same guard.
Two ECS tests pin it per mode (a parked third-person camera and an idle level
flycam each mark themselves `Changed` on no frame at all), and a pure test pins
the `angle_between` trap.
