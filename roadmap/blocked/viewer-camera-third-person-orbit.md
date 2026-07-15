---
id: viewer-camera-third-person-orbit
title: Camera mode machine + third-person orbit
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Introduce the **camera-mode state machine** and the **third-person** mode:
orbit / pan / zoom around the avatar. The camera consumes input **actions**
([[viewer-input-action-map]]) rather than raw keys, and sets the viewport that
render priority / LOD already report against. This is the root of the camera
cluster — mouselook, flycam, collision, focus and presets all extend the mode
machine it introduces.

The debug fly-camera in `camera.rs` becomes a first-class **flycam** mode in
[[viewer-camera-flycam]]; this task carves out the mode-machine scaffolding it
plugs into.

Reference (Firestorm, read-only): `llagentcamera.cpp/h` (mode machine, orbit /
pan / zoom), `llfloatercamera`.

Builds on: the existing `camera.rs` fly-camera and `session.rs` viewport
reporting.
