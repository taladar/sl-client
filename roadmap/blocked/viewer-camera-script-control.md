---
id: viewer-camera-script-control
title: Script control of the camera (llSetCameraParams / follow-cam)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit, viewer-permission-request-dialog]
---

Context: [context/viewer.md](../context/viewer.md).

**Script control of the camera** — `llSetCameraParams` / follow-cam constraints
and eye/focus-offset overrides driven by the object, so scripted vehicles and
HUDs can drive the view. Apply the follow-cam parameters against the camera mode
machine.

Gated on a `PERMISSION_CONTROL_CAMERA` grant from
[[viewer-permission-request-dialog]] (or an auto-grant when the controlling
object is a worn attachment, a seat, or under an accepted experience — see
[[viewer-experience-permission-dialog]]).

Reference (Firestorm, read-only): `llfollowcam`, follow-cam properties in
`llviewermessage`, `llagentcamera` camera-param application.
