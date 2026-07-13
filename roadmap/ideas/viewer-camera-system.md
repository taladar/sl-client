---
id: viewer-camera-system
title: Camera system
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-input-system, viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug fly-camera into the full SL camera model: **mouselook**
(first-person), **third-person** orbit / pan / zoom around the avatar with
focus-on-object, the free **flycam**, smooth transitions and camera collision
(don't clip through walls), plus saveable camera presets.

Include **script control of the camera** — `llSetCameraParams` / follow-cam
constraints and eye/focus-offset overrides driven by the object (permission-
gated), so scripted vehicles and HUDs can drive the view.

Foundational alongside the input system: the camera consumes input actions and
sets the viewport that render priority / LOD already report against.

Reference (Firestorm, read-only): `llagentcamera.cpp/h`, `llfollowcam`,
`llfloatercamera`, `lltoolfocus`, and follow-cam properties in
`llviewermessage`.

Builds on: the existing `camera.rs` fly-camera and `session.rs` viewport
reporting.

Deps: [[viewer-input-system]], [[viewer-ui-framework]].
