---
id: viewer-perf-avatar-ground-probe
title: Avatar ground probe — stop per-frame full-scene raycasts
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

`probe_avatar_ground` (`ground.rs:149-219`) casts, **every frame, for
every rigged avatar**, three vertical `MeshRayCast` rays (root + both
ankles). Bevy's `MeshRayCast` iterates the whole scene of mesh entities
per cast, and the `accept` closure walks each candidate's parent chain
calling `avatar_roots.contains(&current)` — a linear scan of a `Vec`
rebuilt each frame.

Cost ≈ avatars × 3 rays × scene-mesh count, per frame. The dominant factor
is the **scene-mesh count, not the avatar count**: a vertical downward ray
touches every castable mesh in the whole region even though only surfaces
in the avatar's ground column can possibly be hit (the measurement below
shows the cost tracks scene fill, not how many avatars are present).

## ⚠️ Temporal gating and avatar-culling are UNSAFE — reverted (2026-08-10)

A first attempt cached samples and re-probed only on
movement/terrain-change/safety-interval (fix **A**), plus a distance-cull
of far avatars. It was **reverted after a live aditi test**: the avatar's
legs punched through prims and terrain repeatedly. The reason is the
design constraint everything else here must respect:

- The probe result is consumed **every frame** by the foot IK
  (`locomotion_ik.rs:636+` drives the feet toward `ground.root/left/right`)
  **and** by the airborne classification (`locomotion_ik.rs:598`:
  `airborne = !walking && ground.root.is_none()`). It must stay **fresh**
  and **present** every frame.
- **Gating → stale ground.** As the avatar moves (or steps onto a prim /
  off a ledge), a cached ground point from a previous position makes the IK
  plant the foot at the wrong height; the "clamp the goal into reach" step
  then extends the leg toward that stale point — through the surface. A
  horizontal-movement gate also cannot see vertical motion at all.
- **Culling → false airborne.** A skipped avatar reads all-`None`, which
  the consumer reads as *airborne*, not *unprobed* — flipping a distant
  standing avatar into the airborne branch. `AgentGround` cannot currently
  tell "not probed" from "probed, no ground".

The body itself never fell: `drive_avatar_motion` clamps the body Z to the
terrain `land_height`, independent of `AvatarGround`. The visible "falling
through" was the **foot IK**, not physics.

**Conclusion: do not skip or defer probes.** The only safe optimisation is
to make each per-frame probe **cheaper while staying fresh and present** —
fix **B (spatial)** below. Fix A (temporal gating) is off the table. A
distance-cull of far avatars could return **only** if the consumer is first
taught to treat an unprobed avatar as "no IK adjustment" (keep the
animation pose) rather than "airborne", and to tolerate a slightly stale
cached ground for far avatars — a `locomotion_ik` change, not just a probe
change.

## B (spatial) — the safe path: cheaper per-ray, still fresh every frame

Bevy `MeshRayCast`'s broad phase visits every castable mesh regardless of
any filter, so per-ray object pruning needs a real spatial structure. A
vertical ray needs only walkable surfaces in the avatar's (x,z) column:

- **Terrain is a heightfield** — `TerrainState::land_height(region, x, y)`
  already returns the ground height under a region-local (x,y) with no
  raycast. Sampling it (plus a matching slope-normal sampler) gives the
  land ground in O(1). It needs a Bevy→region-local conversion
  (`coords.rs`, `region_offset_bevy`) with region-border handling, and —
  to skip the object raycast for an on-land avatar — a check that the foot
  is actually near the terrain height (else it is standing on a prim floor
  above the land and still needs the object test).
- **Objects:** a lightweight spatial index (uniform grid keyed by world
  x,z cell) of walkable object faces, maintained on spawn/move/despawn; the
  probe queries only the cell(s) under the avatar → bounded candidates per
  ray. This keeps the probe fresh (recomputed every frame) but cheap.
- **avian's `SpatialQuery` is not a drop-in.** Its colliders exist only for
  server-**physical** prims (`physics.rs` P31.2 kinematic movers), not for
  static walkable prims or terrain, so its BVH does not cover the ground.
  Giving all non-phantom geometry avian **static** colliders would let one
  BVH serve every world raycast (ground, camera, pick, teleport), but is a
  much larger change (trimesh colliders for the world) — the "unified"
  option, not the first step.

The one genuinely safe cleanup from the reverted attempt: the accept
closure's `avatar_roots` `Vec` → `HashSet` (O(1) membership) — no effect on
freshness or presence. Re-land that with B.

## Measured — the cost is scene-density-bound (Tracy, aditi, 2026-08-10)

Same 1–3 avatars throughout a session, `probe_avatar_ground` binned by rez
progress:

| phase | mean | max |
| --- | --- | --- |
| t=0–20 s (near-empty scene) | 0.31 ms | 2.6 ms |
| t=40–60 s (filling) | 4.89 ms | 72.5 ms |
| t=80–120 s (rezzed) | ~5.3 ms | 30 ms |

Same avatars, 16× cost as the region's prim faces stream in — direct
evidence the fix is spatial scoping (B), not a faster per-cast, and not a
temporal skip.

## Estimated impact

High, but only via B now. B-terrain (heightfield) makes the common on-land
probe O(1) while keeping it fresh every frame; B-object (a spatial index)
does the same for an avatar on a prim floor. Neither introduces the
staleness that made gating/culling unsafe. Verify with [[viewer-profiling]]
Tracy zones **and** a live foot-planting check (walk across terrain and
onto a prim ramp — feet must stay planted, no punch-through).

Confidence: high on the diagnosis (consumers, physics independence, and the
live failure all verified); the fix is non-trivial (coordinate conversion,
normal sampler, spatial index) and must be foot-planting-verified live.
