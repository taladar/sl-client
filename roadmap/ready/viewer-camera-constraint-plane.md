---
id: viewer-camera-constraint-plane
title: Region/script camera constraint plane
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
refs: [viewer-camera-script-control, viewer-qol-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

The simulator pushes a `CameraConstraint` message (a plane in region
space) when the camera enters a constrained volume — region/parcel camera
limits and the constraint half of scripted camera control. `sl-proto`
already decodes it (`Event::CameraConstraint`), but no viewer system
consumes it, so the third-person camera happily pulls back through
constraint volumes the reference viewer respects.

Scope:

- Consume `Event::CameraConstraint` and keep the latest constraint plane
  per agent.
- Clamp the third-person orbit camera's position against the plane
  (reference: the camera collides with the constraint like a wall; the
  clamp eases rather than snaps).
- Expire/clear the constraint the way the reference does (new plane
  replaces old; teleports and region crossings reset it).
- Honour the **Disable camera constraints** toggle from
  [[viewer-qol-toggles]] as the opt-out.

Reference (Firestorm, read-only): `process_camera_constraint`
(`llstartup.cpp` handler registry, `llviewermessage.cpp`),
`LLAgentCamera` constraint handling.

Builds on: the third-person orbit camera (done) and the session event
stream. The follow-cam half of scripted cameras is separate:
[[viewer-camera-script-control]].
