---
id: viewer-audit-stale-globaltransform-readers
title: Depth-reconstructing passes and the interest camera read a frame-old camera pose
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Eleven systems in the scene crate read the camera's `GlobalTransform` in
`Update`, where `.after(WorldPhase::CameraPositioned)` buys ordering but **not
freshness** — propagation runs in `PostUpdate`. Harmless for a 3000 m sky dome;
a real defect in four places:

- `sl-viewer-world-scene/src/underwater_fog.rs:143` builds `world_from_clip`
  from a frame-old pose and samples a depth buffer rendered from the current
  one;
- `sl-viewer-world-scene/src/water_exclusion.rs:246` slaves the mask camera to a
  frame-old pose while promising pixel-for-pixel alignment at `:238`;
- `sl-viewer-world-view/src/physics.rs:2292` — `sync_dynamic_colliders` is
  `.after(drive_physical_objects)`, but that system writes local `Transform`
  (`:1022`) and propagation is in `PostUpdate`, so the moving-collider set is
  systematically one frame behind the rendered pose. The same file explicitly
  handles this hazard for seats at `:152`;
- `sl-viewer-world-view/src/session.rs:179` — the interest camera reported to
  the simulator, registered with **no** `.after(WorldPhase::CameraPositioned)`
  at all (`sl-client-bevy-viewer/src/lib.rs:2283`). At its ~45 Hz cadence one
  frame is a whole report interval.

`camera.rs:1072` already names the fix: read `&Transform`, since the camera has
no parent.

Two ordering claims in the same family that are misleading rather than wrong:
`physics.rs:2259` says `sync_raycast_index` "runs after the collider is
installed", but `apply_static_colliders` installs via `Commands`, so it is
picked up a frame later via `Added<StaticCollider>`; and
`sl-viewer-world-view/src/media_prim.rs:200` — `claim_media_wheel` reads hover
state that `hover_media_faces` writes with only a
`.before(WorldPhase::CameraOrbited)` edge between them, so which frame's pixel
the wheel lands on is scheduler-order-dependent.
