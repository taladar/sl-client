---
id: viewer-perf-custom-static-raycast-index
title: Replace avian with a custom off-thread static raycast index
topic: viewer
status: done
origin: full-session aditi Tracy capture (2026-08-15) — physics spikes despite the disabled solver
refs:
  - viewer-physics-static-prim-colliders
  - viewer-perf-hover-pick-raycast
  - viewer-perf-render-app-bound-frame
  - viewer-perf-asset-streaming-frame-spikes
  - viewer-perf-pipeline-specialization-stalls
---

Context: [context/viewer.md](../context/viewer.md).

## Decision (locked with user 2026-08-15)

**Remove avian entirely** and replace it with a bespoke, off-thread spatial
index for raycasts against static world geometry. We do not use avian for
physics — the solver is already gutted (`SubstepCount(1)`) and does ~0 work —
yet we pay its full per-substep spatial-index maintenance on the hot path to
answer essentially **one camera raycast per frame**. That is the wrong trade.
Go straight to the custom index; no interim avian mitigations (fixed-timestep
catch-up clamp, change-driven `build_static_colliders` scan) — they are retired
along with avian.

## Motivation — what the 2026-08-15 aditi trace showed

Trace `tracy-captures/aditi-2026-08-15.tracy` (release, `profile-tracy`, 1890
frames / 2:18, clean disconnect). The solver disable *works* — avian's
integrator / constraint-solver / joints are all ~0.003–0.02 ms/frame. But the
full `PhysicsPlugins::default()` set still runs its spatial-index maintenance
**every fixed step over the whole static collider set** (the solver *plugins*
can't be removed — avian's `PhysicsSchedule` panics on ambiguous ordering
without them, `physics.rs:168`):

- `collider_tree::optimization::block_on_optimize_tree` — the broadphase BVH,
  **max 43.7 ms**, and it *blocks* the loop on the optimize task; churns when
  static colliders are added/moved en masse (rez).
- per-collider transform↔position and AABB sync run **every substep over every
  collider** (`transform_to_position` 795 ms total, `collider_transform` plugin
  497 ms, `update_moved_collider_aabbs` 370 ms), though our static colliders
  never move after settling.

These are then **multiplied by fixed-timestep catch-up**: `RunFixedMainLoop`
spikes to **117 ms** on the frame right after the ~1 s occlusion-end stall (a
burst of catch-up substeps, each repeating the whole-set maintenance), and to
40–95 ms during the rez storm (t≈53–54 s, colliders churning → tree
re-optimize). Separately, our own `build_static_colliders` (`physics.rs:2501`)
runs an **O(all prims) candidate scan every frame** (~1.16 ms/frame steady, max
30 ms at rez) — the *builds* are budgeted (`STATIC_COLLIDER_BUDGET = 16`/frame)
but the scan that feeds them is a full sweep regardless.

## Who actually uses avian (the migration surface)

- **`SpatialQuery::cast_ray` — only `camera.rs`** (camera collision). The sole
  production raycast consumer. Every other `cast_ray` in the crate
  (`media_prim`, `gizmos`, `reach`, `objects`, `object_menu`, `edit_create`,
  `hud_pick`) is Bevy's `MeshRayCast`, not avian.
- **`Collisions` / `CollisionStart` — only `world_sounds.rs`** (physical-prim
  contact sounds). The one non-raycast dependency.
- `physics.rs` SpatialQuery uses are `#[cfg(test)]`; `flexi.rs` imports avian
  types but runs its own chain solver.

## Design

- A custom `StaticRaycastIndex` — a BVH (or loose grid / octree) over prim AABBs
  with a reference to each prim's collision shape. Reuse **`parry3d`** directly
  for the ray-vs-shape math (avian is already built on it) so we keep robust
  trimesh / convex / primitive raycasts without the engine, schedule, or solver.
- **Maintained entirely off-thread.** The main thread only pushes deltas —
  insert / remove / update-by-entity — from prim rez / derez / move
  **change-detection** (no O(all prims) scan). A background task owns the
  structure, applies deltas, rebuilds / rebalances, and publishes an immutable
  snapshot (arc-swap / double-buffer).
- **Queries read the latest snapshot, lock-free**, in `Update`. Static geometry
  never moves, so a 1–2 frame lag on a freshly-rezzed prim's collider is
  imperceptible — the same "collider settles a few frames after rez" behaviour
  the 16/frame budget already gives. No fixed-step schedule, no solver, no
  per-substep sync: the entire `FixedMain` / `PhysicsSchedule` cost and the
  `build_static_colliders` scan both disappear from the hot path.
- Built to also serve **future world-space raycasts** — the `MeshRayCast`
  consumers above do O(all meshes) immediate-mode casts today; folding them onto
  this index supersedes [[viewer-perf-hover-pick-raycast]]. GPU picks keep
  screen-space.

## Migration / open decisions

