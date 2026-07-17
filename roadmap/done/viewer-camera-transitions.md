---
id: viewer-camera-transitions
title: Smooth camera-mode transitions
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-mouselook]
---

Context: [context/viewer.md](../context/viewer.md).

Smooth interpolated **transitions** between camera modes — mouselook ↔
third-person ↔ flycam — rather than snapping. Blend camera position/target over
a short ease so entering and leaving mouselook, or dropping into flycam, reads
as a glide.

Reference (Firestorm, read-only): `llagentcamera` `cameraOrbit`/`cameraZoom`
smoothing and the mode-change easing.

## Done

`src/camera.rs` `apply_pose` + the mode-switch seeding. The camera keeps a
smoothed eye / focus that eases toward each mode's desired pose over a ~0.1 s
half-life, so third-person orbit / zoom and the third-person↔mouselook cross
glide rather than snap. Mode switches seed the new mode's state from the old
pose (a dropped-into flycam keeps the aim; leaving mouselook restores an orbit
just outside the head), and the zoom-through into mouselook seeds the aim from
the current view direction. Two exceptions are deliberate, matching the
reference: mouselook aim does **not** lag (only its eye position is smoothed),
and **leaving flycam warps** (the flycam and follow poses are unrelated, so
interpolating between them would fly through the scene).
