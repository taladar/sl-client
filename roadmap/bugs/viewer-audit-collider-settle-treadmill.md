---
id: viewer-audit-collider-settle-treadmill
title: A mesh with no physics block rebuilds its collider and the BVH every frame forever
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-view/src/physics.rs:2058` —
`needs_build = existing.is_none_or(|state| !state.settled || ...)`. When a mesh
carries no physics block the geometry fallback (`:2118-2131`) returns
`settled = false` **deliberately**: the comment says "keep `settled = false` so
it retries for the lighter shape".

That retry can never succeed. `MeshManager::physics()`
(`sl-viewer-world-objects/src/meshes.rs:488`) returns
`Option<&Arc<MeshPhysics>>` whose doc says `None` means "still in flight, the id
was never requested, **or the mesh carried no physics block**" — the caller
cannot distinguish. The backing map is
`HashMap<MeshKey, Option<Arc<MeshPhysics>>>` and caches the absent case
terminally as `Some(None)`; `request_physics` (`:461`) early-returns on
`contains_key`.

So every frame: re-queue -> `gather_object_geometry` -> off-thread trimesh ->
`StaticCollider` re-insert -> `Changed<StaticCollider>` -> `sync_raycast_index`
-> full raycast-index record clone -> **full BVH rebuild**. The same
non-settling condition on the **physical** path (`physics.rs:1611`,
`geometry_pending`) is worse: unbudgeted and on the main thread, re-running
`gather_object_geometry` plus `SharedShape::trimesh` inline. The same loop
occurs when `fetcher.has_cap_url()` is false.

Scope: distinguish "physics absent" from "physics pending" in the mesh manager
and settle on absent. Extract
`fn collider_job_settled(mesh, physics_available, points_empty) -> (bool, bool)`
so the invariant "a mesh with terminally-absent physics and usable geometry
settles" is a one-line assertion — it fails today.
