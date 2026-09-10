---
id: viewer-camera-reset-on-distant-teleport
title: A distant teleport leaves the camera at coordinates that mean nothing in the new region
topic: viewer
status: done
origin: user report while reviewing viewer-arrival-orientation-snap (2026-09-09)
refs: [viewer-arrival-orientation-snap]
---

Context: [context/viewer.md](../context/viewer.md).

Nothing reset the camera on arrival. `reset_camera_view` (`camera.rs`) is
exactly the right reset — leave flycam, focus the avatar, default orbit — but it
was bound to **Escape** alone, and no teleport path called it.

That shows worst in **flycam**, where `drive_flycam` owns the `ViewerCamera`
entity's `Transform` and integrates input onto it. Nothing re-bases or resets
that transform on a region change, so after a distant teleport the camera keeps
its Bevy coordinates — *the same region-local spot in a region it has never
seen*. Teleport from a skybox to the ground (or the reverse) and the view stays
2000 m from anything that streamed. A `FocusTarget::Point` from an alt-click has
the same problem: the object it named was purged with the world, and the camera
goes on orbiting a dead point.

## What now happens

`slam_arrival_facing` (`sl-viewer-world-view/src/arrival.rs`) resets the camera
on a **distant** teleport: `CameraMode::ThirdPerson` (leaving flycam),
`FocusTarget::Avatar`, `CameraRig::reset_orbit`, and un-seeded so it arrives
whole rather than gliding across the world. This is the reference's
`resetView(true, true)`, which likewise leaves its joystick flycam
(`handle_toggle_flycam`) and calls `changeCameraToDefault()`; the reference
spreads the same intent across two paths (`setFocusOnAvatar(true, false)` on the
cross-region arrival, `resetView` under `FSResetCameraOnTP` on the intra-region
one).

Which arrivals reset is carried by the new `Arrival` enum on
[[viewer-arrival-orientation-snap]]'s `Event::AgentArrived` — `Continued`
(login / crossing), `NearTeleport` (the world was kept and re-based),
`DistantTeleport` (the world caches were cleared, the `RegionChanged`
`world_reset`). The facing slam applies to either teleport reach; **only
`DistantTeleport` resets the camera**.

## Why only the distant reach

Three reasons, recorded because the narrow rule is the point and a later reader
would otherwise "fix" it into resetting on every teleport:

- A camera pose is **region-local and has no meaning across regions**. Only a
  distant teleport purges the scene the pose referred to, and the same
  coordinates then name something unrelated.
- Inside a region, or to a neighbour, the pose usually **already frames the
  destination** — for a double-click teleport that framing is *how the
  destination was named*. Resetting would discard the aim the user just chose.
- The asymmetry settles the doubtful cases: a reset the user wanted and did not
  get is one **Escape** away, while nothing brings back a framing an over-eager
  reset threw out.

No preference guards it: the only case it fires in is one where the pose is
meaningless by construction, so an "off" switch would offer to keep something
worth nothing rather than a genuine choice.

Tests: `arrival.rs` — a distant teleport takes a parked flycam with a point
focus and a user-set orbit back to the default rear view; a near teleport and an
intra-region `TeleportLocal` leave all three exactly as they were. The
fake-grid `client_end_to_end` teleport pair (already pinning `world_reset` at
both reaches) now pins the matching `Arrival` alongside it.

**Needs live verify** (local OpenSim, two regions): fly the flycam well away
from the avatar, teleport to the far region, and confirm the camera is back
behind the avatar; then double-click-teleport within a region and confirm the
camera keeps the framing that aimed it.
