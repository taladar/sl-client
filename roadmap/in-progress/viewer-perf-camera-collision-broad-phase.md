---
id: viewer-perf-camera-collision-broad-phase
title: Camera-collision raycast off whole-scene MeshRayCast onto a broad phase
topic: viewer
status: in-progress
origin: GPU-avatar crowd critical-path analysis (2026-08-14)
refs: [viewer-perf-media-hover-gpu-pick, viewer-perf-gpu-avatar-extract-skins-floor, viewer-physics-static-prim-colliders]
---

Context: [context/viewer.md](../context/viewer.md).

**Status (2026-08-15): `SpatialQuery` code landed, inert until the index.**
`collide_camera` now uses `avian3d::SpatialQuery::cast_ray` over the short
head→eye segment (`camera.rs`) — the right shape: BVH broad phase, avatars
excluded for free (no collider). But avian is currently populated with colliders
for **physical prims only**; static world geometry (the walls the camera must
collide with) has no collider, so the query finds nothing and **camera collision
is knowingly non-functional** until [[viewer-physics-static-prim-colliders]]
populates the scene index. Accepted by the user as a temporary broken state
rather than carrying a `MeshRayCast` stopgap. When that task lands, this becomes
done with no further code change.

Alternative not taken (recorded in case collision must work sooner): keep
`MeshRayCast` but filter out skinned meshes (avatars) — kills the ~4,500-submesh
crowd cost while keeping visual collision on all prims; O(prims), not O(log n).

`sl_client_bevy_viewer::camera::collide_camera` (the third-person camera pulling
the eye in when the line of sight to the avatar is obstructed) casts a
`MeshRayCast` with a filter that excludes only the **own** avatar's body, so it
tests **every other mesh in the scene**. `MeshRayCast` has no broad phase →
`O(scene meshes)`.

Measured (aditi, `SL_VIEWER_CROWD=100`, ~4,500 skinned submeshes):
`position_camera` is **median 0.06 ms but spikes to p90 78 ms / max 102 ms**,
and ~39% of frames pay it — the third-person frames, each casting against the
whole crowd. Already correctly **gated to third-person only** (the `Mouselook`
and `Flycam` arms of the mode match never call it), so this is purely the
per-cast cost, not over-running.

Key point (user): the cast only cares about the **short segment from the head to
a few metres back**, so testing every object in the scene is the wrong shape.
A spatial broad phase should test only colliders near that short segment.

## Direction

The viewer already gives scene prims avian3d `Collider`s (`physics.rs`), so a
BVH broad phase exists. Two options:

- **`avian3d::SpatialQuery::cast_ray`** of length head→eye, prim colliders only.
  BVH-accelerated (O(log n)); and because avatars carry **no** avian collider it
  excludes them for free — which is also the correct reference behaviour (the
  camera does not pull in because another avatar walked behind you). Caveat:
  avian colliders come from each prim's **physics** shape, so a prim set to
  physics-shape **None** (phantom) has no collider and the camera would clip
  through it, whereas the current cast collides with *visual* geometry. Verify
  against the reference (LL uses its object spatial-partition) whether camera
  collision should pull in for phantom prims before accepting this edge.

- **Lightweight visual-AABB broad phase**: pre-filter meshes whose `Aabb`
  (world) intersects the short head→eye segment, then `MeshRayCast` only those.
  Keeps visual-geometry occlusion (phantom prims included); more code than the
  avian path.

Either way, keep excluding avatars (the reference does not collide the camera
with them; they also have no colliders today).

## Verify

`CROWD=100` aditi, third person: `position_camera` p90 drops from ~78 ms to
sub-millisecond; the camera still pulls in at a real wall / prim and no longer
pulls in for avatars; no clipping regression on the occluder classes chosen.
