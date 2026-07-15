---
id: viewer-camera-collision
title: Camera collision
topic: viewer
status: blocked
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
