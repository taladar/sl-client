---
id: viewer-audit-stale-globaltransform-readers
title: Depth-reconstructing passes and the interest camera read a frame-old camera pose
topic: viewer
status: done
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

## Fixed (2026-08-29)

All four readers now take the **current-frame `Transform`**. Every one of the
four entities is a top-level entity, so its `Transform` *is* its world pose and
the substitution is exact, not an approximation:

- the camera is spawned with no parent (`sl-client-bevy-viewer/src/lib.rs`),
  which is what `camera.rs`'s own anchor note already relied on;
- a physical prim is a physical *root* — `is_physical_root` requires
  `parent_id == 0` and no attachment point — so it is never a linkset child.

The four:

- `update_underwater_fog` reads `&Transform`, so `world_from_clip` is built
  from the same pose the depth buffer it unprojects was rendered from. This was
  the worst of the four: the reconstruction error is exactly the frame's camera
  motion, so every fogged fragment is displaced by however far the camera moved
  — nothing while parked, and growing with speed.
- `sync_water_exclusion_camera` copies the main camera's `Transform` straight
  across (neither camera has a parent, so it is a plain assignment and the
  `Transform::from_matrix(global.to_matrix())` round-trip goes away). The mask
  now genuinely lines up pixel-for-pixel with the view that samples it, which
  is what the function's own doc promised.
- `sync_dynamic_colliders` reads each physical prim's `Transform`, so camera
  collision and the prim–prim collision sounds test the pose the same frame
  draws instead of trailing it — the hazard the seat placement in the same
  crate already handles this way.
- `report_camera_interest` reads the camera's `Transform` **and** is now
  registered `.after(WorldPhase::CameraPositioned)`; it had no camera edge at
  all. At its ~45 Hz cadence the missing edge cost up to a whole report
  interval, so the simulator's interest list trailed the viewpoint the entire
  time the camera was moving.

The two misleading ordering claims are corrected too:

- the `sync_raycast_index` registration comment no longer reads as
  "same frame". `apply_static_colliders` installs through `Commands`, so a
  freshly built collider is picked up on the *next* frame's
  `Added<StaticCollider>`. That lag is harmless — a collider that does not exist
  yet cannot be missing from the index — but the `.after` edges are not what
  makes it correct, and the comment now says so.
- `claim_media_wheel` gained the edge it was missing:
  `.after(MediaPrimSystems::Drive)`. It decides whether to claim the wheel from
  `MediaFocus::hover` / `hover_pixel`, which `hover_media_faces` writes inside
  that set; with only the `.before(CameraOrbited)` edge the two were unordered,
  so which frame's hovered pixel a scroll landed on was scheduler-order-
  dependent. No cycle: the media `Drive` set's only in-edge is
  `MediaEngineSystems::Pump`, which has no camera constraint.

Not changed: the other seven `GlobalTransform` readers named as harmless (the
sky dome and friends), and `hover_media_faces`, whose ray needs a `Camera` +
`GlobalTransform` pair for `viewport_to_world` — the same class, but a hover
ray a frame behind picks the same face at any plausible camera speed.

### Tests

One regression test per fixed reader, all built the same way: stage the entity
with a `Transform` and a deliberately *different* `GlobalTransform` — the exact
shape of every `Update` frame after the thing has moved — and assert the system
read the `Transform`.

- `underwater_fog::tests::reads_the_current_frame_camera_pose` — the fog eye is
  this frame's pose, and `world_from_clip` unprojects the near-plane centre
  (clip `z = 1`, Bevy's perspective being reverse-Z infinite) next to that eye
  and nowhere near the stale one.
- `water_exclusion::tests::slaves_the_mask_to_the_current_frame_pose`.
- `physics::tests::dynamic_colliders_take_the_current_frame_pose` — probes the
  published set with a ray: it hits the unit ball at this frame's pose, and the
  stale pose is placed past the ray's reach so a stale read is a miss, not a
  near-miss.
- `session::tests::interest_camera_reports_the_current_frame_pose`.

### Verified

- The four unit tests above, plus the full `sl-viewer-world-scene`,
  `sl-viewer-world-view` and `sl-client-bevy-viewer` suites.
- A local OpenSim session, which is what settles the two scheduling changes: a
  `.after` edge that closed a cycle would panic the app before the first frame.
  Walking underwater in it shows no background flicker.
- An aditi session, walking underwater there too.

Those last two close
[`viewer-underwater-fog-background-flicker`](viewer-underwater-fog-background-flicker.md),
whose root cause this turned out to be — a reprojection error proportional to
camera speed is exactly "the background behind the fog flickers while walking".
Both grids were checked because that bug's origin line names the local grid but
the sighting was remembered as aditi.
