---
id: viewer-sit-camera-vehicle-frame-lag
title: Scripted sit camera lags a moving vehicle by a frame (wobble in the driver's view)
topic: viewer
status: done
origin: surfaced fixing viewer-physical-object-motion-not-smooth on aditi (2026-08-06)
refs: [viewer-physical-object-motion-not-smooth, viewer-seated-avatar-vehicle-rubberband]
---

Context: [context/viewer.md](../context/viewer.md).

When a seat sets a **scripted sit camera** (`llSetCameraEyeOffset` /
`llSetCameraAtOffset`), the driver's camera rode the seat's frame-late
`GlobalTransform` instead of its current-frame pose, so on a **moving vehicle**
the whole view lagged the vehicle by one frame and the vehicle **wobbled in the
driver's field of view** on each of the object's dead-reckon / snap corrections.
This is the sit-camera counterpart of the seated-rider fix
([[viewer-seated-avatar-vehicle-rubberband]]), which already composes the rider
from the seat's current-frame locals; the camera path was missed.

## Done

`sit_camera_pose` (`camera.rs`) now composes the seat's world pose
**this frame** from the chain of local `Transform`s up its `ChildOf` parents
(`seat_world_transform`, the same helper `place_seated_avatars` uses), rather
than reading the seat entity's `GlobalTransform` (which Bevy only recomputes in
`PostUpdate`, so it is a frame stale). The scripted eye / focus offsets are then
`transform_point`-composed onto that current-frame pose, so the driver's
viewpoint is locked rigidly to the seat: the vehicle holds its place on screen
and only the world jitters past it.

Mechanics / gotchas:

- `seat_world_transform` and its `SeatChainQuery` alias were made `pub(crate)`
  (they lived in `avatars.rs` for the rider fix). `SeatChainQuery` gained a
  `Without<ViewerCamera>` filter so its read-only `&Transform` access does not
  conflict with `position_camera`'s mutable camera-transform query (the same
  reason `AvatarTransformQuery` carries `Without<ViewerCamera>`).
- Static seats are unaffected: for a non-moving seat the current-frame compose
  equals the `GlobalTransform`, so only moving vehicles change behaviour.

Live-verified on aditi (2026-08-06): driving the "Kart 1.0" — whose seat uses a
scripted sit camera (a one-per-frame diagnostic confirmed the
scripted-sit-camera path engaging on sit and disengaging on stand) — the vehicle
now holds its place in view; reported **"much smoother."** The vehicle's
*world-space* motion is unchanged (still the sim's sparse ~14 Hz stream,
reference parity — see [[viewer-physical-object-motion-not-smooth]]); this fix
hides that jitter where the driver is looking, exactly as the reference viewer's
seat-locked camera does.

Note the scripted **speed-based zoom** (`llSetCameraParams` follow-cam) is a
separate, still-unimplemented mechanism —
[[viewer-scripted-followcam-llsetcameraparams]] — which must be built on this
same rigid current-frame seat pose so it does not reintroduce the jitter.
