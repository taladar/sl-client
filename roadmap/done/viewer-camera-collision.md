---
id: viewer-camera-collision
title: Camera collision
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

Camera **collision**: keep the third-person camera from clipping through walls
and terrain by pulling it in toward the focus point when the line of sight is
obstructed, and easing back out when clear. Uses the same scene the `P`-pick
raycast already queries.

Reference (Firestorm, read-only): `llagentcamera`
`calcCameraPositionTargetGlobal` occlusion pushback.

## Done

`src/camera.rs` `collide_camera`. In third person the camera casts a ray from
the focus out toward the eye; if a world surface is nearer than the eye, the eye
is pulled in to just short of it (`COLLISION_PADDING` clearance), and eased back
out as the line of sight clears (via the pose smoothing). Uses the same
`MeshRayCast` the `P` crosshair pick and the alt-click focus use. Reference
occlusion pushback (`calcCameraPositionTargetGlobal`).
