---
id: server-world-collision-and-physics
title: Enough physics for the collision, target and volume-detect events
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 13
blocked_by: [server-world-heartbeat, server-world-ecs-store]
refs: [server-physics-integration, server-lsl-lib-physics-vehicles,
  server-world-agent-movement]
---

Context: [context/lsl.md](../context/lsl.md).

[[server-physics-integration]] sizes physics for a real simulator. This
is the fake grid's much smaller share: the minimum that makes the
physics-shaped *events* fire and the physics-shaped *library functions*
return something true, on a grid whose job is to be reproducible rather
than plausible.

Seven events depend on it — `collision_start`, `collision`,
`collision_end`, `land_collision_start`, `land_collision`,
`land_collision_end`, plus `at_target` / `not_at_target` and
`at_rot_target` / `not_at_rot_target`, and `moving_start` /
`moving_end`. So do `llVolumeDetect`, `llCastRay`, `llGetStatus`,
`llMoveToTarget`, `llSetForce`, `llApplyImpulse` and `llPushObject`.

The scope decision this task must make and record: **a real rigid-body
engine, or a purpose-built kinematic one.** Arguments both ways —

- `rapier3d` is pure Rust, deterministic within a build, and would give
  stacking, friction and vehicles more or less free. It is also a large
  dependency for a crate started dozens of times in one `cargo test`
  run, its determinism does not survive a compiler or version change,
  and the prim → collision-shape pipeline (convex decomposition for
  meshes and sculpts) is the expensive corner
  [[server-physics-integration]] already names.
- A kinematic solver — AABB / sphere overlap tests from the store's
  spatial index, a ground clamp, velocity integration, no contact
  resolution — covers every event listed above and every function except
  vehicles and stacking, in a fraction of the code, with determinism by
  construction.

The recommendation is the kinematic one, scoped to: overlap detection
between entities with `PrimFlags::Physics` or `VolumeDetect` and
avatars; ground and water collision from the heightfield; velocity and
impulse integration for physical prims; a raycast
(the viewer's own static raycast index — a custom `parry3d` index — is
the precedent, and the same approach fits here); and
`llMoveToTarget` / `llTarget` as a spring toward a point with the
tolerance the API states. Vehicles are explicitly deferred to
[[server-lsl-lib-physics-vehicles]], which may revisit the engine
choice.

The collision detected-block is shared with touch and sensors
([[server-lsl-lib-detection-sensors]]): position, velocity, link number,
and for `land_collision` the ground point.

Acceptance: two prims driven into each other produce exactly one
`collision_start`, N `collision` and one `collision_end`; an avatar
walking into a `VolumeDetect` prim triggers it and passes through;
`llCastRay` against a fixture scene returns the hit the geometry
implies; and the whole thing is reproducible across two runs of one seed.
