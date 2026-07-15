---
id: viewer-camera-transitions
title: Smooth camera-mode transitions
topic: viewer
status: blocked
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
