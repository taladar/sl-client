---
id: viewer-scripted-followcam-llsetcameraparams
title: Apply scripted follow-camera parameters (llSetCameraParams)
topic: viewer
status: ready
origin: raised during viewer-physical-object-motion-not-smooth aditi testing (2026-08-06)
refs: [viewer-physical-object-motion-not-smooth, viewer-seated-avatar-vehicle-rubberband]
---

Context: [context/viewer.md](../context/viewer.md).

A scripted object can drive the agent's camera with `llSetCameraParams` (once
the agent grants `PERMISSION_CONTROL_CAMERA`) — e.g. a vehicle that
**zooms the camera out as it accelerates** (drives `FOLLOWCAM_DISTANCE` off its
speed), plus behindness / lag / focus+position offsets. This arrives on the wire
as `SetFollowCamProperties` (released by `ClearFollowCamProperties`) and is
**fully decoded** by `sl-proto` into
`Event::SetFollowCamProperties { object_id, properties }` — every parameter:
`Pitch`, `FocusOffset[XYZ]`, `PositionLag`, `FocusLag`, `Distance`,
`BehindnessAngle`, `BehindnessLag`, `Position[XYZ]`, `Focus[XYZ]`,
`Position/FocusThreshold`, `Active`.

The **viewer never applies it**, though: only the `sl-client-bevy` examples
consume the event, and they just log it. So a scripted vehicle camera (the Kart
1.0's speed-zoom seen on aditi) has no effect in our viewer.

Scope:

- **Grant flow.** The object requests `PERMISSION_CONTROL_CAMERA` via
  `llRequestPermissions` (the `ScriptQuestion` path the viewer already handles —
  `ScriptPermissionPlugin` / `Command::AnswerScriptPermissions`); confirm
  `CONTROL_CAMERA` is actually granted so the sim sends `SetFollowCamProperties`
  at all.
- **Apply the params to the camera rig.** `Active` toggles the follow-cam;
  `Distance` / `BehindnessAngle` / `Pitch` shape the orbit; `Focus*` /
  `Position*` (absolute point or offset) place eye/focus; `*Lag` / `*Threshold`
  are the reference's `LLFollowCam` smoothing (`indra/newview/llfollowcam.cpp`).
- `ClearFollowCamProperties` releases control back to the ordinary follow.

**CRITICAL — build on the rigid seat frame; do not reintroduce the jerk.** The
per-vehicle-update camera jerk was fixed
([[viewer-physical-object-motion-not-smooth]] /
[[viewer-seated-avatar-vehicle-rubberband]]) by locking the camera to the seat's
**current-frame** world transform — composed from the local transforms up the
`ChildOf` chain (`seat_world_transform`), *not* the frame-stale
`GlobalTransform`. When the scripted follow-cam changes `Distance` (etc.) every
frame, those parameters must be applied
**relative to that same rigid, current-frame seat pose** — as a distance/offset
in the seat's frame, eased only within that frame (exactly as `apply_pose`'s
`follow_avatar` mode eases only the eye-offset and never the focus) — NOT
re-derived against a stale global or a world-space-smoothed position. Otherwise
a per-frame `Distance` change would recompute the eye from a laggy base and
bring back precisely the camera-distance jitter this work removed. The `*Lag`
parameters are the script's intentional smoothing of the offset within the seat
frame, not world-space position lag.
