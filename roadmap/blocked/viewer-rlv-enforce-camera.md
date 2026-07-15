---
id: viewer-rlv-enforce-camera
title: RLV — camera restrictions and vision overlay
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state, viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

Enforce the **camera and vision** restrictions against the camera-mode machine
([[viewer-camera-third-person-orbit]]), consulting
[[viewer-rlv-restriction-state]]:

- `@setcam_*` — constrain field of view, min/max distance, and lock the view to
  the avatar; `@camtextures` forces a replacement texture on rendered surfaces;
- RLVa's render effects (`rlveffects.cpp`) — the **vision sphere / blur
  overlay** that limits how far the user can see, a render pass on top of the
  scene.

The camera limits clamp the parameters the camera module already exposes (fov,
distance, avatar-lock), so they hook the mode machine rather than reimplementing
it. The vision-sphere / blur overlay lands on the Phase 22–33 rendering work as
a post/overlay effect driven by the current restriction set.

Reference (Firestorm, read-only): `rlveffects.cpp` (vision sphere / blur),
`rlvhandler.cpp` (`@setcam_*`), `llagentcamera.cpp` (the camera parameters
being clamped).
