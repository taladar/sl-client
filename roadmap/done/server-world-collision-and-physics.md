---
id: server-world-collision-and-physics
title: Enough physics for the collision, target and volume-detect events
topic: server
status: done
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

## From gridspec-lsl-events (2026-10-25)

`events/collision` (`book/src/gridspec/lsl.md` § Events), a physical prim
dropped two metres onto a scripted one: on Second Life one
`collision_start` and one `collision_end` with about 11 `collision` events
between; on OpenSim each twice (the prim bounced) with 66–76. The faller's
`llDetectedType` is 10 on both. Under `llVolumeDetect(TRUE)` the scripted
prim gets a start and an end and no `collision`, on both. **A collision
that begins and ends while the script sleeps is lost on Second Life** — no
event at all on waking — and queued in full on OpenSim (82–132 `collision`
events). From `events/state-drops`: an `llTarget` and `llRotTarget` set
just before a state change keep raising `at_target` / `at_rot_target` in the
new state on Second Life and are dropped on OpenSim.

## Done (2026-11-13)

The decision: **a kinematic solver**, `sl_fake_grid::physics`, run each
heartbeat tick before the scripts, inside their update burst (and on its own
in a region with no script), documented in the book's `simulator/physics.md`.
Prims collide as the boxes their scale and rotation bound, a physical object
as the union of its prims'; each tick moves it by gravity (less
`llSetBuoyancy`'s share) or `llMoveToTarget`'s pull, axis by axis against
solid prims and the ground. Contacts within a centimetre raise
`collision_start` / `collision` / `collision_end` on both sides,
`land_collision_*` at the root, a volume-detecting prim a start and an end
with the physical object or avatar passing through it unaware, and
`moving_start` / `moving_end` as an object starts and stops. A child's
collision is routed by `llPassCollisions` with the grid's touch-passing rule.

Each flavour as `events/collision` measured it (`PhysicsPolicy`): Second
Life's object sleeps a quarter of a second after coming to rest and its
contacts end, and a script busy when a collision began hears none of it;
OpenSim's bounces at ubODE's material restitutions above 1.5 m/s, keeps
touching, and queues everything. `fake-sl`'s `events/collision` now agrees
with aditi's (eleven `collision` events included), and `fake-opensim`'s with
OpenSim's but for the volume-detect phase's landing, where the local region
has a prim under the probe (a deliberate entry in `accepted.toml`).

The runtime gained `Host::pass_collisions`, `Host::move_to_target` and
`Host::set_buoyancy`, and the library `llPassCollisions`, `llMoveToTarget`
(refusing a tau of 0, and on Second Life a target past 65 m),
`llStopMoveToTarget` and `llSetBuoyancy`. `llCastRay` and `llGetStatus`
were already in place. `tests/scripts.rs` holds the drop on both flavours
(twice, the same events each time), an avatar passing through a
volume-detecting prim, and `llMoveToTarget` reaching its target in 0.7 s.

Avatars and prims, which [[server-world-agent-movement]] left here ("no
prims to stand on or bump into"): the controller is handed the solid prims
within 16 m (`Ground::blocks`, `physics::avatar_blocks`). A prim more than a
step (0.35 m, not measured) above the feet stops a walk where the avatar meets
it, a lower one is stepped on to, a prim's top is stood on with the collision
plane level at it, walking off one higher than a step is a fall, and a climb
stops at a prim's underside. Phantom and volume-detecting prims are walked
through. The avatar is a 0.5 × 0.5 × 1.9 m box for both the controller and
the collisions it raises.

Left to [[server-lsl-lib-physics-vehicles]]: forces and impulses, the
look-at functions, keyframed motion, vehicles, characters, material and mass,
and a physical prim pulled by a grab (`object-touch-grab` leg 10).