1. **Collision sounds (`world_sounds.rs`)** — the only consumer needing *contact
   events*, not raycasts. Answer it with overlap queries against the same index,
   or drive collision sounds from server sound-triggers instead of client
   contact detection — decide during impl (check whether client-side contact
   detection is even faithful for kinematic, server-snapped prims).
2. **`bevy/multi_threaded`** — avian's `parallel` feature currently pulls it in,
   and async pipeline compilation depends on it
   ([[viewer-perf-pipeline-specialization-stalls]]). When avian is dropped, add
   `bevy/multi_threaded` **explicitly** so that does not silently regress.
   Remove `avian3d` (and any now-unused parry/dependency licences from
   `deny.toml`) once nothing references it.
3. **Future dynamics (Phase 32/34)** — dropping avian does **not** foreclose
   real client-side dynamics; that would be a separate engine scoped to the
   handful of genuinely-dynamic bodies, orthogonal to a static raycast index.
   This index is static-only by design.

## Supersedes / relates

Supersedes the avian approach in [[viewer-physics-static-prim-colliders]]
(done); absorbs [[viewer-perf-hover-pick-raycast]]; buys Main-thread headroom
tracked by [[viewer-perf-render-app-bound-frame]]; removes the physics
contributor to the rez-storm spikes in
[[viewer-perf-asset-streaming-frame-spikes]].

## Implemented (2026-08-16)

Shipped as a two-step internal migration (each buildable): (1) new
`raycast_index.rs` fed from the collider path + camera collision repointed onto
it; (2) full avian removal. What landed:

- **`raycast_index.rs`** — `StaticRaycastIndex` (a parry [`Bvh`] over static
  prim colliders, built on the `AsyncComputeTaskPool` and published through an
  `ArcSwap`, queried lock-free) + `DynamicColliders` (the handful of moving
  physical prims, a per-frame linear set, so the static BVH never rebuilds for a
  mover). `RaycastIndexColliders` is the change-driven authoring set. `cast_ray`
  (nearest-hit distance, hollow/solid, solid-only, entity-exclude) and
  `contact_pairs` (parry narrowphase for collision sounds). Unit-tested.
- **`physics.rs`** — no physics engine at all now: `PhysicsPlugin` dropped the
  avian world/gravity/substeps/fixed-clock/`Time<Physics>` dilation. Collider
  construction (`mesh_physics_collider` / `prim_geometry_collider` /
  `run_collider_build`) builds parry `SharedShape`s; `StaticCollider` /
  `RefinedCollider` carry the shape; `sync_raycast_index` (static,
  change-driven) and `sync_dynamic_colliders` (physical, per-frame) feed the
  index. Kinematic movers keep dead-reckoning (they never needed a solver).
- **`camera.rs`** — `collide_camera` casts the static index + the dynamic set,
  nearest wins.
- **`world_sounds.rs`** — `ingest_collisions` reads
  `DynamicColliders::contact_pairs` with contact-*edge* detection (a
  `touching_pairs` set) so a resting pair stays silent, replacing avian
  `CollisionStart`/`Collisions`.
- **Deps** — `avian3d` removed; `parry3d` + `arc-swap` are direct deps;
  `bevy/multi_threaded` declared explicitly (avian's `parallel` used to pull it,
  and async pipeline compilation needs it —
  [[viewer-perf-pipeline-specialization-stalls]]).

parry gotchas paid for: parry 0.27 builds on **glam 0.32** (≠ Bevy's glam), so
all vec/quat/point crossings round-trip through arrays; `Shape: RayCast` is a
supertrait, so a `SharedShape` raycasts directly; `Bvh::cast_ray` takes a
per-leaf `primitive_check` closure and returns the nearest `(leaf, distance)`.

### Verification

- **Done:** `cargo clippy --all-targets` clean; 1274 lib tests pass (index
  raycast + collider-shape construction unit tests); camera collision
  **live-verified on aditi** (normal follow distance in the open, pulls in at
  walls, no clipping / head-jamming), session stable, clean exit, no panics.
- **Pending (env-limited):** collision sounds are unit-tested (contact-edge
  logic) but not live-verified — needs two colliding server-physical prims,
  which a random aditi spot does not provide. Exercise when a physical-prim
  scene is available.
- **Measured (2026-08-16 re-capture,
  `tracy-captures/aditi-2026-08-16-postavian.tracy`):** avian's
  `PhysicsSchedule` / `SubstepSchedule` / `collider_tree` /
  `transform_to_position` are **gone**; `RunFixedMainLoop` dropped
  **max 117.6 → 12.5 ms**, mean 5.35 → 0.89 ms (the residual is Bevy's now-empty
  fixed schedules). Main-thread total fell **34.1 → 29.2 ms**. The steady-state
  median frame is unchanged (~53 ms) — expected, since the frame is render-app
  bound ([[viewer-perf-render-app-bound-frame]]); the win is the eliminated 117
  ms spike class + ~5 ms of Main headroom, not median fps. Correction to an
  earlier draft of this note: `build_static_colliders` still runs its per-frame
  scan (~1 ms) — that scan was never avian and is a separate lever, not removed
  here.
