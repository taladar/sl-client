---
id: viewer-physics-static-prim-colliders
title: Colliders for all non-phantom prims — a shared avian spatial index
topic: viewer
origin: camera-collision analysis + user architecture discussion (2026-08-15)
status: ready
refs: [viewer-perf-camera-collision-broad-phase, viewer-perf-media-hover-gpu-pick]
---

Context: [context/viewer.md](../context/viewer.md); `physics.rs` P31.1–P31.3.

Today avian is populated with colliders for **physical prims only**
(`FLAGS_USE_PHYSICS` → kinematic `RigidBody` + `Collider`, for dead-reckoning,
collision-sound events, and the future Phase 32/34 client sims). Static
(non-physical) world geometry — every wall, floor, building — has **no
collider**, so avian is not a spatial index of the scene.

The recurring need: a **spatial "objects near X" lookup** so per-frame work
stops brute-forcing every object. It has come up for camera collision
([[viewer-perf-camera-collision-broad-phase]] — `collide_camera`'s whole-scene
`MeshRayCast`, ~54 ms of related raycast cost at crowd scale) and would serve
the hover/pick-adjacent paths, physical prims landing on / making sounds against
static surfaces, and the Phase 32 (flexi) / Phase 34 (cloth/body) dynamic bodies
that must collide against static geometry. avian already maintains a BVH and
exposes `SpatialQuery`; populating it fully **is** that index.

## Direction

Give every **non-phantom** prim an avian collider, so `SpatialQuery` becomes the
shared scene index. Build the collider from the best shape available:

- **Mesh objects** — use the already-decoded, already-retained upload-time
  physics shape (`sl_mesh::MeshPhysics`, `Arc`-stored in the mesh store):
  `.convex` (the HACD convex-hull decomposition) → a compound of
  `Collider::convex_hull`s — accurate *and* cheap; its single
  **low-detail bounding hull** is a ready cheap broad-phase shape. `.mesh` (the
  exact physics triangle mesh) only where narrow-phase precision is needed.
  NOTE: today `refine_physical_colliders` builds even physical **mesh**
  colliders from the **visual** `GeometryHolder` geometry, not `MeshPhysics` —
  switch to the physics shape here too (more accurate, far less memory).
- **Non-mesh prims** (box / sphere / torus / sculpt …) — from prim geometry per
  `LLPhysicsShapeType` (`convex_hull` / `trimesh`), as P31.3 already does for
  the physical ones; extend to non-physical prims (default a cheap convex/cuboid
  unless the trimesh is warranted).
- **Phantom prims** (`FLAGS_PHANTOM`) — skip (non-solid; nothing should collide
  with them).
- Linkset children handled (each solid child its own collider, as geometry
  resolves); build lazily once geometry + shape are available.

## Budget (required)

A region hand-off or a busy sim can deliver hundreds of meshes in one frame;
inserting all their colliders at once would spike the frame (collider
construction + BVH insertion). **Cap the number of colliders built per frame**
(a per-frame budget / work queue, like the crowd `SPAWN_BATCH` and the
asset-upload budgets) and drain the backlog over subsequent frames. Colliders
missing for a frame or two is invisible; a stall is not. Consider prioritising
by proximity to the camera so nearby geometry gets its collider first.

## Consumers (follow-ups, once the index exists)

- Camera collision → `SpatialQuery::cast_ray` over the short head→eye segment
  ([[viewer-perf-camera-collision-broad-phase]]).
- Physical prims landing on / sounding against static surfaces.
- Phase 32 (flexi) / Phase 34 (cloth/body) dynamic collisions.
- Any future "operate on objects near X" pass instead of iterating all objects.

## Verify

A region of static prims: `SpatialQuery::cast_ray` finds walls/floors; phantom
prims are not hit; mesh colliders match the uploaded physics shape (not the
visual LOD); a region hand-off does not spike the frame (colliders stream in
under the budget over several frames); memory stays bounded at region scale.
